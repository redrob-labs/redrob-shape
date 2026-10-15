//! More edit-mode operators: bridge edge loops, spin, a knife cut between two edge points, and
//! shrink/fatten of a vertex selection.

use crate::{PolyMesh, Result};
use shape_geom::{EdgeId, FaceId, Mesh, VertexId};
use shape_math::{Quat, Vec3};
use std::collections::{BTreeSet, HashMap, HashSet};

/// Order a set of edges into closed or open vertex loops (each vertex used by at most two edges).
pub fn edge_loops(mesh: &Mesh, edges: &BTreeSet<EdgeId>) -> Vec<Vec<VertexId>> {
    let mut adj: HashMap<VertexId, Vec<VertexId>> = HashMap::new();
    for &e in edges {
        let (a, b) = mesh.edge_vertices(e);
        adj.entry(a).or_default().push(b);
        adj.entry(b).or_default().push(a);
    }
    let mut seen: HashSet<VertexId> = HashSet::new();
    let mut loops = Vec::new();
    // Open chains first (start at an endpoint), then closed rings.
    let mut starts: Vec<VertexId> = adj.iter().filter(|(_, n)| n.len() == 1).map(|(v, _)| *v).collect();
    starts.sort();
    let mut rest: Vec<VertexId> = adj.keys().copied().collect();
    rest.sort();
    for start in starts.into_iter().chain(rest) {
        if seen.contains(&start) {
            continue;
        }
        let mut chain = vec![start];
        seen.insert(start);
        let mut prev = None;
        let mut cur = start;
        loop {
            let next = adj[&cur].iter().copied().find(|&n| Some(n) != prev && !seen.contains(&n));
            match next {
                Some(n) => {
                    seen.insert(n);
                    chain.push(n);
                    prev = Some(cur);
                    cur = n;
                }
                None => break,
            }
        }
        if chain.len() >= 2 {
            loops.push(chain);
        }
    }
    loops
}

/// Bridge Edge Loops: quads between two loops of the same length (Blender: Edge > Bridge Edge
/// Loops). Loops are matched by nearest start and direction.
pub fn bridge_edge_loops(mesh: &Mesh, edges: &BTreeSet<EdgeId>) -> Result<Mesh> {
    let loops = edge_loops(mesh, edges);
    if loops.len() != 2 || loops[0].len() != loops[1].len() {
        return Ok(mesh.clone());
    }
    let a = &loops[0];
    let mut b = loops[1].clone();
    let closed = |l: &Vec<VertexId>| {
        let (x, y) = (l[0], *l.last().unwrap());
        edges.iter().any(|&e| {
            let (p, q) = mesh.edge_vertices(e);
            (p == x && q == y) || (p == y && q == x)
        }) && l.len() > 2
    };
    let is_closed = closed(a) && closed(&b);
    // Rotate b so b[0] is nearest a[0]; flip b if that shortens the sum of rungs.
    if is_closed {
        let p0 = mesh.position(a[0]);
        let k = (0..b.len()).min_by(|&i, &j| (mesh.position(b[i]) - p0).length().total_cmp(&(mesh.position(b[j]) - p0).length())).unwrap();
        b.rotate_left(k);
    }
    let rung_sum = |b: &Vec<VertexId>| a.iter().zip(b).map(|(&x, &y)| (mesh.position(x) - mesh.position(y)).length()).sum::<f32>();
    let mut flipped = b.clone();
    flipped.reverse();
    if is_closed {
        flipped.rotate_right(1);
    }
    if rung_sum(&flipped) < rung_sum(&b) {
        b = flipped;
    }
    let mut pm = PolyMesh::from_mesh(mesh);
    let n = a.len();
    let count = if is_closed { n } else { n - 1 };
    let existing: HashSet<(u32, u32)> = pm.polygons.iter().flat_map(|p| (0..p.len()).map(move |i| (p[i], p[(i + 1) % p.len()]))).collect();
    for i in 0..count {
        let j = (i + 1) % n;
        let (a0, a1, b0, b1) = (a[i].index() as u32, a[j].index() as u32, b[i].index() as u32, b[j].index() as u32);
        // Orient against the existing half-edge along a (if any) so the bridge is consistent.
        let quad = if existing.contains(&(a0, a1)) { vec![a1, a0, b0, b1] } else { vec![a0, a1, b1, b0] };
        pm.polygons.push(quad);
    }
    pm.build()
}

