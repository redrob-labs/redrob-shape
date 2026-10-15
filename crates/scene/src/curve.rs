//! Bezier curve objects, after Blender's: splines of control points with left / right handles and
//! a handle type (Auto, Vector, Aligned, Free), evaluated at `resolution` steps per segment; a
//! Bevel Depth sweeps a circle along the curve into a tube mesh, which is also what Convert to
//! Mesh produces.

use serde::{Deserialize, Serialize};
use shape_geom::Mesh;
use shape_math::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum HandleType {
    /// Smooth: handles follow the neighbours, lengths a third of the way to them.
    #[default]
    Auto,
    /// Straight: handles point at the neighbours, giving a sharp polyline.
    Vector,
    /// Both handles stay on one line through the point; the user moves one, the other follows.
    Aligned,
    Free,
}

impl HandleType {
    pub const ALL: [HandleType; 4] = [HandleType::Auto, HandleType::Vector, HandleType::Aligned, HandleType::Free];
    pub fn name(self) -> &'static str {
        match self {
            HandleType::Auto => "Automatic",
            HandleType::Vector => "Vector",
            HandleType::Aligned => "Aligned",
            HandleType::Free => "Free",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct BezierPoint {
    pub co: Vec3,
    pub handle_left: Vec3,
    pub handle_right: Vec3,
    #[serde(default)]
    pub handle_type: HandleType,
    #[serde(default)]
    pub selected: bool,
}

impl BezierPoint {
    pub fn new(co: Vec3) -> Self {
        Self { co, handle_left: co - Vec3::X * 0.5, handle_right: co + Vec3::X * 0.5, handle_type: HandleType::Auto, selected: false }
    }

    /// Move the point and its handles together.
    pub fn translate(&mut self, d: Vec3) {
        self.co += d;
        self.handle_left += d;
        self.handle_right += d;
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Spline {
    pub points: Vec<BezierPoint>,
    pub cyclic: bool,
}

impl Spline {
    /// Recompute Auto and Vector handles from the neighbours (Blender does this after every edit).
    pub fn recalc_handles(&mut self) {
        let n = self.points.len();
        if n == 0 {
            return;
        }
        let cos: Vec<Vec3> = self.points.iter().map(|p| p.co).collect();
        for i in 0..n {
            let prev = if i > 0 {
                Some(cos[i - 1])
            } else if self.cyclic && n > 1 {
                Some(cos[n - 1])
            } else {
                None
            };
            let next = if i + 1 < n {
                Some(cos[i + 1])
            } else if self.cyclic && n > 1 {
                Some(cos[0])
            } else {
                None
            };
            let p = &mut self.points[i];
            match p.handle_type {
                HandleType::Auto => {
                    let (a, b) = (
                        prev.unwrap_or(p.co - (next.unwrap_or(p.co + Vec3::X) - p.co)),
                        next.unwrap_or(p.co + (p.co - prev.unwrap_or(p.co - Vec3::X))),
                    );
                    let dir = (b - a).normalize_or_zero();
                    let dir = if dir.length_squared() < 0.5 { Vec3::X } else { dir };
                    let (dl, dr) = ((p.co - a).length() / 3.0, (b - p.co).length() / 3.0);
                    p.handle_left = p.co - dir * dl.max(1e-4);
                    p.handle_right = p.co + dir * dr.max(1e-4);
                }
                HandleType::Vector => {
                    p.handle_left = prev.map(|a| p.co + (a - p.co) / 3.0).unwrap_or(p.co - (p.handle_right - p.co));
                    p.handle_right = next.map(|b| p.co + (b - p.co) / 3.0).unwrap_or(p.co - (p.handle_left - p.co));
                }
                HandleType::Aligned => {
                    // Keep the right handle's length, put it opposite the left one.
                    let dir = (p.co - p.handle_left).normalize_or_zero();
                    let len = (p.handle_right - p.co).length();
                    if dir.length_squared() > 0.5 {
                        p.handle_right = p.co + dir * len;
                    }
                }
                HandleType::Free => {}
            }
        }
    }

    /// Points along the curve, `resolution` steps per segment (Blender's Resolution Preview U).
    pub fn evaluate(&self, resolution: u32) -> Vec<Vec3> {
        let n = self.points.len();
        if n == 0 {
            return Vec::new();
        }
        if n == 1 {
            return vec![self.points[0].co];
        }
        let segs = if self.cyclic { n } else { n - 1 };
        let steps = resolution.max(1) as usize;
        let mut out = Vec::with_capacity(segs * steps + 1);
        for s in 0..segs {
            let a = &self.points[s];
            let b = &self.points[(s + 1) % n];
            for k in 0..steps {
                let t = k as f32 / steps as f32;
                out.push(bezier(a.co, a.handle_right, b.handle_left, b.co, t));
            }
        }
        if !self.cyclic {
            out.push(self.points[n - 1].co);
        }
        out
    }
}

fn bezier(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let u = 1.0 - t;
    p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub splines: Vec<Spline>,
    /// Segments per Bezier segment.
    pub resolution: u32,
    /// Radius of the swept circle; 0 draws the curve as a line.
    pub bevel_depth: f32,
    /// Blender's bevel resolution: the circle has `4 + 2 * bevel_resolution`... here simply the
    /// number of extra sides over 4.
    pub bevel_resolution: u32,
}

impl Default for Curve {
    fn default() -> Self {
        Self { splines: Vec::new(), resolution: 12, bevel_depth: 0.0, bevel_resolution: 4 }
    }
}

impl Curve {
    /// Blender's Add > Curve > Bezier: two points from -1 to +1 along X with smooth handles.
    pub fn bezier() -> Self {
        let mut s = Spline {
            points: vec![
                BezierPoint {
                    co: Vec3::new(-1.0, 0.0, 0.0),
                    handle_left: Vec3::new(-1.5, 0.0, 0.5),
                    handle_right: Vec3::new(-0.5, 0.0, -0.5),
                    handle_type: HandleType::Aligned,
                    selected: false,
                },
                BezierPoint {
                    co: Vec3::new(1.0, 0.0, 0.0),
                    handle_left: Vec3::new(0.0, 0.0, 0.0),
                    handle_right: Vec3::new(2.0, 0.0, 0.0),
                    handle_type: HandleType::Aligned,
                    selected: false,
                },
            ],
            cyclic: false,
        };
        s.recalc_handles();
        Self { splines: vec![s], ..Default::default() }
    }

    /// Blender's Add > Curve > Circle: four points, cyclic, with the classic 0.5523 handles.
    pub fn circle(radius: f32) -> Self {
        let k = 0.552_284_8 * radius;
        let pts = [(Vec3::X, Vec3::Z), (Vec3::Z, -Vec3::X), (-Vec3::X, -Vec3::Z), (-Vec3::Z, Vec3::X)];
        let points = pts
            .iter()
            .map(|(dir, tangent)| {
                let co = *dir * radius;
                BezierPoint { co, handle_left: co - *tangent * k, handle_right: co + *tangent * k, handle_type: HandleType::Aligned, selected: false }
            })
            .collect();
        Self { splines: vec![Spline { points, cyclic: true }], ..Default::default() }
    }

    pub fn recalc_handles(&mut self) {
        for s in &mut self.splines {
            s.recalc_handles();
        }
    }

    /// Polylines to draw when there is no bevel.
    pub fn polylines(&self) -> Vec<(Vec<Vec3>, bool)> {
        self.splines.iter().map(|s| (s.evaluate(self.resolution), s.cyclic)).collect()
    }

    /// The tube mesh (bevel depth > 0) or an empty mesh.
    pub fn to_mesh(&self) -> Mesh {
        if self.bevel_depth <= 0.0 {
            return Mesh::from_polygons(Vec::new(), &[]).unwrap_or_else(|_| shape_geom::primitives::cube(0.0));
        }
        let sides = (4 + 2 * self.bevel_resolution).max(3) as usize;
        let mut positions: Vec<Vec3> = Vec::new();
        let mut faces: Vec<Vec<u32>> = Vec::new();
        for spline in &self.splines {
            let pts = spline.evaluate(self.resolution);
            let rings = pts.len();
            if pts.len() < 2 {
                continue;
            }
            // Parallel-transport frames along the polyline.
            let base = positions.len() as u32;
            let mut normal = {
                let t = (pts[1] - pts[0]).normalize_or_zero();
                t.any_orthonormal_vector()
            };
            for i in 0..rings {
                let prev = if i == 0 { if spline.cyclic { pts[pts.len() - 1] } else { pts[0] } } else { pts[i - 1] };
                let next = if i + 1 < pts.len() {
                    pts[i + 1]
                } else if spline.cyclic {
                    pts[0]
                } else {
                    pts[i]
                };
                let t = (next - prev).normalize_or_zero();
                let t = if t.length_squared() < 0.5 { Vec3::X } else { t };
                normal = (normal - t * normal.dot(t)).normalize_or_zero();
                if normal.length_squared() < 0.5 {
                    normal = t.any_orthonormal_vector();
                }
                let binormal = t.cross(normal);
                for k in 0..sides {
                    let a = k as f32 / sides as f32 * std::f32::consts::TAU;
                    positions.push(pts[i] + (normal * a.cos() + binormal * a.sin()) * self.bevel_depth);
                }
            }
            let ring = |i: usize| base + (i * sides) as u32;
            let segs = if spline.cyclic { rings } else { rings - 1 };
            for i in 0..segs {
                let (r0, r1) = (ring(i), ring((i + 1) % rings));
                for k in 0..sides {
                    let k1 = (k + 1) % sides;
                    faces.push(vec![r0 + k as u32, r0 + k1 as u32, r1 + k1 as u32, r1 + k as u32]);
                }
            }
            if !spline.cyclic {
                // Caps.
                faces.push((0..sides).rev().map(|k| ring(0) + k as u32).collect());
                faces.push((0..sides).map(|k| ring(rings - 1) + k as u32).collect());
            }
        }
        Mesh::from_polygons(positions, &faces).unwrap_or_else(|_| shape_geom::primitives::cube(0.0))
    }

    pub fn selected_points(&self) -> Vec<(usize, usize)> {
        self.splines
            .iter()
            .enumerate()
            .flat_map(|(si, s)| s.points.iter().enumerate().filter(|(_, p)| p.selected).map(move |(pi, _)| (si, pi)))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_evaluates_on_the_radius_and_bevels_to_a_closed_tube() {
        let c = Curve::circle(2.0);
        for p in c.splines[0].evaluate(16) {
            assert!((p.length() - 2.0).abs() < 0.03, "{p}");
        }
        let tube = Curve { bevel_depth: 0.2, ..c }.to_mesh();
        assert!(tube.is_closed(), "a cyclic bevel is watertight");
        assert_eq!(tube.face_count(), 4 * 12 * (4 + 2 * 4));
    }

    #[test]
    fn open_bezier_tube_has_caps_and_auto_handles_are_smooth() {
        let mut c = Curve::bezier();
        c.bevel_depth = 0.1;
        let m = c.to_mesh();
        assert!(m.is_closed());
        let mut s = Spline {
            points: vec![BezierPoint::new(Vec3::ZERO), BezierPoint::new(Vec3::X), BezierPoint::new(Vec3::new(1.0, 1.0, 0.0))],
            cyclic: false,
        };
        s.recalc_handles();
        let mid = s.points[1];
        let dir = (mid.handle_right - mid.co).normalize();
        let expect = (Vec3::new(1.0, 1.0, 0.0) - Vec3::ZERO).normalize();
        assert!(dir.dot(expect) > 0.99, "auto handle follows the neighbours: {dir}");
        s.points[1].handle_type = HandleType::Vector;
        s.recalc_handles();
        assert!((s.points[1].handle_right - Vec3::new(1.0, 1.0 / 3.0, 0.0)).length() < 1e-5);
    }
}
