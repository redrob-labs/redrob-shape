//! Sculpt Mode, after Blender's: the active mesh is painted with brushes; a stroke (LMB drag) is
//! one undo step; the brush is a circle of `radius_px` screen pixels at the pointer; F / Shift+F
//! adjust radius / strength interactively; Shift smooths and Ctrl inverts while held; X / Y / Z
//! symmetry mirrors each dab in the mesh's own axes.

use egui::{Color32, Stroke};
use shape_gpu::pick_detail;
use shape_math::{Vec2, Vec3};
use shape_ops::sculpt::{self, Brush, BrushSettings};
use shape_scene::{ObjectId, Scene};

use crate::ShapeApp;
use crate::viewport::ViewportInput;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Adjust {
    Radius,
    Strength,
}

#[derive(Clone, Debug)]
pub struct StrokeState {
    before: Scene,
    last_px: Vec2,
    /// Grab: the previous pointer position unprojected at the stroke's depth, in mesh space.
    last_local: Option<Vec3>,
    /// Grab: ray depth of the first hit, so the pointer keeps mapping onto the same plane.
    depth: f32,
    pub dabs: usize,
}

#[derive(Clone, Debug)]
pub struct SculptMode {
    pub object: ObjectId,
    pub brush: Brush,
    /// Blender's default brush radius: 50 px.
    pub radius_px: f32,
    pub strength: f32,
    pub symmetry: [bool; 3],
    pub stroke: Option<StrokeState>,
    /// F / Shift+F in progress: what is being adjusted, where it began and the value then.
    pub adjust: Option<(Adjust, Vec2, f32)>,
}

impl SculptMode {
    pub fn new(object: ObjectId) -> Self {
        Self { object, brush: Brush::Draw, radius_px: 50.0, strength: 0.5, symmetry: [false; 3], stroke: None, adjust: None }
    }
}

impl ShapeApp {
    /// Enter / leave Sculpt Mode on the active mesh (Ctrl+Tab > Sculpt Mode, header dropdown).
    pub fn toggle_sculpt_mode(&mut self) {
        if self.sculpt.is_some() {
            self.sculpt = None;
            self.status = "Object Mode".into();
            return;
        }
        if let Some(id) = self.scene.selection.active
            && self.scene.object_mesh(id).is_some()
        {
            self.scene.edit = None;
            self.pose = None;
            if self.weight_paint.is_some() {
                self.toggle_weight_paint();
            }
            self.sculpt = Some(SculptMode::new(id));
            self.status = "Sculpt Mode".into();
        }
    }

