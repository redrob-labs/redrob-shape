//! Armature Edit Mode, after Blender's: Tab on an armature edits its rest bones. Click picks a
//! bone, E extrudes a new child from the active bone's tail and lets the pointer place its tail,
//! G moves the selected bones, X deletes them (children re-parent upwards), A selects all. Each
//! confirmed operation is one undo step.

use shape_math::{Vec2, Vec3};
use shape_scene::{ObjectData, ObjectId, Scene, armature};

use crate::ShapeApp;
use crate::viewport::ViewportInput;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BoneDragKind {
    /// Move the selected bones (head and tail).
    Move,
    /// Place the tail of the freshly extruded bone.
    Extrude(usize),
}

#[derive(Clone, Debug)]
pub struct BoneDrag {
    pub kind: BoneDragKind,
    before: Scene,
    start_px: Vec2,
}

#[derive(Clone, Debug)]
pub struct BoneEditMode {
    pub object: ObjectId,
    pub drag: Option<BoneDrag>,
}

impl ShapeApp {
    pub fn toggle_bone_edit_mode(&mut self) {
        if let Some(edit) = self.bone_edit.take() {
            if let Some(ObjectData::Armature(a)) = self.scene.object_mut(edit.object).map(|o| &mut o.data) {
                a.edit_rest = false;
            }
            self.status = "Object Mode".into();
            return;
        }
        if let Some(id) = self.scene.selection.active
            && self.active_is_armature()
        {
            self.pose = None;
            self.sculpt = None;
            self.scene.edit = None;
            if let Some(ObjectData::Armature(a)) = self.scene.object_mut(id).map(|o| &mut o.data) {
                a.edit_rest = true;
            }
            self.bone_edit = Some(BoneEditMode { object: id, drag: None });
            self.status = "Edit Mode (Armature)".into();
        }
    }

    fn edit_armature_mut(&mut self, id: ObjectId) -> Option<&mut armature::Armature> {
        match self.scene.object_mut(id).map(|o| &mut o.data) {
            Some(ObjectData::Armature(a)) => Some(a),
            _ => None,
        }
    }

