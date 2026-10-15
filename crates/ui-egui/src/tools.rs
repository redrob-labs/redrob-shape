//! Tools and modal operators: G/R/S transforms in object mode, E/I/Ctrl+R in edit mode.
//!
//! A modal starts on a key or a tool drag, follows the pointer every frame, and ends with confirm
//! (click / Enter) or cancel (Esc / right click). The scene is edited live from a snapshot taken
//! at the start; confirm records one undo step from that snapshot, cancel restores it.

use crate::viewport::ViewportInput;
use shape_geom::{FaceId, Mesh};
use shape_gpu::OrbitCamera;
use shape_math::{Quat, Transform, Vec2, Vec3};
use shape_scene::{MeshId, ObjectId, Scene};

/// Toolbar tool. Select picks; the others start the matching modal on a left drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Tool {
    #[default]
    Select,
    Move,
    Rotate,
    Scale,
}

impl Tool {
    pub const ALL: [Tool; 4] = [Tool::Select, Tool::Move, Tool::Rotate, Tool::Scale];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Move => "Move",
            Tool::Rotate => "Rotate",
            Tool::Scale => "Scale",
        }
    }

    /// Toolbar glyph, standing in for Blender's tool icons.
    pub fn glyph(self) -> &'static str {
        match self {
            Tool::Select => "↖",
            Tool::Move => "✥",
            Tool::Rotate => "↻",
            Tool::Scale => "⤢",
        }
    }

    /// Default key, as in Blender.
    pub fn key(self) -> &'static str {
        match self {
            Tool::Select => "W",
            Tool::Move => "G",
            Tool::Rotate => "R",
            Tool::Scale => "S",
        }
    }

    pub fn modal(self) -> Option<ModalKind> {
        match self {
            Tool::Select => None,
            Tool::Move => Some(ModalKind::Move),
            Tool::Rotate => Some(ModalKind::Rotate),
            Tool::Scale => Some(ModalKind::Scale),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalKind {
    Move,
    Rotate,
    Scale,
    /// Edit mode: extrude the selected faces by a pointer-driven distance.
    Extrude,
    /// Edit mode: inset the selected faces by a pointer-driven thickness.
    Inset,
    /// Edit mode: bevel the edges of the selected faces by a pointer-driven offset.
    Bevel,
}

impl ModalKind {
    pub fn name(self) -> &'static str {
        match self {
            ModalKind::Move => "Move",
            ModalKind::Rotate => "Rotate",
            ModalKind::Scale => "Scale",
            ModalKind::Extrude => "Extrude",
            ModalKind::Inset => "Inset",
            ModalKind::Bevel => "Bevel",
        }
    }
}

/// What ended a modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalEnd {
    Confirmed,
    Cancelled,
}

/// A running modal operator.
#[derive(Clone, Debug)]
pub struct Modal {
    pub kind: ModalKind,
    /// Scene as it was when the modal started (restored on cancel, `before` of the undo step).
    pub before: Scene,
    pub start_pointer: Vec2,
    /// Axis lock 0/1/2, or `None` for free (view plane / view axis / uniform).
    pub axis: Option<usize>,
    /// Objects being transformed with their original transforms.
    pub objects: Vec<(ObjectId, Transform)>,
    /// World-space pivot: the selection centre.
    pub pivot: Vec3,
    /// Edit-mode payload: mesh id, base mesh, selected faces.
    pub edit: Option<(MeshId, Mesh, Vec<FaceId>)>,
    /// Confirm on primary release instead of click (tool drags).
    pub release_confirms: bool,
    /// Last computed amount, for the status line.
    pub amount: f32,
}

impl Modal {
    /// Start a transform modal on the current selection. `None` when nothing is selected.
    pub fn transform(kind: ModalKind, scene: &Scene, start_pointer: Vec2, release_confirms: bool) -> Option<Self> {
        let objects: Vec<(ObjectId, Transform)> =
            scene.selection.objects.iter().filter_map(|&id| scene.object(id).map(|o| (id, o.transform))).collect();
        if objects.is_empty() {
            return None;
        }
        let pivot = scene.selection_center();
        Some(Self { kind, before: scene.clone(), start_pointer, axis: None, objects, pivot, edit: None, release_confirms, amount: 0.0 })
    }

    /// Start an edit-mode face modal. `None` without an edit selection.
    pub fn faces(kind: ModalKind, scene: &Scene, start_pointer: Vec2) -> Option<Self> {
        let edit = scene.edit.as_ref()?;
        if edit.faces.is_empty() {
            return None;
        }
        let (mesh_id, mesh) = scene.object_mesh(edit.object)?;
        let faces: Vec<FaceId> = edit.faces.iter().copied().collect();
        let pivot = scene.world_transform(edit.object).translation;
        Some(Self {
            kind,
            before: scene.clone(),
            start_pointer,
            axis: None,
            objects: Vec::new(),
            pivot,
            edit: Some((mesh_id, mesh.clone(), faces)),
            release_confirms: false,
            amount: 0.0,
        })
    }

