//! Mesh primitives. Every one is counter-clockwise from outside, centred on the origin, Y up.

use crate::Mesh;
use shape_math::Vec3;
use std::f32::consts::TAU;

/// Axis-aligned cube with the given edge length, six quads.
pub fn cube(size: f32) -> Mesh {
    let h = size * 0.5;
    let p = |x: f32, y: f32, z: f32| Vec3::new(x * h, y * h, z * h);
    let positions =
        vec![p(-1., -1., -1.), p(1., -1., -1.), p(1., 1., -1.), p(-1., 1., -1.), p(-1., -1., 1.), p(1., -1., 1.), p(1., 1., 1.), p(-1., 1., 1.)];
    let faces = vec![
        vec![0, 3, 2, 1], // -Z
        vec![4, 5, 6, 7], // +Z
        vec![0, 1, 5, 4], // -Y
        vec![3, 7, 6, 2], // +Y
        vec![0, 4, 7, 3], // -X
        vec![1, 2, 6, 5], // +X
    ];
    Mesh::from_polygons(positions, &faces).expect("cube is a valid mesh")
}

/// Flat grid in the XZ plane facing +Y, `size` across, split into `nx` by `nz` quads.
pub fn plane(size: f32, nx: u32, nz: u32) -> Mesh {
    let (nx, nz) = (nx.max(1), nz.max(1));
    let mut positions = Vec::new();
    for j in 0..=nz {
        for i in 0..=nx {
            let x = (i as f32 / nx as f32 - 0.5) * size;
            let z = (j as f32 / nz as f32 - 0.5) * size;
            positions.push(Vec3::new(x, 0.0, z));
        }
    }
    let idx = |i: u32, j: u32| j * (nx + 1) + i;
    let mut faces = Vec::new();
    for j in 0..nz {
        for i in 0..nx {
            // counter-clockwise seen from +Y: x right, z towards the viewer
            faces.push(vec![idx(i, j), idx(i, j + 1), idx(i + 1, j + 1), idx(i + 1, j)]);
        }
    }
    Mesh::from_polygons(positions, &faces).expect("plane is a valid mesh")
}

/// Latitude/longitude sphere: `segments` around, `rings` from pole to pole (>= 2). Triangles at the
/// poles, quads elsewhere.
pub fn uv_sphere(radius: f32, segments: u32, rings: u32) -> Mesh {
    let (segments, rings) = (segments.max(3), rings.max(2));
    let mut positions = vec![Vec3::new(0.0, radius, 0.0)];
    for r in 1..rings {
        let phi = r as f32 / rings as f32 * std::f32::consts::PI;
        let (y, s) = (phi.cos() * radius, phi.sin() * radius);
        for i in 0..segments {
            let theta = i as f32 / segments as f32 * TAU;
            positions.push(Vec3::new(s * theta.sin(), y, s * theta.cos()));
        }
    }
    positions.push(Vec3::new(0.0, -radius, 0.0));
    let bottom = positions.len() as u32 - 1;
    let at = |r: u32, i: u32| 1 + (r - 1) * segments + (i % segments);
    let mut faces = Vec::new();
    for i in 0..segments {
        faces.push(vec![0, at(1, i), at(1, i + 1)]);
    }
    for r in 1..rings - 1 {
        for i in 0..segments {
            faces.push(vec![at(r, i), at(r + 1, i), at(r + 1, i + 1), at(r, i + 1)]);
        }
    }
    for i in 0..segments {
        faces.push(vec![bottom, at(rings - 1, i + 1), at(rings - 1, i)]);
    }
    Mesh::from_polygons(positions, &faces).expect("uv sphere is a valid mesh")
}

/// Ring of radius `major` in the XZ plane with tube radius `minor`; all quads.
pub fn torus(major: f32, minor: f32, segments: u32, sides: u32) -> Mesh {
    let (segments, sides) = (segments.max(3), sides.max(3));
    let mut positions = Vec::new();
    for i in 0..segments {
        let u = i as f32 / segments as f32 * TAU;
        let center = Vec3::new(u.sin() * major, 0.0, u.cos() * major);
        let out = Vec3::new(u.sin(), 0.0, u.cos());
        for j in 0..sides {
            let v = j as f32 / sides as f32 * TAU;
            positions.push(center + out * (v.cos() * minor) + Vec3::Y * (v.sin() * minor));
        }
    }
    let at = |i: u32, j: u32| (i % segments) * sides + (j % sides);
    let mut faces = Vec::new();
    for i in 0..segments {
        for j in 0..sides {
            faces.push(vec![at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)]);
        }
    }
    Mesh::from_polygons(positions, &faces).expect("torus is a valid mesh")
}

/// Closed cylinder along Y with n-gon caps.
pub fn cylinder(radius: f32, height: f32, segments: u32) -> Mesh {
    let segments = segments.max(3);
    let h = height * 0.5;
    let mut positions = Vec::new();
    for &y in &[-h, h] {
        for i in 0..segments {
            let t = i as f32 / segments as f32 * TAU;
            positions.push(Vec3::new(t.sin() * radius, y, t.cos() * radius));
        }
    }
    let mut faces = Vec::new();
    for i in 0..segments {
        let n = (i + 1) % segments;
        faces.push(vec![i, n, segments + n, segments + i]);
    }
    faces.push((0..segments).rev().collect());
    faces.push((segments..2 * segments).collect());
    Mesh::from_polygons(positions, &faces).expect("cylinder is a valid mesh")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_primitive_is_consistent_and_outward() {
        for (name, m) in [
            ("cube", cube(1.0)),
            ("plane", plane(1.0, 2, 2)),
            ("sphere", uv_sphere(1.0, 12, 6)),
            ("torus", torus(1.0, 0.25, 12, 8)),
            ("cylinder", cylinder(0.5, 2.0, 10)),
        ] {
            m.check().unwrap_or_else(|e| panic!("{name}: {e}"));
            for f in m.face_ids() {
                let n = m.face_normal(f);
                let c = m.face_centroid(f);
                // plane's reference is +Y; the others are star-shaped around the origin
                let reference = if name == "plane" {
                    Vec3::Y
                } else if name == "torus" {
                    torus_outward(c)
                } else {
                    c
                };
                assert!(n.dot(reference) > 0.0, "{name}: face {f:?} normal {n} inward (centroid {c})");
            }
        }
    }

    fn torus_outward(c: Vec3) -> Vec3 {
        let ring = Vec3::new(c.x, 0.0, c.z).normalize() * 1.0;
        c - ring
    }

    #[test]
    fn cylinder_counts() {
        let m = cylinder(1.0, 1.0, 8);
        assert_eq!((m.vertex_count(), m.edge_count(), m.face_count()), (16, 24, 10));
        assert_eq!(m.euler_characteristic(), 2);
    }
}
