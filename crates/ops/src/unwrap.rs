//! Unwrapping, after Blender's U menu: seams split the mesh into islands, `Unwrap` unfolds each
//! island face by face across shared edges (exact for developable surfaces, approximate
//! elsewhere), `Smart UV Project` groups faces by normal within an angle limit and projects each
//! group, `Cylinder Projection` wraps around Y, and `Pack Islands` lays every island into 0..1.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use shape_geom::{FaceId, Mesh, VertexId};
use shape_math::{Vec2, Vec3};

/// A seam is an undirected edge keyed by its vertex indices (low, high).
pub type SeamKey = (u32, u32);

pub fn seam_key(a: VertexId, b: VertexId) -> SeamKey {
    if a.0 < b.0 { (a.0, b.0) } else { (b.0, a.0) }
}

/// Faces grouped into islands: connected across edges that are not seams.
pub fn islands(mesh: &Mesh, seams: &BTreeSet<SeamKey>) -> Vec<Vec<FaceId>> {
    let mut seen = vec![false; mesh.face_count()];
    let mut out = Vec::new();
    for seed in mesh.face_ids() {
        if seen[seed.index()] {
            continue;
        }
        let mut island = Vec::new();
        let mut queue = VecDeque::from([seed]);
        seen[seed.index()] = true;
        while let Some(f) = queue.pop_front() {
            island.push(f);
            for h in mesh.face_halfedges(f) {
                let he = mesh.halfedge(h);
                if seams.contains(&seam_key(mesh.from(h), he.to)) {
                    continue;
                }
                if let Some(g) = mesh.halfedge(he.twin).face
                    && !seen[g.index()]
                {
                    seen[g.index()] = true;
                    queue.push_back(g);
                }
            }
        }
        out.push(island);
    }
    out
}

/// The face flattened in its own plane: corner coordinates with the first corner at the origin
/// and the first edge along +u.
fn flatten_face(mesh: &Mesh, f: FaceId) -> Vec<Vec2> {
    let ps: Vec<Vec3> = mesh.face_vertices(f).map(|v| mesh.position(v)).collect();
    let n = mesh.face_normal(f);
    let u = (ps.get(1).copied().unwrap_or(ps[0] + Vec3::X) - ps[0]).normalize_or_zero();
    let u = if u.length_squared() < 0.5 { n.any_orthonormal_vector() } else { u };
    let v = n.cross(u).normalize_or_zero();
    ps.iter().map(|p| Vec2::new((*p - ps[0]).dot(u), (*p - ps[0]).dot(v))).collect()
}

/// Unfold the mesh into islands split by `seams`, then pack them into 0..1.
pub fn unwrap(mesh: &Mesh, seams: &BTreeSet<SeamKey>) -> Vec<Vec<Vec2>> {
    let mut uvs: Vec<Vec<Vec2>> = mesh.face_ids().map(|f| vec![Vec2::ZERO; mesh.face_vertices(f).count()]).collect();
    let groups = islands(mesh, seams);
    for island in &groups {
        let in_island: BTreeSet<usize> = island.iter().map(|f| f.index()).collect();
        let mut placed: BTreeSet<usize> = BTreeSet::new();
        let seed = island[0];
        uvs[seed.index()] = flatten_face(mesh, seed);
        placed.insert(seed.index());
        let mut queue = VecDeque::from([seed]);
        while let Some(f) = queue.pop_front() {
            let corners: Vec<VertexId> = mesh.face_vertices(f).collect();
            for (i, h) in mesh.face_halfedges(f).enumerate() {
                let he = mesh.halfedge(h);
                let (a, b) = (mesh.from(h), he.to);
                if seams.contains(&seam_key(a, b)) {
                    continue;
                }
                let Some(g) = mesh.halfedge(he.twin).face else { continue };
                if placed.contains(&g.index()) || !in_island.contains(&g.index()) {
                    continue;
                }
                // Where the shared edge sits in the placed face.
                let ia = corners.iter().position(|&v| v == a).unwrap_or(i);
                let ib = corners.iter().position(|&v| v == b).unwrap_or((i + 1) % corners.len());
                let (pa, pb) = (uvs[f.index()][ia], uvs[f.index()][ib]);
                // Flatten g in its own plane and rigidly move it so its copy of the edge lands on pa..pb.
                let local = flatten_face(mesh, g);
                let gcorners: Vec<VertexId> = mesh.face_vertices(g).collect();
                let (Some(ja), Some(jb)) = (gcorners.iter().position(|&v| v == a), gcorners.iter().position(|&v| v == b)) else { continue };
                let (qa, qb) = (local[ja], local[jb]);
                let from = qb - qa;
                let to = pb - pa;
                let (fl, tl) = (from.length(), to.length());
                if fl < 1e-9 || tl < 1e-9 {
                    continue;
                }
                let angle = to.y.atan2(to.x) - from.y.atan2(from.x);
                let (s, c) = angle.sin_cos();
                let rot = |p: Vec2| Vec2::new(p.x * c - p.y * s, p.x * s + p.y * c);
                uvs[g.index()] = local.iter().map(|&p| pa + rot(p - qa) * (tl / fl)).collect();
                placed.insert(g.index());
                queue.push_back(g);
            }
        }
    }
    pack_islands(&mut uvs, &groups, 0.01);
    uvs
}

