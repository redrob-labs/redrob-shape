//! Curve Edit Mode, after Blender's: Tab on a curve edits its control points. Click picks a point
//! (Shift extends), G moves the selection with its handles, E extrudes a new point from the end of
//! the spline towards the pointer, X deletes, V sets the handle type (Automatic, Vector, Aligned,
//! Free), A selects all. Auto / Vector handles are recomputed after every edit, as Blender does.

use shape_math::Vec2;
use shape_scene::{Curve, HandleType, ObjectData, ObjectId, Scene};

use crate::ShapeApp;
use crate::viewport::ViewportInput;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveDragKind {
    Move,
    Extrude,
}

#[derive(Clone, Debug)]
pub struct CurveDrag {
    pub kind: CurveDragKind,
    before: Scene,
    start_px: Vec2,
}

#[derive(Clone, Debug)]
pub struct CurveEditMode {
    pub object: ObjectId,
    pub drag: Option<CurveDrag>,
}

impl ShapeApp {
    pub fn active_is_curve(&self) -> bool {
        self.scene.selection.active.and_then(|id| self.scene.object(id)).is_some_and(|o| matches!(o.data, ObjectData::Curve(_)))
    }

    fn curve_mut(&mut self, id: ObjectId) -> Option<&mut Curve> {
        match self.scene.object_mut(id).map(|o| &mut o.data) {
            Some(ObjectData::Curve(c)) => Some(c),
            _ => None,
        }
    }

    pub fn toggle_curve_edit_mode(&mut self) {
        if self.curve_edit.take().is_some() {
            self.status = "Object Mode".into();
            return;
        }
        if let Some(id) = self.scene.selection.active
            && self.active_is_curve()
        {
            self.pose = None;
            self.sculpt = None;
            self.scene.edit = None;
            self.curve_edit = Some(CurveEditMode { object: id, drag: None });
            self.status = "Edit Mode (Curve)".into();
        }
    }

    /// Shift+A > Curve entries.
    pub fn add_curve(&mut self, curve: Curve, name: &str) -> ObjectId {
        let at = self.scene.cursor;
        self.add_object(name, ObjectData::Curve(curve), at)
    }

    /// Object > Convert > Mesh: the curve's tube (or its polyline as a flat ribbon of zero
    /// depth is not a mesh, so a bevel depth is required) becomes mesh data.
    pub fn convert_to_mesh(&mut self) {
        let Some(id) = self.scene.selection.active else { return };
        let Some(ObjectData::Curve(c)) = self.scene.object(id).map(|o| o.data.clone()) else {
            self.status = "Convert to Mesh needs a curve object".into();
            return;
        };
        if c.bevel_depth <= 0.0 {
            self.status = "Set a Bevel Depth first: a zero-depth curve has no faces".into();
            return;
        }
        let mesh = c.to_mesh();
        let name = self.scene.object(id).map(|o| o.name.clone()).unwrap_or_default();
        self.curve_edit = None;
        self.edit("Convert to Mesh", |scene| {
            let mid = scene.add_mesh(shape_scene::MeshData::new(name, mesh));
            if let Some(o) = scene.object_mut(id) {
                o.data = ObjectData::Mesh { mesh: mid, material: None, modifiers: Vec::new() };
            }
        });
    }

