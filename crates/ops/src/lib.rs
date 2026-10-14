//! Modeling operators and the modifier stack.
//!
//! Operators take a [`Mesh`] and return a new one; the half-edge structure is rebuilt from polygon
//! lists at the end of each operator, which keeps every operator a plain function over
//! `(positions, polygons)` and guarantees the result is a consistent mesh or an error.
//!
//! Non-destructive edits live in the scene as [`Modifier`] values; [`evaluate`] applies a stack.

pub mod modifiers;
pub mod subdivide;
pub mod uv;

pub use modifiers::{evaluate, evaluated_mesh, scene_mesh};
pub use subdivide::{catmull_clark, subdivide_linear};
pub use uv::{uv_box, uv_planar, uv_sphere};

use shape_geom::{EdgeId, FaceId, Mesh, MeshError, VertexId};
use shape_math::Vec3;
use std::collections::{BTreeSet, HashMap};

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-ops";

pub type Result<T> = std::result::Result<T, MeshError>;

/// Mutable polygon-list form of a mesh, the working representation of every operator.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PolyMesh {
    pub positions: Vec<Vec3>,
    pub polygons: Vec<Vec<u32>>,
}

impl PolyMesh {
    pub fn from_mesh(mesh: &Mesh) -> Self {
        let (positions, polygons) = mesh.to_polygons();
        Self { positions, polygons }
    }

    pub fn build(self) -> Result<Mesh> {
        Mesh::from_polygons(self.positions, &self.polygons)
    }

    pub fn push_vertex(&mut self, p: Vec3) -> u32 {
        self.positions.push(p);
        self.positions.len() as u32 - 1
    }

    /// Append another polygon mesh, re-indexing its polygons.
    pub fn append(&mut self, other: &PolyMesh) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.polygons.extend(other.polygons.iter().map(|p| p.iter().map(|&i| i + base).collect()));
    }

    /// Merge vertices closer than `distance`, dropping polygons that collapse below three sides.
    pub fn merge_by_distance(&mut self, distance: f32) {
        let q = 1.0 / distance.max(1e-9);
        let mut key_to_index: HashMap<[i64; 3], u32> = HashMap::new();
        let mut remap = Vec::with_capacity(self.positions.len());
        let mut positions = Vec::new();
        for &p in &self.positions {
            let key = [(p.x * q).round() as i64, (p.y * q).round() as i64, (p.z * q).round() as i64];
            let idx = *key_to_index.entry(key).or_insert_with(|| {
                positions.push(p);
                positions.len() as u32 - 1
            });
            remap.push(idx);
        }
        let mut polygons = Vec::with_capacity(self.polygons.len());
        for poly in &self.polygons {
            let mut q: Vec<u32> = poly.iter().map(|&i| remap[i as usize]).collect();
            q.dedup();
            while q.len() > 1 && q.first() == q.last() {
                q.pop();
            }
            if q.len() >= 3 {
                polygons.push(q);
            }
        }
        self.positions = positions;
        self.polygons = polygons;
    }

    pub fn polygon_normal(&self, poly: &[u32]) -> Vec3 {
        let mut n = Vec3::ZERO;
        for i in 0..poly.len() {
            let a = self.positions[poly[i] as usize];
            let b = self.positions[poly[(i + 1) % poly.len()] as usize];
            n += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
        }
        n.normalize_or_zero()
    }

    pub fn polygon_centroid(&self, poly: &[u32]) -> Vec3 {
        poly.iter().map(|&i| self.positions[i as usize]).sum::<Vec3>() / poly.len() as f32
    }
}

/// Every face of a mesh, for operators that take a face set.
pub fn all_faces(mesh: &Mesh) -> Vec<FaceId> {
    mesh.face_ids().collect()
}

