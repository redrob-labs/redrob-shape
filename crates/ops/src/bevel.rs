//! Bevel: replace each selected edge with a flat strip (one segment, a chamfer), and each vertex
//! where selected edges meet with a polygon.
//!
//! For every face corner the vertex is pushed inward along each of the corner's two edges that is
//! beveled; a corner pushed along an unbeveled edge is also inserted into the face on the other
//! side of that edge, so no T-junction appears. Then one quad joins the two faces along each
//! beveled edge, and one polygon closes the gap at each vertex. Beveling every edge of a cube gives
//! 6 + 12 + 8 = 26 faces, the classic chamfered cube.

use crate::{PolyMesh, Result};
use shape_geom::{EdgeId, FaceId, HalfEdgeId, Mesh, VertexId};
use shape_math::Vec3;
use std::collections::{BTreeSet, HashMap};

/// Where a new point came from, so the faces that share it get the same index. An unmoved corner
/// keeps the original vertex index and needs no key.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Key {
    /// On edge `e`, `offset` away from vertex `v`.
    OnEdge(EdgeId, VertexId),
    /// Inside face `f` at its corner `v`, pushed along both edges.
    Corner(FaceId, VertexId),
}

/// Bevel `edges` by `offset` (clamped to under half the shortest touched edge). Boundary edges are
/// skipped. An empty selection returns the mesh unchanged.
pub fn bevel_edges(mesh: &Mesh, edges: &[EdgeId], offset: f32) -> Result<Mesh> {
    let selected: BTreeSet<EdgeId> = edges.iter().copied().filter(|&e| !mesh.is_boundary_edge(e)).collect();
    if selected.is_empty() || offset <= 0.0 {
        return Ok(mesh.clone());
    }
    // Clamp so opposite strips of one edge never cross.
    let shortest = selected
        .iter()
        .map(|&e| {
            let (a, b) = mesh.edge_vertices(e);
            (mesh.position(a) - mesh.position(b)).length()
        })
        .fold(f32::INFINITY, f32::min);
    let offset = offset.min(shortest * 0.45);

    let mut pm = PolyMesh { positions: mesh.positions().collect(), polygons: Vec::new() };
    let mut points: HashMap<Key, u32> = HashMap::new();
    let mut point = |pm: &mut PolyMesh, key: Key, p: Vec3| -> u32 { *points.entry(key).or_insert_with(|| pm.push_vertex(p)) };

    // Per face corner: (half-edge leaving the corner inside the face) -> new point index.
    let mut corner_of: HashMap<HalfEdgeId, u32> = HashMap::new();
    let dir = |from: VertexId, to: VertexId| (mesh.position(to) - mesh.position(from)).normalize_or_zero();

    for f in mesh.face_ids() {
        for h in mesh.face_halfedges(f) {
            let v = mesh.from(h);
            let he = mesh.halfedge(h);
            let prev = mesh.halfedge(he.prev);
            let next_v = he.to;
            let prev_v = mesh.from(he.prev);
            let e_next = he.edge;
            let e_prev = prev.edge;
            let b_next = selected.contains(&e_next);
            let b_prev = selected.contains(&e_prev);
            let p = mesh.position(v);
            let idx = match (b_prev, b_next) {
                (false, false) => v.index() as u32,
                (true, true) => point(&mut pm, Key::Corner(f, v), p + dir(v, prev_v) * offset + dir(v, next_v) * offset),
                // Only the next edge is beveled: slide along the previous (unbeveled) edge.
                (false, true) => point(&mut pm, Key::OnEdge(e_prev, v), p + dir(v, prev_v) * offset),
                (true, false) => point(&mut pm, Key::OnEdge(e_next, v), p + dir(v, next_v) * offset),
            };
            corner_of.insert(h, idx);
        }
    }

    // Faces: corners, plus the on-edge points other faces created along unbeveled edges.
    let mut face_polys: Vec<Vec<u32>> = Vec::with_capacity(mesh.face_count());
    for f in mesh.face_ids() {
        let mut poly: Vec<u32> = Vec::new();
        for h in mesh.face_halfedges(f) {
            let he = mesh.halfedge(h);
            let v = mesh.from(h);
            let w = he.to;
            let c_v = corner_of[&h];
            let c_w = corner_of[&he.next];
            poly.push(c_v);
            if !selected.contains(&he.edge) {
                if let Some(&p) = points.get(&Key::OnEdge(he.edge, v))
                    && p != c_v
                {
                    poly.push(p);
                }
                if let Some(&p) = points.get(&Key::OnEdge(he.edge, w))
                    && p != c_w
                {
                    poly.push(p);
                }
            }
        }
        poly.dedup();
        while poly.len() > 1 && poly.first() == poly.last() {
            poly.pop();
        }
        face_polys.push(poly);
    }

    // One quad per beveled edge, oriented with the two faces it joins.
    for &e in &selected {
        let h = mesh.edge(e).halfedge;
        let t = mesh.halfedge(h).twin;
        let (Some(fa), Some(fb)) = (mesh.halfedge(h).face, mesh.halfedge(t).face) else { continue };
        let ha_next = mesh.halfedge(h).next;
        let hb_next = mesh.halfedge(t).next;
        // h runs v0 -> v1 in fa; t runs v1 -> v0 in fb.
        let a0 = corner_of[&h];
        let a1 = corner_of[&ha_next];
        let b1 = corner_of[&t];
        let b0 = corner_of[&hb_next];
        let mut quad = vec![a1, a0, b0, b1];
        quad.dedup();
        if quad.len() >= 3 {
            let want = (mesh.face_normal(fa) + mesh.face_normal(fb)).normalize_or_zero();
            if pm.polygon_normal(&quad).dot(want) < 0.0 {
                quad.reverse();
            }
            pm.polygons.push(quad);
        }
    }

    // One polygon per vertex touched by a beveled edge: its corner points in order around it.
    for v in mesh.vertex_ids() {
        let touched = mesh.vertex_halfedges(v).any(|h| selected.contains(&mesh.halfedge(h).edge));
        if !touched || mesh.is_boundary_vertex(v) {
            continue;
        }
        let mut ring: Vec<u32> = mesh.vertex_halfedges(v).filter_map(|h| corner_of.get(&h).copied()).collect();
        ring.dedup();
        while ring.len() > 1 && ring.first() == ring.last() {
            ring.pop();
        }
        // A strip that ends inside one face (the only beveled edge at a 3-valent vertex): both end
        // points lie on that face's edges, so the face takes the strip's end edge and loses the
        // vertex; a polygon there would be flat on the face and overlap it.
        let vi = v.index() as u32;
        if ring.len() == 3 && ring.contains(&vi) {
            let others: Vec<u32> = ring.iter().copied().filter(|&i| i != vi).collect();
            let host = mesh.vertex_halfedges(v).find(|h| corner_of.get(h) == Some(&vi)).and_then(|h| mesh.halfedge(h).face);
            if let Some(f) = host
                && others.iter().all(|p| face_polys[f.index()].contains(p))
            {
                face_polys[f.index()].retain(|&i| i != vi);
                continue;
            }
        }
        if ring.len() >= 3 {
            if pm.polygon_normal(&ring).dot(mesh.vertex_normal(v)) < 0.0 {
                ring.reverse();
            }
            pm.polygons.push(ring);
        }
    }

    pm.polygons.extend(face_polys.into_iter().filter(|p| p.len() >= 3));

    // Vertices every corner moved away from are now unreferenced; drop them.
    let mut used = vec![false; pm.positions.len()];
    for poly in &pm.polygons {
        for &i in poly {
            used[i as usize] = true;
        }
    }
    let mut remap = vec![u32::MAX; pm.positions.len()];
    let mut positions = Vec::with_capacity(pm.positions.len());
    for (i, &p) in pm.positions.iter().enumerate() {
        if used[i] {
            remap[i] = positions.len() as u32;
            positions.push(p);
        }
    }
    let polygons = pm.polygons.iter().map(|poly| poly.iter().map(|&i| remap[i as usize]).collect()).collect();
    PolyMesh { positions, polygons }.build()
}

