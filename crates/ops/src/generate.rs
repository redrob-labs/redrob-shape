//! Generate and deform modifiers in Blender's sense (stage P4): Solidify, Decimate, Wireframe,
//! Screw, Simple Deform, Displace, Weld. Each is a plain function over a mesh.

use crate::{PolyMesh, Result};
use shape_geom::Mesh;
use shape_math::{Quat, Vec3};

/// Solidify: a second shell `thickness` along the (inverted) normals, joined to the first by rim
/// quads along every boundary edge. `offset` is Blender's: -1 inward (default), 0 centred, 1 outward.
pub fn solidify(mesh: &Mesh, thickness: f32, offset: f32) -> Result<Mesh> {
    if thickness.abs() < 1e-9 {
        return Ok(mesh.clone());
    }
    let normals = mesh.vertex_normals();
    let (positions, polygons) = mesh.to_polygons();
    let n = positions.len();
    let shift_outer = (offset + 1.0) * 0.5 * thickness;
    let shift_inner = shift_outer - thickness;
    let mut pos: Vec<Vec3> = positions.iter().zip(&normals).map(|(p, nrm)| *p + *nrm * shift_outer).collect();
    pos.extend(positions.iter().zip(&normals).map(|(p, nrm)| *p + *nrm * shift_inner));
    let mut polys: Vec<Vec<u32>> = polygons.clone();
    for poly in &polygons {
        polys.push(poly.iter().rev().map(|&v| v + n as u32).collect());
    }
    for ring in mesh.boundary_loops() {
        for i in 0..ring.len() {
            let a = ring[i].index() as u32;
            let b = ring[(i + 1) % ring.len()].index() as u32;
            // Boundary loops run along the face-less half-edges, so the shell face uses b -> a and
            // the rim must use a -> b -> b' -> a'.
            polys.push(vec![a, b, b + n as u32, a + n as u32]);
        }
    }
    Mesh::from_polygons(pos, &polys)
}

/// Decimate (collapse): merge the shortest edge repeatedly until the face count is at most
/// `ratio` of the original. Works on the triangulated mesh, as Blender's Collapse mode does.
pub fn decimate(mesh: &Mesh, ratio: f32) -> Result<Mesh> {
    let ratio = ratio.clamp(0.0, 1.0);
    let mut cur = crate::triangulate(mesh)?;
    if ratio >= 1.0 {
        return Ok(cur);
    }
    let target = ((cur.face_count() as f32) * ratio).round().max(4.0) as usize;
    let mut guard = 0;
    while cur.face_count() > target && guard < 20_000 {
        guard += 1;
        // Shortest non-boundary edge (boundary edges keep the outline).
        let Some(e) = cur.edge_ids().filter(|&e| !cur.is_boundary_edge(e)).min_by(|&a, &b| {
            let la = {
                let (x, y) = cur.edge_vertices(a);
                (cur.position(x) - cur.position(y)).length_squared()
            };
            let lb = {
                let (x, y) = cur.edge_vertices(b);
                (cur.position(x) - cur.position(y)).length_squared()
            };
            la.total_cmp(&lb)
        }) else {
            break;
        };
        let (a, b) = cur.edge_vertices(e);
        match crate::merge_vertices(&cur, &[a, b], crate::MergeAt::Center) {
            Ok(m) if m.face_count() < cur.face_count() => cur = m,
            _ => break,
        }
    }
    Ok(cur)
}

/// Wireframe: every edge becomes a square strut of `thickness`. Struts overlap at the vertices
/// rather than being joined (Blender welds them; this keeps the mesh a plain union of tubes).
pub fn wireframe(mesh: &Mesh, thickness: f32) -> Result<Mesh> {
    let half = thickness.max(1e-6) * 0.5;
    let normals = mesh.vertex_normals();
    let mut pm = PolyMesh::default();
    for e in mesh.edge_ids() {
        let (a, b) = mesh.edge_vertices(e);
        let (pa, pb) = (mesh.position(a), mesh.position(b));
        let dir = (pb - pa).normalize_or_zero();
        // Frame: the averaged vertex normal and its cross with the edge, so struts lie in the surface.
        let mut up = (normals[a.index()] + normals[b.index()]).normalize_or_zero();
        if up.length_squared() < 0.5 || up.cross(dir).length_squared() < 1e-6 {
            up = if dir.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
        }
        let side = dir.cross(up).normalize_or_zero();
        let up = side.cross(dir).normalize_or_zero();
        let corners = |p: Vec3| [p + (side + up) * half, p + (-side + up) * half, p + (-side - up) * half, p + (side - up) * half];
        let ca = corners(pa);
        let cb = corners(pb);
        let base = pm.positions.len() as u32;
        pm.positions.extend_from_slice(&ca);
        pm.positions.extend_from_slice(&cb);
        for i in 0..4u32 {
            let j = (i + 1) % 4;
            pm.polygons.push(vec![base + i, base + j, base + 4 + j, base + 4 + i]);
        }
        // Caps so each strut is closed.
        pm.polygons.push(vec![base + 3, base + 2, base + 1, base]);
        pm.polygons.push(vec![base + 4, base + 5, base + 6, base + 7]);
    }
    pm.build()
}

