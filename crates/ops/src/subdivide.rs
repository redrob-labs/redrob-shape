//! Subdivision surfaces.

use crate::{PolyMesh, Result};
use shape_geom::Mesh;
use shape_math::Vec3;
use std::collections::HashMap;

/// One level of Catmull-Clark. Every n-gon becomes n quads. Boundary edges and vertices use the
/// crease rules (midpoints and the 1/8-3/4-1/8 mask) so open meshes keep a smooth outline.
pub fn catmull_clark(mesh: &Mesh) -> Result<Mesh> {
    subdivide(mesh, true)
}

/// Same topology as Catmull-Clark, no smoothing: positions stay on the original surface.
pub fn subdivide_linear(mesh: &Mesh) -> Result<Mesh> {
    subdivide(mesh, false)
}

fn subdivide(mesh: &Mesh, smooth: bool) -> Result<Mesh> {
    let pm = PolyMesh::from_mesh(mesh);
    let nv = pm.positions.len();
    let mut out = PolyMesh::default();

    // Face points.
    let face_point: Vec<u32> = pm.polygons.iter().map(|poly| out.push_vertex(pm.polygon_centroid(poly))).collect();

    // Edge points, keyed by unordered vertex pair.
    let key = |a: u32, b: u32| if a < b { (a, b) } else { (b, a) };
    let mut edge_faces: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (fi, poly) in pm.polygons.iter().enumerate() {
        for i in 0..poly.len() {
            edge_faces.entry(key(poly[i], poly[(i + 1) % poly.len()])).or_default().push(fi);
        }
    }
    let mut edge_point: HashMap<(u32, u32), u32> = HashMap::new();
    // Reserve vertex slots for the new vertex positions first so indices are stable.
    let vertex_new_base = out.positions.len() as u32 + edge_faces.len() as u32;
    let mut edge_keys: Vec<(u32, u32)> = edge_faces.keys().copied().collect();
    edge_keys.sort_unstable();
    for k in &edge_keys {
        let (a, b) = *k;
        let mid = (pm.positions[a as usize] + pm.positions[b as usize]) * 0.5;
        let faces = &edge_faces[k];
        let p = if smooth && faces.len() == 2 {
            let f = (out.positions[face_point[faces[0]] as usize] + out.positions[face_point[faces[1]] as usize]) * 0.5;
            (mid + f) * 0.5
        } else {
            mid
        };
        edge_point.insert(*k, out.push_vertex(p));
    }
    debug_assert_eq!(out.positions.len() as u32, vertex_new_base);

    // New positions for the original vertices.
    let mut adjacent_faces: Vec<Vec<usize>> = vec![Vec::new(); nv];
    let mut adjacent_edges: Vec<Vec<(u32, u32)>> = vec![Vec::new(); nv];
    for (fi, poly) in pm.polygons.iter().enumerate() {
        for i in 0..poly.len() {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            adjacent_faces[a as usize].push(fi);
            let k = key(a, b);
            for v in [a, b] {
                if !adjacent_edges[v as usize].contains(&k) {
                    adjacent_edges[v as usize].push(k);
                }
            }
        }
    }
    for v in 0..nv {
        let p = pm.positions[v];
        let boundary_edges: Vec<&(u32, u32)> = adjacent_edges[v].iter().filter(|k| edge_faces[k].len() == 1).collect();
        let new_p = if !smooth || adjacent_faces[v].is_empty() {
            p
        } else if !boundary_edges.is_empty() {
            // Crease rule: average of the two boundary neighbours' midpoints weighted 1/8 each.
            if boundary_edges.len() == 2 {
                let other = |k: &(u32, u32)| if k.0 == v as u32 { k.1 } else { k.0 };
                let a = pm.positions[other(boundary_edges[0]) as usize];
                let b = pm.positions[other(boundary_edges[1]) as usize];
                p * 0.75 + (a + b) * 0.125
            } else {
                p
            }
        } else {
            let n = adjacent_faces[v].len() as f32;
            let f = adjacent_faces[v].iter().map(|&fi| out.positions[face_point[fi] as usize]).sum::<Vec3>() / n;
            let r = adjacent_edges[v].iter().map(|k| (pm.positions[k.0 as usize] + pm.positions[k.1 as usize]) * 0.5).sum::<Vec3>()
                / adjacent_edges[v].len() as f32;
            (f + r * 2.0 + p * (n - 3.0)) / n
        };
        out.push_vertex(new_p);
    }

    // Quads: for each corner v of face f: [v', e(prev,v), f, e(v,next)].
    for (fi, poly) in pm.polygons.iter().enumerate() {
        let n = poly.len();
        for i in 0..n {
            let v = poly[i];
            let prev = poly[(i + n - 1) % n];
            let next = poly[(i + 1) % n];
            out.polygons.push(vec![vertex_new_base + v, edge_point[&key(v, next)], face_point[fi], edge_point[&key(prev, v)]]);
        }
    }
    out.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::{cube, plane, uv_sphere};

    #[test]
    fn cube_once_is_24_quads_and_rounder() {
        let c = cube(2.0);
        let m = catmull_clark(&c).unwrap();
        m.check().unwrap();
        assert!(m.is_closed());
        assert_eq!((m.vertex_count(), m.edge_count(), m.face_count()), (26, 48, 24));
        assert_eq!(m.euler_characteristic(), 2);
        assert!(m.face_ids().all(|f| m.face_sides(f) == 4));
        // corners pull in, face centres stay on the surface
        assert!(m.aabb().max.x < 1.0 + 1e-6);
        let corner = m.positions().map(|p| p.length()).fold(0.0f32, f32::max);
        assert!(corner < 3f32.sqrt(), "corner {corner} did not round");
        let twice = catmull_clark(&m).unwrap();
        assert_eq!(twice.face_count(), 96);
        assert_eq!(twice.euler_characteristic(), 2);
    }

    #[test]
    fn linear_keeps_positions() {
        let c = cube(2.0);
        let m = subdivide_linear(&c).unwrap();
        assert_eq!(m.face_count(), 24);
        assert!((m.aabb().max - c.aabb().max).length() < 1e-6);
    }

    #[test]
    fn open_plane_keeps_its_outline_and_sphere_stays_a_sphere() {
        let p = plane(2.0, 2, 2);
        let m = catmull_clark(&p).unwrap();
        m.check().unwrap();
        assert_eq!(m.face_count(), 16);
        assert_eq!(m.boundary_loops().len(), 1);
        // the four corner vertices of a flat plane move along the boundary only
        assert!(m.positions().all(|q| q.y.abs() < 1e-6));
        let s = uv_sphere(1.0, 16, 8);
        let m = catmull_clark(&s).unwrap();
        m.check().unwrap();
        assert_eq!(m.euler_characteristic(), 2);
        assert!(m.positions().all(|q| q.length() > 0.9 && q.length() <= 1.0 + 1e-5));
    }
}
