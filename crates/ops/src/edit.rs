//! Edit-mode operators on a selection: delete, dissolve, fill, merge, and the selection walks
//! (linked, grow/shrink, edge loop, face loop) Blender's edit mode offers.

use crate::{PolyMesh, Result};
use shape_geom::{EdgeId, FaceId, HalfEdgeId, Mesh, VertexId};
use shape_math::Vec3;
use std::collections::{BTreeSet, HashMap, HashSet};

/// Rebuild a mesh from the kept polygons, dropping vertices nothing refers to any more.
fn rebuild(positions: &[Vec3], polygons: Vec<Vec<u32>>) -> Result<Mesh> {
    let mut remap = vec![u32::MAX; positions.len()];
    let mut pos = Vec::new();
    let mut out = Vec::with_capacity(polygons.len());
    for poly in polygons {
        let mut q = Vec::with_capacity(poly.len());
        for v in poly {
            if remap[v as usize] == u32::MAX {
                remap[v as usize] = pos.len() as u32;
                pos.push(positions[v as usize]);
            }
            q.push(remap[v as usize]);
        }
        out.push(q);
    }
    Mesh::from_polygons(pos, &out)
}

/// Delete faces (Blender: Delete > Faces). Vertices and edges nothing else uses go with them.
pub fn delete_faces(mesh: &Mesh, faces: &BTreeSet<FaceId>) -> Result<Mesh> {
    let (positions, polygons) = mesh.to_polygons();
    let kept = polygons.into_iter().enumerate().filter(|(i, _)| !faces.contains(&FaceId(*i as u32))).map(|(_, p)| p).collect();
    rebuild(&positions, kept)
}

/// Delete vertices: every face touching one of them goes (Blender: Delete > Vertices).
pub fn delete_vertices(mesh: &Mesh, verts: &BTreeSet<VertexId>) -> Result<Mesh> {
    let faces: BTreeSet<FaceId> = verts.iter().flat_map(|&v| mesh.vertex_faces(v)).collect();
    delete_faces(mesh, &faces)
}

/// Delete edges: both faces of each edge go (Blender: Delete > Edges).
pub fn delete_edges(mesh: &Mesh, edges: &BTreeSet<EdgeId>) -> Result<Mesh> {
    let mut faces = BTreeSet::new();
    for &e in edges {
        let h = mesh.edge(e).halfedge;
        for hh in [h, mesh.halfedge(h).twin] {
            if let Some(f) = mesh.halfedge(hh).face {
                faces.insert(f);
            }
        }
    }
    delete_faces(mesh, &faces)
}

/// Dissolve faces: each connected region of selected faces becomes one n-gon bounded by the
/// region's outline (Blender: Dissolve Faces). A region with a hole or an open side is left alone.
pub fn dissolve_faces(mesh: &Mesh, faces: &BTreeSet<FaceId>) -> Result<Mesh> {
    let (positions, polygons) = mesh.to_polygons();
    let mut kept: Vec<Vec<u32>> = polygons.iter().enumerate().filter(|(i, _)| !faces.contains(&FaceId(*i as u32))).map(|(_, p)| p.clone()).collect();
    let mut seen: HashSet<FaceId> = HashSet::new();
    for &start in faces {
        if seen.contains(&start) {
            continue;
        }
        // Flood the region.
        let mut region = vec![start];
        let mut stack = vec![start];
        seen.insert(start);
        while let Some(f) = stack.pop() {
            for h in mesh.face_halfedges(f) {
                if let Some(g) = mesh.halfedge(mesh.halfedge(h).twin).face
                    && faces.contains(&g)
                    && seen.insert(g)
                {
                    region.push(g);
                    stack.push(g);
                }
            }
        }
        // Boundary half-edges: those whose twin's face is outside the region.
        let mut next_of: HashMap<u32, u32> = HashMap::new();
        for &f in &region {
            for h in mesh.face_halfedges(f) {
                let twin_face = mesh.halfedge(mesh.halfedge(h).twin).face;
                if !twin_face.is_some_and(|g| faces.contains(&g)) {
                    let (a, b) = (mesh.from(h).index() as u32, mesh.halfedge(h).to.index() as u32);
                    if next_of.insert(a, b).is_some() {
                        // A vertex with two outgoing boundary edges: the region pinches; keep as is.
                        kept.extend(region.iter().map(|&f| polygons[f.index()].clone()));
                        next_of.clear();
                        break;
                    }
                }
            }
            if next_of.is_empty() {
                break;
            }
        }
        if next_of.is_empty() {
            continue;
        }
        let first = *next_of.keys().next().unwrap();
        let mut ring = vec![first];
        let mut cur = next_of[&first];
        while cur != first && ring.len() <= next_of.len() {
            ring.push(cur);
            cur = next_of[&cur];
        }
        if ring.len() == next_of.len() && ring.len() >= 3 {
            kept.push(ring);
        } else {
            // More than one loop (a hole): leave the region untouched.
            kept.extend(region.iter().map(|&f| polygons[f.index()].clone()));
        }
    }
    rebuild(&positions, kept)
}

