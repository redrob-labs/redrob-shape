//! The 3D viewport panel: owns the camera and the GPU viewport, turns egui input into camera moves
//! and picks, and shows the rendered texture.

use egui::{Color32, RichText, Sense};
use shape_gpu::{OrbitCamera, ShadingMode, ViewOptions, Viewport};
use shape_math::Vec2;
use shape_scene::{ObjectId, Scene};
use std::sync::Arc;

/// GPU handles the viewport needs from eframe.
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub format: wgpu::TextureFormat,
    pub renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
}

impl GpuContext {
    pub fn from_eframe(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        let rs = cc.wgpu_render_state.as_ref()?;
        Some(Self { device: rs.device.clone(), queue: rs.queue.clone(), format: rs.target_format, renderer: rs.renderer.clone() })
    }
}

/// What the viewport reports back to the app after one frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewportEvents {
    /// Left click: the object under the cursor, or `None` for empty space. `Some(..)` only on click.
    pub clicked: Option<Option<ObjectId>>,
    pub shift: bool,
}

pub struct ViewportPanel {
    pub camera: OrbitCamera,
    pub options: ViewOptions,
    gpu: Option<(GpuContext, Viewport)>,
    texture: Option<(egui::TextureId, u64)>,
}

impl ViewportPanel {
    pub fn new(gpu: Option<GpuContext>) -> Self {
        let gpu = gpu.map(|g| {
            let vp = Viewport::new(g.device.clone(), g.queue.clone(), g.format);
            (g, vp)
        });
        Self { camera: OrbitCamera::default(), options: ViewOptions::default(), gpu, texture: None }
    }

    pub fn has_gpu(&self) -> bool {
        self.gpu.is_some()
    }

    pub fn frame_scene(&mut self, scene: &Scene) {
        let b = scene.bounds();
        if b.is_empty() {
            self.camera = OrbitCamera { yaw: self.camera.yaw, pitch: self.camera.pitch, ..OrbitCamera::default() };
        } else {
            self.camera.frame(b.min, b.max);
        }
    }

    /// Draw the panel into `ui`'s full rect, render the scene, and report input.
    pub fn ui(&mut self, ui: &mut egui::Ui, scene: &Scene) -> ViewportEvents {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let mut events = ViewportEvents::default();

        // ---- input ----
        let ppp = ui.ctx().pixels_per_point();
        let size_px = Vec2::new(rect.width() * ppp, rect.height() * ppp);
        let mods = ui.input(|i| i.modifiers);
        events.shift = mods.shift;
        if response.dragged_by(egui::PointerButton::Middle) || (response.dragged_by(egui::PointerButton::Secondary) && !mods.shift) {
            let d = response.drag_delta();
            self.camera.orbit(Vec2::new(d.x, d.y));
        }
        if response.dragged_by(egui::PointerButton::Secondary) && mods.shift || response.dragged_by(egui::PointerButton::Middle) && mods.shift {
            let d = response.drag_delta();
            self.camera.pan(Vec2::new(d.x, d.y), rect.height());
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.camera.zoom(scroll / 50.0);
            }
        }
        if response.clicked_by(egui::PointerButton::Primary)
            && let Some(pos) = response.interact_pointer_pos()
        {
            let px = Vec2::new((pos.x - rect.left()) * ppp, (pos.y - rect.top()) * ppp);
            events.clicked = Some(shape_gpu::pick(scene, &self.camera, px, size_px).map(|(id, _)| id));
        }

        // ---- render ----
        match &mut self.gpu {
            Some((ctx, vp)) => {
                let (w, h) = (size_px.x.round().max(1.0) as u32, size_px.y.round().max(1.0) as u32);
                vp.resize(w, h);
                vp.render(scene, &self.camera, &self.options);
                let generation = vp.texture_generation;
                let id = match self.texture {
                    Some((id, g)) if g == generation => id,
                    Some((id, _)) => {
                        ctx.renderer.write().update_egui_texture_from_wgpu_texture(&ctx.device, vp.color_view(), wgpu::FilterMode::Linear, id);
                        self.texture = Some((id, generation));
                        id
                    }
                    None => {
                        let id = ctx.renderer.write().register_native_texture(&ctx.device, vp.color_view(), wgpu::FilterMode::Linear);
                        self.texture = Some((id, generation));
                        id
                    }
                };
                ui.painter().image(id, rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            None => {
                ui.painter().rect_filled(rect, 0.0, Color32::from_rgb(0x29, 0x2e, 0x37));
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Viewport (no GPU)",
                    egui::FontId::proportional(18.0),
                    Color32::from_rgb(0x7c, 0x83, 0x90),
                );
            }
        }

        // ---- overlay ----
        let label = match self.options.shading {
            ShadingMode::Wireframe => "Wireframe",
            ShadingMode::Solid => "Solid",
            ShadingMode::SolidWire => "Solid + Wire",
        };
        ui.painter().text(
            rect.left_top() + egui::vec2(8.0, 6.0),
            egui::Align2::LEFT_TOP,
            format!("{label}  ·  {} objects", scene.object_count()),
            egui::FontId::proportional(12.0),
            Color32::from_rgb(0xaa, 0xb0, 0xbb),
        );
        let _ = RichText::new("");
        events
    }
}
