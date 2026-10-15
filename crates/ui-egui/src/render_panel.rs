//! The Render window: a progressive path-traced image of the scene, rendered on a worker thread.

use egui::{ColorImage, RichText, TextureHandle, TextureOptions};
use shape_render::{RenderCamera, RenderScene, Renderer, Settings};
use shape_scene::Scene;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Snapshot the worker publishes after each pass.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub samples: u32,
    pub seconds: f32,
}

struct Job {
    stop: Arc<AtomicBool>,
    latest: Arc<Mutex<Option<Frame>>>,
    renderer: Arc<Mutex<Option<Renderer>>>,
}

#[derive(Default)]
pub struct RenderPanel {
    pub open: bool,
    job: Option<Job>,
    texture: Option<TextureHandle>,
    pub last: Option<Frame>,
    pub width: u32,
    pub height: u32,
    pub target_samples: u32,
    pub settings: Settings,
    /// Blender's Resolution %: the render is width*height scaled by this.
    pub resolution_percent: u32,
    /// Render Animation writes `<output_path><frame:04>.png`.
    pub output_path: String,
    /// A running Render Animation job: frames done and total.
    animation: Option<Arc<Mutex<(u32, u32)>>>,
}

impl RenderPanel {
    pub fn new() -> Self {
        Self {
            width: 640,
            height: 400,
            target_samples: 64,
            settings: Settings::default(),
            resolution_percent: 100,
            output_path: "render/frame_".into(),
            ..Default::default()
        }
    }

    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    /// Output size after Resolution %.
    pub fn output_size(&self) -> (u32, u32) {
        let s = self.resolution_percent.max(1) as f32 / 100.0;
        (((self.width as f32 * s).round() as u32).max(1), ((self.height as f32 * s).round() as u32).max(1))
    }

    pub fn animation_progress(&self) -> Option<(u32, u32)> {
        let a = self.animation.as_ref()?;
        let (done, total) = *a.lock().ok()?;
        (done < total).then_some((done, total))
    }

    /// Render every frame of the range on a worker thread, evaluating the animation at each frame
    /// and saving PNGs. `camera_of` picks the camera for a frame's scene.
    pub fn start_animation(&mut self, scene: &Scene, camera_of: impl Fn(&Scene) -> RenderCamera + Send + 'static, ctx: egui::Context) {
        let (w, h) = self.output_size();
        let (target, settings, path) = (self.target_samples.max(1), self.settings, self.output_path.clone());
        let (start, end) = (scene.timeline.start, scene.timeline.end.max(scene.timeline.start));
        let total = (end - start + 1) as u32;
        let progress = Arc::new(Mutex::new((0u32, total)));
        self.animation = Some(progress.clone());
        let mut scene = scene.clone();
        std::thread::spawn(move || {
            if let Some(parent) = std::path::Path::new(&path).parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            for frame in start..=end {
                scene.evaluate_animation(frame as f32);
                let rs = RenderScene::build(&scene, camera_of(&scene), &settings);
                let mut r = Renderer::new(w, h, settings);
                for _ in 0..target {
                    r.pass(&rs);
                }
                let _ = r.save_png(std::path::Path::new(&format!("{path}{frame:04}.png")));
                if let Ok(mut p) = progress.lock() {
                    p.0 += 1;
                }
                ctx.request_repaint();
            }
        });
    }

