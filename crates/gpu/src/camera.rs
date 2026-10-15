//! Orbit camera: looks at `target` from a distance, turned by yaw (about Y) and pitch.

use shape_math::{Mat4, Quat, Ray, Vec2, Vec3};

#[derive(Clone, Debug, PartialEq)]
pub struct OrbitCamera {
    pub target: Vec3,
    /// Radians about +Y. 0 looks down -Z... from +Z, i.e. the camera sits on +Z.
    pub yaw: f32,
    /// Radians above the XZ plane; clamped short of the poles.
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
    /// Orthographic projection (Blender's numpad 5). Axis views switch it on; orbiting switches it
    /// back off, as Blender's "auto perspective" does.
    pub ortho: bool,
}

/// The axis-aligned views of Blender's numpad, in Shape's Y-up world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisView {
    Front,
    Back,
    Right,
    Left,
    Top,
    Bottom,
}

impl AxisView {
    /// Blender's numpad key for the view ("Ctrl+" for the opposite side).
    pub fn key(self) -> &'static str {
        match self {
            AxisView::Front => "Numpad 1",
            AxisView::Back => "Ctrl+Numpad 1",
            AxisView::Right => "Numpad 3",
            AxisView::Left => "Ctrl+Numpad 3",
            AxisView::Top => "Numpad 7",
            AxisView::Bottom => "Ctrl+Numpad 7",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            AxisView::Front => "Front",
            AxisView::Back => "Back",
            AxisView::Right => "Right",
            AxisView::Left => "Left",
            AxisView::Top => "Top",
            AxisView::Bottom => "Bottom",
        }
    }
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self { target: Vec3::ZERO, yaw: 0.6, pitch: 0.5, distance: 8.0, fov_y: 50f32.to_radians(), near: 0.05, far: 2000.0, ortho: false }
    }
}

impl OrbitCamera {
    const MAX_PITCH: f32 = std::f32::consts::FRAC_PI_2 - 0.01;

    pub fn rotation(&self) -> Quat {
        Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(-self.pitch)
    }

    pub fn position(&self) -> Vec3 {
        self.target + self.rotation() * Vec3::new(0.0, 0.0, self.distance)
    }

