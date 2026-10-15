//! Path tracer and render output.
//!
//! [`RenderScene`] flattens a [`Scene`] into world-space triangles with material indices, a BVH, the
//! lights and a pinhole camera. [`Renderer`] accumulates one sample per pixel per pass
//! (progressive, deterministic per pixel and sample index, rows in parallel through rayon) and
//! produces a tone-mapped sRGB image. Materials are Lambert diffuse plus a glossy lobe whose width
//! follows roughness and whose tint follows metallic; delta lights (point, sun, spot) are sampled
//! directly at every bounce, emissive surfaces light what sees them, and rays that escape pick up
//! the sky colour.

mod bvh;
mod camera;
mod integrator;
mod rng;
mod scene;

pub use camera::RenderCamera;
pub use integrator::Settings;
pub use scene::RenderScene;

use rayon::prelude::*;
use shape_math::Vec3;

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-render";

/// Linear RGB film with sample count, growing one pass at a time.
pub struct Renderer {
    pub width: u32,
    pub height: u32,
    pub settings: Settings,
    film: Vec<Vec3>,
    pub samples: u32,
}

impl Renderer {
    pub fn new(width: u32, height: u32, settings: Settings) -> Self {
        let n = (width.max(1) * height.max(1)) as usize;
        Self { width: width.max(1), height: height.max(1), settings, film: vec![Vec3::ZERO; n], samples: 0 }
    }

    /// Add one sample to every pixel. Rows run in parallel; the result does not depend on how.
    pub fn pass(&mut self, scene: &RenderScene) {
        let (w, h, s) = (self.width, self.height, self.samples);
        let settings = self.settings;
        self.film.par_chunks_mut(w as usize).enumerate().for_each(|(y, row)| {
            for (x, px) in row.iter_mut().enumerate() {
                let mut rng = rng::Pcg::new(((y as u64) << 32) | x as u64, s as u64 + 1);
                let jitter = shape_math::Vec2::new(rng.next_f32(), rng.next_f32());
                let ray = scene.camera.ray(x as f32 + jitter.x, y as f32 + jitter.y, w as f32, h as f32);
                let c = integrator::trace(scene, ray, &mut rng, &settings);
                *px += c;
            }
        });
        self.samples += 1;
    }

    /// Mean radiance of a pixel so far (linear).
    pub fn pixel(&self, x: u32, y: u32) -> Vec3 {
        if self.samples == 0 {
            return Vec3::ZERO;
        }
        self.film[(y * self.width + x) as usize] / self.samples as f32
    }

    /// Tone-mapped 8-bit sRGB RGBA, top row first.
    pub fn to_rgba8(&self) -> Vec<u8> {
        let inv = if self.samples == 0 { 0.0 } else { 1.0 / self.samples as f32 };
        let mut out = Vec::with_capacity(self.film.len() * 4);
        for p in &self.film {
            let c = tonemap(*p * inv * self.settings.exposure);
            out.extend_from_slice(&[to_srgb8(c.x), to_srgb8(c.y), to_srgb8(c.z), 255]);
        }
        out
    }

    pub fn save_png(&self, path: &std::path::Path) -> Result<(), String> {
        let img = image::RgbaImage::from_raw(self.width, self.height, self.to_rgba8()).ok_or("film size mismatch")?;
        img.save(path).map_err(|e| e.to_string())
    }
}

/// Reinhard with a little shoulder; keeps colour ratios.
pub fn tonemap(c: Vec3) -> Vec3 {
    let l = c.dot(Vec3::new(0.2126, 0.7152, 0.0722)).max(1e-6);
    let mapped = l / (1.0 + l);
    (c * (mapped / l)).clamp(Vec3::ZERO, Vec3::ONE)
}