/// Bevel every non-boundary edge of the mesh.
pub fn bevel_all(mesh: &Mesh, offset: f32) -> Result<Mesh> {
    let edges: Vec<EdgeId> = mesh.edge_ids().collect();
    bevel_edges(mesh, &edges, offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives;

    #[test]
    fn chamfered_cube_has_26_faces_and_is_closed() {
        let cube = primitives::cube(1.0);
        let out = bevel_all(&cube, 0.2).unwrap();
        assert_eq!(out.face_count(), 26);
        assert_eq!(out.vertex_count(), 24);
        assert_eq!(out.euler_characteristic(), 2);
        assert!(out.is_closed());
        out.check().unwrap();
        // Six quads from the faces, twelve edge quads, eight corner triangles.
        let mut sides: Vec<usize> = out.face_ids().map(|f| out.face_sides(f)).collect();
        sides.sort_unstable();
        assert_eq!(sides.iter().filter(|&&s| s == 3).count(), 8);
        assert_eq!(sides.iter().filter(|&&s| s == 4).count(), 18);
        // Every face normal points away from the centre.
        for f in out.face_ids() {
            assert!(out.face_normal(f).dot(out.face_centroid(f)) > 0.0, "face {f:?} points inward");
        }
    }

    #[test]
    fn one_edge_of_a_cube_adds_a_strip_the_end_faces_absorb() {
        let cube = primitives::cube(1.0);
        let e = cube.edge_ids().next().unwrap();
        let out = bevel_edges(&cube, &[e], 0.2).unwrap();
        // 6 faces + 1 strip; the two end faces take the strip's end edges (5-gons).
        assert_eq!(out.face_count(), 7);
        assert_eq!(out.euler_characteristic(), 2);
        assert!(out.is_closed());
        out.check().unwrap();
        let fives = out.face_ids().filter(|&f| out.face_sides(f) == 5).count();
        assert_eq!(fives, 2);
        assert_eq!(out.vertex_count(), 10);
        for f in out.face_ids() {
            assert!(out.face_normal(f).dot(out.face_centroid(f)) > 0.0, "face {f:?} points inward");
        }
    }

    #[test]
    fn offset_is_clamped_and_empty_selection_is_identity() {
        let cube = primitives::cube(1.0);
        assert_eq!(bevel_edges(&cube, &[], 0.3).unwrap(), cube);
        let out = bevel_all(&cube, 10.0).unwrap();
        assert_eq!(out.face_count(), 26);
        out.check().unwrap();
    }

    #[test]
    fn open_mesh_skips_boundary_edges() {
        let plane = primitives::plane(2.0, 2, 2);
        let before = plane.face_count();
        let out = bevel_all(&plane, 0.1).unwrap();
        out.check().unwrap();
        // Interior edges are beveled; the outline stays where it was.
        assert!(out.face_count() > before);
        assert_eq!(out.boundary_loops().len(), 1);
    }
}