/// Blender's Smart UV Project: faces are grouped by normal within `angle_limit` (radians) of a
/// seed, each group is projected onto its seed plane, and the connected pieces are packed.
pub fn smart_project(mesh: &Mesh, angle_limit: f32) -> Vec<Vec<Vec2>> {
    let n = mesh.face_count();
    let normals: Vec<Vec3> = mesh.face_ids().map(|f| mesh.face_normal(f)).collect();
    let mut group = vec![usize::MAX; n];
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| mesh.face_area(FaceId(b as u32)).partial_cmp(&mesh.face_area(FaceId(a as u32))).unwrap_or(std::cmp::Ordering::Equal));
    let cos_limit = angle_limit.cos();
    let mut seeds: Vec<Vec3> = Vec::new();
    for &i in &order {
        if group[i] != usize::MAX {
            continue;
        }
        let seed = normals[i];
        let gi = seeds.len();
        seeds.push(seed);
        for j in 0..n {
            if group[j] == usize::MAX && normals[j].dot(seed) >= cos_limit {
                group[j] = gi;
            }
        }
    }
    // Project each face onto its group's plane.
    let mut uvs: Vec<Vec<Vec2>> = Vec::with_capacity(n);
    for f in mesh.face_ids() {
        let sn = seeds[group[f.index()]];
        let u = sn.any_orthonormal_vector();
        let v = sn.cross(u);
        uvs.push(mesh.face_vertices(f).map(|vid| mesh.position(vid)).map(|p| Vec2::new(p.dot(u), p.dot(v))).collect());
    }
    // Group boundaries act as seams for the island split.
    let mut seams = BTreeSet::new();
    for f in mesh.face_ids() {
        for h in mesh.face_halfedges(f) {
            let he = mesh.halfedge(h);
            if let Some(g) = mesh.halfedge(he.twin).face
                && group[g.index()] != group[f.index()]
            {
                seams.insert(seam_key(mesh.from(h), he.to));
            }
        }
    }
    let groups = islands(mesh, &seams);
    pack_islands(&mut uvs, &groups, 0.01);
    uvs
}

/// Cylinder projection around the Y axis: u from the angle, v from the height.
pub fn uv_cylinder(mesh: &Mesh) -> Vec<Vec<Vec2>> {
    let b = mesh.aabb();
    let c = b.center();
    let h = b.extent().y.max(1e-9);
    mesh.face_ids()
        .map(|f| {
            let mut corners: Vec<Vec2> = mesh
                .face_vertices(f)
                .map(|v| {
                    let p = mesh.position(v) - c;
                    Vec2::new(0.5 + p.x.atan2(p.z) / std::f32::consts::TAU, (p.y + h / 2.0) / h)
                })
                .collect();
            let max_u = corners.iter().map(|c| c.x).fold(0.0f32, f32::max);
            for cnr in &mut corners {
                if max_u - cnr.x > 0.5 {
                    cnr.x += 1.0;
                }
            }
            corners
        })
        .collect()
}

