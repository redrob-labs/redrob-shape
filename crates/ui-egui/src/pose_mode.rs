//! Pose Mode and rigging, after Blender's: an armature's bones are picked by clicking near them,
//! G / R move / rotate the selected bones' pose (LMB or Enter confirms, Esc cancels, one undo step),
//! Alt+G / Alt+R clear location / rotation, A selects all; Ctrl+P on meshes with an armature active
//! offers Armature Deform and With Automatic Weights, which parents the meshes, fills their vertex
//! groups from bone distance and adds an Armature modifier.

use egui::Color32;
use shape_math::{Quat, Vec2, Vec3};
use shape_scene::{Modifier, ObjectData, ObjectId, Scene, armature};

use crate::ShapeApp;
use crate::viewport::ViewportInput;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PoseDragKind {
    Move,
    Rotate,
}

#[derive(Clone, Debug)]
pub struct PoseDrag {
    pub kind: PoseDragKind,
    before: Scene,
    start_px: Vec2,
}

#[derive(Clone, Debug)]
pub struct PoseMode {
    pub object: ObjectId,
    pub drag: Option<PoseDrag>,
}

/// Ctrl+P menu entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParentKind {
    Object,
    ArmatureDeform,
    AutomaticWeights,
}

impl ParentKind {
    pub fn name(self) -> &'static str {
        match self {
            ParentKind::Object => "Object (Keep Transform)",
            ParentKind::ArmatureDeform => "Armature Deform",
            ParentKind::AutomaticWeights => "With Automatic Weights",
        }
    }
}

impl ShapeApp {
    pub fn active_is_armature(&self) -> bool {
        self.scene.selection.active.and_then(|id| self.scene.object(id)).is_some_and(|o| matches!(o.data, ObjectData::Armature(_)))
    }

    pub fn toggle_pose_mode(&mut self) {
        if self.pose.is_some() {
            self.pose = None;
            self.status = "Object Mode".into();
            return;
        }
        if let Some(id) = self.scene.selection.active
            && self.active_is_armature()
        {
            self.scene.edit = None;
            self.sculpt = None;
            self.pose = Some(PoseMode { object: id, drag: None });
            self.status = "Pose Mode".into();
        }
    }

    fn armature_mut(&mut self, id: ObjectId) -> Option<&mut armature::Armature> {
        match self.scene.object_mut(id).map(|o| &mut o.data) {
            Some(ObjectData::Armature(a)) => Some(a),
            _ => None,
        }
    }

