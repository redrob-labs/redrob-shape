//! Object tree, materials, lights, cameras and selection.
//!
//! A [`Scene`] owns objects, meshes and materials in dense vectors with stable ids; removed slots
//! become tombstones so ids held by undo commands or the agent stay valid. Objects form a tree
//! through `parent`; an object's world transform is its parents' transforms composed with its own.

use serde::{Deserialize, Serialize};
use shape_geom::{EdgeId, FaceId, Mesh, VertexId};
use shape_math::{Aabb, Transform, Vec2, Vec3};
use std::collections::BTreeSet;

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-scene";

macro_rules! id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub u32);
        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id!(ObjectId);
id!(MeshId);
id!(MaterialId);

/// Physically based surface parameters, metal/rough workflow.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    /// Linear RGBA.
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    /// Linear RGB, added to the shaded colour.
    pub emissive: [f32; 3],
}

impl Default for Material {
    fn default() -> Self {
        Self { name: "Material".into(), base_color: [0.8, 0.8, 0.8, 1.0], metallic: 0.0, roughness: 0.5, emissive: [0.0; 3] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum LightKind {
    Point,
    /// Shines along the object's -Z.
    Sun,
    Spot {
        inner_angle: f32,
        outer_angle: f32,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Light {
    pub kind: LightKind,
    pub color: [f32; 3],
    /// Watts for point/spot, irradiance for sun.
    pub power: f32,
}

impl Default for Light {
    fn default() -> Self {
        Self { kind: LightKind::Point, color: [1.0; 3], power: 1000.0 }
    }
}

impl Light {
    pub fn point(power: f32) -> Self {
        Self { kind: LightKind::Point, power, ..Default::default() }
    }
    pub fn sun(strength: f32) -> Self {
        Self { kind: LightKind::Sun, power: strength, ..Default::default() }
    }
    pub fn spot(power: f32) -> Self {
        Self { kind: LightKind::Spot { inner_angle: 30f32.to_radians(), outer_angle: 40f32.to_radians() }, power, ..Default::default() }
    }
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            LightKind::Point => "Point",
            LightKind::Sun => "Sun",
            LightKind::Spot { .. } => "Spot",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    /// Vertical field of view in radians.
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

impl Default for Camera {
    fn default() -> Self {
        Self { fov_y: 50f32.to_radians(), near: 0.1, far: 1000.0 }
    }
}

/// A non-destructive edit on a mesh object, applied in stack order by `shape-ops`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Modifier {
    /// Mirror across the plane perpendicular to `axis` (0 X, 1 Y, 2 Z) through the object origin.
    Mirror {
        axis: u8,
        merge_distance: f32,
    },
    /// `count` copies spaced `offset` apart.
    Array {
        count: u32,
        offset: Vec3,
        merge_distance: f32,
    },
    /// Catmull-Clark (`smooth`) or linear subdivision, `levels` times.
    Subdivision {
        levels: u32,
        smooth: bool,
    },
    /// Laplacian smoothing.
    Smooth {
        iterations: u32,
        factor: f32,
    },
    /// Bevel every edge by `offset` in `segments` strips.
    Bevel {
        offset: f32,
        #[serde(default = "one")]
        segments: u32,
    },
    /// A second shell `thickness` away; `offset` -1 inward, 0 centred, 1 outward.
    Solidify {
        thickness: f32,
        offset: f32,
    },
    /// Collapse edges until `ratio` of the triangles remain.
    Decimate {
        ratio: f32,
    },
    /// Every edge becomes a strut of `thickness`.
    Wireframe {
        thickness: f32,
    },
    Triangulate,
    /// Revolve the mesh's edges around `axis` by `angle` radians in `steps`, lifting `screw` per turn.
    Screw {
        axis: u8,
        angle: f32,
        steps: u32,
        screw: f32,
    },
    /// Twist / bend (radians) or taper / stretch (factor) along `axis`.
    SimpleDeform {
        mode: DeformMode,
        axis: u8,
        factor: f32,
    },
    /// Noise along the normals.
    Displace {
        strength: f32,
        scale: f32,
    },
    /// Merge vertices closer than `distance`.
    Weld {
        distance: f32,
    },
    /// Union / difference / intersect with another mesh object (its base mesh, in this object's space).
    Boolean {
        op: BooleanOp,
        target: Option<ObjectId>,
    },
    /// Project every vertex onto another mesh object's surface, then `offset` away from it.
    Shrinkwrap {
        target: Option<ObjectId>,
        offset: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BooleanOp {
    Union,
    Difference,
    Intersect,
}

impl BooleanOp {
    pub const ALL: [BooleanOp; 3] = [BooleanOp::Union, BooleanOp::Difference, BooleanOp::Intersect];
    pub fn name(self) -> &'static str {
        match self {
            BooleanOp::Union => "Union",
            BooleanOp::Difference => "Difference",
            BooleanOp::Intersect => "Intersect",
        }
    }
}

fn one() -> u32 {
    1
}

/// Simple Deform modes (Blender's names).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeformMode {
    Twist,
    Bend,
    Taper,
    Stretch,
}

impl DeformMode {
    pub const ALL: [DeformMode; 4] = [DeformMode::Twist, DeformMode::Bend, DeformMode::Taper, DeformMode::Stretch];
    pub fn name(self) -> &'static str {
        match self {
            DeformMode::Twist => "Twist",
            DeformMode::Bend => "Bend",
            DeformMode::Taper => "Taper",
            DeformMode::Stretch => "Stretch",
        }
    }
}

impl Modifier {
    pub fn name(&self) -> &'static str {
        match self {
            Modifier::Mirror { .. } => "Mirror",
            Modifier::Array { .. } => "Array",
            Modifier::Subdivision { .. } => "Subdivision",
            Modifier::Smooth { .. } => "Smooth",
            Modifier::Bevel { .. } => "Bevel",
            Modifier::Solidify { .. } => "Solidify",
            Modifier::Decimate { .. } => "Decimate",
            Modifier::Wireframe { .. } => "Wireframe",
            Modifier::Triangulate => "Triangulate",
            Modifier::Screw { .. } => "Screw",
            Modifier::SimpleDeform { .. } => "Simple Deform",
            Modifier::Displace { .. } => "Displace",
            Modifier::Weld { .. } => "Weld",
            Modifier::Boolean { .. } => "Boolean",
            Modifier::Shrinkwrap { .. } => "Shrinkwrap",
        }
    }

    /// A modifier with nothing to do is skipped by the evaluator.
    pub fn enabled(&self) -> bool {
        match self {
            Modifier::Array { count, .. } => *count > 1,
            Modifier::Subdivision { levels, .. } => *levels > 0,
            Modifier::Smooth { iterations, .. } => *iterations > 0,
            Modifier::Bevel { offset, .. } => *offset > 0.0,
            Modifier::Solidify { thickness, .. } => thickness.abs() > 0.0,
            Modifier::Decimate { ratio } => *ratio < 1.0,
            Modifier::Wireframe { thickness } => *thickness > 0.0,
            Modifier::Screw { steps, .. } => *steps > 0,
            Modifier::Displace { strength, .. } => *strength != 0.0,
            Modifier::Weld { distance } => *distance > 0.0,
            Modifier::Boolean { target, .. } | Modifier::Shrinkwrap { target, .. } => target.is_some(),
            Modifier::Mirror { .. } | Modifier::Triangulate | Modifier::SimpleDeform { .. } => true,
        }
    }
}

/// What an object is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ObjectData {
    Empty,
    Mesh {
        mesh: MeshId,
        material: Option<MaterialId>,
        #[serde(default)]
        modifiers: Vec<Modifier>,
    },
    Light(Light),
    Camera(Camera),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Object {
    pub name: String,
    /// Relative to the parent.
    pub transform: Transform,
    pub parent: Option<ObjectId>,
    pub data: ObjectData,
    pub visible: bool,
}

impl Object {
    pub fn new(name: impl Into<String>, data: ObjectData) -> Self {
        Self { name: name.into(), transform: Transform::IDENTITY, parent: None, data, visible: true }
    }
}

/// Mesh geometry shared by any number of objects.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshData {
    pub name: String,
    pub mesh: Mesh,
    /// Texture coordinates per face corner, in `face_vertices` order, when the mesh has them.
    /// Cleared by every operator that rebuilds the mesh.
    #[serde(default)]
    pub uvs: Option<Vec<Vec<Vec2>>>,
}

impl MeshData {
    pub fn new(name: impl Into<String>, mesh: Mesh) -> Self {
        Self { name: name.into(), mesh, uvs: None }
    }

    /// Replace the mesh and drop UVs that no longer match it.
    pub fn set_mesh(&mut self, mesh: Mesh) {
        self.mesh = mesh;
        self.uvs = None;
    }
}

/// Selected objects plus the one that is active (last clicked).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Selection {
    pub objects: BTreeSet<ObjectId>,
    pub active: Option<ObjectId>,
}

impl Selection {
    pub fn clear(&mut self) {
        self.objects.clear();
        self.active = None;
    }
    pub fn set(&mut self, id: ObjectId) {
        self.objects.clear();
        self.objects.insert(id);
        self.active = Some(id);
    }
    pub fn toggle(&mut self, id: ObjectId) {
        if !self.objects.remove(&id) {
            self.objects.insert(id);
            self.active = Some(id);
        } else if self.active == Some(id) {
            self.active = self.objects.iter().next_back().copied();
        }
    }
    pub fn contains(&self, id: ObjectId) -> bool {
        self.objects.contains(&id)
    }
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }
}