/// Dissolve edges: the two faces of each edge merge into one (Blender: Dissolve Edges).
pub fn dissolve_edges(mesh: &Mesh, edges: &BTreeSet<EdgeId>) -> Result<Mesh> {
    let mut faces = BTreeSet::new();
    for &e in edges {
        if mesh.is_boundary_edge(e) {
            continue;
        }
        let h = mesh.edge(e).halfedge;
        for hh in [h, mesh.halfedge(h).twin] {
            if let Some(f) = mesh.halfedge(hh).face {
                faces.insert(f);
            }
        }
    }
    dissolve_faces(mesh, &faces)
}

/// Delete Only Faces: the faces go but their edges and vertices... have nowhere to live in a
/// polygon mesh, so this equals [`delete_faces`]. Kept as a name for the menu.
pub fn delete_only_faces(mesh: &Mesh, faces: &BTreeSet<FaceId>) -> Result<Mesh> {
    delete_faces(mesh, faces)
}

/// Fill (F): one new face through the selected vertices, ordered around their centre in their
/// best-fit plane, facing away from the mesh's neighbouring faces when it can tell.
pub fn fill(mesh: &Mesh, verts: &BTreeSet<VertexId>) -> Result<Mesh> {
    if verts.len() < 3 {
        return Ok(mesh.clone());
    }
    let (positions, mut polygons) = mesh.to_polygons();
    let set: BTreeSet<u32> = verts.iter().map(|v| v.index() as u32).collect();
    if polygons.iter().any(|p| p.len() == set.len() && p.iter().all(|v| set.contains(v))) {
        return Ok(mesh.clone());
    }
    let pts: Vec<Vec3> = verts.iter().map(|&v| mesh.position(v)).collect();
    let c = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
    // Normal: Newell over the points in their current order is meaningless; use the average of
    // the existing face normals around these vertices, else a plane fit from the first three.
    let mut n = Vec3::ZERO;
    for &v in verts {
        for f in mesh.vertex_faces(v) {
            n += mesh.face_normal(f);
        }
    }
    if n.length_squared() < 1e-8 {
        n = (pts[1] - pts[0]).cross(pts[2] - pts[0]);
    }
    let n = n.normalize_or_zero();
    let u = {
        let t = if n.x.abs() < 0.9 { Vec3::X } else { Vec3::Y };
        (t - n * t.dot(n)).normalize_or_zero()
    };
    let w = n.cross(u);
    let mut order: Vec<(f32, u32)> = verts
        .iter()
        .map(|&v| {
            let d = mesh.position(v) - c;
            (d.dot(w).atan2(d.dot(u)), v.index() as u32)
        })
        .collect();
    order.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut poly: Vec<u32> = order.into_iter().map(|(_, v)| v).collect();
    // Orientation: the new face must run opposite to any existing half-edge it shares.
    let shares_direction = |p: &[u32]| {
        p.iter().enumerate().any(|(i, &a)| {
            let b = p[(i + 1) % p.len()];
            polygons.iter().any(|q| q.iter().enumerate().any(|(j, &x)| x == a && q[(j + 1) % q.len()] == b))
        })
    };
    if shares_direction(&poly) {
        poly.reverse();
    }
    polygons.push(poly);
    Mesh::from_polygons(positions, &polygons)
}

/// Where Merge puts the surviving vertex.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MergeAt {
    Center,
    First,
    Last,
}