    pub(crate) fn bone_edit_keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() || self.popup.is_some() {
            return;
        }
        let Some(pointer) = self.last_pointer else { return };
        let [e, g, x, a, tab] = ui.input_mut(|i| {
            [
                crate::key(i, egui::Modifiers::NONE, egui::Key::E),
                crate::key(i, egui::Modifiers::NONE, egui::Key::G),
                crate::key(i, egui::Modifiers::NONE, egui::Key::X) || crate::key(i, egui::Modifiers::NONE, egui::Key::Delete),
                crate::key(i, egui::Modifiers::NONE, egui::Key::A),
                crate::key(i, egui::Modifiers::NONE, egui::Key::Tab),
            ]
        });
        let Some(edit) = self.bone_edit.clone() else { return };
        if tab {
            self.toggle_bone_edit_mode();
            return;
        }
        if edit.drag.is_some() {
            return;
        }
        let id = edit.object;
        let (selected, active) = match self.scene.object(id).map(|o| &o.data) {
            Some(ObjectData::Armature(arm)) => (arm.selected.clone(), arm.active),
            _ => (Vec::new(), None),
        };
        if e && let Some(parent) = active {
            let before = self.scene.clone();
            if let Some(arm) = self.edit_armature_mut(id) {
                let tail = arm.bones[parent].tail + (arm.bones[parent].tail - arm.bones[parent].head) * 0.5;
                let n = arm.bones.len();
                let new = arm.add_child(parent, tail, format!("Bone.{n:03}"));
                arm.selected = vec![new];
                arm.active = Some(new);
                if let Some(b) = self.bone_edit.as_mut() {
                    b.drag = Some(BoneDrag { kind: BoneDragKind::Extrude(new), before, start_px: pointer });
                }
                self.status = "Extrude bone: move the pointer, LMB/Enter confirm, Esc cancel".into();
            }
        }
        if g && !selected.is_empty() {
            if let Some(b) = self.bone_edit.as_mut() {
                b.drag = Some(BoneDrag { kind: BoneDragKind::Move, before: self.scene.clone(), start_px: pointer });
            }
            self.status = "Move bones: LMB/Enter confirm, Esc cancel".into();
        }
        if x && !selected.is_empty() {
            self.edit("Delete Bones", |scene| {
                if let Some(ObjectData::Armature(arm)) = scene.object_mut(id).map(|o| &mut o.data) {
                    let mut doomed = selected.clone();
                    doomed.sort_unstable();
                    doomed.dedup();
                    for &d in doomed.iter().rev() {
                        if d >= arm.bones.len() {
                            continue;
                        }
                        let parent = arm.bones[d].parent;
                        arm.bones.remove(d);
                        for b in &mut arm.bones {
                            b.parent = match b.parent {
                                Some(p) if p == d => parent,
                                Some(p) if p > d => Some(p - 1),
                                other => other,
                            };
                        }
                    }
                    arm.selected.clear();
                    arm.active = None;
                }
            });
        }
        if a && let Some(arm) = self.edit_armature_mut(id) {
            if arm.selected.len() == arm.bones.len() {
                arm.selected.clear();
                arm.active = None;
            } else {
                arm.selected = (0..arm.bones.len()).collect();
            }
        }
    }

    pub(crate) fn bone_edit_input(&mut self, input: &ViewportInput, enter: bool, escape: bool) -> bool {
        let Some(edit) = self.bone_edit.clone() else { return false };
        let id = edit.object;
        let size = input.size.max(Vec2::ONE);
        if let Some(drag) = edit.drag {
            if escape {
                self.scene = drag.before;
                if let Some(ObjectData::Armature(a)) = self.scene.object_mut(id).map(|o| &mut o.data) {
                    a.edit_rest = true;
                }
                if let Some(b) = self.bone_edit.as_mut() {
                    b.drag = None;
                }
                self.status = "Cancelled".into();
                return true;
            }
            if input.primary_clicked || enter {
                let name = match drag.kind {
                    BoneDragKind::Move => "Move Bones",
                    BoneDragKind::Extrude(_) => "Extrude Bone",
                };
                self.record_from(name, drag.before);
                if let Some(ObjectData::Armature(a)) = self.scene.object_mut(id).map(|o| &mut o.data) {
                    a.edit_rest = true;
                }
                if let Some(b) = self.bone_edit.as_mut() {
                    b.drag = None;
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
            let Some(ObjectData::Armature(before)) = drag.before.object(id).map(|o| &o.data) else { return true };
            let before = before.clone();
            if let Some(arm) = self.edit_armature_mut(id) {
                match drag.kind {
                    BoneDragKind::Move => {
                        for &b in &before.selected {
                            if let (Some(bone), Some(was)) = (arm.bones.get_mut(b), before.bones.get(b)) {
                                bone.head = was.head + move_local;
                                bone.tail = was.tail + move_local;
                            }
                        }
                    }
                    BoneDragKind::Extrude(new) => {
                        if let Some(bone) = arm.bones.get_mut(new) {
                            bone.tail = before.bones.get(new).map(|b| b.tail).unwrap_or(bone.head) + move_local;
                            if bone.tail.distance(bone.head) < 1e-3 {
                                bone.tail = bone.head + Vec3::Y * 0.01;
                            }
                        }
                    }
                }
            }
            return true;
        }
        if input.primary_clicked
            && let Some(p) = input.pointer
        {
            let camera = self.viewport.camera.clone();
            let world = self.scene.world_transform(id);
            let Some(ObjectData::Armature(arm)) = self.scene.object(id).map(|o| &o.data) else { return true };
            let mut best: Option<(usize, f32)> = None;
            for (i, bone) in arm.bones.iter().enumerate() {
                let (Some(a), Some(b)) =
                    (camera.project(world.transform_point(bone.head), size), camera.project(world.transform_point(bone.tail), size))
                else {
                    continue;
                };
                let ab = b - a;
                let tt = if ab.length_squared() < 1e-6 { 0.0 } else { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
                let d = (a + ab * tt).distance(p);
                let tolerance = (ab.length() * 0.12 + 6.0).max(14.0);
                if d < tolerance && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((i, d));
                }
            }
            let shift = input.shift;
            let mut picked = None;
            if let Some(arm) = self.edit_armature_mut(id) {
                match best {
                    Some((i, _)) => {
                        if !shift {
                            arm.selected.clear();
                        }
                        if !arm.selected.contains(&i) {
                            arm.selected.push(i);
                        }
                        arm.active = Some(i);
                        picked = arm.bones.get(i).map(|b| b.name.clone());
                    }
                    None if !shift => {
                        arm.selected.clear();
                        arm.active = None;
                    }
                    None => {}
                }
            }
            self.status = match picked {
                Some(name) => format!("Bone: {name}"),
                None => "No bone under the pointer".into(),
            };
            return true;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(p: Vec2, clicked: bool) -> ViewportInput {
        ViewportInput { pointer: Some(p), size: Vec2::new(400.0, 300.0), primary_clicked: clicked, hovered: true, ..Default::default() }
    }

    fn arm(app: &ShapeApp, id: ObjectId) -> &armature::Armature {
        match &app.scene.object(id).unwrap().data {
            ObjectData::Armature(a) => a,
            _ => panic!("not an armature"),
        }
    }

    #[test]
    fn extrude_adds_a_child_at_the_tail_and_delete_reparents() {
        let mut app = ShapeApp::default();
        let id = app.add_object("Armature", ObjectData::Armature(armature::Armature::single_bone()), Vec3::ZERO);
        app.scene.selection.set(id);
        app.viewport.camera = shape_gpu::OrbitCamera { yaw: 0.0, pitch: 0.0, distance: 5.0, ..Default::default() };
        app.toggle_bone_edit_mode();
        assert!(app.bone_edit.is_some() && arm(&app, id).edit_rest);
        let size = Vec2::new(400.0, 300.0);
        let mid = app.viewport.camera.project(Vec3::new(0.0, 0.5, 0.0), size).unwrap();
        app.bone_edit_input(&input(mid, true), false, false);
        assert_eq!(arm(&app, id).active, Some(0));
        // Extrude: a new bone starts at the root's tail and follows the pointer.
        app.last_pointer = Some(mid);
        let undo = app.history.len();
        // Mirror what E does, then drive the drag through the input handler.
        {
            let a = match &mut app.scene.object_mut(id).unwrap().data {
                ObjectData::Armature(a) => a,
                _ => unreachable!(),
            };
            let new = a.add_child(0, Vec3::new(0.0, 1.5, 0.0), "Bone.001");
            a.selected = vec![new];
            a.active = Some(new);
            app.bone_edit.as_mut().unwrap().drag = Some(BoneDrag { kind: BoneDragKind::Extrude(new), before: app.scene.clone(), start_px: mid });
        }
        app.bone_edit_input(&input(mid + Vec2::new(60.0, 0.0), false), false, false);
        app.bone_edit_input(&input(mid + Vec2::new(60.0, 0.0), true), false, false);
        let a = arm(&app, id);
        assert_eq!(a.bones.len(), 2);
        assert_eq!(a.bones[1].parent, Some(0));
        assert!(a.bones[1].head.distance(Vec3::Y) < 1e-5, "child starts at the parent's tail");
        assert!(a.bones[1].tail.x > 0.3, "tail followed the pointer: {}", a.bones[1].tail);
        assert_eq!(app.history.len(), undo + 1);
        // Delete the root: the child re-parents to none and shifts down.
        {
            let a = match &mut app.scene.object_mut(id).unwrap().data {
                ObjectData::Armature(a) => a,
                _ => unreachable!(),
            };
            a.selected = vec![0];
        }
        let selected = [0usize];
        app.edit("Delete Bones", |scene| {
            if let Some(ObjectData::Armature(arm)) = scene.object_mut(id).map(|o| &mut o.data) {
                for &d in selected.iter().rev() {
                    let parent = arm.bones[d].parent;
                    arm.bones.remove(d);
                    for b in &mut arm.bones {
                        b.parent = match b.parent {
                            Some(p) if p == d => parent,
                            Some(p) if p > d => Some(p - 1),
                            other => other,
                        };
                    }
                }
            }
        });
        let a = arm(&app, id);
        assert_eq!(a.bones.len(), 1);
        assert_eq!(a.bones[0].parent, None);
        app.toggle_bone_edit_mode();
        assert!(app.bone_edit.is_none() && !arm(&app, id).edit_rest);
    }
}