/// Extrude a region of faces along their averaged normals by `distance`.
///
/// Faces inside the region move together; side quads are added along the region's boundary (edges
/// whose other face is outside the region or missing). Extruding every face of a closed mesh is a
/// uniform inflate; extruding one face of a cube adds four sides.
pub fn extrude_faces(mesh: &Mesh, faces: &[FaceId], distance: f32) -> Result<Mesh> {
    let region: BTreeSet<FaceId> = faces.iter().copied().collect();
    let mut pm = PolyMesh::from_mesh(mesh);

    // Average region normal per vertex.
    let mut normal_sum: HashMap<u32, Vec3> = HashMap::new();
    for &f in &region {
        let n = mesh.face_normal(f);
        for v in mesh.face_vertices(f) {
            *normal_sum.entry(v.0).or_default() += n;
        }
    }
    let mut new_index: HashMap<u32, u32> = HashMap::new();
    for (&v, n) in &normal_sum {
        let p = pm.positions[v as usize] + n.normalize_or_zero() * distance;
        let id = pm.push_vertex(p);
        new_index.insert(v, id);
    }
    // Rewrite region faces onto the new vertices.
    for &f in &region {
        let poly = &mut pm.polygons[f.index()];
        for v in poly.iter_mut() {
            *v = new_index[v];
        }
    }
    // Sides along the region boundary.
    let mut sides = Vec::new();
    for &f in &region {
        for h in mesh.face_halfedges(f) {
            let twin = mesh.halfedge(h).twin;
            let outside = match mesh.halfedge(twin).face {
                Some(other) => !region.contains(&other),
                None => true,
            };
            if outside {
                let a = mesh.from(h).0;
                let b = mesh.halfedge(h).to.0;
                sides.push(vec![a, b, new_index[&b], new_index[&a]]);
            }
        }
    }
    pm.polygons.extend(sides);
    pm.build()
}

/// Inset each face individually: a smaller copy of the face inside it, joined by a ring of quads.
pub fn inset_faces(mesh: &Mesh, faces: &[FaceId], thickness: f32) -> Result<Mesh> {
    let mut pm = PolyMesh::from_mesh(mesh);
    let mut extra = Vec::new();
    for &f in faces {
        let poly = pm.polygons[f.index()].clone();
        let c = pm.polygon_centroid(&poly);
        let inner: Vec<u32> = poly
            .iter()
            .map(|&v| {
                let p = pm.positions[v as usize];
                let to_c = c - p;
                let d = to_c.length();
                let step = thickness.min(d * 0.9);
                pm.push_vertex(p + to_c.normalize_or_zero() * step)
            })
            .collect();
        for i in 0..poly.len() {
            let j = (i + 1) % poly.len();
            extra.push(vec![poly[i], poly[j], inner[j], inner[i]]);
        }
        pm.polygons[f.index()] = inner;
    }
    pm.polygons.extend(extra);
    pm.build()
}

/// Move every vertex by `offset` along its normal (shrink/fatten).
pub fn shrink_fatten(mesh: &Mesh, offset: f32) -> Result<Mesh> {
    let normals = mesh.vertex_normals();
    let mut pm = PolyMesh::from_mesh(mesh);
    for (p, n) in pm.positions.iter_mut().zip(normals) {
        *p += n * offset;
    }
    pm.build()
}

/// Laplacian smoothing: each vertex moves `factor` of the way to the average of its neighbours,
/// `iterations` times. Boundary vertices stay put so open meshes keep their outline.
pub fn smooth(mesh: &Mesh, iterations: u32, factor: f32) -> Result<Mesh> {
    let mut pm = PolyMesh::from_mesh(mesh);
    let neighbors: Vec<Vec<usize>> = mesh.vertex_ids().map(|v| mesh.vertex_neighbors(v).map(|n| n.index()).collect()).collect();
    let fixed: Vec<bool> = mesh.vertex_ids().map(|v| mesh.is_boundary_vertex(v)).collect();
    for _ in 0..iterations {
        let prev = pm.positions.clone();
        for (i, p) in pm.positions.iter_mut().enumerate() {
            if fixed[i] || neighbors[i].is_empty() {
                continue;
            }
            let avg = neighbors[i].iter().map(|&n| prev[n]).sum::<Vec3>() / neighbors[i].len() as f32;
            *p = prev[i].lerp(avg, factor);
        }
    }
    pm.build()
}