    /// Apply the modal for the current pointer. Called every frame while it runs.
    pub fn update(&mut self, scene: &mut Scene, camera: &OrbitCamera, pointer: Vec2, size: Vec2) {
        let delta = pointer - self.start_pointer;
        let world_per_pixel = 2.0 * camera.distance * (camera.fov_y * 0.5).tan() / size.y.max(1.0);
        match self.kind {
            ModalKind::Move => {
                let mut offset = (camera.right() * delta.x - camera.up() * delta.y) * world_per_pixel;
                if let Some(a) = self.axis {
                    let mut axis = Vec3::ZERO;
                    axis[a] = 1.0;
                    offset = axis * offset.dot(axis);
                }
                self.amount = offset.length();
                for (id, original) in &self.objects {
                    if let Some(o) = scene.object_mut(*id) {
                        o.transform = Transform { translation: original.translation + offset, ..*original };
                    }
                }
            }
            ModalKind::Rotate => {
                let centre = camera.project(self.pivot, size).unwrap_or(size * 0.5);
                let a0 = (self.start_pointer - centre).to_angle();
                let a1 = (pointer - centre).to_angle();
                // screen y is down, so flip to keep counter-clockwise positive
                let angle = -(a1 - a0);
                self.amount = angle.to_degrees();
                let axis = match self.axis {
                    Some(a) => {
                        let mut v = Vec3::ZERO;
                        v[a] = 1.0;
                        v
                    }
                    None => -camera.forward(),
                };
                let rot = Quat::from_axis_angle(axis, angle);
                for (id, original) in &self.objects {
                    if let Some(o) = scene.object_mut(*id) {
                        let rel = original.translation - self.pivot;
                        o.transform = Transform { translation: self.pivot + rot * rel, rotation: rot * original.rotation, ..*original };
                    }
                }
            }
            ModalKind::Scale => {
                let centre = camera.project(self.pivot, size).unwrap_or(size * 0.5);
                let d0 = (self.start_pointer - centre).length().max(1.0);
                let d1 = (pointer - centre).length();
                let factor = (d1 / d0).max(0.001);
                self.amount = factor;
                let mut s = Vec3::splat(factor);
                if let Some(a) = self.axis {
                    s = Vec3::ONE;
                    s[a] = factor;
                }
                for (id, original) in &self.objects {
                    if let Some(o) = scene.object_mut(*id) {
                        let rel = original.translation - self.pivot;
                        o.transform = Transform { translation: self.pivot + rel * s, scale: original.scale * s, ..*original };
                    }
                }
            }
            ModalKind::Extrude | ModalKind::Inset | ModalKind::Bevel => {
                let Some((mesh_id, base, faces)) = &self.edit else { return };
                let amount = -delta.y * world_per_pixel;
                self.amount = amount;
                let result = match self.kind {
                    ModalKind::Extrude => shape_ops::extrude_faces(base, faces, amount),
                    ModalKind::Bevel => shape_ops::bevel_edges(base, &shape_ops::face_edges(base, faces), amount.max(0.0)),
                    _ => shape_ops::inset_faces(base, faces, amount.max(0.0)),
                };
                if let Ok(m) = result
                    && let Some(data) = scene.mesh_mut(*mesh_id)
                {
                    data.set_mesh(m);
                }
            }
        }
    }

    /// Toggle an axis lock (pressing the same axis twice frees it).
    pub fn set_axis(&mut self, axis: usize) {
        self.axis = if self.axis == Some(axis) { None } else { Some(axis) };
    }

    pub fn status(&self) -> String {
        let axis = match self.axis {
            Some(0) => " [X]",
            Some(1) => " [Y]",
            Some(2) => " [Z]",
            _ => "",
        };
        match self.kind {
            ModalKind::Move => format!("Move{axis}  {:.3}  ·  click/Enter confirm, Esc cancel, X/Y/Z axis", self.amount),
            ModalKind::Rotate => format!("Rotate{axis}  {:.1}°  ·  click/Enter confirm, Esc cancel, X/Y/Z axis", self.amount),
            ModalKind::Scale => format!("Scale{axis}  ×{:.3}  ·  click/Enter confirm, Esc cancel, X/Y/Z axis", self.amount),
            ModalKind::Extrude => format!("Extrude  {:.3}  ·  move pointer up/down, click/Enter confirm, Esc cancel", self.amount),
            ModalKind::Inset => format!("Inset  {:.3}  ·  move pointer up/down, click/Enter confirm, Esc cancel", self.amount),
            ModalKind::Bevel => format!("Bevel  {:.3}  ·  move pointer up/down, click/Enter confirm, Esc cancel", self.amount),
        }
    }

