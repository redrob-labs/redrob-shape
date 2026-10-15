//! Tools and modal operators: G/R/S transforms in object mode, E/I/Ctrl+R in edit mode.
//!
//! A modal starts on a key or a tool drag, follows the pointer every frame, and ends with confirm
//! (click / Enter) or cancel (Esc / right click). The scene is edited live from a snapshot taken
//! at the start; confirm records one undo step from that snapshot, cancel restores it.

use crate::viewport::ViewportInput;
use shape_geom::{FaceId, Mesh};
use shape_gpu::OrbitCamera;
use shape_math::{Quat, Transform, Vec2, Vec3};
use shape_scene::{EditSelection, MeshId, ObjectId, Scene};

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
    /// Edit mode: bevel the selected edges by a pointer-driven offset; the wheel sets segments.
    Bevel,
    /// Edit mode: Alt+S, move the selected vertices along their normals.
    ShrinkFatten,
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
            ModalKind::ShrinkFatten => "Shrink/Fatten",
        }
    }
}

/// What ended a modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModalEnd {
    Confirmed,
    Cancelled,
}

/// Blender's pivot point choices (header dropdown, period key pie).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PivotPoint {
    #[default]
    MedianPoint,
    ActiveElement,
    IndividualOrigins,
    Cursor3D,
}

impl PivotPoint {
    pub const ALL: [PivotPoint; 4] = [PivotPoint::MedianPoint, PivotPoint::ActiveElement, PivotPoint::IndividualOrigins, PivotPoint::Cursor3D];
    pub fn name(self) -> &'static str {
        match self {
            PivotPoint::MedianPoint => "Median Point",
            PivotPoint::ActiveElement => "Active Element",
            PivotPoint::IndividualOrigins => "Individual Origins",
            PivotPoint::Cursor3D => "3D Cursor",
        }
    }
}

/// Blender's transform orientations for axis locks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Global,
    Local,
    View,
}

impl Orientation {
    pub const ALL: [Orientation; 3] = [Orientation::Global, Orientation::Local, Orientation::View];
    pub fn name(self) -> &'static str {
        match self {
            Orientation::Global => "Global",
            Orientation::Local => "Local",
            Orientation::View => "View",
        }
    }
}

/// How a transform modal is set up: pivot, orientation, proportional editing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TransformSettings {
    pub pivot: PivotPoint,
    pub orientation: Orientation,
    /// Proportional editing (O) in edit mode, with its falloff radius in world units.
    pub proportional: bool,
    pub proportional_size: f32,
}

/// Edit-mode vertex transform payload: original positions and a weight per vertex (1 selected,
/// a smooth falloff for proportional neighbours, 0 otherwise).
#[derive(Clone, Debug)]
pub struct EditTransform {
    pub mesh_id: MeshId,
    pub positions: Vec<Vec3>,
    pub polygons: Vec<Vec<u32>>,
    pub weights: Vec<f32>,
    /// Object-to-world and back, since the modal works in object space.
    pub world: Transform,
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
    /// Edit-mode payload: mesh id, base mesh, the selection when the modal started.
    pub edit: Option<(MeshId, Mesh, EditSelection)>,
    /// Confirm on primary release instead of click (tool drags).
    pub release_confirms: bool,
    /// Last computed amount, for the status line.
    pub amount: f32,
    /// Bevel segments (wheel).
    pub segments: u32,
    pub settings: TransformSettings,
    /// Per-object pivot for Individual Origins (else every entry equals `pivot`).
    pub pivots: Vec<Vec3>,
    /// Axes the X/Y/Z locks refer to (world space).
    pub basis: [Vec3; 3],
    /// Ctrl held: snap to increments.
    pub snap: bool,
    pub edit_xf: Option<EditTransform>,
}

impl Modal {
    /// Start a transform modal on the current selection. `None` when nothing is selected.
    pub fn transform(kind: ModalKind, scene: &Scene, start_pointer: Vec2, release_confirms: bool) -> Option<Self> {
        Self::transform_with(kind, scene, start_pointer, release_confirms, TransformSettings::default(), None)
    }