/// Which element kind clicks pick in edit mode (Blender's 1 / 2 / 3).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SelectMode {
    Vertex,
    Edge,
    #[default]
    Face,
}

impl SelectMode {
    pub fn name(self) -> &'static str {
        match self {
            SelectMode::Vertex => "Vertex",
            SelectMode::Edge => "Edge",
            SelectMode::Face => "Face",
        }
    }
}

/// Edit mode: one mesh object whose vertices, edges and faces are being selected and edited.
///
/// The three sets are kept consistent by [`EditSelection::flush`], the way Blender flushes a
/// selection: in vertex mode an edge is selected when both ends are, a face when all its
/// vertices are; in edge mode a face is selected when all its edges are; in face mode the vertices
/// and edges of the selected faces are selected. Operators read whichever set they need.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditSelection {
    pub object: ObjectId,
    pub mode: SelectMode,
    pub verts: BTreeSet<VertexId>,
    pub edges: BTreeSet<EdgeId>,
    pub faces: BTreeSet<FaceId>,
}

impl EditSelection {
    pub fn new(object: ObjectId) -> Self {
        Self { object, mode: SelectMode::Face, verts: BTreeSet::new(), edges: BTreeSet::new(), faces: BTreeSet::new() }
    }

    pub fn is_empty(&self) -> bool {
        match self.mode {
            SelectMode::Vertex => self.verts.is_empty(),
            SelectMode::Edge => self.edges.is_empty(),
            SelectMode::Face => self.faces.is_empty(),
        }
    }

