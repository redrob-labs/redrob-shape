//! Pinhole camera for the renderer.

use shape_math::{Mat4, Ray, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderCamera {
    pub position: Vec3,
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub fov_y: f32,
}

impl RenderCamera {
    pub fn look_at(position: Vec3, target: Vec3, fov_y: f32) -> Self {
        let forward = (target - position).normalize_or_zero();
        let world_up = if forward.y.abs() > 0.999 { Vec3::Z } else { Vec3::Y };
        let right = forward.cross(world_up).normalize_or_zero();
        let up = right.cross(forward);
        Self { position, forward, right, up, fov_y }
    }

    /// From an object transform: the camera looks along its local -Z with +Y up.
    pub fn from_matrix(m: Mat4, fov_y: f32) -> Self {
        let position = m.transform_point3(Vec3::ZERO);
        let forward = m.transform_vector3(-Vec3::Z).normalize_or_zero();
        let up = m.transform_vector3(Vec3::Y).normalize_or_zero();
        let right = forward.cross(up).normalize_or_zero();
        Self { position, forward, right, up: right.cross(forward), fov_y }
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