    /// [`Self::transform`] with the pivot / orientation settings and the camera (for the View
    /// orientation).
    pub fn transform_with(
        kind: ModalKind,
        scene: &Scene,
        start_pointer: Vec2,
        release_confirms: bool,
        settings: TransformSettings,
        camera: Option<&OrbitCamera>,
    ) -> Option<Self> {
        let objects: Vec<(ObjectId, Transform)> =
            scene.selection.objects.iter().filter_map(|&id| scene.object(id).map(|o| (id, o.transform))).collect();
        if objects.is_empty() {
            return None;
        }
        let pivot = match settings.pivot {
            PivotPoint::MedianPoint | PivotPoint::IndividualOrigins => scene.selection_center(),
            PivotPoint::ActiveElement => {
                scene.selection.active.map(|a| scene.world_transform(a).translation).unwrap_or_else(|| scene.selection_center())
            }
            PivotPoint::Cursor3D => scene.cursor,
        };
        let pivots = objects
            .iter()
            .map(|(id, _)| if settings.pivot == PivotPoint::IndividualOrigins { scene.world_transform(*id).translation } else { pivot })
            .collect();
        let basis = Self::basis_for(settings.orientation, scene, camera);
        Some(Self {
            kind,
            before: scene.clone(),
            start_pointer,
            axis: None,
            objects,
            pivot,
            edit: None,
            release_confirms,
            amount: 0.0,
            segments: 1,
            settings,
            pivots,
            basis,
            snap: false,
            edit_xf: None,
        })
    }

    fn basis_for(orientation: Orientation, scene: &Scene, camera: Option<&OrbitCamera>) -> [Vec3; 3] {
        match orientation {
            Orientation::Global => [Vec3::X, Vec3::Y, Vec3::Z],
            Orientation::Local => {
                let rot = scene.selection.active.map(|a| scene.world_transform(a).rotation).unwrap_or(Quat::IDENTITY);
                [rot * Vec3::X, rot * Vec3::Y, rot * Vec3::Z]
            }
            Orientation::View => match camera {
                Some(c) => [c.right(), c.up(), -c.forward()],
                None => [Vec3::X, Vec3::Y, Vec3::Z],
            },
        }
    }

    /// Start a G/R/S modal on the selected vertices in edit mode, with proportional falloff when
    /// the settings ask for it. `None` without a vertex selection.
    pub fn edit_transform(
        kind: ModalKind,
        scene: &Scene,
        start_pointer: Vec2,
        settings: TransformSettings,
        camera: Option<&OrbitCamera>,
    ) -> Option<Self> {
        let edit = scene.edit.as_ref()?;
        if edit.verts.is_empty() {
            return None;
        }
        let (mesh_id, mesh) = scene.object_mesh(edit.object)?;
        let (positions, polygons) = mesh.to_polygons();
        let world = scene.world_transform(edit.object);
        let selected: Vec<Vec3> = edit.verts.iter().map(|&v| mesh.position(v)).collect();
        let weights: Vec<f32> = mesh
            .vertex_ids()
            .map(|v| {
                if edit.verts.contains(&v) {
                    return 1.0;
                }
                if !settings.proportional {
                    return 0.0;
                }
                let p = mesh.position(v);
                let d = selected.iter().map(|s| (*s - p).length()).fold(f32::INFINITY, f32::min);
                let r = settings.proportional_size.max(1e-6);
                if d >= r {
                    0.0
                } else {
                    // Smooth falloff (Blender's default).
                    let t = 1.0 - d / r;
                    t * t * (3.0 - 2.0 * t)
                }
            })
            .collect();
        let centroid_local = selected.iter().copied().sum::<Vec3>() / selected.len() as f32;
        let pivot = match settings.pivot {
            PivotPoint::Cursor3D => scene.cursor,
            _ => world.transform_point(centroid_local),
        };
        let basis = Self::basis_for(settings.orientation, scene, camera);
        Some(Self {
            kind,
            before: scene.clone(),
            start_pointer,
            axis: None,
            objects: Vec::new(),
            pivot,
            edit: None,
            release_confirms: false,
            amount: 0.0,
            segments: 1,
            settings,
            pivots: Vec::new(),
            basis,
            snap: false,
            edit_xf: Some(EditTransform { mesh_id, positions, polygons, weights, world }),
        })
    }