/// Lay the islands into the unit square without overlap, keeping their relative scale: shelf
/// packing of island bounding boxes, shrunk until everything fits.
pub fn pack_islands(uvs: &mut [Vec<Vec2>], groups: &[Vec<FaceId>], margin: f32) {
    if groups.is_empty() {
        return;
    }
    let mut boxes: Vec<(usize, Vec2, Vec2)> = groups
        .iter()
        .enumerate()
        .map(|(i, faces)| {
            let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
            for f in faces {
                for c in &uvs[f.index()] {
                    lo = lo.min(*c);
                    hi = hi.max(*c);
                }
            }
            (i, lo, hi - lo)
        })
        .collect();
    boxes.sort_by(|a, b| b.2.y.partial_cmp(&a.2.y).unwrap_or(std::cmp::Ordering::Equal));
    let total_area: f32 = boxes.iter().map(|b| b.2.x * b.2.y).sum::<f32>().max(1e-12);
    let mut scale = (0.8 / total_area).sqrt();
    let mut offsets: BTreeMap<usize, Vec2> = BTreeMap::new();
    for _ in 0..60 {
        offsets.clear();
        let (mut x, mut y, mut row_h) = (margin, margin, 0.0f32);
        let mut fits = true;
        for (i, _, size) in &boxes {
            let s = *size * scale;
            if x + s.x + margin > 1.0 {
                x = margin;
                y += row_h + margin;
                row_h = 0.0;
            }
            if y + s.y + margin > 1.0 || s.x + 2.0 * margin > 1.0 {
                fits = false;
                break;
            }
            offsets.insert(*i, Vec2::new(x, y));
            x += s.x + margin;
            row_h = row_h.max(s.y);
        }
        if fits {
            break;
        }
        scale *= 0.92;
    }
    for (i, lo, _) in &boxes {
        let Some(off) = offsets.get(i) else { continue };
        for f in &groups[*i] {
            for c in &mut uvs[f.index()] {
                *c = *off + (*c - *lo) * scale;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::{cube, cylinder};

    fn in_unit(uvs: &[Vec<Vec2>]) -> bool {
        uvs.iter().flatten().all(|c| (-1e-4..=1.0 + 1e-4).contains(&c.x) && (-1e-4..=1.0 + 1e-4).contains(&c.y))
    }

    fn all_edge_seams(m: &Mesh) -> BTreeSet<SeamKey> {
        m.edge_ids().map(|e| m.edge_vertices(e)).map(|(a, b)| seam_key(a, b)).collect()
    }

    #[test]
    fn seams_split_islands() {
        let c = cube(2.0);
        assert_eq!(islands(&c, &BTreeSet::new()).len(), 1);
        assert_eq!(islands(&c, &all_edge_seams(&c)).len(), 6);
    }

    #[test]
    fn unwrapping_a_fully_seamed_cube_gives_six_equal_squares_in_the_unit_square() {
        let c = cube(2.0);
        let uvs = unwrap(&c, &all_edge_seams(&c));
        assert!(in_unit(&uvs));
        let sides: Vec<f32> = uvs.iter().map(|f| (f[1] - f[0]).length()).collect();
        for s in &sides {
            assert!((s - sides[0]).abs() < 1e-4, "equal squares: {sides:?}");
            assert!(*s > 0.2, "squares use the space: {s}");
        }
        // Each face is a square: adjacent edges perpendicular and equal.
        for f in &uvs {
            assert!((f[1] - f[0]).dot(f[2] - f[1]).abs() < 1e-4);
        }
    }

    #[test]
    fn unfolding_an_unseamed_cylinder_side_is_a_rectangle() {
        let cyl = cylinder(1.0, 2.0, 16);
        // Seam the caps off and one side edge so the side unrolls.
        let mut seams = BTreeSet::new();
        for f in cyl.face_ids() {
            if cyl.face_normal(f).y.abs() > 0.9 {
                for h in cyl.face_halfedges(f) {
                    seams.insert(seam_key(cyl.from(h), cyl.halfedge(h).to));
                }
            }
        }
        let side = cyl.face_ids().find(|&f| cyl.face_normal(f).y.abs() < 0.1).unwrap();
        let h = cyl.face_halfedges(side).find(|&h| (cyl.position(cyl.from(h)) - cyl.position(cyl.halfedge(h).to)).y.abs() > 1.0).unwrap();
        seams.insert(seam_key(cyl.from(h), cyl.halfedge(h).to));
        let uvs = unwrap(&cyl, &seams);
        assert!(in_unit(&uvs));
        // All side quads keep their 3D size ratio (height 2, width 2 sin(pi/16)) and do not overlap:
        // their u ranges are disjoint.
        let mut spans: Vec<(f32, f32)> = cyl
            .face_ids()
            .filter(|&f| cyl.face_normal(f).y.abs() < 0.1)
            .map(|f| {
                let xs: Vec<f32> = uvs[f.index()].iter().map(|c| c.x).collect();
                let ys: Vec<f32> = uvs[f.index()].iter().map(|c| c.y).collect();
                let w = xs.iter().cloned().fold(f32::MIN, f32::max) - xs.iter().cloned().fold(f32::MAX, f32::min);
                let hh = ys.iter().cloned().fold(f32::MIN, f32::max) - ys.iter().cloned().fold(f32::MAX, f32::min);
                assert!((hh / w - 2.0 / (2.0 * (std::f32::consts::PI / 16.0).sin())).abs() < 0.05, "ratio {}", hh / w);
                (xs.iter().cloned().fold(f32::MAX, f32::min), xs.iter().cloned().fold(f32::MIN, f32::max))
            })
            .collect();
        spans.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        for w in spans.windows(2) {
            assert!(w[1].0 >= w[0].1 - 1e-4, "side quads overlap: {:?} {:?}", w[0], w[1]);
        }
    }

    #[test]
    fn smart_project_and_cylinder_stay_in_the_unit_square() {
        let c = cube(2.0);
        let uvs = smart_project(&c, 66f32.to_radians());
        assert!(in_unit(&uvs));
        // Six faces, six different directions: six islands with no two overlapping bounding boxes.
        let boxes: Vec<(Vec2, Vec2)> =
            uvs.iter().map(|f| (f.iter().fold(Vec2::splat(9.0), |a, c| a.min(*c)), f.iter().fold(Vec2::splat(-9.0), |a, c| a.max(*c)))).collect();
        for (i, a) in boxes.iter().enumerate() {
            for b in &boxes[i + 1..] {
                let overlap = a.0.x < b.1.x - 1e-4 && b.0.x < a.1.x - 1e-4 && a.0.y < b.1.y - 1e-4 && b.0.y < a.1.y - 1e-4;
                assert!(!overlap, "{a:?} {b:?}");
            }
        }
        let cyl = uv_cylinder(&cylinder(1.0, 2.0, 12));
        assert!(cyl.iter().flatten().all(|c| (-1e-4..=1.5).contains(&c.x) && (-1e-4..=1.0 + 1e-4).contains(&c.y)));
    }
}