/// Merge (M): the selected vertices become one. Faces that collapse below three sides vanish.
pub fn merge_vertices(mesh: &Mesh, verts: &[VertexId], at: MergeAt) -> Result<Mesh> {
    if verts.len() < 2 {
        return Ok(mesh.clone());
    }
    let mut pm = PolyMesh::from_mesh(mesh);
    let target = match at {
        MergeAt::Center => verts.iter().map(|&v| mesh.position(v)).sum::<Vec3>() / verts.len() as f32,
        MergeAt::First => mesh.position(verts[0]),
        MergeAt::Last => mesh.position(*verts.last().unwrap()),
    };
    let keep = verts[0].index() as u32;
    pm.positions[keep as usize] = target;
    let set: HashSet<u32> = verts.iter().map(|v| v.index() as u32).collect();
    let mut polygons = Vec::with_capacity(pm.polygons.len());
    for poly in &pm.polygons {
        let mut q: Vec<u32> = poly.iter().map(|&v| if set.contains(&v) { keep } else { v }).collect();
        q.dedup();
        while q.len() > 1 && q.first() == q.last() {
            q.pop();
        }
        if q.len() >= 3 {
            polygons.push(q);
        }
    }
    rebuild(&pm.positions, polygons)
}

// ---- selection walks ------------------------------------------------------------------------

/// Faces connected (through shared edges) to any of the given faces.
pub fn linked_faces(mesh: &Mesh, seeds: &BTreeSet<FaceId>) -> BTreeSet<FaceId> {
    let mut out = seeds.clone();
    let mut stack: Vec<FaceId> = seeds.iter().copied().collect();
    while let Some(f) = stack.pop() {
        for h in mesh.face_halfedges(f) {
            if let Some(g) = mesh.halfedge(mesh.halfedge(h).twin).face
                && out.insert(g)
            {
                stack.push(g);
            }
        }
    }
    out
}

/// Grow by one step: add every vertex that shares an edge with a selected one.
pub fn grow_vertices(mesh: &Mesh, verts: &BTreeSet<VertexId>) -> BTreeSet<VertexId> {
    let mut out = verts.clone();
    for &v in verts {
        out.extend(mesh.vertex_neighbors(v));
    }
    out
}

/// Shrink by one step: drop every vertex with an unselected neighbour.
pub fn shrink_vertices(mesh: &Mesh, verts: &BTreeSet<VertexId>) -> BTreeSet<VertexId> {
    verts.iter().copied().filter(|&v| mesh.vertex_neighbors(v).all(|n| verts.contains(&n))).collect()
}

/// Edge loop through `edge` (Alt+click): at each 4-valent vertex continue with the edge that
/// shares no face with the current one, until the loop closes or hits a pole/boundary.
pub fn edge_loop(mesh: &Mesh, edge: EdgeId) -> BTreeSet<EdgeId> {
    let mut out = BTreeSet::new();
    out.insert(edge);
    let (a, b) = mesh.edge_vertices(edge);
    for (mut v, mut e) in [(b, edge), (a, edge)] {
        loop {
            let faces_of = |e: EdgeId| -> [Option<FaceId>; 2] {
                let h = mesh.edge(e).halfedge;
                [mesh.halfedge(h).face, mesh.halfedge(mesh.halfedge(h).twin).face]
            };
            let around: Vec<HalfEdgeId> = mesh.vertex_halfedges(v).collect();
            if mesh.is_boundary_vertex(v) {
                break;
            }
            let cur = faces_of(e);
            let next = match around.len() {
                // Straight on: the edge sharing no face with this one.
                4 => around.iter().map(|&h| mesh.halfedge(h).edge).find(|&other| other != e && faces_of(other).iter().all(|f| !cur.contains(f))),
                // Around an n-gon (a cylinder cap): the other edge that borders the same n-gon.
                3 => cur.iter().flatten().copied().find(|&f| mesh.face_sides(f) > 4).and_then(|ngon| {
                    around.iter().map(|&h| mesh.halfedge(h).edge).find(|&other| other != e && faces_of(other).contains(&Some(ngon)))
                }),
                _ => None,
            };
            let Some(next) = next else { break };
            if !out.insert(next) {
                break;
            }
            let (x, y) = mesh.edge_vertices(next);
            v = if x == v { y } else { x };
            e = next;
        }
    }
    out
}