    /// Wheel during proportional editing: grow or shrink the falloff.
    pub fn scroll_proportional(&mut self, notches: f32, scene: &Scene) {
        if notches == 0.0 || !self.settings.proportional || self.edit_xf.is_none() {
            return;
        }
        self.settings.proportional_size = (self.settings.proportional_size * if notches > 0.0 { 1.25 } else { 0.8 }).clamp(0.01, 1000.0);
        // Recompute the weights from the modal's starting scene.
        if let Some(fresh) = Self::edit_transform(self.kind, &self.before, self.start_pointer, self.settings, None) {
            self.edit_xf = fresh.edit_xf;
        }
        let _ = scene;
    }

    /// Apply a world-space point transform to the edit vertices.
    fn apply_edit(&self, scene: &mut Scene, f: impl Fn(Vec3, f32) -> Vec3) {
        let Some(xf) = &self.edit_xf else { return };
        let inv = xf.world.inverse();
        let positions: Vec<Vec3> = xf
            .positions
            .iter()
            .zip(&xf.weights)
            .map(|(p, &w)| if w <= 0.0 { *p } else { inv.transform_point(f(xf.world.transform_point(*p), w)) })
            .collect();
        if let Ok(m) = Mesh::from_polygons(positions, &xf.polygons)
            && let Some(data) = scene.mesh_mut(xf.mesh_id)
        {
            data.set_mesh(m);
        }
    }

