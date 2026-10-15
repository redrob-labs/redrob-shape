//! Weight Paint mode, after Blender's: a mesh with vertex groups shows the active group's weights
//! as the blue-to-red ramp; LMB strokes add weight under the brush (Ctrl subtracts) with the same
//! radius / strength / falloff as sculpting; the active group is chosen in the header. One stroke
//! is one undo step.

use shape_gpu::pick_detail;
use shape_math::Vec2;
use shape_ops::sculpt::falloff;
use shape_scene::{ObjectId, Scene};

use crate::ShapeApp;
use crate::viewport::ViewportInput;

#[derive(Clone, Debug)]
pub struct WeightPaint {
    pub object: ObjectId,
    pub group: String,
    pub radius_px: f32,
    pub strength: f32,
    stroke: Option<(Scene, usize)>,
}

impl ShapeApp {
    /// A mesh with vertex groups can be weight painted.
    pub fn active_can_weight_paint(&self) -> bool {
        self.scene
            .selection
            .active
            .and_then(|id| self.scene.object_mesh(id))
            .and_then(|(mid, _)| self.scene.mesh(mid))
            .is_some_and(|d| !d.vertex_groups.is_empty())
    }

    pub fn toggle_weight_paint(&mut self) {
        if self.weight_paint.take().is_some() {
            self.viewport.options.weight_paint = None;
            self.status = "Object Mode".into();
            return;
        }
        let Some(id) = self.scene.selection.active else { return };
        let Some((mid, _)) = self.scene.object_mesh(id) else { return };
        let Some(group) = self.scene.mesh(mid).and_then(|d| d.vertex_groups.first()).map(|g| g.name.clone()) else {
            self.status = "Weight Paint needs vertex groups (parent to an armature first)".into();
            return;
        };
        self.scene.edit = None;
        self.sculpt = None;
        self.pose = None;
        self.bone_edit = None;
        self.viewport.options.weight_paint = Some((id, group.clone()));
        self.weight_paint = Some(WeightPaint { object: id, group, radius_px: 50.0, strength: 0.5, stroke: None });
        self.status = "Weight Paint".into();
    }

    pub(crate) fn weight_paint_header(&mut self, ui: &mut egui::Ui) {
        let Some(wp) = self.weight_paint.as_mut() else { return };
        let Some((mid, _)) = self.scene.object_mesh(wp.object) else { return };
        let groups: Vec<String> = self.scene.mesh(mid).map(|d| d.vertex_groups.iter().map(|g| g.name.clone()).collect()).unwrap_or_default();
        egui::ComboBox::from_id_salt("weight-group").selected_text(wp.group.clone()).width(110.0).show_ui(ui, |ui| {
            for g in groups {
                if ui.selectable_label(wp.group == g, &g).clicked() {
                    wp.group = g;
                }
            }
        });
        ui.add(egui::DragValue::new(&mut wp.radius_px).range(2.0..=1000.0).prefix("Radius ").suffix(" px"));
        ui.add(egui::DragValue::new(&mut wp.strength).range(0.0..=1.0).speed(0.01).prefix("Weight "));
        self.viewport.options.weight_paint = Some((wp.object, wp.group.clone()));
    }