/// Face loop through `face` across `edge` (Alt+click on a face): step through quads out the
/// opposite edge each time.
pub fn face_loop(mesh: &Mesh, face: FaceId, edge: EdgeId) -> BTreeSet<FaceId> {
    let mut out = BTreeSet::new();
    out.insert(face);
    let h0 = mesh.edge(edge).halfedge;
    let start = if mesh.halfedge(h0).face == Some(face) { h0 } else { mesh.halfedge(h0).twin };
    for first in [start, mesh.halfedge(start).twin] {
        let mut h = first;
        while let Some(f) = mesh.halfedge(h).face {
            if mesh.face_sides(f) != 4 {
                break;
            }
            if f != face && !out.insert(f) {
                break;
            }
            // Opposite edge of the quad: two steps along.
            let opposite = mesh.halfedge(mesh.halfedge(h).next).next;
            h = mesh.halfedge(opposite).twin;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives;

    fn set<T: Ord>(v: impl IntoIterator<Item = T>) -> BTreeSet<T> {
        v.into_iter().collect()
    }

    #[test]
    fn delete_and_dissolve_faces_on_a_cube() {
        let cube = primitives::cube(1.0);
        let top = cube.face_ids().find(|&f| cube.face_normal(f).y > 0.9).unwrap();
        let open = delete_faces(&cube, &set([top])).unwrap();
        assert_eq!(open.face_count(), 5);
        assert_eq!(open.boundary_loops().len(), 1);
        // Dissolving two neighbouring faces of a subdivided plane gives one hexagon-ish n-gon.
        let plane = primitives::plane(2.0, 2, 1);
        assert_eq!(plane.face_count(), 2);
        let all: BTreeSet<_> = plane.face_ids().collect();
        let one = dissolve_faces(&plane, &all).unwrap();
        assert_eq!(one.face_count(), 1);
        assert_eq!(one.face_sides(FaceId(0)), 6);
        // Dissolving the shared edge does the same.
        let shared = plane.edge_ids().find(|&e| !plane.is_boundary_edge(e)).unwrap();
        let one2 = dissolve_edges(&plane, &set([shared])).unwrap();
        assert_eq!(one2.face_count(), 1);
    }

    #[test]
    fn fill_closes_the_hole_and_merge_collapses() {
        let cube = primitives::cube(1.0);
        let top = cube.face_ids().find(|&f| cube.face_normal(f).y > 0.9).unwrap();
        let open = delete_faces(&cube, &set([top])).unwrap();
        let rim: BTreeSet<VertexId> = open.boundary_loops()[0].iter().copied().collect();
        let closed = fill(&open, &rim).unwrap();
        assert!(closed.is_closed());
        assert_eq!(closed.face_count(), 6);
        for f in closed.face_ids() {
            assert!(closed.face_normal(f).dot(closed.face_centroid(f)) > 0.0, "outward");
        }
        // Filling again changes nothing.
        assert_eq!(fill(&closed, &rim).unwrap().face_count(), 6);
        // Merge the whole top ring to its centre: the cube becomes a pyramid.
        let verts: Vec<VertexId> = closed.vertex_ids().filter(|&v| closed.position(v).y > 0.0).collect();
        let pyramid = merge_vertices(&closed, &verts, MergeAt::Center).unwrap();
        assert_eq!(pyramid.vertex_count(), 5);
        assert_eq!(pyramid.face_count(), 5);
        pyramid.check().unwrap();
    }

    #[test]
    fn loops_and_linked_on_a_cylinder() {
        let cyl = primitives::cylinder(1.0, 2.0, 8);
        // Rim vertices are 3-valent with the cap n-gon: the rim edge's loop is the whole rim.
        let rim = cyl
            .edge_ids()
            .find(|&e| {
                let (a, b) = cyl.edge_vertices(e);
                cyl.position(a).y > 0.9 && cyl.position(b).y > 0.9
            })
            .unwrap();
        let lp = edge_loop(&cyl, rim);
        assert_eq!(lp.len(), 8, "rim loop closes around the cap");
        let side = cyl.face_ids().find(|&f| cyl.face_normal(f).y.abs() < 0.1).unwrap();
        let vertical = cyl
            .face_halfedges(side)
            .map(|h| cyl.halfedge(h).edge)
            .find(|&e| {
                let (a, b) = cyl.edge_vertices(e);
                (cyl.position(a).y - cyl.position(b).y).abs() > 1.0
            })
            .unwrap();
        let ring = face_loop(&cyl, side, vertical);
        assert_eq!(ring.len(), 8, "face loop around the barrel");
        let linked = linked_faces(&cyl, &set([side]));
        assert_eq!(linked.len(), cyl.face_count());
        let v0 = cyl.vertex_ids().next().unwrap();
        let grown = grow_vertices(&cyl, &set([v0]));
        assert!(grown.len() > 1);
        assert_eq!(shrink_vertices(&cyl, &grown), set([v0]));
    }
}
