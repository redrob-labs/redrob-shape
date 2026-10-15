//! Metaballs, after Blender's: a set of spheres with a smooth falloff whose summed field is
//! polygonised at a threshold, so nearby balls merge into blobs. The surface comes from marching
//! tetrahedra on a regular grid (each cell split into six tetrahedra; a tetrahedron with mixed
//! signs yields one or two triangles), with vertices welded along shared edges.

use std::collections::HashMap;

use shape_geom::Mesh;
use shape_math::Vec3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetaElement {
    pub position: Vec3,
    pub radius: f32,
    /// How steeply the ball's influence falls off (Blender's Stiffness, 1..10).
    pub stiffness: f32,
}

/// Field strength at `p`: Wyvill's soft falloff, zero beyond the radius.
pub fn field(elements: &[MetaElement], p: Vec3) -> f32 {
    elements
        .iter()
        .map(|e| {
            let r2 = (e.radius * e.radius).max(1e-9);
            let d2 = p.distance_squared(e.position);
            if d2 >= r2 {
                0.0
            } else {
                let t = 1.0 - d2 / r2;
                e.stiffness * t * t * t
            }
        })
        .sum()
}

fn empty() -> Mesh {
    Mesh::from_polygons(Vec::new(), &[]).unwrap_or_else(|_| shape_geom::primitives::cube(0.0))
}

