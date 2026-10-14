//! Vectors, matrices, rays and transforms shared by every layer.
//!
//! `glam` supplies the arithmetic; this crate fixes which types the rest of the workspace speaks
//! (`f32`, right-handed, Y up, counter-clockwise front faces) and adds the few shapes every layer
//! needs: [`Ray`], [`Aabb`] and [`Transform`].

pub use glam::{Mat3, Mat4, Quat, Vec2, Vec3, Vec3A, Vec4};
use serde::{Deserialize, Serialize};

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-math";

/// Tolerance for "equal enough" comparisons on positions in scene units (metres).
pub const EPSILON: f32 = 1e-5;

/// A half-line: `origin + t * direction` for `t >= 0`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    /// Not required to be unit length; `t` is then in units of `direction`.
    pub direction: Vec3,
}

impl Ray {
    pub fn new(origin: Vec3, direction: Vec3) -> Self {
        Self { origin, direction }
    }

    /// Point at parameter `t`.
    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.direction * t
    }

    /// Slab test. Returns the entry/exit parameters when the ray meets the box.
    pub fn intersect_aabb(&self, aabb: &Aabb) -> Option<(f32, f32)> {
        let mut tmin = 0.0f32;
        let mut tmax = f32::INFINITY;
        for axis in 0..3 {
            let d = self.direction[axis];
            let o = self.origin[axis];
            if d == 0.0 {
                // Parallel to this slab: inside it or never. Avoids the 0 * inf = NaN of the plain
                // slab test when the origin sits exactly on a box plane.
                if o < aabb.min[axis] || o > aabb.max[axis] {
                    return None;
                }
                continue;
            }
            let inv = 1.0 / d;
            let (t0, t1) = ((aabb.min[axis] - o) * inv, (aabb.max[axis] - o) * inv);
            tmin = tmin.max(t0.min(t1));
            tmax = tmax.min(t0.max(t1));
        }
        (tmax >= tmin).then_some((tmin, tmax))
    }

    /// Möller-Trumbore triangle test. Returns `(t, u, v)`; back faces hit too.
    pub fn intersect_triangle(&self, a: Vec3, b: Vec3, c: Vec3) -> Option<(f32, f32, f32)> {
        let e1 = b - a;
        let e2 = c - a;
        let p = self.direction.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-12 {
            return None;
        }
        let inv_det = 1.0 / det;
        let s = self.origin - a;
        let u = s.dot(p) * inv_det;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(e1);
        let v = self.direction.dot(q) * inv_det;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = e2.dot(q) * inv_det;
        (t >= 0.0).then_some((t, u, v))
    }
}

/// Axis-aligned bounding box. An empty box has `min > max` on every axis.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Default for Aabb {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Aabb {
    pub const EMPTY: Self = Self { min: Vec3::splat(f32::INFINITY), max: Vec3::splat(f32::NEG_INFINITY) };

    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    pub fn from_points(points: impl IntoIterator<Item = Vec3>) -> Self {
        points.into_iter().fold(Self::EMPTY, |b, p| b.with_point(p))
    }

    pub fn is_empty(&self) -> bool {
        self.min.cmpgt(self.max).any()
    }

    pub fn with_point(mut self, p: Vec3) -> Self {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
        self
    }

    pub fn union(&self, other: &Self) -> Self {
        Self { min: self.min.min(other.min), max: self.max.max(other.max) }
    }

    pub fn center(&self) -> Vec3 {
        (self.min + self.max) * 0.5
    }

    pub fn extent(&self) -> Vec3 {
        self.max - self.min
    }

    /// Index of the longest axis (0, 1 or 2).
    pub fn longest_axis(&self) -> usize {
        let e = self.extent();
        if e.x >= e.y && e.x >= e.z {
            0
        } else if e.y >= e.z {
            1
        } else {
            2
        }
    }

    pub fn surface_area(&self) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        let e = self.extent();
        2.0 * (e.x * e.y + e.y * e.z + e.z * e.x)
    }

    pub fn contains(&self, p: Vec3) -> bool {
        p.cmpge(self.min).all() && p.cmple(self.max).all()
    }
}

/// Translation, rotation and scale, composed as `T * R * S`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Default for Transform {
    fn default() -> Self {
        Self::IDENTITY
    }
}

impl Transform {
    pub const IDENTITY: Self = Self { translation: Vec3::ZERO, rotation: Quat::IDENTITY, scale: Vec3::ONE };

    pub fn from_translation(translation: Vec3) -> Self {
        Self { translation, ..Self::IDENTITY }
    }