fn to_srgb8(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.0031308 { v * 12.92 } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0 + 0.5) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::{cube, plane};
    use shape_math::{Quat, Transform};
    use shape_scene::{Light, Material, Object, ObjectData, Scene};

    fn sun_from_above(strength: f32) -> Object {
        let mut sun = Object::new("Sun", ObjectData::Light(Light::sun(strength)));
        sun.transform = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2));
        sun
    }

    /// Camera straight above the origin looking down.
    fn top_camera(height: f32) -> RenderCamera {
        RenderCamera::look_at(Vec3::new(0.0, height, 0.001), Vec3::ZERO, 60f32.to_radians())
    }

    #[test]
    fn crate_name() {
        assert_eq!(CRATE, "shape-render");
    }

    #[test]
    fn an_image_texture_tints_the_surface_through_the_uvs() {
        // A 2x1 texture: left half red, right half blue; planar UVs put u = 0 at x = -50.
        let mut scene = Scene::new("t");
        let tex = scene.add_texture(shape_scene::Texture::from_rgba("stripes", 2, 1, vec![[1.0, 0.0, 0.0, 1.0], [0.0, 0.0, 1.0, 1.0]]));
        let mat =
            scene.add_material(Material { base_color: [1.0, 1.0, 1.0, 1.0], roughness: 1.0, base_color_texture: Some(tex), ..Default::default() });
        let id = scene.add_mesh_object("Floor", plane(100.0, 1, 1), Some(mat));
        let (mesh_id, mesh) = scene.object_mesh(id).unwrap();
        let uvs = shape_ops::uv_planar(mesh, 1);
        scene.mesh_mut(mesh_id).unwrap().uvs = Some(uvs);
        scene.add_object(sun_from_above(2.0));
        let settings = Settings { sky: Vec3::ZERO, max_depth: 1, ..Default::default() };
        let rs = RenderScene::build(&scene, top_camera(40.0), &settings);
        let mut r = Renderer::new(16, 16, settings);
        for _ in 0..8 {
            r.pass(&rs);
        }
        let left = r.pixel(2, 8);
        let right = r.pixel(13, 8);
        assert!(left.x > left.z * 4.0 || right.x > right.z * 4.0, "one side is red: {left} {right}");
        assert!(left.z > left.x * 4.0 || right.z > right.x * 4.0, "the other is blue: {left} {right}");
    }

    #[test]
    fn lambert_plane_under_a_sun_matches_the_formula() {
        let mut scene = Scene::new("t");
        let mat = scene.add_material(Material { base_color: [0.5, 0.5, 0.5, 1.0], roughness: 1.0, ..Default::default() });
        scene.add_mesh_object("Floor", plane(100.0, 1, 1), Some(mat));
        scene.add_object(sun_from_above(2.0));
        let settings = Settings { sky: Vec3::ZERO, max_depth: 1, ..Default::default() };
        let rs = RenderScene::build(&scene, top_camera(5.0), &settings);
        let mut r = Renderer::new(8, 8, settings);
        for _ in 0..16 {
            r.pass(&rs);
        }
        // Lambert: albedo / pi * irradiance (the sun's strength is its irradiance), plus the 4 %
        // dielectric gloss lobe, which at roughness 1 is a normalised Phong lobe of exponent 1
        // seen head-on: 0.04 * (1 + 2) / (2 pi).
        let gloss = 0.04 * 3.0 / (2.0 * std::f32::consts::PI);
        let expected = (0.5 / std::f32::consts::PI + gloss) * 2.0;
        let got = r.pixel(4, 4).x;
        assert!((got - expected).abs() < 0.02, "got {got} expected {expected}");
    }

    #[test]
    fn occluder_casts_a_shadow() {
        let mut scene = Scene::new("t");
        scene.add_mesh_object("Floor", plane(100.0, 1, 1), None);
        let blocker = scene.add_mesh_object("Blocker", cube(2.0), None);
        scene.object_mut(blocker).unwrap().transform = Transform::from_translation(Vec3::new(0.0, 2.0, 0.0));
        scene.add_object(sun_from_above(3.0));
        let settings = Settings { sky: Vec3::ZERO, max_depth: 2, ..Default::default() };
        // Camera off to the side, looking down at the floor beside the cube's shadow.
        let cam = RenderCamera::look_at(Vec3::new(0.0, 6.0, 6.0), Vec3::new(0.0, 0.0, 0.0), 70f32.to_radians());
        let rs = RenderScene::build(&scene, cam, &settings);
        let mut r = Renderer::new(32, 32, settings);
        for _ in 0..8 {
            r.pass(&rs);
        }
        // The bottom row looks at the floor near the camera, in full sun; between it and the cube
        // the floor lies in the cube's shadow.
        let lit = r.pixel(16, 31).x;
        assert!(lit > 0.3, "lit {lit}");
        let mut darkest = f32::INFINITY;
        for y in 16..31 {
            darkest = darkest.min(r.pixel(16, y).x);
        }
        assert!(darkest < lit * 0.5, "no shadow found: darkest {darkest} vs lit {lit}");
    }

    #[test]
    fn emissive_surface_and_sky_are_seen_directly() {
        let mut scene = Scene::new("t");
        let glow = scene.add_material(Material { base_color: [0.0; 4], emissive: [1.5, 0.5, 0.25], ..Default::default() });
        scene.add_mesh_object("Panel", plane(1.0, 1, 1), Some(glow));
        let settings = Settings { sky: Vec3::new(0.1, 0.2, 0.3), max_depth: 3, ..Default::default() };
        let rs = RenderScene::build(&scene, top_camera(3.0), &settings);
        let mut r = Renderer::new(16, 16, settings);
        r.pass(&rs);
        let centre = r.pixel(8, 8);
        // emission exactly, plus a little sky in the 4 % gloss lobe
        assert!((centre - Vec3::new(1.5, 0.5, 0.25)).length() < 0.05, "{centre}");
        let corner = r.pixel(0, 0);
        assert!((corner - settings.sky).length() < 1e-3, "{corner}");
        let rgba = r.to_rgba8();
        assert_eq!(rgba.len(), 16 * 16 * 4);
        assert_eq!(rgba[3], 255);
    }

    #[test]
    fn passes_are_deterministic() {
        let mut scene = Scene::new("t");
        scene.add_mesh_object("Box", cube(2.0), None);
        scene.add_object(sun_from_above(2.0));
        let settings = Settings::default();
        let rs = RenderScene::build(&scene, RenderCamera::look_at(Vec3::new(4.0, 3.0, 4.0), Vec3::ZERO, 50f32.to_radians()), &settings);
        let mut a = Renderer::new(12, 12, settings);
        let mut b = Renderer::new(12, 12, settings);
        for _ in 0..3 {
            a.pass(&rs);
            b.pass(&rs);
        }
        assert_eq!(a.to_rgba8(), b.to_rgba8());
    }
}
