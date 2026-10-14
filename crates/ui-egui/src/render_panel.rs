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
}

impl RenderPanel {
    pub fn new() -> Self {
        Self { width: 640, height: 400, target_samples: 64, settings: Settings::default(), ..Default::default() }
    }

    pub fn is_running(&self) -> bool {
        self.job.is_some()
    }

    /// Start rendering `scene` from `camera` on a worker thread; progress arrives in [`Self::ui`].
    pub fn start(&mut self, scene: &Scene, camera: RenderCamera, ctx: egui::Context) {
        self.stop();
        self.open = true;
        let rs = RenderScene::build(scene, camera, &self.settings);
        let stop = Arc::new(AtomicBool::new(false));
        let latest: Arc<Mutex<Option<Frame>>> = Arc::new(Mutex::new(None));
        let renderer = Arc::new(Mutex::new(Some(Renderer::new(self.width, self.height, self.settings))));
        let (w, h, target, settings) = (self.width, self.height, self.target_samples.max(1), self.settings);
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
