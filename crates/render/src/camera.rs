//! Pinhole camera for the renderer.

use shape_math::{Mat4, Ray, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderCamera {
    pub position: Vec3,
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
    /// Thin-lens radius in world units (0 = pinhole) and the distance of the sharp plane.
    pub lens_radius: f32,
    pub focus_distance: f32,
}

impl RenderCamera {
    pub fn look_at(position: Vec3, target: Vec3, fov_y: f32) -> Self {
        let forward = (target - position).normalize_or_zero();
        let world_up = if forward.y.abs() > 0.999 { Vec3::Z } else { Vec3::Y };
        let right = forward.cross(world_up).normalize_or_zero();
        let up = right.cross(forward);
        Self { position, forward, right, up, fov_y, lens_radius: 0.0, focus_distance: 10.0 }
    }

    /// Blender's camera DOF: the aperture from the f-stop and a 50 mm-equivalent sensor scale.
    pub fn with_dof(mut self, fstop: f32, focus_distance: f32) -> Self {
        // Lens radius = focal length / (2 f). With a 36 mm sensor the focal length follows the fov.
        let focal_length_m = 0.036 / (2.0 * (self.fov_y * 0.5).tan());
        self.lens_radius = if fstop > 0.0 { focal_length_m / (2.0 * fstop) } else { 0.0 };
        self.focus_distance = focus_distance.max(0.01);
        self
    }

    /// From an object transform: the camera looks along its local -Z with +Y up.
    pub fn from_matrix(m: Mat4, fov_y: f32) -> Self {
        let position = m.transform_point3(Vec3::ZERO);
        let forward = m.transform_vector3(-Vec3::Z).normalize_or_zero();
        let up = m.transform_vector3(Vec3::Y).normalize_or_zero();
        let right = forward.cross(up).normalize_or_zero();
        Self { position, forward, right, up: right.cross(forward), fov_y, lens_radius: 0.0, focus_distance: 10.0 }
    }

    /// Ray through pixel coordinates (`px`, `py`, y down, fractional for jitter).
    pub fn ray(&self, px: f32, py: f32, width: f32, height: f32) -> Ray {
        let aspect = width / height;
        let tan_half = (self.fov_y * 0.5).tan();
        let ndc_x = (px / width) * 2.0 - 1.0;
        let ndc_y = 1.0 - (py / height) * 2.0;
        let dir = self.forward + self.right * (ndc_x * tan_half * aspect) + self.up * (ndc_y * tan_half);
        Ray::new(self.position, dir.normalize())
    }

    /// Thin-lens ray: the pinhole ray's point on the focus plane stays sharp while the origin
    /// moves across the lens disc (`u`, `v` uniform in 0..1).
    pub fn ray_dof(&self, px: f32, py: f32, width: f32, height: f32, u: f32, v: f32) -> Ray {
        let pin = self.ray(px, py, width, height);
        if self.lens_radius <= 0.0 {
            return pin;
        }
        let t = self.focus_distance / pin.direction.dot(self.forward).max(1e-4);
        let focus = pin.origin + pin.direction * t;
        let r = self.lens_radius * u.sqrt();
        let a = v * std::f32::consts::TAU;
        let origin = self.position + self.right * (r * a.cos()) + self.up * (r * a.sin());
        Ray::new(origin, (focus - origin).normalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_ray_is_forward() {
        let c = RenderCamera::look_at(Vec3::new(0.0, 0.0, 5.0), Vec3::ZERO, 1.0);
        let r = c.ray(50.0, 50.0, 100.0, 100.0);
        assert!((r.direction - Vec3::NEG_Z).length() < 1e-5);
        let right = c.ray(100.0, 50.0, 100.0, 100.0);
        assert!(right.direction.x > 0.3);
        let top = c.ray(50.0, 0.0, 100.0, 100.0);
        assert!(top.direction.y > 0.3);
    }
}