/// Cut across a ring of quads starting at `edge`: every edge in the ring is split at its midpoint
/// and every quad in the ring becomes two. The ring stops at a non-quad face or a boundary; on a
/// closed ring (a cylinder's side) it wraps.
pub fn loop_cut(mesh: &Mesh, edge: EdgeId) -> Result<Mesh> {
    // Collect the ring as edges (unordered pairs) by walking across quads in both directions.
    let mut ring: BTreeSet<(u32, u32)> = BTreeSet::new();
    let key = |a: VertexId, b: VertexId| if a.0 < b.0 { (a.0, b.0) } else { (b.0, a.0) };
    let start = mesh.edge(edge).halfedge;
    for &dir in &[start, mesh.halfedge(start).twin] {
        let mut h = dir;
        loop {
            let (a, b) = (mesh.from(h), mesh.halfedge(h).to);
            if !ring.insert(key(a, b)) {
                break; // wrapped around
            }
            let Some(f) = mesh.halfedge(h).face else { break };
            if mesh.face_sides(f) != 4 {
                break;
            }
            // opposite edge of the quad: next.next
            let opposite = mesh.halfedge(mesh.halfedge(h).next).next;
            h = mesh.halfedge(opposite).twin;
        }
    }
    let mut pm = PolyMesh::from_mesh(mesh);
    let mut midpoint: HashMap<(u32, u32), u32> = HashMap::new();
    for &(a, b) in &ring {
        let m = (pm.positions[a as usize] + pm.positions[b as usize]) * 0.5;
        let id = pm.push_vertex(m);
        midpoint.insert((a, b), id);
    }
    let k = |a: u32, b: u32| if a < b { (a, b) } else { (b, a) };
    let mut out = Vec::with_capacity(pm.polygons.len() + ring.len());
    for poly in &pm.polygons {
        // Insert midpoints into the polygon's cycle.
        let mut cut_positions = Vec::new();
        let mut with_mids = Vec::new();
        for i in 0..poly.len() {
            let a = poly[i];
            let b = poly[(i + 1) % poly.len()];
            with_mids.push(a);
            if let Some(&m) = midpoint.get(&k(a, b)) {
                cut_positions.push(with_mids.len());
                with_mids.push(m);
            }
        }
        if poly.len() == 4 && cut_positions.len() == 2 {
            // Split into two polygons along the two midpoints.
            let (i, j) = (cut_positions[0], cut_positions[1]);
            let first: Vec<u32> = (i..=j).map(|x| with_mids[x]).collect();
            let mut second: Vec<u32> = (j..with_mids.len()).map(|x| with_mids[x]).collect();
            second.extend((0..=i).map(|x| with_mids[x]));
            out.push(first);
            out.push(second);
        } else {
            out.push(with_mids);
        }
    }
    pm.polygons = out;
    pm.build()
}

/// Mirror across a plane through the origin, perpendicular to `axis` (0 = X, 1 = Y, 2 = Z), and
/// merge vertices that land on the plane.
pub fn mirror(mesh: &Mesh, axis: usize, merge_distance: f32) -> Result<Mesh> {
    let mut pm = PolyMesh::from_mesh(mesh);
    let mut flipped = pm.clone();
    for p in &mut flipped.positions {
        p[axis] = -p[axis];
    }
    for poly in &mut flipped.polygons {
        poly.reverse();
    }
    pm.append(&flipped);
    if merge_distance > 0.0 {
        pm.merge_by_distance(merge_distance);
    }
    pm.build()
}

/// `count` copies, each shifted by `offset` more than the previous (the original is copy 0).
pub fn array(mesh: &Mesh, count: u32, offset: Vec3, merge_distance: f32) -> Result<Mesh> {
    let base = PolyMesh::from_mesh(mesh);
    let mut pm = base.clone();
    for i in 1..count.max(1) {
        let mut copy = base.clone();
        for p in &mut copy.positions {
            *p += offset * i as f32;
        }
        pm.append(&copy);
    }
    if merge_distance > 0.0 {
        pm.merge_by_distance(merge_distance);
    }
    pm.build()
}

/// Flip every face so normals point the other way.
pub fn flip_normals(mesh: &Mesh) -> Result<Mesh> {
    let mut pm = PolyMesh::from_mesh(mesh);
    for poly in &mut pm.polygons {
        poly.reverse();
    }
    pm.build()
}