    pub fn from_rotation(rotation: Quat) -> Self {
        Self { rotation, ..Self::IDENTITY }
    }

    pub fn from_scale(scale: Vec3) -> Self {
        Self { scale, ..Self::IDENTITY }
    }

    pub fn to_matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.translation)
    }

    /// Decompose an affine matrix. Shear is lost; `scale` keeps sign only on X for a mirror.
    pub fn from_matrix(m: Mat4) -> Self {
        let (scale, rotation, translation) = m.to_scale_rotation_translation();
        Self { translation, rotation, scale }
    }

    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        self.translation + self.rotation * (self.scale * p)
    }

    pub fn transform_vector(&self, v: Vec3) -> Vec3 {
        self.rotation * (self.scale * v)
    }

    /// `self` applied after `child`: the world transform of a child under this parent.
    pub fn mul_transform(&self, child: &Transform) -> Transform {
        Transform { translation: self.transform_point(child.translation), rotation: self.rotation * child.rotation, scale: self.scale * child.scale }
    }

    pub fn inverse(&self) -> Transform {
        let rotation = self.rotation.inverse();
        let scale = self.scale.recip();
        Transform { translation: rotation * (-self.translation * scale), rotation, scale }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec3, b: Vec3) -> bool {
        (a - b).length() < 1e-4
    }

    #[test]
    fn crate_name() {
        assert_eq!(CRATE, "shape-math");
    }

    #[test]
    fn ray_hits_box_and_misses_box() {
        let b = Aabb::new(Vec3::splat(-1.0), Vec3::splat(1.0));
        let hit = Ray::new(Vec3::new(0.0, 0.0, -5.0), Vec3::Z).intersect_aabb(&b).unwrap();
        assert!((hit.0 - 4.0).abs() < 1e-5 && (hit.1 - 6.0).abs() < 1e-5);
        assert!(Ray::new(Vec3::new(3.0, 0.0, -5.0), Vec3::Z).intersect_aabb(&b).is_none());
        assert!(Ray::new(Vec3::new(0.0, 0.0, 5.0), Vec3::Z).intersect_aabb(&b).is_none(), "box behind the ray");
        let on_plane = Ray::new(Vec3::new(1.0, 0.0, -5.0), Vec3::Z).intersect_aabb(&b);
        assert!(on_plane.is_some(), "origin on the x=1 face plane must not NaN out");
        assert!(Ray::new(Vec3::new(1.5, 0.0, -5.0), Vec3::Z).intersect_aabb(&b).is_none());
    }

    #[test]
    fn ray_hits_triangle() {
        let r = Ray::new(Vec3::new(0.2, 0.2, 1.0), -Vec3::Z);
        let (t, u, v) = r.intersect_triangle(Vec3::ZERO, Vec3::X, Vec3::Y).unwrap();
        assert!((t - 1.0).abs() < 1e-5);
        assert!((u - 0.2).abs() < 1e-5 && (v - 0.2).abs() < 1e-5);
        assert!(Ray::new(Vec3::new(0.8, 0.8, 1.0), -Vec3::Z).intersect_triangle(Vec3::ZERO, Vec3::X, Vec3::Y).is_none());
    }

    #[test]
    fn aabb_from_points() {
        let b = Aabb::from_points([Vec3::new(1.0, -2.0, 3.0), Vec3::new(-1.0, 2.0, 0.0)]);
        assert_eq!(b.min, Vec3::new(-1.0, -2.0, 0.0));
        assert_eq!(b.max, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(b.longest_axis(), 1);
        assert!(Aabb::EMPTY.is_empty() && !b.is_empty());
        assert_eq!(Aabb::EMPTY.surface_area(), 0.0);
    }

    #[test]
    fn transform_round_trips_through_matrix_and_inverse() {
        let t = Transform { translation: Vec3::new(1.0, 2.0, 3.0), rotation: Quat::from_rotation_y(0.7), scale: Vec3::new(2.0, 2.0, 2.0) };
        let p = Vec3::new(0.3, -0.4, 0.9);
        let via_matrix = t.to_matrix().transform_point3(p);
        assert!(close(via_matrix, t.transform_point(p)));
        let back = Transform::from_matrix(t.to_matrix());
        assert!(close(back.transform_point(p), t.transform_point(p)));
        assert!(close(t.inverse().transform_point(t.transform_point(p)), p));
        let child = Transform::from_translation(Vec3::X);
        assert!(close(t.mul_transform(&child).transform_point(p), t.transform_point(child.transform_point(p))));
    }
}
