//! UV generation: projections that give every face corner a coordinate in 0..1.

use shape_geom::Mesh;
use shape_math::{Aabb, Vec2, Vec3};

/// Project along one axis (0 X, 1 Y, 2 Z): the other two coordinates, normalised to the mesh
/// bounds. Right for floors and walls.
pub fn uv_planar(mesh: &Mesh, axis: usize) -> Vec<Vec<Vec2>> {
    let b = mesh.aabb();
    let (u_axis, v_axis) = match axis {
        0 => (2, 1),
        1 => (0, 2),
        _ => (0, 1),
    };
    mesh.face_ids().map(|f| mesh.face_vertices(f).map(|v| normalise(mesh.position(v), &b, u_axis, v_axis)).collect()).collect()
}

/// Box projection: each face projects along the axis its normal points along most. Every corner
/// lands in 0..1; seams fall on the edges between differently facing faces.
pub fn uv_box(mesh: &Mesh) -> Vec<Vec<Vec2>> {
    let b = mesh.aabb();
    mesh.face_ids()
        .map(|f| {
            let n = mesh.face_normal(f).abs();
            let axis = if n.x >= n.y && n.x >= n.z {
                0
            } else if n.y >= n.z {
                1
            } else {
                2
            };
            let (u_axis, v_axis) = match axis {
                0 => (2, 1),
                1 => (0, 2),
                _ => (0, 1),
            };
            mesh.face_vertices(f).map(|v| normalise(mesh.position(v), &b, u_axis, v_axis)).collect()
        })
        .collect()
}

/// Spherical projection around the mesh centre, Y up: longitude -> u, latitude -> v.
pub fn uv_sphere(mesh: &Mesh) -> Vec<Vec<Vec2>> {
    let c = mesh.aabb().center();
    mesh.face_ids()
        .map(|f| {
            let dirs: Vec<Vec3> = mesh.face_vertices(f).map(|v| (mesh.position(v) - c).normalize_or_zero()).collect();
            let is_pole = |d: &Vec3| d.y.abs() > 0.9999;
            let mut corners: Vec<Vec2> = dirs
                .iter()
                .map(|d| {
                    let u = 0.5 + d.x.atan2(d.z) / std::f32::consts::TAU;
                    let v = 0.5 + d.y.clamp(-1.0, 1.0).asin() / std::f32::consts::PI;
                    Vec2::new(u, v)
                })
                .collect();
            // Faces that cross the seam get their low-u corners wrapped so they stay contiguous.
            let max_u = corners.iter().zip(&dirs).filter(|(_, d)| !is_pole(d)).map(|(c, _)| c.x).fold(0.0f32, f32::max);
            for (cnr, d) in corners.iter_mut().zip(&dirs) {
                if !is_pole(d) && max_u - cnr.x > 0.5 {
                    cnr.x += 1.0;
                }
            }
            // A pole has no longitude of its own: give it the face's average so the triangle is a
            // proper wedge instead of a sliver.
            let (sum, n) = corners.iter().zip(&dirs).filter(|(_, d)| !is_pole(d)).fold((0.0, 0), |(s, n), (c, _)| (s + c.x, n + 1));
            if n > 0 {
                for (cnr, d) in corners.iter_mut().zip(&dirs) {
                    if is_pole(d) {
                        cnr.x = sum / n as f32;
                    }
                }
            }
            corners
        })
        .collect()
}

fn normalise(p: Vec3, b: &Aabb, u_axis: usize, v_axis: usize) -> Vec2 {
    let e = b.extent();
    let norm = |axis: usize| if e[axis] > 1e-9 { (p[axis] - b.min[axis]) / e[axis] } else { 0.5 };
    Vec2::new(norm(u_axis), norm(v_axis))
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::{cube, plane, uv_sphere as sphere};

    fn in_unit_square(uvs: &[Vec<Vec2>]) -> bool {
        uvs.iter().flatten().all(|c| (-1e-5..=1.0 + 1e-5).contains(&c.x) && (-1e-5..=1.0 + 1e-5).contains(&c.y))
    }

    #[test]
    fn planar_plane_fills_the_square() {
        let p = plane(2.0, 2, 2);
        let uvs = uv_planar(&p, 1);
        assert_eq!(uvs.len(), p.face_count());
        assert!(in_unit_square(&uvs));
        let corners: Vec<Vec2> = uvs.iter().flatten().copied().collect();
        assert!(corners.iter().any(|c| c.x < 1e-5 && c.y < 1e-5));
        assert!(corners.iter().any(|c| (c.x - 1.0).abs() < 1e-5 && (c.y - 1.0).abs() < 1e-5));
    }

    #[test]
    fn box_cube_every_face_is_a_full_square() {
        let c = cube(2.0);
        let uvs = uv_box(&c);
        assert!(in_unit_square(&uvs));
        for face in &uvs {
            let b = Aabb::from_points(face.iter().map(|c| Vec3::new(c.x, c.y, 0.0)));
            assert!((b.extent().x - 1.0).abs() < 1e-5 && (b.extent().y - 1.0).abs() < 1e-5, "{face:?}");
        }
    }

    #[test]
    fn sphere_corners_stay_contiguous_across_the_seam() {
        let s = sphere(1.0, 16, 8);
        let uvs = uv_sphere(&s);
        for face in &uvs {
            let min = face.iter().map(|c| c.x).fold(f32::INFINITY, f32::min);
            let max = face.iter().map(|c| c.x).fold(f32::NEG_INFINITY, f32::max);
            assert!(max - min < 0.5, "face spans the seam: {face:?}");
            assert!(face.iter().all(|c| (0.0..=1.0 + 1e-5).contains(&c.y)));
        }
    }
}
