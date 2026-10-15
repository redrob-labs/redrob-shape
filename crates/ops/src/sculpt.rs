//! Sculpt brushes, behaving like Blender's: a stroke is a run of *dabs*, each moving the vertices
//! within the brush radius of the hit point by a smooth falloff. Positions are in the mesh's own
//! space; the caller converts the pointer hit and the grab delta into it.

use shape_geom::{Mesh, VertexId};
use shape_math::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Brush {
    /// Push along the area normal (Ctrl pulls in).
    #[default]
    Draw,
    /// Draw towards a plane slightly above the surface: builds up flat layers.
    Clay,
    /// Relax towards the neighbour average (Shift with any brush).
    Smooth,
    /// Drag the vertices under the brush along with the pointer.
    Grab,
    /// Push every vertex along its own normal.
    Inflate,
    /// Pull vertices onto the area plane.
    Flatten,
}

impl Brush {
    pub const ALL: [Brush; 6] = [Brush::Draw, Brush::Clay, Brush::Smooth, Brush::Grab, Brush::Inflate, Brush::Flatten];
    pub fn name(self) -> &'static str {
        match self {
            Brush::Draw => "Draw",
            Brush::Clay => "Clay",
            Brush::Smooth => "Smooth",
            Brush::Grab => "Grab",
            Brush::Inflate => "Inflate",
            Brush::Flatten => "Flatten",
        }
    }
    /// Blender's default key for the brush.
    pub fn key(self) -> &'static str {
        match self {
            Brush::Draw => "X",
            Brush::Clay => "C",
            Brush::Smooth => "S / Shift",
            Brush::Grab => "G",
            Brush::Inflate => "I",
            Brush::Flatten => "Shift+T",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushSettings {
    pub brush: Brush,
    /// Radius in mesh units.
    pub radius: f32,
    /// 0..1.
    pub strength: f32,
    /// Ctrl: push the other way.
    pub invert: bool,
    /// Mirror every dab across the mesh's X / Y / Z axes.
    pub symmetry: [bool; 3],
}

impl Default for BrushSettings {
    fn default() -> Self {
        Self { brush: Brush::Draw, radius: 0.5, strength: 0.5, invert: false, symmetry: [false; 3] }
    }
}

/// Blender's "Smooth" falloff curve: 1 at the centre, 0 at the radius.
pub fn falloff(distance: f32, radius: f32) -> f32 {
    if radius <= 0.0 || distance >= radius {
        return 0.0;
    }
    let t = 1.0 - distance / radius;
    t * t * (3.0 - 2.0 * t)
}

/// One dab at `center` (mesh space). `delta` is the pointer movement since the last dab in mesh
/// space (used by Grab). Returns how many vertices moved.
pub fn dab(mesh: &mut Mesh, center: Vec3, delta: Vec3, s: &BrushSettings) -> usize {
    let mut moved = 0;
    for (c, d) in mirrored(center, delta, s.symmetry) {
        moved += dab_once(mesh, c, d, s);
    }
    moved
}

/// The dab centres a symmetric brush paints: the original plus its reflections across each
/// enabled axis (and their combinations).
fn mirrored(center: Vec3, delta: Vec3, symmetry: [bool; 3]) -> Vec<(Vec3, Vec3)> {
    let mut out = vec![(center, delta)];
    for (axis, &on) in symmetry.iter().enumerate() {
        if !on {
            continue;
        }
        let mut flip = Vec3::ONE;
        flip[axis] = -1.0;
        let more: Vec<_> = out.iter().map(|(c, d)| (*c * flip, *d * flip)).collect();
        out.extend(more);
    }
    out.dedup_by(|a, b| a.0.distance(b.0) < 1e-6);
    out
}

fn dab_once(mesh: &mut Mesh, center: Vec3, delta: Vec3, s: &BrushSettings) -> usize {
    let strength = s.strength.clamp(0.0, 1.0);
    let sign = if s.invert { -1.0 } else { 1.0 };
    // Vertices under the brush with their falloff weights.
    let under: Vec<(VertexId, f32)> = mesh
        .vertex_ids()
        .filter_map(|v| {
            let w = falloff(mesh.position(v).distance(center), s.radius);
            (w > 0.0).then_some((v, w))
        })
        .collect();
    if under.is_empty() {
        return 0;
    }
    // Area normal and centre: the brush plane.
    let normals = mesh.vertex_normals();
    let mut area_n = Vec3::ZERO;
    let mut area_c = Vec3::ZERO;
    let mut wsum = 0.0;
    for &(v, w) in &under {
        area_n += normals[v.index()] * w;
        area_c += mesh.position(v) * w;
        wsum += w;
    }
    let area_n = area_n.normalize_or_zero();
    let area_c = if wsum > 0.0 { area_c / wsum } else { center };
    // Displacement scales with the radius so the brush looks the same at every zoom.
    let step = s.radius * 0.15 * strength;
    match s.brush {
        Brush::Draw => {
            for &(v, w) in &under {
                mesh.set_position(v, mesh.position(v) + area_n * (step * w * sign));
            }
        }
        Brush::Clay => {
            // A plane above the surface: pull towards it and push up, so the result is a flat pad.
            let plane_c = area_c + area_n * (s.radius * 0.1 * sign);
            for &(v, w) in &under {
                let p = mesh.position(v);
                let to_plane = area_n * (plane_c - p).dot(area_n);
                mesh.set_position(v, p + to_plane * (w * strength) + area_n * (step * w * sign * 0.5));
            }
        }
        Brush::Smooth => {
            let before: Vec<Vec3> = mesh.positions().collect();
            for &(v, w) in &under {
                let nb: Vec<usize> = mesh.vertex_neighbors(v).map(|n| n.index()).collect();
                if nb.is_empty() {
                    continue;
                }
                let avg = nb.iter().map(|&n| before[n]).sum::<Vec3>() / nb.len() as f32;
                mesh.set_position(v, before[v.index()].lerp(avg, w * strength));
            }
        }
        Brush::Grab => {
            for &(v, w) in &under {
                mesh.set_position(v, mesh.position(v) + delta * (w * strength * 2.0));
            }
        }
        Brush::Inflate => {
            for &(v, w) in &under {
                mesh.set_position(v, mesh.position(v) + normals[v.index()] * (step * w * sign));
            }
        }
        Brush::Flatten => {
            for &(v, w) in &under {
                let p = mesh.position(v);
                let h = (p - area_c).dot(area_n);
                mesh.set_position(v, p - area_n * (h * w * strength));
            }
        }
    }
    under.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::{plane, uv_sphere};

    fn heights(m: &Mesh) -> Vec<f32> {
        m.positions().map(|p| p.y).collect()
    }

    #[test]
    fn falloff_is_one_at_centre_and_zero_at_the_edge() {
        assert_eq!(falloff(0.0, 1.0), 1.0);
        assert_eq!(falloff(1.0, 1.0), 0.0);
        assert!((falloff(0.5, 1.0) - 0.5).abs() < 1e-6);
        assert!(falloff(0.2, 1.0) > falloff(0.4, 1.0));
    }

    #[test]
    fn draw_raises_the_centre_of_a_grid_and_ctrl_lowers_it() {
        let mut m = plane(4.0, 16, 16);
        let s = BrushSettings { radius: 1.0, strength: 1.0, ..Default::default() };
        let moved = dab(&mut m, Vec3::ZERO, Vec3::ZERO, &s);
        assert!(moved > 10);
        let hs = heights(&m);
        let max = hs.iter().cloned().fold(f32::MIN, f32::max);
        assert!(max > 0.05, "centre rose: {max}");
        // Far vertices untouched.
        let corner = m.positions().find(|p| p.x.abs() > 1.9 && p.z.abs() > 1.9).unwrap();
        assert_eq!(corner.y, 0.0);
        let mut m2 = plane(4.0, 16, 16);
        dab(&mut m2, Vec3::ZERO, Vec3::ZERO, &BrushSettings { invert: true, ..s });
        assert!(heights(&m2).iter().cloned().fold(f32::MAX, f32::min) < -0.05, "ctrl pushes in");
    }

    #[test]
    fn smooth_reduces_bumps_and_flatten_removes_them() {
        let mut m = plane(4.0, 16, 16);
        let s = BrushSettings { radius: 1.0, strength: 1.0, ..Default::default() };
        dab(&mut m, Vec3::ZERO, Vec3::ZERO, &s);
        let var = |m: &Mesh| {
            let h = heights(m);
            let mean = h.iter().sum::<f32>() / h.len() as f32;
            h.iter().map(|x| (x - mean).powi(2)).sum::<f32>()
        };
        let bumpy = var(&m);
        let mut smoothed = m.clone();
        for _ in 0..3 {
            dab(&mut smoothed, Vec3::ZERO, Vec3::ZERO, &BrushSettings { brush: Brush::Smooth, radius: 1.5, strength: 1.0, ..Default::default() });
        }
        assert!(var(&smoothed) < bumpy * 0.7, "smooth {} < {}", var(&smoothed), bumpy);
        let mut flat = m.clone();
        for _ in 0..4 {
            dab(&mut flat, Vec3::ZERO, Vec3::ZERO, &BrushSettings { brush: Brush::Flatten, radius: 1.5, strength: 1.0, ..Default::default() });
        }
        assert!(var(&flat) < bumpy * 0.4, "flatten {} < {}", var(&flat), bumpy);
    }

    #[test]
    fn grab_moves_with_the_pointer_and_inflate_grows_a_sphere() {
        let mut m = plane(4.0, 8, 8);
        let s = BrushSettings { brush: Brush::Grab, radius: 1.0, strength: 0.5, ..Default::default() };
        dab(&mut m, Vec3::ZERO, Vec3::new(0.3, 0.0, 0.0), &s);
        let centre = m.positions().min_by(|a, b| (a.x.abs() + a.z.abs()).partial_cmp(&(b.x.abs() + b.z.abs())).unwrap()).unwrap();
        assert!(centre.x > 0.2, "grabbed centre {centre}");
        let mut sph = uv_sphere(1.0, 16, 8);
        let before: f32 = sph.positions().map(|p| p.length()).sum();
        dab(
            &mut sph,
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::ZERO,
            &BrushSettings { brush: Brush::Inflate, radius: 1.0, strength: 1.0, ..Default::default() },
        );
        assert!(sph.positions().map(|p| p.length()).sum::<f32>() > before);
    }

    #[test]
    fn x_symmetry_mirrors_the_dab() {
        let mut m = plane(4.0, 16, 16);
        let s = BrushSettings { radius: 0.6, strength: 1.0, symmetry: [true, false, false], ..Default::default() };
        dab(&mut m, Vec3::new(1.0, 0.0, 0.0), Vec3::ZERO, &s);
        let at = |x: f32| m.positions().filter(|p| (p.x - x).abs() < 0.2 && p.z.abs() < 0.2).map(|p| p.y).fold(f32::MIN, f32::max);
        assert!(at(1.0) > 0.03 && (at(1.0) - at(-1.0)).abs() < 1e-4, "{} vs {}", at(1.0), at(-1.0));
        assert!(at(0.0) < 1e-6);
        assert_eq!(mirrored(Vec3::ONE, Vec3::ZERO, [true, true, true]).len(), 8);
        assert_eq!(mirrored(Vec3::ZERO, Vec3::ZERO, [true, true, true]).len(), 1, "a centre on the axes mirrors onto itself");
    }
}