    /// Paint strokes. Returns true while Weight Paint owns the pointer.
    pub(crate) fn weight_paint_input(&mut self, input: &ViewportInput) -> bool {
        let Some(wp) = self.weight_paint.as_mut() else { return false };
        let size = input.size.max(Vec2::ONE);
        if input.primary_pressed && input.pointer.is_some() && wp.stroke.is_none() {
            wp.stroke = Some((self.scene.clone(), 0));
        }
        if input.primary_released
            && let Some((before, dabs)) = wp.stroke.take()
        {
            if dabs > 0 {
                self.record_from("Weight Paint", before);
            }
            return true;
        }
        if !input.primary_down || wp.stroke.is_none() {
            return true;
        }
        let Some(p) = input.pointer else { return true };
        let (object, group, radius_px, strength) = (wp.object, wp.group.clone(), wp.radius_px, wp.strength);
        let camera = self.viewport.camera.clone();
        let Some(hit) = pick_detail(&self.scene, &camera, p, size).filter(|h| h.object == object) else { return true };
        let world = self.scene.world_transform(object);
        let scale = (world.scale.x + world.scale.y + world.scale.z) / 3.0;
        let radius = radius_px * 2.0 * hit.t * (camera.fov_y / 2.0).tan() / size.y / scale.max(1e-6);
        let sign = if input.ctrl { -1.0 } else { 1.0 };
        // Paint on the deformed (displayed) mesh; the weights index the base vertices, which the
        // Armature modifier keeps in order.
        let shown: Vec<shape_math::Vec3> = shape_ops::evaluated_mesh(&self.scene, object).map(|m| m.positions().collect()).unwrap_or_default();
        let Some((mid, _)) = self.scene.object_mesh(object) else { return true };
        let Some(data) = self.scene.mesh_mut(mid) else { return true };
        let positions: Vec<shape_math::Vec3> = if shown.len() == data.mesh.vertex_count() { shown } else { data.mesh.positions().collect() };
        let Some(g) = data.vertex_groups.iter_mut().find(|g| g.name == group) else { return true };
        let mut touched = 0;
        for (i, pos) in positions.iter().copied().enumerate() {
            let w = falloff(pos.distance(hit.local_point), radius);
            if w <= 0.0 {
                continue;
            }
            touched += 1;
            let delta = sign * strength * w * 0.25;
            match g.weights.iter_mut().find(|(v, _)| *v as usize == i) {
                Some((_, value)) => *value = (*value + delta).clamp(0.0, 1.0),
                None if delta > 0.0 => g.weights.push((i as u32, delta.min(1.0))),
                None => {}
            }
        }
        if touched > 0
            && let Some(wp) = self.weight_paint.as_mut()
            && let Some((_, dabs)) = wp.stroke.as_mut()
        {
            *dabs += 1;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Primitive;
    use shape_scene::armature;

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
    fn a_stroke_adds_weight_to_the_active_group_under_the_brush() {
        let mut app = ShapeApp::default();
        let plane = app.add_primitive(Primitive::Plane);
        let (mid, _) = app.scene.object_mesh(plane).unwrap();
        app.scene.mesh_mut(mid).unwrap().mesh = shape_geom::primitives::plane(2.0, 10, 10);
        app.scene.mesh_mut(mid).unwrap().vertex_groups = vec![armature::VertexGroup { name: "Bone".into(), weights: Vec::new() }];
        app.scene.selection.set(plane);
        app.viewport.camera = shape_gpu::OrbitCamera { yaw: 0.0, pitch: 1.3, distance: 4.0, ..Default::default() };
        app.toggle_weight_paint();
        assert!(app.weight_paint.is_some());
        assert_eq!(app.viewport.options.weight_paint.as_ref().map(|(_, g)| g.as_str()), Some("Bone"));
        let centre = Vec2::new(200.0, 150.0);
        let undo = app.history.len();
        app.weight_paint_input(&input(centre, true, true, false));
        app.weight_paint_input(&input(centre + Vec2::new(5.0, 0.0), false, true, false));
        app.weight_paint_input(&input(centre, false, false, true));
        let g = &app.scene.mesh(mid).unwrap().vertex_groups[0];
        assert!(!g.weights.is_empty(), "weights were painted");
        assert!(g.weights.iter().all(|(_, w)| *w > 0.0 && *w <= 1.0));
        assert_eq!(app.history.len(), undo + 1, "one undo step per stroke");
        // Corners stay unpainted.
        let corner = app.scene.mesh(mid).unwrap().mesh.positions().position(|p| p.x < -0.9 && p.z < -0.9).unwrap();
        assert_eq!(g.weight(corner as u32), 0.0);
        app.toggle_weight_paint();
        assert!(app.weight_paint.is_none() && app.viewport.options.weight_paint.is_none());
    }

    #[test]
    fn the_weight_ramp_runs_blue_to_red() {
        assert_eq!(shape_gpu::weight_color(0.0), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(shape_gpu::weight_color(0.5), [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(shape_gpu::weight_color(1.0), [1.0, 0.0, 0.0, 1.0]);
    }
}