/// Split every n-gon into triangles (fan).
pub fn triangulate(mesh: &Mesh) -> Result<Mesh> {
    let pm = PolyMesh::from_mesh(mesh);
    let polygons = mesh.triangles().into_iter().map(|t| vec![t[0].0, t[1].0, t[2].0]).collect();
    PolyMesh { positions: pm.positions, polygons }.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::{cube, cylinder, plane, uv_sphere};

    #[test]
    fn crate_name() {
        assert_eq!(CRATE, "shape-ops");
    }

    #[test]
    fn extrude_one_cube_face_adds_four_sides() {
        let c = cube(2.0);
        let top = c.face_ids().find(|&f| c.face_normal(f).y > 0.9).unwrap();
        let m = extrude_faces(&c, &[top], 1.0).unwrap();
        m.check().unwrap();
        assert!(m.is_closed());
        assert_eq!((m.vertex_count(), m.edge_count(), m.face_count()), (12, 20, 10));
        assert_eq!(m.euler_characteristic(), 2);
        assert!((m.aabb().max.y - 2.0).abs() < 1e-5);
    }

    #[test]
    fn extrude_whole_closed_mesh_inflates() {
        let s = uv_sphere(1.0, 12, 6);
        let m = extrude_faces(&s, &all_faces(&s), 0.5).unwrap();
        m.check().unwrap();
        assert_eq!(m.face_count(), s.face_count(), "no boundary, so no sides");
        assert!((m.aabb().max.y - 1.5).abs() < 1e-4);
    }

    #[test]
    fn extrude_open_plane_adds_skirt() {
        let p = plane(2.0, 2, 2);
        let m = extrude_faces(&p, &all_faces(&p), 1.0).unwrap();
        m.check().unwrap();
        assert_eq!(m.face_count(), 4 + 8);
        assert_eq!(m.boundary_loops().len(), 1);
    }

    #[test]
    fn inset_every_cube_face() {
        let c = cube(2.0);
        let m = inset_faces(&c, &all_faces(&c), 0.3).unwrap();
        m.check().unwrap();
        assert!(m.is_closed());
        assert_eq!(m.face_count(), 6 + 24);
        assert_eq!(m.euler_characteristic(), 2);
    }

    #[test]
    fn loop_cut_cylinder_wraps_and_cube_stops() {
        let cyl = cylinder(1.0, 2.0, 8);
        // a vertical side edge: both endpoints share x and z
        let e = cyl
            .edge_ids()
            .find(|&e| {
                let (a, b) = cyl.edge_vertices(e);
                let (pa, pb) = (cyl.position(a), cyl.position(b));
                (pa.x - pb.x).abs() < 1e-6 && (pa.z - pb.z).abs() < 1e-6
            })
            .unwrap();
        let m = loop_cut(&cyl, e).unwrap();
        m.check().unwrap();
        assert!(m.is_closed());
        assert_eq!(m.vertex_count(), 16 + 8);
        assert_eq!(m.face_count(), 16 + 2);
        assert_eq!(m.euler_characteristic(), 2);

        let c = cube(2.0);
        let m = loop_cut(&c, EdgeId(0)).unwrap();
        m.check().unwrap();
        assert_eq!(m.vertex_count(), 12);
        assert_eq!(m.face_count(), 10);
        assert_eq!(m.euler_characteristic(), 2);
    }

    #[test]
    fn mirror_and_array_merge() {
        let p = plane(2.0, 2, 2);
        let mut pm = PolyMesh::from_mesh(&p);
        for q in &mut pm.positions {
            q.x += 1.0; // plane spans x in 0..2
        }
        let shifted = pm.build().unwrap();
        let m = mirror(&shifted, 0, 1e-4).unwrap();
        m.check().unwrap();
        assert_eq!(m.face_count(), 8);
        assert_eq!(m.vertex_count(), 15, "the three seam vertices are shared");
        assert_eq!(m.boundary_loops().len(), 1);

        let a = array(&cube(1.0), 3, Vec3::new(2.0, 0.0, 0.0), 1e-4).unwrap();
        a.check().unwrap();
        assert_eq!(a.face_count(), 18);
        assert_eq!(a.vertex_count(), 24);
        assert!((a.aabb().max.x - 4.5).abs() < 1e-6);
        // Touching closed solids would share a face twice: that is not a manifold, so it is an error.
        assert!(array(&cube(1.0), 2, Vec3::new(1.0, 0.0, 0.0), 1e-4).is_err());
    }

    #[test]
    fn smooth_shrinks_a_cube_but_keeps_plane_outline() {
        let c = cube(2.0);
        let m = smooth(&c, 3, 0.5).unwrap();
        assert!(m.aabb().max.x < 1.0);
        let p = plane(2.0, 3, 3);
        let m = smooth(&p, 5, 0.5).unwrap();
        assert!((m.aabb().max.x - 1.0).abs() < 1e-6);
    }

    #[test]
    fn flip_and_triangulate_and_shrink() {
        let c = cube(2.0);
        let f = flip_normals(&c).unwrap();
        for face in f.face_ids() {
            assert!(f.face_normal(face).dot(f.face_centroid(face)) < 0.0);
        }
        let t = triangulate(&c).unwrap();
        assert_eq!(t.face_count(), 12);
        assert!(t.is_closed());
        let s = shrink_fatten(&c, 0.5).unwrap();
        assert!(s.aabb().max.x > 1.2);
    }
}