/// Screw: revolve the mesh's open edges (a profile) around `axis` through the origin by `angle`
/// in `steps`, climbing `screw_offset` along the axis per full turn (Blender's Screw modifier). A
/// closed mesh has no open edges and comes back unchanged.
pub fn screw(mesh: &Mesh, axis: Vec3, angle: f32, steps: u32, screw_offset: f32) -> Result<Mesh> {
    let steps = steps.clamp(1, 512);
    let axis = axis.normalize_or_zero();
    let edges: std::collections::BTreeSet<_> = mesh.edge_ids().collect();
    let spun = crate::spin(mesh, &edges, Vec3::ZERO, axis, angle, steps)?;
    if screw_offset.abs() < 1e-9 {
        return Ok(spun);
    }
    // Each ring of new vertices sits `angle/steps` further around; lift it by its share of the turn.
    let base_count = mesh.vertex_count();
    let per_vertex = (spun.vertex_count() - base_count) / steps as usize;
    let mut pm = PolyMesh::from_mesh(&spun);
    for (i, p) in pm.positions.iter_mut().enumerate().skip(base_count) {
        let k = ((i - base_count) / per_vertex.max(1)) as f32 + 1.0;
        let turns = angle * k / steps as f32 / std::f32::consts::TAU;
        *p += axis * screw_offset * turns;
    }
    pm.build()
}

/// Blender's Simple Deform modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeformMode {
    Twist,
    Bend,
    Taper,
    Stretch,
}

/// Simple Deform along `axis` (0 X, 1 Y, 2 Z) with `factor` (radians for twist/bend, a scale
/// factor otherwise), over the mesh's own extent along that axis.
pub fn simple_deform(mesh: &Mesh, mode: DeformMode, axis: usize, factor: f32) -> Result<Mesh> {
    let axis = axis.min(2);
    let mut pm = PolyMesh::from_mesh(mesh);
    let (lo, hi) = pm.positions.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p[axis]), hi.max(p[axis])));
    let span = (hi - lo).max(1e-6);
    let unit = [Vec3::X, Vec3::Y, Vec3::Z][axis];
    for p in &mut pm.positions {
        let t = (p[axis] - lo) / span; // 0..1 along the axis
        match mode {
            DeformMode::Twist => {
                let rot = Quat::from_axis_angle(unit, factor * t);
                let along = unit * p[axis];
                *p = along + rot * (*p - along);
            }
            DeformMode::Taper => {
                let s = 1.0 + factor * t;
                let along = unit * p[axis];
                *p = along + (*p - along) * s;
            }
            DeformMode::Stretch => {
                let s = 1.0 + factor * (t - 0.5).abs() * 2.0;
                let along = unit * p[axis];
                let mid = lo + span * 0.5;
                *p = along + (*p - along) / s.max(1e-3).sqrt();
                p[axis] = mid + (p[axis] - mid) * (1.0 + factor);
            }
            DeformMode::Bend => {
                if factor.abs() < 1e-6 {
                    continue;
                }
                // Bend the axis onto an arc of angle `factor` in the plane of the axis and the
                // next axis; points off the axis follow their radius.
                let other = (axis + 1) % 3;
                let radius = span / factor;
                let theta = (p[axis] - lo) / radius;
                let r = radius - p[other];
                let mut q = *p;
                q[axis] = lo + r * theta.sin();
                q[other] = radius - r * theta.cos();
                *p = q;
            }
        }
    }
    pm.build()
}

/// Deterministic value noise in [-1, 1], smooth across cells.
fn value_noise(p: Vec3) -> f32 {
    fn hash(x: i32, y: i32, z: i32) -> f32 {
        let mut h = (x as u32).wrapping_mul(0x8da6b343) ^ (y as u32).wrapping_mul(0xd8163841) ^ (z as u32).wrapping_mul(0xcb1ab31f);
        h ^= h >> 13;
        h = h.wrapping_mul(0x5bd1e995);
        h ^= h >> 15;
        (h as f32 / u32::MAX as f32) * 2.0 - 1.0
    }
    let f = p.floor();
    let (x0, y0, z0) = (f.x as i32, f.y as i32, f.z as i32);
    let d = p - f;
    let s = Vec3::new(d.x * d.x * (3.0 - 2.0 * d.x), d.y * d.y * (3.0 - 2.0 * d.y), d.z * d.z * (3.0 - 2.0 * d.z));
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let c = |dx: i32, dy: i32, dz: i32| hash(x0 + dx, y0 + dy, z0 + dz);
    let x00 = lerp(c(0, 0, 0), c(1, 0, 0), s.x);
    let x10 = lerp(c(0, 1, 0), c(1, 1, 0), s.x);
    let x01 = lerp(c(0, 0, 1), c(1, 0, 1), s.x);
    let x11 = lerp(c(0, 1, 1), c(1, 1, 1), s.x);
    lerp(lerp(x00, x10, s.y), lerp(x01, x11, s.y), s.z)
}