/// Spin: sweep the selected edges around `axis` through `center` by `angle`, in `steps`, adding a
/// ring of quads per step (Blender: Mesh > Spin). The seed edges keep their faces, so a full turn
/// leaves a seam there rather than closing onto them.
pub fn spin(mesh: &Mesh, edges: &BTreeSet<EdgeId>, center: Vec3, axis: Vec3, angle: f32, steps: u32) -> Result<Mesh> {
    let steps = steps.clamp(1, 256);
    // Only edges with a free side can grow a sweep; an interior edge already has faces on both.
    let edges: BTreeSet<EdgeId> = edges.iter().copied().filter(|&e| mesh.is_boundary_edge(e)).collect();
    let edges = &edges;
    if edges.is_empty() {
        return Ok(mesh.clone());
    }
    let axis = axis.normalize_or_zero();
    let mut pm = PolyMesh::from_mesh(mesh);
    let verts: Vec<VertexId> = {
        let mut set = BTreeSet::new();
        for &e in edges {
            let (a, b) = mesh.edge_vertices(e);
            set.insert(a);
            set.insert(b);
        }
        set.into_iter().collect()
    };
    let index_of: HashMap<VertexId, usize> = verts.iter().enumerate().map(|(i, v)| (*v, i)).collect();
    let mut prev: Vec<u32> = verts.iter().map(|v| v.index() as u32).collect();
    let existing: HashSet<(u32, u32)> = pm.polygons.iter().flat_map(|p| (0..p.len()).map(move |i| (p[i], p[(i + 1) % p.len()]))).collect();
    for k in 1..=steps {
        let rot = Quat::from_axis_angle(axis, angle * k as f32 / steps as f32);
        let cur: Vec<u32> = verts.iter().map(|&v| pm.push_vertex(center + rot * (mesh.position(v) - center))).collect();
        for &e in edges {
            let (a, b) = mesh.edge_vertices(e);
            let (ia, ib) = (index_of[&a], index_of[&b]);
            let (a0, b0, a1, b1) = (prev[ia], prev[ib], cur[ia], cur[ib]);
            let mut quad = vec![a0, b0, b1, a1];
            // Keep the sweep consistent with any face already using the seed edge.
            if k == 1 && existing.contains(&(a0, b0)) {
                quad.reverse();
            } else if k > 1 {
                // Follow the orientation chosen at the first step via the shared edge prev row.
                if existing_dir(&pm, a0, b0) {
                    quad.reverse();
                }
            }
            quad.dedup();
            if quad.len() >= 3 {
                pm.polygons.push(quad);
            }
        }
        prev = cur;
    }
    pm.build()
}

/// Whether some polygon already runs `a -> b` in that direction.
fn existing_dir(pm: &PolyMesh, a: u32, b: u32) -> bool {
    pm.polygons.iter().any(|p| (0..p.len()).any(|i| p[i] == a && p[(i + 1) % p.len()] == b))
}

/// A point on an edge of a face, as the knife picks it: `t` along the edge's stored direction.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgePoint {
    pub edge: EdgeId,
    pub t: f32,
}

/// Knife (one cut): split `face` between two points on two of its edges. Each edge is split at its
/// point (the face across it gains the vertex too) and the face becomes two faces.
pub fn knife_cut(mesh: &Mesh, face: FaceId, a: EdgePoint, b: EdgePoint) -> Result<Mesh> {
    if a.edge == b.edge {
        return Ok(mesh.clone());
    }
    let (positions, mut polygons) = mesh.to_polygons();
    let mut positions = positions;
    let split_point = |ep: EdgePoint, positions: &mut Vec<Vec3>, polygons: &mut Vec<Vec<u32>>| -> u32 {
        let (va, vb) = mesh.edge_vertices(ep.edge);
        let (ia, ib) = (va.index() as u32, vb.index() as u32);
        let t = ep.t.clamp(0.02, 0.98);
        let p = positions[ia as usize] * (1.0 - t) + positions[ib as usize] * t;
        let idx = positions.len() as u32;
        positions.push(p);
        // Insert into every polygon that has the edge (either direction).
        for poly in polygons.iter_mut() {
            let n = poly.len();
            if let Some(i) = (0..n).find(|&i| (poly[i] == ia && poly[(i + 1) % n] == ib) || (poly[i] == ib && poly[(i + 1) % n] == ia)) {
                poly.insert(i + 1, idx);
            }
        }
        idx
    };
    let pa = split_point(a, &mut positions, &mut polygons);
    let pb = split_point(b, &mut positions, &mut polygons);
    // Split the face polygon at pa and pb.
    let poly = polygons[face.index()].clone();
    let (Some(i), Some(j)) = (poly.iter().position(|&v| v == pa), poly.iter().position(|&v| v == pb)) else {
        return Ok(mesh.clone());
    };
    let (i, j) = if i < j { (i, j) } else { (j, i) };
    let first: Vec<u32> = poly[i..=j].to_vec();
    let mut second: Vec<u32> = poly[j..].to_vec();
    second.extend_from_slice(&poly[..=i]);
    polygons[face.index()] = first;
    polygons.push(second);
    Mesh::from_polygons(positions, &polygons)
}

