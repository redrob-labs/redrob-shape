//! The 3D viewport panel: owns the camera and the GPU viewport, turns egui input into camera moves
//! and reports pointer state so the app can run tools and modals on top.

use egui::{Color32, Sense};
use shape_gpu::{OrbitCamera, ViewOptions, Viewport};
use shape_math::Vec2;
use shape_scene::Scene;
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

/// Pointer state over the viewport for one frame, in viewport pixels (y down).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewportInput {
    /// Pointer position when it is over the viewport.
    pub pointer: Option<Vec2>,
    /// Viewport size in pixels.
    pub size: Vec2,
    pub primary_clicked: bool,
    pub primary_pressed: bool,
    pub primary_down: bool,
    pub primary_released: bool,
    pub secondary_clicked: bool,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    /// Wheel notches this frame (positive = up), when the camera does not use them.
    pub scroll: f32,
    pub hovered: bool,
}

impl ViewportInput {
    pub fn pixel(&self, pos: egui::Pos2, rect: egui::Rect, ppp: f32) -> Vec2 {
        Vec2::new((pos.x - rect.left()) * ppp, (pos.y - rect.top()) * ppp)
    }
}

pub struct ViewportPanel {
    pub camera: OrbitCamera,
    pub options: ViewOptions,
    /// When false the viewport does not orbit/pan/zoom (a modal owns the pointer).
    pub camera_enabled: bool,
    gpu: Option<(GpuContext, Viewport)>,
    texture: Option<(egui::TextureId, u64)>,
    /// Last panel rect and pixel ratio, for pixel <-> point conversions by tools.
    pub last_rect: egui::Rect,
    pub last_ppp: f32,
}

impl ViewportPanel {
    pub fn new(gpu: Option<GpuContext>) -> Self {
        let gpu = gpu.map(|g| {
            let vp = Viewport::new(g.device.clone(), g.queue.clone(), g.format);
            (g, vp)
        });
        Self {
            camera: OrbitCamera::default(),
            options: ViewOptions::default(),
            camera_enabled: true,
            gpu,
            texture: None,
            last_rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0)),
            last_ppp: 1.0,
        }
    }

    pub fn has_gpu(&self) -> bool {
        self.gpu.is_some()
    }

    /// Viewport size in pixels as of the last frame.
    pub fn size_px(&self) -> Vec2 {
        Vec2::new(self.last_rect.width() * self.last_ppp, self.last_rect.height() * self.last_ppp).max(Vec2::ONE)
    }

    pub fn frame_scene(&mut self, scene: &Scene) {
        let b = scene.bounds();
        if b.is_empty() {
            self.camera = OrbitCamera { yaw: self.camera.yaw, pitch: self.camera.pitch, ..OrbitCamera::default() };
        } else {
            self.camera.frame(b.min, b.max);
        }
    }

    /// Draw the panel into `ui`'s full rect, render the scene, and report pointer input.
    /// `overlay` is drawn by the caller on top (status text, modal hints).
    pub fn ui(&mut self, ui: &mut egui::Ui, scene: &Scene, overlay: &str) -> ViewportInput {
        let rect = ui.max_rect();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        let ppp = ui.ctx().pixels_per_point();
        self.last_rect = rect;
        self.last_ppp = ppp;
        let size_px = Vec2::new(rect.width() * ppp, rect.height() * ppp);

        let mods = ui.input(|i| i.modifiers);
        let mut input =
            ViewportInput { size: size_px, shift: mods.shift, ctrl: mods.command, alt: mods.alt, hovered: response.hovered(), ..Default::default() };
        let (pointer_pos, primary_down, primary_pressed, primary_released) = ui.input(|i| {
            (
                i.pointer.latest_pos(),
                i.pointer.primary_down(),
                i.pointer.button_pressed(egui::PointerButton::Primary),
                i.pointer.button_released(egui::PointerButton::Primary),
            )
        });
        if let Some(pos) = pointer_pos
            && (rect.contains(pos) || response.dragged() || primary_released)
        {
            input.pointer = Some(input.pixel(pos, rect, ppp));
        }
        if response.hovered() || response.dragged() || response.clicked() {
            input.primary_clicked = response.clicked_by(egui::PointerButton::Primary);
            input.secondary_clicked = response.clicked_by(egui::PointerButton::Secondary);
            input.primary_pressed = primary_pressed && response.hovered();
            input.primary_down = primary_down;
            input.primary_released = primary_released;
        }

        // ---- camera ----
        if self.camera_enabled {
            if response.dragged_by(egui::PointerButton::Middle) || (response.dragged_by(egui::PointerButton::Secondary) && !mods.shift) {
                let d = response.drag_delta();
                self.camera.orbit(Vec2::new(d.x, d.y));
            }
            if (response.dragged_by(egui::PointerButton::Secondary) || response.dragged_by(egui::PointerButton::Middle)) && mods.shift {
                let d = response.drag_delta();
                self.camera.pan(Vec2::new(d.x, d.y), rect.height());
            }
            if response.hovered() {
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if scroll != 0.0 {
                    self.camera.zoom(scroll / 50.0);
                }
            }
        } else if response.hovered() {
            // Whole notches only, so a bevel's segment count moves by one per wheel click.
            input.scroll = ui.input(|i| {
                i.events
                    .iter()
                    .filter_map(|e| match e {
                        egui::Event::MouseWheel { delta, .. } => Some(delta.y.signum()),
                        _ => None,
                    })
                    .sum()
            });
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
                ui.painter().rect_filled(rect, 0.0, crate::theme::VIEWPORT_BG);
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
        let label = self.options.shading.name();
        let mode = if scene.edit.is_some() { "Edit Mode" } else { "Object Mode" };
        ui.painter().text(
            rect.left_top() + egui::vec2(8.0, 6.0),
            egui::Align2::LEFT_TOP,
            format!("{mode}  ·  {label}  ·  {} objects", scene.object_count()),
            egui::FontId::proportional(12.0),
            Color32::from_rgb(0xaa, 0xb0, 0xbb),
        );
        if !overlay.is_empty() {
            ui.painter().text(
                rect.left_bottom() + egui::vec2(8.0, -6.0),
                egui::Align2::LEFT_BOTTOM,
                overlay,
                egui::FontId::proportional(13.0),
                Color32::from_rgb(0xf0, 0xb0, 0x50),
            );
        }
        input
    }
}