    /// Decide from this frame's input whether the modal ends.
    pub fn end_for(&self, input: &ViewportInput, enter: bool, escape: bool) -> Option<ModalEnd> {
        if escape || input.secondary_clicked {
            return Some(ModalEnd::Cancelled);
        }
        if enter || (self.release_confirms && input.primary_released) || (!self.release_confirms && input.primary_clicked) {
            return Some(ModalEnd::Confirmed);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::cube;

    fn scene_with_cube() -> (Scene, ObjectId) {
        let mut s = Scene::new("t");
        let id = s.add_mesh_object("Cube", cube(1.0), None);
        s.selection.set(id);
        (s, id)
    }

    #[test]
    fn move_follows_pointer_and_axis_lock() {
        let (mut scene, id) = scene_with_cube();
        let cam = OrbitCamera { yaw: 0.0, pitch: 0.0, distance: 5.0, ..Default::default() };
        let size = Vec2::new(800.0, 600.0);
        let mut m = Modal::transform(ModalKind::Move, &scene, size * 0.5, false).unwrap();
        m.update(&mut scene, &cam, size * 0.5 + Vec2::new(100.0, 0.0), size);
        let t = scene.object(id).unwrap().transform.translation;
        assert!(t.x > 0.5 && t.y.abs() < 1e-4 && t.z.abs() < 1e-4, "{t}");
        m.set_axis(1);
        m.update(&mut scene, &cam, size * 0.5 + Vec2::new(100.0, 0.0), size);
        let t = scene.object(id).unwrap().transform.translation;
        assert!(t.x.abs() < 1e-4, "locked to Y: {t}");
        m.set_axis(1);
        assert_eq!(m.axis, None);
    }

    #[test]
    fn rotate_and_scale_about_pivot() {
        let (mut scene, id) = scene_with_cube();
        let cam = OrbitCamera { yaw: 0.0, pitch: 0.0, distance: 5.0, ..Default::default() };
        let size = Vec2::new(800.0, 600.0);
        let mut m = Modal::transform(ModalKind::Scale, &scene, size * 0.5 + Vec2::new(100.0, 0.0), false).unwrap();
        m.update(&mut scene, &cam, size * 0.5 + Vec2::new(200.0, 0.0), size);
        let s = scene.object(id).unwrap().transform.scale;
        assert!((s.x - 2.0).abs() < 1e-3, "{s}");
        let mut r = Modal::transform(ModalKind::Rotate, &scene, size * 0.5 + Vec2::new(100.0, 0.0), false).unwrap();
        r.update(&mut scene, &cam, size * 0.5 + Vec2::new(0.0, -100.0), size);
        let q = scene.object(id).unwrap().transform.rotation;
        let (axis, angle) = q.to_axis_angle();
        assert!((angle.to_degrees() - 90.0).abs() < 1.0, "{}", angle.to_degrees());
        assert!(axis.z.abs() > 0.99, "about the view axis: {axis}");
    }

    #[test]
    fn face_modal_extrudes_from_snapshot_each_frame() {
        let (mut scene, id) = scene_with_cube();
        let top = scene.object_mesh(id).unwrap().1.face_ids().find(|&f| scene.object_mesh(id).unwrap().1.face_normal(f).y > 0.9).unwrap();
        let mut e = shape_scene::EditSelection::new(id);
        e.faces.insert(top);
        scene.edit = Some(e);
        let cam = OrbitCamera { yaw: 0.0, pitch: 0.0, distance: 5.0, ..Default::default() };
        let size = Vec2::new(800.0, 600.0);
        let mut m = Modal::faces(ModalKind::Extrude, &scene, size * 0.5).unwrap();
        m.update(&mut scene, &cam, size * 0.5 - Vec2::new(0.0, 60.0), size);
        let h1 = scene.object_mesh(id).unwrap().1.aabb().max.y;
        m.update(&mut scene, &cam, size * 0.5 - Vec2::new(0.0, 120.0), size);
        let h2 = scene.object_mesh(id).unwrap().1.aabb().max.y;
        assert!(h2 > h1 && h1 > 0.5, "{h1} {h2}");
        assert_eq!(scene.object_mesh(id).unwrap().1.face_count(), 10, "sides added once, not twice");
        let input = ViewportInput { primary_clicked: true, ..Default::default() };
        assert_eq!(m.end_for(&input, false, false), Some(ModalEnd::Confirmed));
        assert_eq!(m.end_for(&ViewportInput::default(), false, true), Some(ModalEnd::Cancelled));
    }
}