    pub fn clear(&mut self) {
        self.verts.clear();
        self.edges.clear();
        self.faces.clear();
    }

    /// Switch mode, converting the selection the way Blender does (down: everything the
    /// elements touch; up: only elements fully selected).
    pub fn set_mode(&mut self, mode: SelectMode, mesh: &Mesh) {
        if mode == self.mode {
            return;
        }
        self.flush(mesh);
        self.mode = mode;
        self.flush(mesh);
    }

    /// Recompute the two derived sets from the one the current mode owns.
    pub fn flush(&mut self, mesh: &Mesh) {
        match self.mode {
            SelectMode::Vertex => {
                self.verts.retain(|v| v.index() < mesh.vertex_count());
                self.edges = mesh
                    .edge_ids()
                    .filter(|&e| {
                        let (a, b) = mesh.edge_vertices(e);
                        self.verts.contains(&a) && self.verts.contains(&b)
                    })
                    .collect();
                self.faces = mesh.face_ids().filter(|&f| mesh.face_vertices(f).all(|v| self.verts.contains(&v))).collect();
            }
            SelectMode::Edge => {
                self.edges.retain(|e| e.index() < mesh.edge_count());
                self.verts = self
                    .edges
                    .iter()
                    .flat_map(|&e| {
                        let (a, b) = mesh.edge_vertices(e);
                        [a, b]
                    })
                    .collect();
                self.faces = mesh.face_ids().filter(|&f| mesh.face_halfedges(f).all(|h| self.edges.contains(&mesh.halfedge(h).edge))).collect();
            }
            SelectMode::Face => {
                self.faces.retain(|f| f.index() < mesh.face_count());
                self.verts = self.faces.iter().flat_map(|&f| mesh.face_vertices(f)).collect();
                self.edges = self.faces.iter().flat_map(|&f| mesh.face_halfedges(f).map(|h| mesh.halfedge(h).edge)).collect();
            }
        }
    }

