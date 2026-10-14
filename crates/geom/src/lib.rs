//! Half-edge mesh, BVH and topology queries.
//!
//! [`Mesh`] is the one mesh type every layer edits. It is a half-edge structure: each edge is two
//! directed half-edges, each half-edge knows its twin, the next half-edge around its face and the
//! face it borders (`None` on a boundary). Polygons may have any number of sides; triangulation is a
//! view ([`Mesh::triangles`]) and never mutates the mesh.
//!
//! Ids are plain indices into dense vectors. Deleting elements is a later stage's job (ops keep the
//! structure compact by rebuilding); stage 1 only builds and queries.

pub mod bvh;
pub mod primitives;

use serde::{Deserialize, Serialize};
pub use shape_math::{Aabb, Ray, Vec3};

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-geom";

macro_rules! id {
    ($name:ident) => {
        #[doc = concat!("Index of a ", stringify!($name), " in its mesh.")]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(pub u32);

        impl $name {
            pub fn index(self) -> usize {
                self.0 as usize
            }
        }
    };
}

id!(VertexId);
id!(HalfEdgeId);
id!(EdgeId);
id!(FaceId);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Vertex {
    pub position: Vec3,
    /// One outgoing half-edge; a boundary half-edge when the vertex lies on a boundary.
    pub halfedge: Option<HalfEdgeId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HalfEdge {
    /// Vertex this half-edge points to.
    pub to: VertexId,
    pub twin: HalfEdgeId,
    pub next: HalfEdgeId,
    pub prev: HalfEdgeId,
    pub edge: EdgeId,
    /// `None` when this half-edge runs along the outside of a boundary.
    pub face: Option<FaceId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub halfedge: HalfEdgeId,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Face {
    pub halfedge: HalfEdgeId,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum MeshError {
    #[error("face {face} has only {sides} vertices")]
    DegenerateFace { face: usize, sides: usize },
    #[error("face {face} references vertex {vertex} but the mesh has {count}")]
    VertexOutOfRange { face: usize, vertex: u32, count: usize },
    #[error("directed edge {from}->{to} appears in two faces: the mesh is not orientable or has a duplicate face")]
    NonManifoldEdge { from: u32, to: u32 },
    #[error("face {face} repeats vertex {vertex}")]
    RepeatedVertex { face: usize, vertex: u32 },
}

/// A polygon mesh with half-edge connectivity.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Mesh {
    vertices: Vec<Vertex>,
    halfedges: Vec<HalfEdge>,
    edges: Vec<Edge>,
    faces: Vec<Face>,
}

impl Mesh {
    /// Build connectivity from positions and counter-clockwise polygons.
    ///
    /// Open meshes are fine: edges used by one face get a boundary twin with `face == None`, and
    /// those boundary half-edges are linked into loops so [`Mesh::boundary_loops`] can walk them.
    pub fn from_polygons(positions: Vec<Vec3>, polygons: &[Vec<u32>]) -> Result<Self, MeshError> {
        use std::collections::HashMap;

        let vcount = positions.len();
        let mut mesh = Mesh { vertices: positions.into_iter().map(|position| Vertex { position, halfedge: None }).collect(), ..Default::default() };
        let mut directed: HashMap<(u32, u32), HalfEdgeId> = HashMap::new();

        for (fi, poly) in polygons.iter().enumerate() {
            if poly.len() < 3 {
                return Err(MeshError::DegenerateFace { face: fi, sides: poly.len() });
            }
            for &v in poly {
                if v as usize >= vcount {
                    return Err(MeshError::VertexOutOfRange { face: fi, vertex: v, count: vcount });
                }
            }
            for (i, &v) in poly.iter().enumerate() {
                if poly[i + 1..].contains(&v) {
                    return Err(MeshError::RepeatedVertex { face: fi, vertex: v });
                }
            }
            let face = FaceId(mesh.faces.len() as u32);
            let first = mesh.halfedges.len() as u32;
            let n = poly.len() as u32;
            for i in 0..n {
                let from = poly[i as usize];
                let to = poly[((i + 1) % n) as usize];
                let id = HalfEdgeId(first + i);
                if directed.insert((from, to), id).is_some() {
                    return Err(MeshError::NonManifoldEdge { from, to });
                }
                mesh.halfedges.push(HalfEdge {
                    to: VertexId(to),
                    twin: id, // patched below
                    next: HalfEdgeId(first + (i + 1) % n),
                    prev: HalfEdgeId(first + (i + n - 1) % n),
                    edge: EdgeId(u32::MAX), // patched below
                    face: Some(face),
                });
                mesh.vertices[from as usize].halfedge.get_or_insert(id);
            }
            mesh.faces.push(Face { halfedge: HalfEdgeId(first) });
        }

        // Pair twins; create boundary half-edges for unmatched directed edges.
        let interior = mesh.halfedges.len();
        let mut boundary_out: HashMap<u32, HalfEdgeId> = HashMap::new(); // from-vertex -> boundary he
        for i in 0..interior {
            let he = HalfEdgeId(i as u32);
            // Twins are not paired yet, so take the start vertex from the previous half-edge.
            let from = mesh.halfedges[mesh.halfedges[i].prev.index()].to.0;
            let to = mesh.halfedges[i].to.0;
            if mesh.halfedges[i].edge.0 != u32::MAX {
                continue;
            }
            let edge = EdgeId(mesh.edges.len() as u32);
            mesh.edges.push(Edge { halfedge: he });
            mesh.halfedges[i].edge = edge;
            if let Some(&twin) = directed.get(&(to, from)) {
                mesh.halfedges[i].twin = twin;
                mesh.halfedges[twin.index()].twin = he;
                mesh.halfedges[twin.index()].edge = edge;
            } else {
                let b = HalfEdgeId(mesh.halfedges.len() as u32);
                mesh.halfedges.push(HalfEdge { to: VertexId(from), twin: he, next: b, prev: b, edge, face: None });
                mesh.halfedges[i].twin = b;
                boundary_out.insert(to, b);
                // A boundary vertex must point along the boundary so vertex walks see the gap.
                mesh.vertices[to as usize].halfedge = Some(b);
            }
        }
        // Link boundary half-edges into loops: the boundary he leaving vertex `to` is `next`.
        for i in interior..mesh.halfedges.len() {
            let to = mesh.halfedges[i].to.0;
            let next = boundary_out[&to];
            mesh.halfedges[i].next = next;
            mesh.halfedges[next.index()].prev = HalfEdgeId(i as u32);
        }
        Ok(mesh)
    }

    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }
    pub fn face_count(&self) -> usize {
        self.faces.len()
    }
    pub fn halfedge_count(&self) -> usize {
        self.halfedges.len()
    }

    pub fn vertex(&self, v: VertexId) -> &Vertex {
        &self.vertices[v.index()]
    }
    pub fn halfedge(&self, h: HalfEdgeId) -> &HalfEdge {
        &self.halfedges[h.index()]
    }
    pub fn edge(&self, e: EdgeId) -> &Edge {
        &self.edges[e.index()]
    }
    pub fn face(&self, f: FaceId) -> &Face {
        &self.faces[f.index()]
    }

    pub fn position(&self, v: VertexId) -> Vec3 {
        self.vertices[v.index()].position
    }
    pub fn set_position(&mut self, v: VertexId, p: Vec3) {
        self.vertices[v.index()].position = p;
    }
    pub fn positions(&self) -> impl Iterator<Item = Vec3> + '_ {
        self.vertices.iter().map(|v| v.position)
    }

    pub fn vertex_ids(&self) -> impl Iterator<Item = VertexId> {
        (0..self.vertices.len() as u32).map(VertexId)
    }
    pub fn edge_ids(&self) -> impl Iterator<Item = EdgeId> {
        (0..self.edges.len() as u32).map(EdgeId)
    }
    pub fn face_ids(&self) -> impl Iterator<Item = FaceId> {
        (0..self.faces.len() as u32).map(FaceId)
    }

    /// Vertex a half-edge starts at.
    pub fn from(&self, h: HalfEdgeId) -> VertexId {
        self.halfedges[self.halfedges[h.index()].twin.index()].to
    }

    /// Both endpoints of an edge, in the direction of its stored half-edge.
    pub fn edge_vertices(&self, e: EdgeId) -> (VertexId, VertexId) {
        let h = self.edges[e.index()].halfedge;
        (self.from(h), self.halfedges[h.index()].to)
    }

    pub fn is_boundary_edge(&self, e: EdgeId) -> bool {
        let h = self.edges[e.index()].halfedge;
        self.halfedges[h.index()].face.is_none() || self.halfedges[self.halfedges[h.index()].twin.index()].face.is_none()
    }

    /// Half-edges around a face, starting at the face's stored one.
    pub fn face_halfedges(&self, f: FaceId) -> impl Iterator<Item = HalfEdgeId> + '_ {
        let start = self.faces[f.index()].halfedge;
        let mut cur = Some(start);
        std::iter::from_fn(move || {
            let h = cur?;
            let next = self.halfedges[h.index()].next;
            cur = (next != start).then_some(next);
            Some(h)
        })
    }

    /// Vertices of a face in counter-clockwise order.
    pub fn face_vertices(&self, f: FaceId) -> impl Iterator<Item = VertexId> + '_ {
        self.face_halfedges(f).map(move |h| self.from(h))
    }

    pub fn face_sides(&self, f: FaceId) -> usize {
        self.face_halfedges(f).count()
    }

    /// Outgoing half-edges around a vertex.
    pub fn vertex_halfedges(&self, v: VertexId) -> impl Iterator<Item = HalfEdgeId> + '_ {
        let start = self.vertices[v.index()].halfedge;
        let mut cur = start;
        std::iter::from_fn(move || {
            let h = cur?;
            // next outgoing = twin's next
            let next = self.halfedges[self.halfedges[h.index()].twin.index()].next;
            cur = (Some(next) != start).then_some(next);
            Some(h)
        })
    }

    /// One-ring neighbours of a vertex.
    pub fn vertex_neighbors(&self, v: VertexId) -> impl Iterator<Item = VertexId> + '_ {
        self.vertex_halfedges(v).map(move |h| self.halfedges[h.index()].to)
    }

    /// Faces touching a vertex (boundary gaps skipped).
    pub fn vertex_faces(&self, v: VertexId) -> impl Iterator<Item = FaceId> + '_ {
        self.vertex_halfedges(v).filter_map(move |h| self.halfedges[h.index()].face)
    }

    pub fn is_boundary_vertex(&self, v: VertexId) -> bool {
        self.vertex_halfedges(v).any(|h| self.halfedges[h.index()].face.is_none())
    }

    /// Each boundary loop as the ordered list of its vertices.
    pub fn boundary_loops(&self) -> Vec<Vec<VertexId>> {
        let mut seen = vec![false; self.halfedges.len()];
        let mut loops = Vec::new();
        for (i, he) in self.halfedges.iter().enumerate() {
            if he.face.is_some() || seen[i] {
                continue;
            }
            let mut ring = Vec::new();
            let mut h = HalfEdgeId(i as u32);
            loop {
                seen[h.index()] = true;
                ring.push(self.from(h));
                h = self.halfedges[h.index()].next;
                if h.index() == i {
                    break;
                }
            }
            loops.push(ring);
        }
        loops
    }

    /// `V - E + F`. Two for a closed sphere-like surface, zero for a torus.
    pub fn euler_characteristic(&self) -> i64 {
        self.vertices.len() as i64 - self.edges.len() as i64 + self.faces.len() as i64
    }

    pub fn is_closed(&self) -> bool {
        self.halfedges.iter().all(|h| h.face.is_some())
    }

    /// Newell's method: works for any planar polygon, points away from a counter-clockwise front.
    pub fn face_normal(&self, f: FaceId) -> Vec3 {
        let pts: Vec<Vec3> = self.face_vertices(f).map(|v| self.position(v)).collect();
        let mut n = Vec3::ZERO;
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            n += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
        }
        n.normalize_or_zero()
    }

    /// Unnormalised Newell normal; its length is twice the polygon area.
    fn face_area_vector(&self, f: FaceId) -> Vec3 {
        let pts: Vec<Vec3> = self.face_vertices(f).map(|v| self.position(v)).collect();
        let mut n = Vec3::ZERO;
        for i in 0..pts.len() {
            let a = pts[i];
            let b = pts[(i + 1) % pts.len()];
            n += a.cross(b);
        }
        n * 0.5
    }

    pub fn face_area(&self, f: FaceId) -> f32 {
        self.face_area_vector(f).length()
    }

    pub fn face_centroid(&self, f: FaceId) -> Vec3 {
        let mut sum = Vec3::ZERO;
        let mut n = 0.0;
        for v in self.face_vertices(f) {
            sum += self.position(v);
            n += 1.0;
        }
        sum / n
    }

    /// Area-weighted average of the surrounding face normals.
    pub fn vertex_normal(&self, v: VertexId) -> Vec3 {
        self.vertex_faces(v).map(|f| self.face_area_vector(f)).sum::<Vec3>().normalize_or_zero()
    }

    pub fn vertex_normals(&self) -> Vec<Vec3> {
        self.vertex_ids().map(|v| self.vertex_normal(v)).collect()
    }

    pub fn aabb(&self) -> Aabb {
        Aabb::from_points(self.positions())
    }

    /// Fan triangulation of every face as vertex index triples. Faces are convex in stage 1.
    pub fn triangles(&self) -> Vec<[VertexId; 3]> {
        let mut out = Vec::new();
        for f in self.face_ids() {
            let vs: Vec<VertexId> = self.face_vertices(f).collect();
            for i in 1..vs.len() - 1 {
                out.push([vs[0], vs[i], vs[i + 1]]);
            }
        }
        out
    }

    /// Face index for each triangle from [`Mesh::triangles`], in the same order.
    pub fn triangle_faces(&self) -> Vec<FaceId> {
        self.face_ids().flat_map(|f| std::iter::repeat_n(f, self.face_sides(f) - 2)).collect()
    }

    /// Positions and polygons, the inverse of [`Mesh::from_polygons`].
    pub fn to_polygons(&self) -> (Vec<Vec3>, Vec<Vec<u32>>) {
        let positions = self.positions().collect();
        let polygons = self.face_ids().map(|f| self.face_vertices(f).map(|v| v.0).collect()).collect();
        (positions, polygons)
    }

    /// Apply `f` to every position in place.
    pub fn map_positions(&mut self, mut f: impl FnMut(Vec3) -> Vec3) {
        for v in &mut self.vertices {
            v.position = f(v.position);
        }
    }

    /// Internal consistency: every twin/next/prev pointer agrees. Cheap enough for tests and ops.
    pub fn check(&self) -> Result<(), String> {
        for (i, he) in self.halfedges.iter().enumerate() {
            let h = HalfEdgeId(i as u32);
            let twin = &self.halfedges[he.twin.index()];
            if twin.twin != h {
                return Err(format!("halfedge {i}: twin.twin != self"));
            }
            if twin.edge != he.edge {
                return Err(format!("halfedge {i}: twin on a different edge"));
            }
            if self.halfedges[he.next.index()].prev != h {
                return Err(format!("halfedge {i}: next.prev != self"));
            }
            if self.halfedges[he.next.index()].face != he.face {
                return Err(format!("halfedge {i}: next on a different face"));
            }
            if self.from(he.next) != he.to {
                return Err(format!("halfedge {i}: next does not start where self ends"));
            }
        }
        for (i, v) in self.vertices.iter().enumerate() {
            if let Some(h) = v.halfedge
                && self.from(h).index() != i
            {
                return Err(format!("vertex {i}: stored halfedge does not leave it"));
            }
        }
        for (i, f) in self.faces.iter().enumerate() {
            if self.halfedges[f.halfedge.index()].face != Some(FaceId(i as u32)) {
                return Err(format!("face {i}: stored halfedge belongs elsewhere"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{cube, plane, torus, uv_sphere};

    #[test]
    fn crate_name() {
        assert_eq!(CRATE, "shape-geom");
    }

    #[test]
    fn cube_counts_and_euler() {
        let m = cube(1.0);
        assert_eq!((m.vertex_count(), m.edge_count(), m.face_count()), (8, 12, 6));
        assert_eq!(m.halfedge_count(), 24);
        assert_eq!(m.euler_characteristic(), 2);
        assert!(m.is_closed());
        m.check().unwrap();
        for v in m.vertex_ids() {
            assert_eq!(m.vertex_neighbors(v).count(), 3);
            assert_eq!(m.vertex_faces(v).count(), 3);
        }
        for f in m.face_ids() {
            assert_eq!(m.face_sides(f), 4);
            assert!((m.face_area(f) - 1.0).abs() < 1e-5);
        }
        assert_eq!(m.triangles().len(), 12);
        assert_eq!(m.triangle_faces().len(), 12);
    }

    #[test]
    fn cube_normals_point_outward() {
        let m = cube(2.0);
        for f in m.face_ids() {
            let n = m.face_normal(f);
            let c = m.face_centroid(f);
            assert!(n.dot(c) > 0.9, "face normal {n} vs centroid {c}");
        }
        for v in m.vertex_ids() {
            let n = m.vertex_normal(v);
            assert!((n - m.position(v).normalize()).length() < 1e-5);
        }
    }

    #[test]
    fn sphere_and_torus_topology() {
        let s = uv_sphere(1.0, 16, 8);
        s.check().unwrap();
        assert!(s.is_closed());
        assert_eq!(s.euler_characteristic(), 2);
        let t = torus(1.0, 0.3, 24, 12);
        t.check().unwrap();
        assert!(t.is_closed());
        assert_eq!(t.euler_characteristic(), 0);
        assert_eq!(t.face_count(), 24 * 12);
    }

    #[test]
    fn plane_has_one_boundary_loop() {
        let p = plane(2.0, 3, 2);
        p.check().unwrap();
        assert!(!p.is_closed());
        assert_eq!((p.vertex_count(), p.face_count()), (12, 6));
        assert_eq!(p.edge_count(), 17);
        assert_eq!(p.euler_characteristic(), 1);
        let loops = p.boundary_loops();
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].len(), 10);
        assert_eq!(p.vertex_ids().filter(|&v| p.is_boundary_vertex(v)).count(), 10);
        assert_eq!(p.edge_ids().filter(|&e| p.is_boundary_edge(e)).count(), 10);
        // interior vertices see all 4 faces; corners see 1
        let interior = p.vertex_ids().filter(|&v| !p.is_boundary_vertex(v)).collect::<Vec<_>>();
        assert_eq!(interior.len(), 2);
        assert!(interior.iter().all(|&v| p.vertex_faces(v).count() == 4 && p.vertex_neighbors(v).count() == 4));
    }

    #[test]
    fn rejects_bad_input() {
        let pts = vec![Vec3::ZERO, Vec3::X, Vec3::Y];
        assert_eq!(Mesh::from_polygons(pts.clone(), &[vec![0, 1]]), Err(MeshError::DegenerateFace { face: 0, sides: 2 }));
        assert_eq!(Mesh::from_polygons(pts.clone(), &[vec![0, 1, 7]]), Err(MeshError::VertexOutOfRange { face: 0, vertex: 7, count: 3 }));
        assert_eq!(Mesh::from_polygons(pts.clone(), &[vec![0, 1, 1]]), Err(MeshError::RepeatedVertex { face: 0, vertex: 1 }));
        assert_eq!(Mesh::from_polygons(pts, &[vec![0, 1, 2], vec![0, 1, 2]]), Err(MeshError::NonManifoldEdge { from: 0, to: 1 }));
    }

    #[test]
    fn polygons_round_trip() {
        let m = uv_sphere(1.0, 8, 4);
        let (p, f) = m.to_polygons();
        let back = Mesh::from_polygons(p, &f).unwrap();
        assert_eq!(back.to_polygons(), m.to_polygons());
    }
}