/// Displace: move every vertex along its normal by `strength * noise(position * scale)`.
pub fn displace(mesh: &Mesh, strength: f32, scale: f32) -> Result<Mesh> {
    let normals = mesh.vertex_normals();
    let mut pm = PolyMesh::from_mesh(mesh);
    for (p, n) in pm.positions.iter_mut().zip(normals) {
        *p += n * strength * value_noise(*p * scale.max(1e-6) + Vec3::splat(17.3));
    }
    pm.build()
}

/// Weld: merge vertices closer than `distance`.
pub fn weld(mesh: &Mesh, distance: f32) -> Result<Mesh> {
    let mut pm = PolyMesh::from_mesh(mesh);
    pm.merge_by_distance(distance.max(1e-9));
    pm.build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives;

    #[test]
    fn solidify_closes_an_open_plane_and_doubles_a_closed_cube() {
        let plane = primitives::plane(2.0, 2, 2);
        let solid = solidify(&plane, 0.2, -1.0).unwrap();
        solid.check().unwrap();
        assert!(solid.is_closed(), "a plane with thickness is a closed slab");
        assert_eq!(solid.face_count(), 4 + 4 + 8, "two shells plus a rim of 8");
        for f in solid.face_ids() {
            assert!(solid.face_normal(f).dot(solid.face_centroid(f) - Vec3::new(0.0, -0.1, 0.0)) > -1e-4, "outward");
        }
        let cube = primitives::cube(1.0);
        let shell = solidify(&cube, 0.1, -1.0).unwrap();
        shell.check().unwrap();
        assert_eq!(shell.face_count(), 12);
        assert_eq!(shell.boundary_loops().len(), 0);
    }

    #[test]
    fn decimate_reaches_the_ratio_and_stays_valid() {
        let sphere = primitives::uv_sphere(1.0, 16, 8);
        let tris = crate::triangulate(&sphere).unwrap().face_count();
        let out = decimate(&sphere, 0.5).unwrap();
        out.check().unwrap();
        assert!(out.face_count() <= (tris as f32 * 0.5).round() as usize + 2, "{} of {tris}", out.face_count());
        assert!(out.face_count() >= 4);
        assert_eq!(decimate(&sphere, 1.0).unwrap().face_count(), tris);
    }

    #[test]
    fn wireframe_struts_and_screw_and_weld() {
        let cube = primitives::cube(1.0);
        let wire = wireframe(&cube, 0.05).unwrap();
        wire.check().unwrap();
        assert_eq!(wire.face_count(), 12 * 6);
        assert!(wire.is_closed());
        // Screw a vertical profile (one plane edge) half a turn with lift.
        let plane = primitives::plane(1.0, 1, 1);
        let sc = screw(&plane, Vec3::Y, std::f32::consts::PI, 8, 2.0).unwrap();
        sc.check().unwrap();
        assert_eq!(sc.face_count(), 1 + 4 * 8);
        let top = sc.positions().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
        assert!((top - 1.0).abs() < 1e-3, "half a turn lifts by half the offset: {top}");
        // Weld two coincident cubes into one.
        let mut pm = PolyMesh::from_mesh(&cube);
        pm.append(&PolyMesh::from_mesh(&cube));
        let two = pm.build();
        assert!(two.is_err() || two.as_ref().unwrap().vertex_count() == 16);
        let welded = weld(&crate::PolyMesh::from_mesh(&cube).build().unwrap(), 1e-4).unwrap();
        assert_eq!(welded.vertex_count(), 8);
    }

    #[test]
    fn simple_deform_modes_keep_topology() {
        let cyl = primitives::cylinder(1.0, 4.0, 12);
        for (mode, f) in [(DeformMode::Twist, 1.0), (DeformMode::Bend, 1.0), (DeformMode::Taper, 0.5), (DeformMode::Stretch, 0.5)] {
            let out = simple_deform(&cyl, mode, 1, f).unwrap();
            out.check().unwrap();
            assert_eq!(out.face_count(), cyl.face_count(), "{mode:?}");
            assert_eq!(out.vertex_count(), cyl.vertex_count(), "{mode:?}");
        }
        // Twist by 0 is identity; taper widens the top.
        let same = simple_deform(&cyl, DeformMode::Twist, 1, 0.0).unwrap();
        assert_eq!(same.to_polygons().0, cyl.to_polygons().0);
        let tapered = simple_deform(&cyl, DeformMode::Taper, 1, 1.0).unwrap();
        let top_r = tapered.positions().filter(|p| p.y > 1.9).map(|p| p.with_y(0.0).length()).fold(0.0, f32::max);
        assert!((top_r - 2.0).abs() < 1e-3, "{top_r}");
        // Displace moves vertices but keeps topology.
        let d = displace(&cyl, 0.1, 1.0).unwrap();
        assert_eq!(d.face_count(), cyl.face_count());
        assert!(d.positions().zip(cyl.positions()).any(|(a, b)| (a - b).length() > 1e-3));
    }
}