    pub(crate) fn curve_edit_keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() || self.popup.is_some() {
            return;
        }
        let Some(pointer) = self.last_pointer else { return };
        let [g, e, x, a, v, tab] = ui.input_mut(|i| {
            [
                crate::key(i, egui::Modifiers::NONE, egui::Key::G),
                crate::key(i, egui::Modifiers::NONE, egui::Key::E),
                crate::key(i, egui::Modifiers::NONE, egui::Key::X) || crate::key(i, egui::Modifiers::NONE, egui::Key::Delete),
                crate::key(i, egui::Modifiers::NONE, egui::Key::A),
                crate::key(i, egui::Modifiers::NONE, egui::Key::V),
                crate::key(i, egui::Modifiers::NONE, egui::Key::Tab),
            ]
        });
        let Some(edit) = self.curve_edit.clone() else { return };
        if tab {
            self.toggle_curve_edit_mode();
            return;
        }
        if edit.drag.is_some() {
            return;
        }
        let id = edit.object;
        let selected = match self.scene.object(id).map(|o| &o.data) {
            Some(ObjectData::Curve(c)) => c.selected_points(),
            _ => Vec::new(),
        };
        if g && !selected.is_empty() {
            if let Some(ce) = self.curve_edit.as_mut() {
                ce.drag = Some(CurveDrag { kind: CurveDragKind::Move, before: self.scene.clone(), start_px: pointer });
            }
            self.status = "Move points: LMB/Enter confirm, Esc cancel".into();
        }
        if e && let Some(&(si, pi)) = selected.last() {
            let before = self.scene.clone();
            if let Some(c) = self.curve_mut(id) {
                let spline = &mut c.splines[si];
                let n = spline.points.len();
                // Extrude from an end point: the new point continues the spline.
                let at_end = pi + 1 == n;
                let src = spline.points[pi];
                let mut new = src;
                new.selected = true;
                for p in &mut spline.points {
                    p.selected = false;
                }
                if at_end || n == 1 {
                    spline.points.push(new);
                } else if pi == 0 {
                    spline.points.insert(0, new);
                } else {
                    spline.points.insert(pi + 1, new);
                }
                c.recalc_handles();
            }
            if let Some(ce) = self.curve_edit.as_mut() {
                ce.drag = Some(CurveDrag { kind: CurveDragKind::Extrude, before, start_px: pointer });
            }
            self.status = "Extrude point: move the pointer, LMB/Enter confirm, Esc cancel".into();
        }
        if x && !selected.is_empty() {
            self.edit("Delete Points", |scene| {
                if let Some(ObjectData::Curve(c)) = scene.object_mut(id).map(|o| &mut o.data) {
                    for s in &mut c.splines {
                        s.points.retain(|p| !p.selected);
                    }
                    c.splines.retain(|s| !s.points.is_empty());
                    c.recalc_handles();
                }
            });
        }
        if v && !selected.is_empty() {
            self.popup = Some(crate::Popup::HandleType);
        }
        if a && let Some(c) = self.curve_mut(id) {
            let all = c.splines.iter().all(|s| s.points.iter().all(|p| p.selected));
            for s in &mut c.splines {
                for p in &mut s.points {
                    p.selected = !all;
                }
            }
        }
    }

    /// V menu: set the handle type of the selected points.
    pub fn set_handle_type(&mut self, ty: HandleType) {
        let Some(edit) = self.curve_edit.clone() else { return };
        let id = edit.object;
        self.edit(&format!("Set Handle Type {}", ty.name()), |scene| {
            if let Some(ObjectData::Curve(c)) = scene.object_mut(id).map(|o| &mut o.data) {
                for s in &mut c.splines {
                    for p in &mut s.points {
                        if p.selected {
                            p.handle_type = ty;
                        }
                    }
                }
                c.recalc_handles();
            }
        });
    }

    pub(crate) fn curve_edit_input(&mut self, input: &ViewportInput, enter: bool, escape: bool) -> bool {
        let Some(edit) = self.curve_edit.clone() else { return false };
        let id = edit.object;
        let size = input.size.max(Vec2::ONE);
        if let Some(drag) = edit.drag {
            if escape {
                self.scene = drag.before;
                if let Some(ce) = self.curve_edit.as_mut() {
                    ce.drag = None;
                }
                self.status = "Cancelled".into();
                return true;
            }
            if input.primary_clicked || enter {
                let name = match drag.kind {
                    CurveDragKind::Move => "Move Points",
                    CurveDragKind::Extrude => "Extrude Point",
                };
                self.record_from(name, drag.before);
                if let Some(ce) = self.curve_edit.as_mut() {
                    ce.drag = None;
                }
                self.status = format!("{name} confirmed");
                return true;
            }
            let Some(p) = input.pointer else { return true };
            let delta = p - drag.start_px;
            let camera = self.viewport.camera.clone();
            let world = self.scene.world_transform(id);
            let per_px = 2.0 * camera.distance * (camera.fov_y / 2.0).tan() / size.y;
            let move_world = (camera.right() * delta.x - camera.up() * delta.y) * per_px;
            let move_local = world.inverse().transform_vector(move_world);
            // Positions come from the snapshot so the drag is absolute, not accumulated.
            let Some(ObjectData::Curve(before)) = drag.before.object(id).map(|o| &o.data) else { return true };
            let before = before.clone();
            if let Some(c) = self.curve_mut(id) {
                for (si, s) in c.splines.iter_mut().enumerate() {
                    for (pi, pt) in s.points.iter_mut().enumerate() {
                        if !pt.selected {
                            continue;
                        }
                        // An extruded point is not in the snapshot: it starts where it is now.
                        let was = before.splines.get(si).and_then(|s| s.points.get(pi)).copied().unwrap_or(*pt);
                        pt.co = was.co + move_local;
                        pt.handle_left = was.handle_left + move_local;
                        pt.handle_right = was.handle_right + move_local;
                    }
                }
                c.recalc_handles();
            }
            return true;
        }
        if input.primary_clicked
            && let Some(p) = input.pointer
        {
            let camera = self.viewport.camera.clone();
            let world = self.scene.world_transform(id);
            let Some(ObjectData::Curve(c)) = self.scene.object(id).map(|o| &o.data) else { return true };
            let mut best: Option<((usize, usize), f32)> = None;
            for (si, s) in c.splines.iter().enumerate() {
                for (pi, pt) in s.points.iter().enumerate() {
                    if let Some(q) = camera.project(world.transform_point(pt.co), size) {
                        let d = q.distance(p);
                        if d < 14.0 && best.is_none_or(|(_, bd)| d < bd) {
                            best = Some(((si, pi), d));
                        }
                    }
                }
            }
            let shift = input.shift;
            if let Some(c) = self.curve_mut(id) {
                match best {
                    Some(((si, pi), _)) => {
                        if !shift {
                            for s in &mut c.splines {
                                for p in &mut s.points {
                                    p.selected = false;
                                }
                            }
                        }
                        c.splines[si].points[pi].selected = true;
                        self.status = format!("Point {pi}");
                    }
                    None if !shift => {
                        for s in &mut c.splines {
                            for p in &mut s.points {
                                p.selected = false;
                            }
                        }
                        self.status = "No point under the pointer".into();
                    }
                    None => {}
                }
            }
            return true;
        }
        true
    }

    /// Data tab for curves: resolution and bevel, as in Blender's Geometry > Bevel panel.
    pub(crate) fn curve_data_panel(&mut self, ui: &mut egui::Ui, id: ObjectId) {
        let Some(ObjectData::Curve(original)) = self.scene.object(id).map(|o| o.data.clone()) else { return };
        let mut c = original.clone();
        egui::CollapsingHeader::new("Shape").default_open(true).show(ui, |ui| {
            ui.add(egui::DragValue::new(&mut c.resolution).range(1..=64).prefix("Resolution Preview U "));
            if let Some(s) = c.splines.first_mut() {
                ui.checkbox(&mut s.cyclic, "Cyclic U");
            }
        });
        egui::CollapsingHeader::new("Bevel").default_open(true).show(ui, |ui| {
            ui.add(egui::DragValue::new(&mut c.bevel_depth).speed(0.005).range(0.0..=100.0).prefix("Depth "));
            ui.add(egui::DragValue::new(&mut c.bevel_resolution).range(0..=32).prefix("Resolution "));
        });
        if ui.button("Convert to Mesh").clicked() {
            self.convert_to_mesh();
            return;
        }
        if c != original {
            c.recalc_handles();
            if self.property_before.is_none() {
                self.property_before = Some(self.scene.clone());
            }
            if let Some(o) = self.scene.object_mut(id) {
                o.data = ObjectData::Curve(c);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_math::Vec3;

    fn input(p: Vec2, clicked: bool) -> ViewportInput {
        ViewportInput { pointer: Some(p), size: Vec2::new(400.0, 300.0), primary_clicked: clicked, hovered: true, ..Default::default() }
    }

    fn curve(app: &ShapeApp, id: ObjectId) -> &Curve {
        match &app.scene.object(id).unwrap().data {
            ObjectData::Curve(c) => c,
            _ => panic!("not a curve"),
        }
    }

    #[test]
    fn pick_move_extrude_and_convert() {
        let mut app = ShapeApp::default();
        let id = app.add_curve(Curve::bezier(), "BezierCurve");
        app.scene.selection.set(id);
        app.viewport.camera = shape_gpu::OrbitCamera { yaw: 0.0, pitch: 1.5, distance: 6.0, ..Default::default() };
        app.toggle_curve_edit_mode();
        assert!(app.curve_edit.is_some());
        let size = Vec2::new(400.0, 300.0);
        let right = app.viewport.camera.project(Vec3::new(1.0, 0.0, 0.0), size).unwrap();
        app.curve_edit_input(&input(right, true), false, false);
        assert_eq!(curve(&app, id).selected_points(), vec![(0, 1)]);
        // Move it.
        let undo = app.history.len();
        app.last_pointer = Some(right);
        app.curve_edit.as_mut().unwrap().drag = Some(CurveDrag { kind: CurveDragKind::Move, before: app.scene.clone(), start_px: right });
        app.curve_edit_input(&input(right + Vec2::new(40.0, 0.0), false), false, false);
        app.curve_edit_input(&input(right + Vec2::new(40.0, 0.0), true), false, false);
        assert!(curve(&app, id).splines[0].points[1].co.x > 1.2);
        assert_eq!(app.history.len(), undo + 1);
        // Extrude from the end: a third point appears and follows the pointer.
        {
            let before = app.scene.clone();
            let c = match &mut app.scene.object_mut(id).unwrap().data {
                ObjectData::Curve(c) => c,
                _ => unreachable!(),
            };
            let mut new = c.splines[0].points[1];
            new.selected = true;
            c.splines[0].points[1].selected = false;
            c.splines[0].points.push(new);
            app.curve_edit.as_mut().unwrap().drag = Some(CurveDrag { kind: CurveDragKind::Extrude, before, start_px: right });
        }
        app.curve_edit_input(&input(right + Vec2::new(0.0, -50.0), false), false, false);
        app.curve_edit_input(&input(right + Vec2::new(0.0, -50.0), true), false, false);
        let c = curve(&app, id);
        assert_eq!(c.splines[0].points.len(), 3);
        assert!(c.splines[0].points[2].co.distance(c.splines[0].points[1].co) > 0.3);
        // Handle type and convert.
        app.set_handle_type(HandleType::Vector);
        assert_eq!(curve(&app, id).splines[0].points[2].handle_type, HandleType::Vector);
        app.convert_to_mesh();
        assert!(app.active_is_curve(), "zero depth refuses to convert");
        if let ObjectData::Curve(c) = &mut app.scene.object_mut(id).unwrap().data {
            c.bevel_depth = 0.1;
        }
        app.convert_to_mesh();
        assert!(app.scene.object_mesh(id).is_some(), "converted to a mesh");
        assert!(app.scene.object_mesh(id).unwrap().1.face_count() > 20);
    }
}