    /// Pose-mode keys: G / R start a drag, Alt+G / Alt+R clear, A selects all.
    pub(crate) fn pose_keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() || self.popup.is_some() {
            return;
        }
        let Some(pointer) = self.last_pointer else { return };
        let [g, r, alt_g, alt_r, a, key_i] = ui.input_mut(|i| {
            [
                crate::key(i, egui::Modifiers::NONE, egui::Key::G),
                crate::key(i, egui::Modifiers::NONE, egui::Key::R),
                crate::key(i, egui::Modifiers::ALT, egui::Key::G),
                crate::key(i, egui::Modifiers::ALT, egui::Key::R),
                crate::key(i, egui::Modifiers::NONE, egui::Key::A),
                crate::key(i, egui::Modifiers::NONE, egui::Key::I),
            ]
        });
        if key_i {
            self.popup = Some(crate::Popup::InsertKeyframe);
            return;
        }
        let Some(pose) = self.pose.clone() else { return };
        if pose.drag.is_some() {
            return;
        }
        let id = pose.object;
        let selected = match self.scene.object(id).map(|o| &o.data) {
            Some(ObjectData::Armature(arm)) => arm.selected.clone(),
            _ => Vec::new(),
        };
        if (g || r) && !selected.is_empty() {
            let kind = if g { PoseDragKind::Move } else { PoseDragKind::Rotate };
            if let Some(p) = self.pose.as_mut() {
                p.drag = Some(PoseDrag { kind, before: self.scene.clone(), start_px: pointer });
            }
            self.status = if g { "Move bone: LMB/Enter confirm, Esc cancel".into() } else { "Rotate bone: LMB/Enter confirm, Esc cancel".into() };
        }
        if (alt_g || alt_r) && !selected.is_empty() {
            self.edit(if alt_g { "Clear Pose Location" } else { "Clear Pose Rotation" }, |scene| {
                if let Some(ObjectData::Armature(arm)) = scene.object_mut(id).map(|o| &mut o.data) {
                    for &b in &selected {
                        if let Some(bone) = arm.bones.get_mut(b) {
                            if alt_g {
                                bone.pose.translation = Vec3::ZERO;
                            } else {
                                bone.pose.rotation = Quat::IDENTITY;
                            }
                        }
                    }
                }
            });
        }
        if a && let Some(arm) = self.armature_mut(id) {
            if arm.selected.len() == arm.bones.len() {
                arm.selected.clear();
                arm.active = None;
            } else {
                arm.selected = (0..arm.bones.len()).collect();
            }
        }
    }

    /// Pose-mode pointer handling: click picks a bone, a drag moves / rotates the selection.
    pub(crate) fn pose_input(&mut self, input: &ViewportInput, enter: bool, escape: bool) -> bool {
        let Some(pose) = self.pose.clone() else { return false };
        let id = pose.object;
        let size = input.size.max(Vec2::ONE);
        if let Some(drag) = pose.drag {
            if escape {
                self.scene = drag.before;
                if let Some(p) = self.pose.as_mut() {
                    p.drag = None;
                }
                self.status = "Cancelled".into();
                return true;
            }
            if input.primary_clicked || enter {
                let name = match drag.kind {
                    PoseDragKind::Move => "Move Bone",
                    PoseDragKind::Rotate => "Rotate Bone",
                };
                self.record_from(name, drag.before);
                if let Some(p) = self.pose.as_mut() {
                    p.drag = None;
                }
                self.status = format!("{name} confirmed");
                return true;
            }
            let Some(p) = input.pointer else { return true };
            let delta = p - drag.start_px;
            let camera = self.viewport.camera.clone();
            let world = self.scene.world_transform(id);
            // Screen-plane move: pixels to world at the armature's distance.
            let per_px = 2.0 * camera.distance * (camera.fov_y / 2.0).tan() / size.y;
            let move_world = (camera.right() * delta.x - camera.up() * delta.y) * per_px;
            let view_axis = camera.forward();
            let before = drag.before.clone();
            let Some(ObjectData::Armature(arm_before)) = before.object(id).map(|o| &o.data) else { return true };
            let posed = arm_before.pose_matrices();
            let selected = arm_before.selected.clone();
            if let Some(arm) = self.armature_mut(id) {
                for &b in &selected {
                    let Some(bone) = arm.bones.get_mut(b) else { continue };
                    let was = arm_before.bones[b].pose;
                    // Bone-space frame of this bone in world: parent pose chain * rest.
                    let frame_world = world.to_matrix() * posed[b] * arm_before.bones[b].pose.to_matrix().inverse();
                    let inv = frame_world.inverse();
                    match drag.kind {
                        PoseDragKind::Move => {
                            let local = inv.transform_vector3(move_world);
                            bone.pose.translation = was.translation + local;
                        }
                        PoseDragKind::Rotate => {
                            let angle = -delta.x * 0.01;
                            let axis_local = inv.transform_vector3(view_axis).normalize_or_zero();
                            let axis_local = if axis_local.length_squared() < 0.5 { Vec3::Z } else { axis_local };
                            bone.pose.rotation = (Quat::from_axis_angle(axis_local, angle) * was.rotation).normalize();
                        }
                    }
                }
            }
            return true;
        }
        // Click: pick the nearest bone (within 14 px of its segment), Shift extends.
        if input.primary_clicked
            && let Some(p) = input.pointer
        {
            let camera = self.viewport.camera.clone();
            let world = self.scene.world_transform(id);
            let Some(ObjectData::Armature(arm)) = self.scene.object(id).map(|o| &o.data) else { return true };
            let mut best: Option<(usize, f32)> = None;
            for (i, (h, t)) in arm.posed_segments().into_iter().enumerate() {
                let (Some(a), Some(b)) = (camera.project(world.transform_point(h), size), camera.project(world.transform_point(t), size)) else {
                    continue;
                };
                let ab = b - a;
                let tt = if ab.length_squared() < 1e-6 { 0.0 } else { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
                let d = (a + ab * tt).distance(p);
                // The octahedron is about a tenth of the bone wide; accept clicks on its body.
                let tolerance = (ab.length() * 0.12 + 6.0).max(14.0);
                if d < tolerance && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((i, d));
                }
            }
            let shift = input.shift;
            let mut picked = None;
            if let Some(arm) = self.armature_mut(id) {
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
        // Other pointer input (orbit etc.) passes through to the camera; nothing else to do.
        true
    }

    /// Ctrl+P menu: parent the other selected objects to the active one. With an armature active,
    /// meshes can also be deformed by it, with weights filled in automatically.
    pub fn parent_selected(&mut self, kind: ParentKind) {
        let Some(active) = self.scene.selection.active else { return };
        if kind == ParentKind::Object {
            self.parent_to_active();
            return;
        }
        if !self.active_is_armature() {
            self.status = "Armature Deform needs an armature as the active object".into();
            return;
        }
        let children: Vec<ObjectId> =
            self.scene.selection.objects.iter().copied().filter(|&c| c != active && self.scene.object_mesh(c).is_some()).collect();
        if children.is_empty() {
            self.status = "Select the mesh, then the armature (active), then Ctrl+P".into();
            return;
        }
        let name = if kind == ParentKind::AutomaticWeights { "Parent: With Automatic Weights" } else { "Parent: Armature Deform" };
        self.edit(name, |scene| {
            let arm_world = scene.world_transform(active);
            let Some(ObjectData::Armature(arm)) = scene.object(active).map(|o| o.data.clone()) else { return };
            for c in children {
                let world = scene.world_transform(c);
                let to_arm = arm_world.inverse().mul_transform(&world);
                if scene.set_parent(c, Some(active))
                    && let Some(o) = scene.object_mut(c)
                {
                    o.transform = to_arm;
                }
                if let Some((mesh_id, mesh)) = scene.object_mesh(c) {
                    let groups = if kind == ParentKind::AutomaticWeights { armature::automatic_weights(mesh, &arm, &to_arm) } else { Vec::new() };
                    if let Some(d) = scene.mesh_mut(mesh_id) {
                        d.vertex_groups = groups;
                    }
                }
                if let Some(stack) = scene.modifiers_mut(c)
                    && !stack.iter().any(|m| matches!(m, Modifier::Armature { object: Some(o) } if *o == active))
                {
                    stack.push(Modifier::Armature { object: Some(active) });
                }
            }
        });
    }

    /// Pose-mode header widgets: the active bone's pose values.
    pub(crate) fn pose_header(&mut self, ui: &mut egui::Ui) {
        let Some(pose) = self.pose.clone() else { return };
        let Some(ObjectData::Armature(arm)) = self.scene.object(pose.object).map(|o| &o.data) else { return };
        let label = match arm.active.and_then(|i| arm.bones.get(i)) {
            Some(b) => format!("{} · {} bones", b.name, arm.bones.len()),
            None => format!("{} bones · click a bone", arm.bones.len()),
        };
        ui.label(egui::RichText::new(label).color(Color32::from_rgb(0x9a, 0x9a, 0x9a)));
        if ui.button("Clear Pose").on_hover_text("Alt+G, Alt+R on all bones").clicked() {
            let id = pose.object;
            self.edit("Clear Pose", |scene| {
                if let Some(ObjectData::Armature(arm)) = scene.object_mut(id).map(|o| &mut o.data) {
                    arm.clear_pose();
                }
            });
        }
    }
}

/// Convenience for tests and the agent: the armature of an object, if it is one.
pub fn armature_of(scene: &Scene, id: ObjectId) -> Option<&armature::Armature> {
    match scene.object(id).map(|o| &o.data) {
        Some(ObjectData::Armature(a)) => Some(a),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Primitive;
    use shape_scene::Object;

    fn input(p: Vec2, clicked: bool) -> ViewportInput {
        ViewportInput { pointer: Some(p), size: Vec2::new(400.0, 300.0), primary_clicked: clicked, hovered: true, ..Default::default() }
    }

    #[test]
    fn automatic_weights_parent_skins_the_cylinder_to_the_armature() {
        let mut app = ShapeApp::default();
        let cyl = app.add_primitive(Primitive::Cylinder);
        let mut arm = armature::Armature::single_bone();
        arm.add_child(0, Vec3::new(0.0, 2.0, 0.0), "Bone.001");
        let arm_id = app.add_object("Armature", ObjectData::Armature(arm), Vec3::new(0.0, -1.0, 0.0));
        app.scene.selection.objects.insert(cyl);
        app.scene.selection.objects.insert(arm_id);
        app.scene.selection.active = Some(arm_id);
        app.parent_selected(ParentKind::AutomaticWeights);
        let (mesh_id, _) = app.scene.object_mesh(cyl).unwrap();
        assert_eq!(app.scene.mesh(mesh_id).unwrap().vertex_groups.len(), 2);
        assert_eq!(app.scene.object(cyl).unwrap().parent, Some(arm_id));
        assert!(matches!(app.scene.object(cyl).unwrap().data, ObjectData::Mesh { ref modifiers, .. } if modifiers.len() == 1));
        // Posing the child bone bends the evaluated mesh.
        let flat = shape_ops::evaluated_mesh(&app.scene, cyl).unwrap().into_owned();
        if let Some(ObjectData::Armature(a)) = app.scene.object_mut(arm_id).map(|o| &mut o.data) {
            a.bones[1].pose.rotation = Quat::from_rotation_z(1.0);
        }
        let bent = shape_ops::evaluated_mesh(&app.scene, cyl).unwrap().into_owned();
        let moved = flat.positions().zip(bent.positions()).filter(|(a, b)| a.distance(*b) > 0.1).count();
        assert!(moved > 0 && moved < flat.vertex_count(), "only the top moved: {moved}");
        drop(Object::new("x", ObjectData::Empty));
    }

    #[test]
    fn pose_mode_picks_and_moves_a_bone_with_one_undo_step() {
        let mut app = ShapeApp::default();
        let arm_id = app.add_object("Armature", ObjectData::Armature(armature::Armature::single_bone()), Vec3::ZERO);
        app.scene.selection.set(arm_id);
        app.viewport.camera = shape_gpu::OrbitCamera { yaw: 0.0, pitch: 0.0, distance: 5.0, ..Default::default() };
        app.toggle_pose_mode();
        assert!(app.pose.is_some());
        // The bone runs from the origin up: its midpoint projects near the screen centre.
        let size = Vec2::new(400.0, 300.0);
        let mid = app.viewport.camera.project(Vec3::new(0.0, 0.5, 0.0), size).unwrap();
        assert!(app.pose_input(&input(mid, true), false, false));
        assert_eq!(armature_of(&app.scene, arm_id).unwrap().selected, vec![0]);
        let undo = app.history.len();
        app.last_pointer = Some(mid);
        app.pose.as_mut().unwrap().drag = Some(PoseDrag { kind: PoseDragKind::Move, before: app.scene.clone(), start_px: mid });
        app.pose_input(&input(mid + Vec2::new(40.0, 0.0), false), false, false);
        app.pose_input(&input(mid + Vec2::new(40.0, 0.0), true), false, false);
        let t = armature_of(&app.scene, arm_id).unwrap().bones[0].pose.translation;
        assert!(t.length() > 0.1, "bone moved: {t}");
        assert_eq!(app.history.len(), undo + 1);
        assert!(app.pose.as_ref().unwrap().drag.is_none());
    }
}

#[cfg(test)]
mod outliner_tests {
    use super::*;
    use crate::Primitive;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn a_parented_mesh_shows_indented_under_the_armature() {
        let mut app = ShapeApp::default();
        let cyl = app.add_primitive(Primitive::Cylinder);
        let arm_id = app.add_object("Armature", ObjectData::Armature(armature::Armature::single_bone()), Vec3::ZERO);
        app.scene.selection.objects.insert(cyl);
        app.scene.selection.active = Some(arm_id);
        app.parent_selected(ParentKind::ArmatureDeform);
        let mut harness = Harness::new_ui(|ui| app.ui(ui));
        harness.run();
        // Both rows render (the child indented under its parent) without panicking.
        assert!(!harness.query_all_by_label_contains("🦴 Armature").collect::<Vec<_>>().is_empty());
        assert!(!harness.query_all_by_label_contains("▲ Cylinder").collect::<Vec<_>>().is_empty());
    }
}