    /// Start an edit-mode face modal. `None` without an edit selection.
    pub fn faces(kind: ModalKind, scene: &Scene, start_pointer: Vec2) -> Option<Self> {
        let edit = scene.edit.as_ref()?;
        let enough = match kind {
            ModalKind::Bevel => !edit.edges.is_empty(),
            ModalKind::ShrinkFatten => !edit.verts.is_empty(),
            _ => !edit.faces.is_empty(),
        };
        if !enough {
            return None;
        }
        let (mesh_id, mesh) = scene.object_mesh(edit.object)?;
        let pivot = scene.world_transform(edit.object).translation;
        Some(Self {
            kind,
            before: scene.clone(),
            start_pointer,
            axis: None,
            objects: Vec::new(),
            pivot,
            edit: Some((mesh_id, mesh.clone(), edit.clone())),
            release_confirms: false,
            amount: 0.0,
            segments: 1,
            settings: TransformSettings::default(),
            pivots: Vec::new(),
            basis: [Vec3::X, Vec3::Y, Vec3::Z],
            snap: false,
            edit_xf: None,
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
                    let axis = self.basis[a];
                    offset = axis * offset.dot(axis);
                }
                if self.snap {
                    let step = if offset.length() < 1.0 { 0.1 } else { 1.0 };
                    offset = (offset / step).round() * step;
                }
                self.amount = offset.length();
                if self.edit_xf.is_some() {
                    self.apply_edit(scene, |p, w| p + offset * w);
                    return;
                }
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
                let mut angle = -(a1 - a0);
                if self.snap {
                    let step = 5f32.to_radians();
                    angle = (angle / step).round() * step;
                }
                self.amount = angle.to_degrees();
                let axis = match self.axis {
                    Some(a) => self.basis[a],
                    None => -camera.forward(),
                };
                let rot = Quat::from_axis_angle(axis, angle);
                if self.edit_xf.is_some() {
                    let pivot = self.pivot;
                    self.apply_edit(scene, |p, w| {
                        let r = Quat::from_axis_angle(axis, angle * w);
                        pivot + r * (p - pivot)
                    });
                    return;
                }
                for (i, (id, original)) in self.objects.iter().enumerate() {
                    let pivot = self.pivots.get(i).copied().unwrap_or(self.pivot);
                    if let Some(o) = scene.object_mut(*id) {
                        let rel = original.translation - pivot;
                        o.transform = Transform { translation: pivot + rot * rel, rotation: rot * original.rotation, ..*original };
                    }
                }
            }
            ModalKind::Scale => {
                let centre = camera.project(self.pivot, size).unwrap_or(size * 0.5);
                let d0 = (self.start_pointer - centre).length().max(1.0);
                let d1 = (pointer - centre).length();
                let mut factor = (d1 / d0).max(0.001);
                if self.snap {
                    factor = (factor / 0.1).round().max(1.0) * 0.1;
                }
                self.amount = factor;
                // Scale along the chosen axis (in the orientation's basis) or uniformly.
                let scale_point = |p: Vec3, pivot: Vec3, f: f32, axis: Option<usize>, basis: &[Vec3; 3]| -> Vec3 {
                    let rel = p - pivot;
                    match axis {
                        Some(a) => {
                            let ax = basis[a];
                            pivot + rel + ax * rel.dot(ax) * (f - 1.0)
                        }
                        None => pivot + rel * f,
                    }
                };
                if self.edit_xf.is_some() {
                    let (pivot, axis, basis) = (self.pivot, self.axis, self.basis);
                    self.apply_edit(scene, |p, w| scale_point(p, pivot, 1.0 + (factor - 1.0) * w, axis, &basis));
                    return;
                }
                let mut s = Vec3::splat(factor);
                if let Some(a) = self.axis {
                    s = Vec3::ONE;
                    s[a] = factor;
                }
                for (i, (id, original)) in self.objects.iter().enumerate() {
                    let pivot = self.pivots.get(i).copied().unwrap_or(self.pivot);
                    if let Some(o) = scene.object_mut(*id) {
                        let translation = scale_point(original.translation, pivot, factor, self.axis, &self.basis);
                        o.transform = Transform { translation, scale: original.scale * s, ..*original };
                    }
                }
            }
            ModalKind::Extrude | ModalKind::Inset | ModalKind::Bevel | ModalKind::ShrinkFatten => {
                let Some((mesh_id, base, sel)) = &self.edit else { return };
                let amount = -delta.y * world_per_pixel;
                self.amount = amount;
                let faces: Vec<FaceId> = sel.faces.iter().copied().collect();
                let edges: Vec<shape_geom::EdgeId> = sel.edges.iter().copied().collect();
                let result = match self.kind {
                    ModalKind::Extrude => shape_ops::extrude_faces(base, &faces, amount),
                    ModalKind::Bevel => shape_ops::bevel_edges_segments(base, &edges, amount.max(0.0), self.segments),
                    ModalKind::ShrinkFatten => shape_ops::shrink_fatten_vertices(base, &sel.verts, amount),
                    _ => shape_ops::inset_faces(base, &faces, amount.max(0.0)),
                };
                if let Ok(m) = result
                    && let Some(data) = scene.mesh_mut(*mesh_id)
                {
                    data.set_mesh(m);
                }
            }
        }
    }

    /// Wheel during a bevel: one segment per notch.
    pub fn scroll(&mut self, notches: f32) {
        if self.kind == ModalKind::Bevel && notches != 0.0 {
            let n = self.segments as i64 + if notches > 0.0 { 1 } else { -1 };
            self.segments = n.clamp(1, 32) as u32;
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
            ModalKind::Move => format!(
                "Move{axis}  {:.3}{}  ·  click/Enter confirm, Esc cancel, X/Y/Z axis, Ctrl snap{}",
                self.amount,
                if self.snap { "  [snap]" } else { "" },
                if self.settings.proportional && self.edit_xf.is_some() {
                    format!(", proportional {:.2} (wheel)", self.settings.proportional_size)
                } else {
                    String::new()
                }
            ),
            ModalKind::Rotate => format!("Rotate{axis}  {:.1}°  ·  click/Enter confirm, Esc cancel, X/Y/Z axis", self.amount),
            ModalKind::Scale => format!("Scale{axis}  ×{:.3}  ·  click/Enter confirm, Esc cancel, X/Y/Z axis", self.amount),
            ModalKind::Extrude => format!("Extrude  {:.3}  ·  move pointer up/down, click/Enter confirm, Esc cancel", self.amount),
            ModalKind::Inset => format!("Inset  {:.3}  ·  move pointer up/down, click/Enter confirm, Esc cancel", self.amount),
            ModalKind::Bevel => format!("Bevel  {:.3}  segments {}  ·  wheel segments, click/Enter confirm, Esc cancel", self.amount, self.segments),
            ModalKind::ShrinkFatten => format!("Shrink/Fatten  {:.3}  ·  move pointer up/down, click/Enter confirm, Esc cancel", self.amount),
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

#[cfg(test)]
mod p5_tests {
    use super::*;
    use shape_geom::primitives;

    #[test]
    fn edit_move_with_proportional_falloff_and_snap() {
        let mut scene = Scene::new("t");
        let id = scene.add_mesh_object("Cube", primitives::cube(2.0), None);
        scene.selection.set(id);
        let mesh = scene.object_mesh(id).unwrap().1.clone();
        let mut e = shape_scene::EditSelection::new(id);
        e.mode = shape_scene::SelectMode::Vertex;
        e.verts = mesh.vertex_ids().filter(|&v| mesh.position(v).y > 0.0).collect();
        e.flush(&mesh);
        scene.edit = Some(e);
        let cam = OrbitCamera { yaw: 0.0, pitch: 0.0, distance: 10.0, ..Default::default() };
        let size = Vec2::new(800.0, 600.0);
        // Without proportional: only the top four move.
        let settings = TransformSettings::default();
        let mut m = Modal::edit_transform(ModalKind::Move, &scene, size * 0.5, settings, Some(&cam)).unwrap();
        m.set_axis(0);
        m.update(&mut scene, &cam, size * 0.5 + Vec2::new(100.0, 0.0), size);
        let moved = scene.object_mesh(id).unwrap().1.clone();
        let dx: Vec<f32> = moved.vertex_ids().map(|v| moved.position(v).x - mesh.position(v).x).collect();
        assert!(dx.iter().zip(mesh.vertex_ids()).all(|(d, v)| if mesh.position(v).y > 0.0 { *d > 0.5 } else { d.abs() < 1e-6 }));
        // With proportional (radius larger than the cube): the bottom moves too, less.
        scene = m.before.clone();
        let settings = TransformSettings { proportional: true, proportional_size: 3.0, ..Default::default() };
        let mut m = Modal::edit_transform(ModalKind::Move, &scene, size * 0.5, settings, Some(&cam)).unwrap();
        m.set_axis(0);
        m.update(&mut scene, &cam, size * 0.5 + Vec2::new(100.0, 0.0), size);
        let moved = scene.object_mesh(id).unwrap().1.clone();
        let (mut top, mut bottom) = (0.0f32, 0.0f32);
        for v in mesh.vertex_ids() {
            let d = moved.position(v).x - mesh.position(v).x;
            if mesh.position(v).y > 0.0 { top = d } else { bottom = d }
        }
        assert!(bottom > 0.0 && bottom < top, "bottom {bottom} top {top}");
        // Snap: the offset lands on whole units.
        scene = m.before.clone();
        let mut m = Modal::edit_transform(ModalKind::Move, &scene, size * 0.5, TransformSettings::default(), Some(&cam)).unwrap();
        m.snap = true;
        m.set_axis(0);
        m.update(&mut scene, &cam, size * 0.5 + Vec2::new(123.0, 0.0), size);
        assert!((m.amount - m.amount.round()).abs() < 1e-5, "{}", m.amount);
    }

    #[test]
    fn individual_origins_rotate_each_object_in_place() {
        let mut scene = Scene::new("t");
        let a = scene.add_mesh_object("A", primitives::cube(1.0), None);
        let b = scene.add_mesh_object("B", primitives::cube(1.0), None);
        scene.object_mut(b).unwrap().transform.translation = Vec3::new(4.0, 0.0, 0.0);
        scene.selection.set(a);
        scene.selection.toggle(b);
        let cam = OrbitCamera { yaw: 0.0, pitch: 0.5, distance: 10.0, ..Default::default() };
        let size = Vec2::new(800.0, 600.0);
        let settings = TransformSettings { pivot: PivotPoint::IndividualOrigins, ..Default::default() };
        let mut m = Modal::transform_with(ModalKind::Rotate, &scene, size * 0.5 + Vec2::new(100.0, 0.0), false, settings, Some(&cam)).unwrap();
        m.set_axis(1);
        m.update(&mut scene, &cam, size * 0.5 + Vec2::new(0.0, 100.0), size);
        assert_eq!(scene.object(b).unwrap().transform.translation, Vec3::new(4.0, 0.0, 0.0), "each object spins about its own origin");
        assert!(scene.object(b).unwrap().transform.rotation.angle_between(Quat::IDENTITY) > 0.5);
    }
}