    /// Start rendering `scene` from `camera` on a worker thread; progress arrives in [`Self::ui`].
    pub fn start(&mut self, scene: &Scene, camera: RenderCamera, ctx: egui::Context) {
        self.stop();
        self.open = true;
        let rs = RenderScene::build(scene, camera, &self.settings);
        let stop = Arc::new(AtomicBool::new(false));
        let latest: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));
        let (w, h) = self.output_size();
        let renderer = Arc::new(Mutex::new(Some(Renderer::new(w, h, self.settings))));
        let (target, settings) = (self.target_samples.max(1), self.settings);
        let job = Job { stop: stop.clone(), latest: latest.clone(), renderer: renderer.clone() };
        std::thread::spawn(move || {
            let start = std::time::Instant::now();
            let mut r = Renderer::new(w, h, settings);
            for _ in 0..target {
                if stop.load(Ordering::Relaxed) {
                    break;
                }
                r.pass(&rs);
                let frame = Frame { width: w, height: h, rgba: r.to_rgba8(), samples: r.samples, seconds: start.elapsed().as_secs_f32() };
                if let Ok(mut l) = latest.lock() {
                    *l = Some(frame);
                }
                ctx.request_repaint();
            }
            if let Ok(mut slot) = renderer.lock() {
                *slot = Some(r);
            }
            stop.store(true, Ordering::Relaxed);
            ctx.request_repaint();
        });
        self.job = Some(job);
    }

    pub fn stop(&mut self) {
        if let Some(job) = &self.job {
            job.stop.store(true, Ordering::Relaxed);
        }
        self.job = None;
    }

    /// Save the latest image as PNG.
    pub fn save_png(&self, path: &std::path::Path) -> Result<(), String> {
        let frame = self.last.as_ref().ok_or("nothing rendered yet")?;
        let img = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone()).ok_or("bad frame")?;
        img.save(path).map_err(|e| e.to_string())
    }

    fn pull(&mut self, ctx: &egui::Context) {
        let Some(job) = &self.job else { return };
        let frame = job.latest.lock().ok().and_then(|mut l| l.take());
        if let Some(frame) = frame {
            let image = ColorImage::from_rgba_unmultiplied([frame.width as usize, frame.height as usize], &frame.rgba);
            match &mut self.texture {
                Some(t) => t.set(image, TextureOptions::LINEAR),
                None => self.texture = Some(ctx.load_texture("render", image, TextureOptions::LINEAR)),
            }
            self.last = Some(frame);
        }
        if job.stop.load(Ordering::Relaxed) && job.renderer.lock().ok().is_some_and(|r| r.is_some()) {
            // worker finished
            self.job = None;
        }
    }

    pub fn ui(&mut self, ctx: &egui::Context) -> Option<RenderAction> {
        if !self.open {
            return None;
        }
        self.pull(ctx);
        let mut action = None;
        let mut open = self.open;
        egui::Window::new("Render").open(&mut open).default_size([700.0, 520.0]).resizable(true).show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut self.width).range(16..=4096).prefix("w "));
                ui.add(egui::DragValue::new(&mut self.height).range(16..=4096).prefix("h "));
                ui.add(egui::DragValue::new(&mut self.target_samples).range(1..=4096).prefix("samples "));
                ui.add(egui::DragValue::new(&mut self.settings.exposure).speed(0.01).range(0.05..=16.0).prefix("exposure "));
                if self.is_running() {
                    if ui.button("Stop").clicked() {
                        self.stop();
                    }
                    ui.spinner();
                } else if ui.button("Render (F12)").clicked() {
                    action = Some(RenderAction::Start);
                }
                if ui.add_enabled(self.last.is_some(), egui::Button::new("Save PNG…")).clicked() {
                    action = Some(RenderAction::Save);
                }
            });
            if let Some(f) = &self.last {
                ui.label(RichText::new(format!("{} × {}  ·  {} samples  ·  {:.1}s", f.width, f.height, f.samples, f.seconds)).weak());
            }
            if let Some(t) = &self.texture {
                let avail = ui.available_size();
                let (w, h) = (t.size()[0] as f32, t.size()[1] as f32);
                let scale = (avail.x / w).min(avail.y / h).clamp(0.05, 1.0);
                ui.add(egui::Image::new((t.id(), egui::vec2(w * scale, h * scale))));
            } else {
                ui.label(RichText::new("Press Render to start.").weak());
            }
        });
        self.open = open;
        action
    }
}

/// Buttons the window asks the app to act on (the app owns the scene and the file dialog).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderAction {
    Start,
    Save,
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_scene::KeyingSet;

    #[test]
    fn render_animation_writes_one_png_per_frame_at_the_scaled_size() {
        let dir = std::env::temp_dir().join(format!("shape-anim-{}", std::process::id()));
        let mut scene = Scene::new("a");
        let id = scene.add_mesh_object("Cube", shape_geom::primitives::cube(1.0), None);
        scene.insert_keyframe(id, 1, KeyingSet::Location);
        scene.timeline.start = 1;
        scene.timeline.end = 3;
        let mut panel = RenderPanel::new();
        panel.width = 40;
        panel.height = 20;
        panel.resolution_percent = 50;
        panel.target_samples = 1;
        panel.output_path = format!("{}/f_", dir.display());
        assert_eq!(panel.output_size(), (20, 10));
        let ctx = egui::Context::default();
        panel.start_animation(&scene, |_| RenderCamera::look_at(shape_math::Vec3::new(0.0, 2.0, 5.0), shape_math::Vec3::ZERO, 1.0), ctx);
        for _ in 0..200 {
            if panel.animation_progress().is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(panel.animation_progress().is_none(), "finished");
        for f in 1..=3 {
            let img = image::open(dir.join(format!("f_{f:04}.png"))).expect("frame written");
            assert_eq!((img.width(), img.height()), (20, 10));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