    /// Sculpt-mode keys: brushes, F / Shift+F, symmetry. Runs instead of the object-mode keys.
    pub(crate) fn sculpt_keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let Some(pointer) = self.last_pointer else { return };
        let keys = ui.input_mut(|i| {
            [
                crate::key(i, egui::Modifiers::NONE, egui::Key::X),
                crate::key(i, egui::Modifiers::NONE, egui::Key::C),
                crate::key(i, egui::Modifiers::NONE, egui::Key::S),
                crate::key(i, egui::Modifiers::NONE, egui::Key::G),
                crate::key(i, egui::Modifiers::NONE, egui::Key::I),
                crate::key(i, egui::Modifiers::SHIFT, egui::Key::T),
                crate::key(i, egui::Modifiers::NONE, egui::Key::F),
                crate::key(i, egui::Modifiers::SHIFT, egui::Key::F),
                crate::key(i, egui::Modifiers::SHIFT, egui::Key::X),
                crate::key(i, egui::Modifiers::SHIFT, egui::Key::Y),
                crate::key(i, egui::Modifiers::SHIFT, egui::Key::Z),
            ]
        });
        let Some(s) = self.sculpt.as_mut() else { return };
        let [draw, clay, smooth, grab, inflate, flatten, radius, strength, sx, sy, sz] = keys;
        for (on, brush) in [
            (draw, Brush::Draw),
            (clay, Brush::Clay),
            (smooth, Brush::Smooth),
            (grab, Brush::Grab),
            (inflate, Brush::Inflate),
            (flatten, Brush::Flatten),
        ] {
            if on {
                s.brush = brush;
                self.status = format!("Brush: {}", brush.name());
            }
        }
        if radius && s.adjust.is_none() {
            s.adjust = Some((Adjust::Radius, pointer, s.radius_px));
        }
        if strength && s.adjust.is_none() {
            s.adjust = Some((Adjust::Strength, pointer, s.strength));
        }
        for (on, axis) in [(sx, 0), (sy, 1), (sz, 2)] {
            if on {
                s.symmetry[axis] = !s.symmetry[axis];
            }
        }
    }

    /// Pointer handling in Sculpt Mode. Returns true when sculpting owns the pointer.
    pub(crate) fn sculpt_input(&mut self, input: &ViewportInput, enter: bool, escape: bool) -> bool {
        let Some(s) = self.sculpt.as_mut() else { return false };
        let size = input.size.max(Vec2::ONE);
        // F / Shift+F: the value follows the pointer's distance from where the key was pressed.
        if let Some((what, origin, start)) = s.adjust {
            if let Some(p) = input.pointer {
                let d = (p - origin).length();
                match what {
                    // The circle is drawn at `origin`; its radius is the pointer distance.
                    Adjust::Radius => s.radius_px = d.clamp(2.0, 1000.0),
                    Adjust::Strength => s.strength = (d / 200.0).clamp(0.0, 1.0),
                }
            }
            if input.primary_clicked || enter {
                s.adjust = None;
                self.status = format!("Radius {:.0} px · Strength {:.2}", s.radius_px, s.strength);
            } else if escape || input.secondary_clicked {
                match what {
                    Adjust::Radius => s.radius_px = start,
                    Adjust::Strength => s.strength = start,
                }
                s.adjust = None;
            }
            return true;
        }
        if input.primary_pressed && input.pointer.is_some() && s.stroke.is_none() {
            s.stroke =
                Some(StrokeState { before: self.scene.clone(), last_px: Vec2::new(f32::MAX, f32::MAX), last_local: None, depth: 0.0, dabs: 0 });
        }
        if input.primary_released
            && let Some(stroke) = s.stroke.take()
        {
            if stroke.dabs > 0 {
                let brush = s.brush;
                self.record_from(&format!("Sculpt {}", brush.name()), stroke.before);
            }
            return true;
        }
        if !input.primary_down || s.stroke.is_none() {
            return true;
        }
        let Some(p) = input.pointer else { return true };
        let object = s.object;
        let brush = if input.shift { Brush::Smooth } else { s.brush };
        let spacing = (s.radius_px * 0.1).max(1.0);
        let stroke = s.stroke.as_mut().expect("stroke present");
        if stroke.dabs > 0 && (p - stroke.last_px).length() < spacing {
            return true;
        }
        let camera = self.viewport.camera.clone();
        let world = self.scene.world_transform(object);
        let inv = world.inverse();
        let scale = (world.scale.x + world.scale.y + world.scale.z) / 3.0;
        let hit = pick_detail(&self.scene, &camera, p, size).filter(|h| h.object == object);
        // World units per pixel at the hit depth, so the circle on screen is the brush.
        let world_per_px = |t: f32| 2.0 * t * (camera.fov_y / 2.0).tan() / size.y;
        let (center, delta, depth) = match brush {
            Brush::Grab => {
                // Grab follows the pointer on the plane of the first hit.
                let depth = if stroke.dabs == 0 { hit.as_ref().map(|h| h.t) } else { Some(stroke.depth) };
                let Some(depth) = depth else { return true };
                let ray = camera.ray_pixel(p, size);
                let local = inv.transform_point(ray.origin + ray.direction * depth);
                let delta = stroke.last_local.map(|l| local - l).unwrap_or(Vec3::ZERO);
                let center = stroke.last_local.unwrap_or(local);
                stroke.last_local = Some(local);
                (center, delta, depth)
            }
            _ => {
                let Some(h) = hit else {
                    stroke.last_px = p;
                    return true;
                };
                (h.local_point, Vec3::ZERO, h.t)
            }
        };
        let settings = BrushSettings {
            brush,
            radius: s.radius_px * world_per_px(depth) / scale.max(1e-6),
            strength: s.strength,
            invert: input.ctrl,
            symmetry: s.symmetry,
        };
        stroke.last_px = p;
        stroke.depth = depth;
        let Some((mesh_id, _)) = self.scene.object_mesh(object) else { return true };
        if let Some(data) = self.scene.mesh_mut(mesh_id) {
            let moved = sculpt::dab(&mut data.mesh, center, delta, &settings);
            if moved > 0 {
                stroke.dabs += 1;
            }
        }
        true
    }

    /// The brush circle (Blender's red ring) and the F / Shift+F preview.
    pub(crate) fn sculpt_overlay(&self, ui: &egui::Ui) {
        let Some(s) = &self.sculpt else { return };
        let rect = self.viewport.last_rect;
        let ppp = self.viewport.last_ppp.max(0.01);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("sculpt-cursor")));
        let to_screen = |p: Vec2| egui::pos2(rect.left() + p.x / ppp, rect.top() + p.y / ppp);
        let ring = Color32::from_rgb(0xff, 0x64, 0x64);
        match (s.adjust, self.last_pointer) {
            (Some((Adjust::Radius, origin, _)), _) => {
                painter.circle_stroke(to_screen(origin), s.radius_px / ppp, Stroke::new(1.5, ring));
                painter.text(
                    to_screen(origin) + egui::vec2(0.0, -s.radius_px / ppp - 8.0),
                    egui::Align2::CENTER_BOTTOM,
                    format!("Radius {:.0} px", s.radius_px),
                    egui::FontId::proportional(12.0),
                    Color32::WHITE,
                );
            }
            (Some((Adjust::Strength, origin, _)), _) => {
                painter.circle_stroke(to_screen(origin), s.radius_px / ppp, Stroke::new(1.0, ring));
                painter.circle_filled(to_screen(origin), s.radius_px / ppp * s.strength, Color32::from_rgba_unmultiplied(0xff, 0x64, 0x64, 60));
                painter.text(
                    to_screen(origin) + egui::vec2(0.0, -s.radius_px / ppp - 8.0),
                    egui::Align2::CENTER_BOTTOM,
                    format!("Strength {:.2}", s.strength),
                    egui::FontId::proportional(12.0),
                    Color32::WHITE,
                );
            }
            (None, Some(p)) if rect.contains(to_screen(p)) => {
                painter.circle_stroke(to_screen(p), s.radius_px / ppp, Stroke::new(1.0, ring));
            }
            _ => {}
        }
    }

    /// Header / sidebar widgets for the active brush (Blender's tool settings).
    pub(crate) fn sculpt_tool_settings(&mut self, ui: &mut egui::Ui) {
        let Some(s) = self.sculpt.as_mut() else { return };
        egui::ComboBox::from_id_salt("brush").selected_text(s.brush.name()).width(90.0).show_ui(ui, |ui| {
            for b in Brush::ALL {
                ui.selectable_value(&mut s.brush, b, format!("{} ({})", b.name(), b.key()));
            }
        });
        ui.add(egui::DragValue::new(&mut s.radius_px).range(2.0..=1000.0).prefix("Radius ").suffix(" px"));
        ui.add(egui::DragValue::new(&mut s.strength).range(0.0..=1.0).speed(0.01).prefix("Strength "));
        ui.label(egui::RichText::new("Symmetry").color(crate::theme::TEXT_WEAK));
        for (i, name) in ["X", "Y", "Z"].iter().enumerate() {
            ui.toggle_value(&mut s.symmetry[i], *name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Primitive;

    fn input(p: Vec2, pressed: bool, down: bool, released: bool) -> ViewportInput {
        ViewportInput {
            pointer: Some(p),
            size: Vec2::new(400.0, 300.0),
            primary_pressed: pressed,
            primary_down: down,
            primary_released: released,
            hovered: true,
            ..Default::default()
        }
    }

    #[test]
    fn a_stroke_raises_the_mesh_and_is_one_undo_step() {
        let mut app = ShapeApp::default();
        let id = app.add_primitive(Primitive::Plane);
        // A denser plane so the brush has vertices to move.
        if let Some((mid, _)) = app.scene.object_mesh(id)
            && let Some(d) = app.scene.mesh_mut(mid)
        {
            d.mesh = shape_geom::primitives::plane(2.0, 20, 20);
        }
        app.scene.selection.set(id);
        app.viewport.camera = shape_gpu::OrbitCamera { yaw: 0.0, pitch: 1.2, distance: 4.0, ..Default::default() };
        app.toggle_sculpt_mode();
        assert!(app.sculpt.is_some());
        let undo_before = app.history.len();
        let centre = Vec2::new(200.0, 150.0);
        assert!(app.sculpt_input(&input(centre, true, true, false), false, false));
        for i in 1..6 {
            app.sculpt_input(&input(centre + Vec2::new(i as f32 * 8.0, 0.0), false, true, false), false, false);
        }
        app.sculpt_input(&input(centre + Vec2::new(48.0, 0.0), false, false, true), false, false);
        let (_, mesh) = app.scene.object_mesh(id).unwrap();
        let top = mesh.positions().map(|p| p.y).fold(f32::MIN, f32::max);
        assert!(top > 0.01, "Draw raised the plane: {top}");
        assert_eq!(app.history.len(), undo_before + 1, "one undo step per stroke");
        // F adjusts the radius from the pointer distance; Escape restores.
        app.last_pointer = Some(centre);
        app.sculpt.as_mut().unwrap().adjust = Some((Adjust::Radius, centre, 50.0));
        app.sculpt_input(&input(centre + Vec2::new(80.0, 0.0), false, false, false), false, false);
        assert!((app.sculpt.as_ref().unwrap().radius_px - 80.0).abs() < 0.01);
        app.sculpt_input(&input(centre, false, false, false), false, true);
        assert_eq!(app.sculpt.as_ref().unwrap().radius_px, 50.0);
        assert!(app.sculpt.as_ref().unwrap().adjust.is_none());
    }
}