    pub fn forward(&self) -> Vec3 {
        self.rotation() * -Vec3::Z
    }
    pub fn right(&self) -> Vec3 {
        self.rotation() * Vec3::X
    }
    pub fn up(&self) -> Vec3 {
        self.rotation() * Vec3::Y
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.position(), self.target, Vec3::Y)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        let aspect = aspect.max(1e-3);
        if self.ortho {
            // Same framing as the perspective view at the target distance.
            let half_h = self.distance * (self.fov_y * 0.5).tan();
            let half_w = half_h * aspect;
            Mat4::orthographic_rh(-half_w, half_w, -half_h, half_h, self.near, self.far)
        } else {
            Mat4::perspective_rh(self.fov_y, aspect, self.near, self.far)
        }
    }

    /// Snap to an axis view and go orthographic (Blender numpad 1/3/7 with auto perspective).
    pub fn set_axis_view(&mut self, view: AxisView) {
        use std::f32::consts::{FRAC_PI_2, PI};
        let (yaw, pitch) = match view {
            AxisView::Front => (0.0, 0.0),
            AxisView::Back => (PI, 0.0),
            AxisView::Right => (FRAC_PI_2, 0.0),
            AxisView::Left => (-FRAC_PI_2, 0.0),
            AxisView::Top => (0.0, Self::MAX_PITCH),
            AxisView::Bottom => (0.0, -Self::MAX_PITCH),
        };
        self.yaw = yaw;
        self.pitch = pitch;
        self.ortho = true;
    }

    /// The axis view the camera is exactly on, if any.
    pub fn axis_view(&self) -> Option<AxisView> {
        use std::f32::consts::{FRAC_PI_2, PI};
        let eps = 1e-4;
        let near = |a: f32, b: f32| (a - b).abs() < eps;
        if near(self.pitch, Self::MAX_PITCH) {
            return Some(AxisView::Top);
        }
        if near(self.pitch, -Self::MAX_PITCH) {
            return Some(AxisView::Bottom);
        }
        if !near(self.pitch, 0.0) {
            return None;
        }
        let yaw = self.yaw.rem_euclid(2.0 * PI);
        if near(yaw, 0.0) || near(yaw, 2.0 * PI) {
            Some(AxisView::Front)
        } else if near(yaw, PI) {
            Some(AxisView::Back)
        } else if near(yaw, FRAC_PI_2) {
            Some(AxisView::Right)
        } else if near(yaw, 3.0 * FRAC_PI_2) {
            Some(AxisView::Left)
        } else {
            None
        }
    }

    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// Turn by screen-space drag deltas (radians per pixel baked in).
    pub fn orbit(&mut self, delta: Vec2) {
        if delta != Vec2::ZERO {
            self.ortho = false;
        }
        self.yaw -= delta.x * 0.01;
        self.pitch = (self.pitch + delta.y * 0.01).clamp(-Self::MAX_PITCH, Self::MAX_PITCH);
    }

    /// Slide the target in the view plane; `delta` in pixels, `viewport_height` in pixels.
    pub fn pan(&mut self, delta: Vec2, viewport_height: f32) {
        let world_per_pixel = 2.0 * self.distance * (self.fov_y * 0.5).tan() / viewport_height.max(1.0);
        self.target += (-self.right() * delta.x + self.up() * delta.y) * world_per_pixel;
    }

    /// Multiply the distance; `steps` positive zooms in.
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * 0.9f32.powf(steps)).clamp(0.01, 1e5);
    }

    /// Frame a box: look at its centre from far enough to see all of it.
    pub fn frame(&mut self, min: Vec3, max: Vec3) {
        if min.cmpgt(max).any() {
            return;
        }
        self.target = (min + max) * 0.5;
        let radius = ((max - min).length() * 0.5).max(0.01);
        self.distance = radius / (self.fov_y * 0.5).sin() * 1.1;
    }

    /// World ray through a point given in normalised device coordinates (-1..1, y up).
    pub fn ray_ndc(&self, ndc: Vec2, aspect: f32) -> Ray {
        let inv = self.view_projection(aspect).inverse();
        let near = inv.project_point3(Vec3::new(ndc.x, ndc.y, 0.0));
        let far = inv.project_point3(Vec3::new(ndc.x, ndc.y, 1.0));
        Ray::new(near, (far - near).normalize())
    }

    /// World ray through a pixel; `pixel` has y down, `size` is the viewport in pixels.
    pub fn ray_pixel(&self, pixel: Vec2, size: Vec2) -> Ray {
        let ndc = Vec2::new(pixel.x / size.x * 2.0 - 1.0, 1.0 - pixel.y / size.y * 2.0);
        self.ray_ndc(ndc, size.x / size.y)
    }

    /// Pixel (y down) a world point lands on, or `None` when behind the camera.
    pub fn project(&self, world: Vec3, size: Vec2) -> Option<Vec2> {
        let clip = self.view_projection(size.x / size.y) * world.extend(1.0);
        if clip.w <= 0.0 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Vec2::new((ndc.x + 1.0) * 0.5 * size.x, (1.0 - ndc.y) * 0.5 * size.y))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_views_look_down_the_axes_in_ortho_and_orbit_restores_perspective() {
        let mut cam = OrbitCamera::default();
        cam.set_axis_view(AxisView::Front);
        assert!(cam.ortho);
        assert!((cam.forward() - -Vec3::Z).length() < 1e-4, "front looks along -Z: {:?}", cam.forward());
        assert_eq!(cam.axis_view(), Some(AxisView::Front));
        cam.set_axis_view(AxisView::Right);
        assert!((cam.forward() - -Vec3::X).length() < 1e-4, "right looks along -X: {:?}", cam.forward());
        cam.set_axis_view(AxisView::Top);
        assert!(cam.forward().y < -0.99, "top looks down: {:?}", cam.forward());
        assert_eq!(cam.axis_view(), Some(AxisView::Top));
        // Ortho rays are parallel: two pixels give the same direction.
        let size = Vec2::new(800.0, 600.0);
        let a = cam.ray_pixel(Vec2::new(100.0, 100.0), size);
        let b = cam.ray_pixel(Vec2::new(700.0, 500.0), size);
        assert!((a.direction - b.direction).length() < 1e-4);
        cam.orbit(Vec2::new(5.0, -20.0));
        assert!(!cam.ortho);
        assert_eq!(cam.axis_view(), None);
    }

    #[test]
    fn centre_ray_passes_through_target() {
        let cam = OrbitCamera::default();
        let size = Vec2::new(800.0, 600.0);
        let ray = cam.ray_pixel(size * 0.5, size);
        let to_target = (cam.target - ray.origin).normalize();
        assert!((ray.direction - to_target).length() < 1e-3, "{ray:?}");
        assert!((ray.direction - cam.forward()).length() < 1e-3);
    }

    #[test]
    fn project_unproject_round_trip() {
        let cam = OrbitCamera { yaw: 1.1, pitch: 0.3, distance: 5.0, ..Default::default() };
        let size = Vec2::new(1280.0, 720.0);
        let world = Vec3::new(0.7, -0.2, 0.4);
        let px = cam.project(world, size).unwrap();
        let ray = cam.ray_pixel(px, size);
        // the ray must pass within a hair of the point
        let t = (world - ray.origin).dot(ray.direction);
        assert!((ray.at(t) - world).length() < 1e-3);
        assert!(cam.project(cam.position() + cam.forward() * -1.0, size).is_none(), "behind the camera");
    }

    #[test]
    fn orbit_pan_zoom_frame() {
        let mut cam = OrbitCamera::default();
        let before = cam.position();
        cam.orbit(Vec2::new(10.0, 0.0));
        assert!((cam.position() - before).length() > 0.1);
        cam.orbit(Vec2::new(0.0, 1e6));
        assert!(cam.pitch < std::f32::consts::FRAC_PI_2);
        cam.zoom(1.0);
        assert!(cam.distance < 8.0);
        let d = cam.distance;
        cam.pan(Vec2::new(100.0, 0.0), 600.0);
        assert!((cam.distance - d).abs() < 1e-6 && cam.target != Vec3::ZERO);
        cam.frame(Vec3::splat(-1.0), Vec3::splat(1.0));
        assert_eq!(cam.target, Vec3::ZERO);
        let size = Vec2::new(100.0, 100.0);
        for corner in [Vec3::splat(1.0), Vec3::splat(-1.0), Vec3::new(1.0, -1.0, 1.0)] {
            let p = cam.project(corner, size).unwrap();
            assert!(p.x >= 0.0 && p.x <= 100.0 && p.y >= 0.0 && p.y <= 100.0, "{corner} -> {p}");
        }
    }
}