    /// Select everything of the current mode's kind.
    pub fn select_all(&mut self, mesh: &Mesh) {
        match self.mode {
            SelectMode::Vertex => self.verts = mesh.vertex_ids().collect(),
            SelectMode::Edge => self.edges = mesh.edge_ids().collect(),
            SelectMode::Face => self.faces = mesh.face_ids().collect(),
        }
        self.flush(mesh);
    }

    /// Everything of the current kind selected?
    pub fn is_all(&self, mesh: &Mesh) -> bool {
        match self.mode {
            SelectMode::Vertex => self.verts.len() == mesh.vertex_count(),
            SelectMode::Edge => self.edges.len() == mesh.edge_count(),
            SelectMode::Face => self.faces.len() == mesh.face_count(),
        }
    }

    pub fn invert(&mut self, mesh: &Mesh) {
        match self.mode {
            SelectMode::Vertex => self.verts = mesh.vertex_ids().filter(|v| !self.verts.contains(v)).collect(),
            SelectMode::Edge => self.edges = mesh.edge_ids().filter(|e| !self.edges.contains(e)).collect(),
            SelectMode::Face => self.faces = mesh.face_ids().filter(|f| !self.faces.contains(f)).collect(),
        }
        self.flush(mesh);
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub name: String,
    objects: Vec<Option<Object>>,
    meshes: Vec<Option<MeshData>>,
    materials: Vec<Option<Material>>,
    pub selection: Selection,
    /// `Some` while in edit mode. Not saved: a file always opens in object mode.
    #[serde(skip)]
    pub edit: Option<EditSelection>,
}

impl Scene {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Default::default() }
    }

    // ---- objects ----

    pub fn add_object(&mut self, object: Object) -> ObjectId {
        let id = ObjectId(self.objects.len() as u32);
        self.objects.push(Some(object));
        id
    }

    /// Convenience: a mesh object at the origin with its own mesh data.
    pub fn add_mesh_object(&mut self, name: impl Into<String>, mesh: Mesh, material: Option<MaterialId>) -> ObjectId {
        let name = name.into();
        let mesh = self.add_mesh(MeshData::new(name.clone(), mesh));
        self.add_object(Object::new(name, ObjectData::Mesh { mesh, material, modifiers: Vec::new() }))
    }

    /// Remove an object and reparent its children to its parent. Its mesh data stays.
    pub fn remove_object(&mut self, id: ObjectId) -> Option<Object> {
        let removed = self.objects.get_mut(id.index())?.take()?;
        for child in self.objects.iter_mut().flatten() {
            if child.parent == Some(id) {
                child.parent = removed.parent;
            }
        }
        self.selection.objects.remove(&id);
        if self.selection.active == Some(id) {
            self.selection.active = None;
        }
        if self.edit.as_ref().is_some_and(|e| e.object == id) {
            self.edit = None;
        }
        Some(removed)
    }

    /// Put an object back in its old slot (undo of remove). The slot must be empty.
    pub fn restore_object(&mut self, id: ObjectId, object: Object) {
        if id.index() >= self.objects.len() {
            self.objects.resize(id.index() + 1, None);
        }
        debug_assert!(self.objects[id.index()].is_none());
        self.objects[id.index()] = Some(object);
    }

    pub fn object(&self, id: ObjectId) -> Option<&Object> {
        self.objects.get(id.index()).and_then(Option::as_ref)
    }
    pub fn object_mut(&mut self, id: ObjectId) -> Option<&mut Object> {
        self.objects.get_mut(id.index()).and_then(Option::as_mut)
    }
    pub fn objects(&self) -> impl Iterator<Item = (ObjectId, &Object)> {
        self.objects.iter().enumerate().filter_map(|(i, o)| o.as_ref().map(|o| (ObjectId(i as u32), o)))
    }
    pub fn object_count(&self) -> usize {
        self.objects.iter().flatten().count()
    }
    pub fn children(&self, parent: Option<ObjectId>) -> impl Iterator<Item = ObjectId> + '_ {
        self.objects().filter(move |(_, o)| o.parent == parent).map(|(id, _)| id)
    }
    pub fn roots(&self) -> impl Iterator<Item = ObjectId> + '_ {
        self.children(None)
    }

    /// Set the parent, refusing a cycle. Returns false when `parent` is `id` or a descendant.
    pub fn set_parent(&mut self, id: ObjectId, parent: Option<ObjectId>) -> bool {
        let mut cur = parent;
        while let Some(p) = cur {
            if p == id {
                return false;
            }
            cur = self.object(p).and_then(|o| o.parent);
        }
        match self.object_mut(id) {
            Some(o) => {
                o.parent = parent;
                true
            }
            None => false,
        }
    }

    pub fn world_transform(&self, id: ObjectId) -> Transform {
        let Some(o) = self.object(id) else { return Transform::IDENTITY };
        match o.parent {
            Some(p) => self.world_transform(p).mul_transform(&o.transform),
            None => o.transform,
        }
    }

    /// Modifier stack of a mesh object (empty for anything else).
    pub fn modifiers(&self, id: ObjectId) -> &[Modifier] {
        match self.object(id).map(|o| &o.data) {
            Some(ObjectData::Mesh { modifiers, .. }) => modifiers,
            _ => &[],
        }
    }

    pub fn modifiers_mut(&mut self, id: ObjectId) -> Option<&mut Vec<Modifier>> {
        match self.object_mut(id).map(|o| &mut o.data) {
            Some(ObjectData::Mesh { modifiers, .. }) => Some(modifiers),
            _ => None,
        }
    }

    /// Base mesh of a mesh object (modifiers not applied), if it is one and its data exists.
    pub fn object_mesh(&self, id: ObjectId) -> Option<(MeshId, &Mesh)> {
        match &self.object(id)?.data {
            ObjectData::Mesh { mesh, .. } => Some((*mesh, &self.mesh(*mesh)?.mesh)),
            _ => None,
        }
    }

    /// World-space bounds of every visible mesh object.
    pub fn bounds(&self) -> Aabb {
        let mut b = Aabb::EMPTY;
        for (id, o) in self.objects() {
            if !o.visible {
                continue;
            }
            if let Some((_, mesh)) = self.object_mesh(id) {
                let t = self.world_transform(id);
                for p in mesh.positions() {
                    b = b.with_point(t.transform_point(p));
                }
            }
        }
        b
    }

    // ---- meshes ----

    pub fn add_mesh(&mut self, data: MeshData) -> MeshId {
        let id = MeshId(self.meshes.len() as u32);
        self.meshes.push(Some(data));
        id
    }
    pub fn mesh(&self, id: MeshId) -> Option<&MeshData> {
        self.meshes.get(id.index()).and_then(Option::as_ref)
    }
    pub fn mesh_mut(&mut self, id: MeshId) -> Option<&mut MeshData> {
        self.meshes.get_mut(id.index()).and_then(Option::as_mut)
    }
    pub fn meshes(&self) -> impl Iterator<Item = (MeshId, &MeshData)> {
        self.meshes.iter().enumerate().filter_map(|(i, m)| m.as_ref().map(|m| (MeshId(i as u32), m)))
    }
    /// Drop mesh data no object uses. Returns how many were removed.
    pub fn prune_meshes(&mut self) -> usize {
        let used: BTreeSet<MeshId> =
            self.objects().filter_map(|(_, o)| if let ObjectData::Mesh { mesh, .. } = o.data { Some(mesh) } else { None }).collect();
        let mut n = 0;
        for (i, m) in self.meshes.iter_mut().enumerate() {
            if m.is_some() && !used.contains(&MeshId(i as u32)) {
                *m = None;
                n += 1;
            }
        }
        n
    }

    // ---- materials ----

    pub fn add_material(&mut self, material: Material) -> MaterialId {
        let id = MaterialId(self.materials.len() as u32);
        self.materials.push(Some(material));
        id
    }
    pub fn material(&self, id: MaterialId) -> Option<&Material> {
        self.materials.get(id.index()).and_then(Option::as_ref)
    }
    pub fn material_mut(&mut self, id: MaterialId) -> Option<&mut Material> {
        self.materials.get_mut(id.index()).and_then(Option::as_mut)
    }
    pub fn materials(&self) -> impl Iterator<Item = (MaterialId, &Material)> {
        self.materials.iter().enumerate().filter_map(|(i, m)| m.as_ref().map(|m| (MaterialId(i as u32), m)))
    }

    /// Centre of the selection's world bounds, or the origin.
    pub fn selection_center(&self) -> Vec3 {
        let mut b = Aabb::EMPTY;
        for &id in &self.selection.objects {
            b = b.with_point(self.world_transform(id).translation);
        }
        if b.is_empty() { Vec3::ZERO } else { b.center() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::cube;
    use shape_math::Quat;

    #[test]
    fn crate_name() {
        assert_eq!(CRATE, "shape-scene");
    }

    #[test]
    fn tree_and_world_transforms() {
        let mut s = Scene::new("test");
        let parent = s.add_object(Object::new("parent", ObjectData::Empty));
        s.object_mut(parent).unwrap().transform = Transform::from_translation(Vec3::new(1.0, 0.0, 0.0));
        let child = s.add_mesh_object("child", cube(1.0), None);
        assert!(s.set_parent(child, Some(parent)));
        assert!(!s.set_parent(parent, Some(child)), "cycle refused");
        s.object_mut(child).unwrap().transform = Transform { translation: Vec3::Y, rotation: Quat::from_rotation_y(1.0), scale: Vec3::splat(2.0) };
        let w = s.world_transform(child);
        assert!((w.translation - Vec3::new(1.0, 1.0, 0.0)).length() < 1e-6);
        assert_eq!(w.scale, Vec3::splat(2.0));
        assert_eq!(s.roots().collect::<Vec<_>>(), vec![parent]);
        assert_eq!(s.children(Some(parent)).collect::<Vec<_>>(), vec![child]);
        let b = s.bounds();
        // unit half-extent (cube 1.0 scaled 2) rotated 1 rad about Y: reach = cos 1 + sin 1
        let r = 1f32.cos() + 1f32.sin();
        assert!((b.min - Vec3::new(1.0 - r, 0.0, -r)).length() < 1e-4, "{b:?}");
        assert!((b.max - Vec3::new(1.0 + r, 2.0, r)).length() < 1e-4, "{b:?}");
    }

    #[test]
    fn remove_reparents_children_and_restore_undoes() {
        let mut s = Scene::new("test");
        let a = s.add_object(Object::new("a", ObjectData::Empty));
        let b = s.add_object(Object::new("b", ObjectData::Empty));
        let c = s.add_object(Object::new("c", ObjectData::Empty));
        s.set_parent(b, Some(a));
        s.set_parent(c, Some(b));
        s.selection.set(b);
        let removed = s.remove_object(b).unwrap();
        assert_eq!(s.object(c).unwrap().parent, Some(a));
        assert!(s.selection.is_empty());
        assert_eq!(s.object_count(), 2);
        s.restore_object(b, removed);
        assert_eq!(s.object(b).unwrap().name, "b");
        assert_eq!(s.object_count(), 3);
    }

    #[test]
    fn selection_toggle_tracks_active() {
        let mut sel = Selection::default();
        let (a, b) = (ObjectId(0), ObjectId(1));
        sel.toggle(a);
        sel.toggle(b);
        assert_eq!(sel.active, Some(b));
        sel.toggle(b);
        assert_eq!(sel.active, Some(a));
        assert!(sel.contains(a) && !sel.contains(b));
        sel.clear();
        assert!(sel.is_empty() && sel.active.is_none());
    }

    #[test]
    fn prune_drops_unused_meshes() {
        let mut s = Scene::new("test");
        let o = s.add_mesh_object("used", cube(1.0), None);
        s.add_mesh(MeshData::new("orphan", cube(2.0)));
        assert_eq!(s.prune_meshes(), 1);
        assert_eq!(s.meshes().count(), 1);
        assert!(s.object_mesh(o).is_some());
    }
}