/// Polygonise `field >= threshold` on a grid of `resolution` cells across the elements' bounds.
pub fn polygonise(elements: &[MetaElement], threshold: f32, resolution: u32) -> Mesh {
    if elements.is_empty() {
        return empty();
    }
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    for e in elements {
        lo = lo.min(e.position - Vec3::splat(e.radius));
        hi = hi.max(e.position + Vec3::splat(e.radius));
    }
    let n = resolution.clamp(4, 128) as usize;
    let cell = (hi - lo) / n as f32;
    let lo = lo - cell;
    let nx = n + 2;
    let at = |i: usize, j: usize, k: usize| lo + Vec3::new(i as f32, j as f32, k as f32) * cell;
    let idx = |i: usize, j: usize, k: usize| (i * (nx + 1) + j) * (nx + 1) + k;
    let mut values = vec![0.0f32; (nx + 1) * (nx + 1) * (nx + 1)];
    for i in 0..=nx {
        for j in 0..=nx {
            for k in 0..=nx {
                values[idx(i, j, k)] = field(elements, at(i, j, k)) - threshold;
            }
        }
    }
    let mut positions: Vec<Vec3> = Vec::new();
    let mut faces: Vec<Vec<u32>> = Vec::new();
    let mut weld: HashMap<[i64; 3], u32> = HashMap::new();
    let mut vertex = |p: Vec3, positions: &mut Vec<Vec3>| -> u32 {
        let key = [(p.x * 1e5).round() as i64, (p.y * 1e5).round() as i64, (p.z * 1e5).round() as i64];
        *weld.entry(key).or_insert_with(|| {
            positions.push(p);
            positions.len() as u32 - 1
        })
    };
    // The six tetrahedra of a cube, as corner indices (corner bits: x, y, z).
    // Kuhn's split around the main diagonal 0-7; neighbouring cubes split shared faces alike.
    const TETS: [[usize; 4]; 6] = [[0, 1, 3, 7], [0, 3, 2, 7], [0, 2, 6, 7], [0, 6, 4, 7], [0, 4, 5, 7], [0, 5, 1, 7]];
    for i in 0..nx {
        for j in 0..nx {
            for k in 0..nx {
                let corners: [(Vec3, f32); 8] = std::array::from_fn(|c| {
                    let (di, dj, dk) = (c & 1, (c >> 1) & 1, (c >> 2) & 1);
                    (at(i + di, j + dj, k + dk), values[idx(i + di, j + dj, k + dk)])
                });
                for tet in TETS {
                    let v: [(Vec3, f32); 4] = [corners[tet[0]], corners[tet[1]], corners[tet[2]], corners[tet[3]]];
                    let inside: Vec<usize> = (0..4).filter(|&a| v[a].1 >= 0.0).collect();
                    let outside: Vec<usize> = (0..4).filter(|&a| v[a].1 < 0.0).collect();
                    let cross = |a: usize, b: usize| {
                        let (pa, fa) = v[a];
                        let (pb, fb) = v[b];
                        let t = (fa / (fa - fb)).clamp(0.0, 1.0);
                        pa + (pb - pa) * t
                    };
                    let mut tri = |a: Vec3, b: Vec3, c: Vec3| {
                        let ia = vertex(a, &mut positions);
                        let ib = vertex(b, &mut positions);
                        let ic = vertex(c, &mut positions);
                        if ia == ib || ib == ic || ia == ic {
                            return;
                        }
                        faces.push(vec![ia, ib, ic]);
                    };
                    match (inside.len(), outside.len()) {
                        (1, 3) => {
                            let p = inside[0];
                            tri(cross(p, outside[0]), cross(p, outside[1]), cross(p, outside[2]));
                        }
                        (3, 1) => {
                            let q = outside[0];
                            tri(cross(inside[0], q), cross(inside[1], q), cross(inside[2], q));
                        }
                        (2, 2) => {
                            let (a, b) = (inside[0], inside[1]);
                            let (c, d) = (outside[0], outside[1]);
                            let (ac, ad, bc, bd) = (cross(a, c), cross(a, d), cross(b, c), cross(b, d));
                            tri(ac, ad, bd);
                            tri(ac, bd, bc);
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    let faces = orient_consistently(&positions, faces);
    Mesh::from_polygons(positions, &faces).unwrap_or_else(|_| empty())
}

/// Make neighbouring triangles agree on winding (flood fill across shared edges), then flip the
/// whole surface outwards when its signed volume is negative.
fn orient_consistently(positions: &[Vec3], mut faces: Vec<Vec<u32>>) -> Vec<Vec<u32>> {
    let key = |a: u32, b: u32| if a < b { (a, b) } else { (b, a) };
    let mut by_edge: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
    for (fi, f) in faces.iter().enumerate() {
        for i in 0..3 {
            by_edge.entry(key(f[i], f[(i + 1) % 3])).or_default().push(fi);
        }
    }
    let has_directed = |f: &[u32], a: u32, b: u32| (0..3).any(|i| f[i] == a && f[(i + 1) % 3] == b);
    let mut visited = vec![false; faces.len()];
    for seed in 0..faces.len() {
        if visited[seed] {
            continue;
        }
        visited[seed] = true;
        let mut stack = vec![seed];
        while let Some(fi) = stack.pop() {
            let f = faces[fi].clone();
            for i in 0..3 {
                let (a, b) = (f[i], f[(i + 1) % 3]);
                for &g in &by_edge[&key(a, b)] {
                    if g == fi || visited[g] {
                        continue;
                    }
                    visited[g] = true;
                    // A consistent neighbour runs the shared edge the other way.
                    if has_directed(&faces[g], a, b) {
                        faces[g].swap(1, 2);
                    }
                    stack.push(g);
                }
            }
        }
    }
    let volume: f32 = faces
        .iter()
        .map(|f| {
            let (a, b, c) = (positions[f[0] as usize], positions[f[1] as usize], positions[f[2] as usize]);
            a.dot(b.cross(c)) / 6.0
        })
        .sum();
    if volume < 0.0 {
        for f in &mut faces {
            f.swap(1, 2);
        }
    }
    faces
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_ball_is_a_closed_blob_of_about_the_right_size() {
        let m = polygonise(&[MetaElement { position: Vec3::ZERO, radius: 1.0, stiffness: 2.0 }], 0.6, 24);
        assert!(m.face_count() > 100 && m.is_closed());
        // field = 2 (1 - d^2)^3 = 0.6  ->  d = sqrt(1 - 0.3^(1/3)) ~ 0.57
        let expect = (1.0 - 0.3f32.cbrt()).sqrt();
        for p in m.positions() {
            assert!((p.length() - expect).abs() < 0.08, "{} vs {expect}", p.length());
        }
    }

    #[test]
    fn two_near_balls_merge_into_one_surface_and_far_ones_stay_apart() {
        let two = |dx: f32| {
            polygonise(
                &[
                    MetaElement { position: Vec3::ZERO, radius: 1.0, stiffness: 2.0 },
                    MetaElement { position: Vec3::X * dx, radius: 1.0, stiffness: 2.0 },
                ],
                0.6,
                24,
            )
        };
        let near = two(0.9);
        assert!(near.positions().any(|p| (p.x - 0.45).abs() < 0.1 && p.y.abs() > 0.3), "bridge between the balls");
        let far = two(3.0);
        assert!(!far.positions().any(|p| (p.x - 1.5).abs() < 0.3), "a gap between far balls");
    }
}