/// Alt+S: move the selected vertices along their normals.
pub fn shrink_fatten_vertices(mesh: &Mesh, verts: &BTreeSet<VertexId>, offset: f32) -> Result<Mesh> {
    let mut pm = PolyMesh::from_mesh(mesh);
    for &v in verts {
        let n = mesh.vertex_normal(v);
        pm.positions[v.index()] += n * offset;
    }
    pm.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives;

    #[test]
    fn bridge_the_facing_rims_of_two_tubes() {
        // Two open tubes stacked with a gap; bridge the top rim of the lower one to the bottom rim
        // of the upper one.
        let tube = |y: f32| {
            let cyl = primitives::cylinder(1.0, 2.0, 8);
            let caps: BTreeSet<FaceId> = cyl.face_ids().filter(|&f| cyl.face_normal(f).y.abs() > 0.9).collect();
            let mut pm = PolyMesh::from_mesh(&crate::delete_faces(&cyl, &caps).unwrap());
            for p in &mut pm.positions {
                p.y += y;
            }
            pm
        };
        let mut both = tube(0.0);
        both.append(&tube(3.0));
        let mesh = both.build().unwrap();
        assert_eq!(mesh.boundary_loops().len(), 4);
        let rims: BTreeSet<EdgeId> = mesh
            .edge_ids()
            .filter(|&e| {
                let (a, b) = mesh.edge_vertices(e);
                mesh.is_boundary_edge(e)
                    && mesh.position(a).y > 0.5
                    && mesh.position(a).y < 2.5
                    && mesh.position(b).y > 0.5
                    && mesh.position(b).y < 2.5
            })
            .collect();
        assert_eq!(rims.len(), 16);
        assert_eq!(edge_loops(&mesh, &rims).len(), 2);
        let bridged = bridge_edge_loops(&mesh, &rims).unwrap();
        bridged.check().unwrap();
        assert_eq!(bridged.face_count(), 24, "16 side quads plus 8 bridging quads");
        assert_eq!(bridged.boundary_loops().len(), 2, "only the far rims stay open");
    }

    #[test]
    fn spin_sweeps_an_edge_into_a_wall() {
        let plane = primitives::plane(1.0, 1, 1);
        let e = plane.edge_ids().next().unwrap();
        let (a, _) = plane.edge_vertices(e);
        let center = Vec3::new(5.0, 0.0, 5.0);
        let out = spin(&plane, &BTreeSet::from([e]), center, Vec3::Y, 1.5, 6).unwrap();
        out.check().unwrap();
        assert_eq!(out.face_count(), 1 + 6);
        assert_eq!(out.boundary_loops().len(), 1, "one open sheet");
        let r = (plane.position(a) - center).with_y(0.0).length();
        let on_ring = out.positions().filter(|p| ((p.with_y(0.0) - center.with_y(0.0)).length() - r).abs() < 1e-3).count();
        assert!(on_ring >= 7, "{on_ring}");
    }

    #[test]
    fn knife_splits_a_quad_into_two_and_splits_the_neighbours() {
        let cube = primitives::cube(1.0);
        let top = cube.face_ids().find(|&f| cube.face_normal(f).y > 0.9).unwrap();
        let hs: Vec<_> = cube.face_halfedges(top).collect();
        let e0 = cube.halfedge(hs[0]).edge;
        let e2 = cube.halfedge(hs[2]).edge;
        let out = knife_cut(&cube, top, EdgePoint { edge: e0, t: 0.5 }, EdgePoint { edge: e2, t: 0.5 }).unwrap();
        out.check().unwrap();
        assert!(out.is_closed());
        assert_eq!(out.vertex_count(), 10);
        assert_eq!(out.face_count(), 7, "the top became two quads, the two side faces became 5-gons");
        assert_eq!(out.face_ids().filter(|&f| out.face_sides(f) == 5).count(), 2);
    }

    #[test]
    fn shrink_fatten_moves_only_the_selection() {
        let cube = primitives::cube(1.0);
        let top: BTreeSet<VertexId> = cube.vertex_ids().filter(|&v| cube.position(v).y > 0.0).collect();
        let out = shrink_fatten_vertices(&cube, &top, 0.5).unwrap();
        for v in out.vertex_ids() {
            let p = out.position(v);
            if cube.position(v).y > 0.0 {
                assert!(p.y > 0.5 + 0.2, "{p}");
            } else {
                assert_eq!(p, cube.position(v));
            }
        }
    }
}
