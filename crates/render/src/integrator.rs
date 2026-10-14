//! The path integrator: one camera path per call, direct light at every vertex.

use crate::rng::Pcg;
use crate::scene::{RenderLight, RenderMaterial, RenderScene};
use shape_math::{Ray, Vec3};
use std::f32::consts::{FRAC_1_PI, PI};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Settings {
    pub max_depth: u32,
    /// Radiance of a ray that leaves the scene.
    pub sky: Vec3,
    pub exposure: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self { max_depth: 5, sky: Vec3::new(0.18, 0.21, 0.26), exposure: 1.0 }
    }
}

const EPS: f32 = 1e-4;

/// Radiance arriving along `ray`.
pub fn trace(scene: &RenderScene, mut ray: Ray, rng: &mut Pcg, settings: &Settings) -> Vec3 {
    let mut radiance = Vec3::ZERO;
    let mut throughput = Vec3::ONE;
    for depth in 0..settings.max_depth.max(1) {
        let Some(hit) = scene.bvh.intersect(&ray, EPS, f32::INFINITY) else {
            radiance += throughput * scene.sky;
            break;
        };
        let tri = &scene.bvh.tris[hit.tri];
        let m = scene.materials[tri.material as usize];
        let p = ray.at(hit.t);
        // Face the normal towards the incoming ray so back faces shade too.
        let mut n = tri.normal;
        if n.dot(ray.direction) > 0.0 {
            n = -n;
        }
        let wo = -ray.direction;

        radiance += throughput * m.emissive;

        // Direct light from every delta light.
        for light in &scene.lights {
            let Some((wi, dist, li)) = light_sample(light, p) else { continue };
            let cos = n.dot(wi);
            if cos <= 0.0 {
                continue;
            }
            let shadow = Ray::new(p + n * EPS, wi);
            if scene.bvh.occluded(&shadow, EPS, dist - EPS) {
                continue;
            }
            radiance += throughput * brdf(&m, n, wo, wi) * li * cos;
        }

        if depth + 1 >= settings.max_depth {
            break;
        }
        // Continue the path: pick the diffuse or the glossy lobe.
        let (wi, pdf, f) = sample_brdf(&m, n, wo, rng);
        if pdf <= 0.0 {
            break;
        }
        let cos = n.dot(wi).max(0.0);
        throughput *= f * cos / pdf;
        if throughput.max_element() <= 0.0 {
            break;
        }
        // Russian roulette after a few bounces.
        if depth >= 2 {
            let q = throughput.max_element().clamp(0.05, 0.95);
            if rng.next_f32() > q {
                break;
            }
            throughput /= q;
        }
        ray = Ray::new(p + n * EPS, wi);
    }
    radiance
}

/// Direction to the light, distance (infinite for a sun) and incident radiance at `p`.
fn light_sample(light: &RenderLight, p: Vec3) -> Option<(Vec3, f32, Vec3)> {
    match *light {
        RenderLight::Point { position, intensity } => {
            let to = position - p;
            let d2 = to.length_squared().max(1e-6);
            Some((to / d2.sqrt(), d2.sqrt(), intensity / d2))
        }
        RenderLight::Sun { direction, irradiance } => Some((direction, f32::INFINITY, irradiance)),
        RenderLight::Spot { position, intensity, direction, cos_inner, cos_outer } => {
            let to = position - p;
            let d2 = to.length_squared().max(1e-6);
            let wi = to / d2.sqrt();
            let cos_angle = (-wi).dot(direction);
            let falloff = smoothstep(cos_outer, cos_inner, cos_angle);
            (falloff > 0.0).then(|| (wi, d2.sqrt(), intensity * falloff / d2))
        }
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0).max(1e-6)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Diffuse weight, specular tint and Phong exponent for a material.
fn lobes(m: &RenderMaterial) -> (Vec3, Vec3, f32) {
    let diffuse = m.base * (1.0 - m.metallic);
    let spec_tint = Vec3::splat(0.04).lerp(m.base, m.metallic);
    // roughness 0.02 -> very sharp, 1 -> almost diffuse
    let exponent = 2.0 / (m.roughness * m.roughness).max(1e-4) - 2.0;
    (diffuse, spec_tint, exponent.clamp(1.0, 5000.0))
}

/// BRDF value for directions `wo` (to viewer) and `wi` (to light), both away from the surface.
fn brdf(m: &RenderMaterial, n: Vec3, wo: Vec3, wi: Vec3) -> Vec3 {
    let (diffuse, spec_tint, e) = lobes(m);
    let r = reflect(-wo, n);
    let cos_a = r.dot(wi).max(0.0);
    // Normalised modified Phong lobe.
    let spec = spec_tint * ((e + 2.0) / (2.0 * PI) * cos_a.powf(e));
    diffuse * FRAC_1_PI + spec
}

fn reflect(d: Vec3, n: Vec3) -> Vec3 {
    d - n * (2.0 * d.dot(n))
}

/// Sample a bounce direction. Returns `(wi, pdf, brdf)`.
fn sample_brdf(m: &RenderMaterial, n: Vec3, wo: Vec3, rng: &mut Pcg) -> (Vec3, f32, Vec3) {
    let (diffuse, spec_tint, e) = lobes(m);
    let d_w = diffuse.max_element();
    let s_w = spec_tint.max_element();
    let p_spec = if d_w + s_w > 0.0 { s_w / (d_w + s_w) } else { 0.0 };
    let (u1, u2, pick) = (rng.next_f32(), rng.next_f32(), rng.next_f32());
    let wi = if pick < p_spec {
        // Phong lobe around the mirror direction.
        let r = reflect(-wo, n);
        let cos_a = u1.powf(1.0 / (e + 1.0));
        let sin_a = (1.0 - cos_a * cos_a).max(0.0).sqrt();
        let phi = 2.0 * PI * u2;
        let (t, b) = basis(r);
        (t * (sin_a * phi.cos()) + b * (sin_a * phi.sin()) + r * cos_a).normalize()
    } else {
        // Cosine-weighted hemisphere.
        let rad = u1.sqrt();
        let phi = 2.0 * PI * u2;
        let (t, b) = basis(n);
        (t * (rad * phi.cos()) + b * (rad * phi.sin()) + n * (1.0 - u1).max(0.0).sqrt()).normalize()
    };
    if n.dot(wi) <= 0.0 {
        return (wi, 0.0, Vec3::ZERO);
    }
    // Mixture pdf of the two strategies.
    let r = reflect(-wo, n);
    let cos_a = r.dot(wi).max(0.0);
    let pdf_spec = (e + 1.0) / (2.0 * PI) * cos_a.powf(e);
    let pdf_diff = n.dot(wi) * FRAC_1_PI;
    let pdf = p_spec * pdf_spec + (1.0 - p_spec) * pdf_diff;
    (wi, pdf, brdf(m, n, wo, wi))
}

fn basis(n: Vec3) -> (Vec3, Vec3) {
    let a = if n.x.abs() > 0.9 { Vec3::Y } else { Vec3::X };
    let t = a.cross(n).normalize();
    (t, n.cross(t))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffuse_brdf_integrates_to_albedo() {
        // Monte Carlo estimate of the albedo of a rough dielectric: integral of f cos over the hemisphere.
        let m = RenderMaterial { base: Vec3::splat(0.5), metallic: 0.0, roughness: 1.0, emissive: Vec3::ZERO };
        let n = Vec3::Y;
        let wo = Vec3::new(0.3, 0.8, 0.2).normalize();
        let mut rng = Pcg::new(1, 1);
        let mut sum = Vec3::ZERO;
        let samples = 20000;
        for _ in 0..samples {
            let (wi, pdf, f) = sample_brdf(&m, n, wo, &mut rng);
            if pdf > 0.0 {
                sum += f * n.dot(wi).max(0.0) / pdf;
            }
        }
        let albedo = sum / samples as f32;
        // 0.5 diffuse plus the 4% glossy lobe, minus what leaves below the horizon
        assert!(albedo.x > 0.45 && albedo.x < 0.6, "{albedo}");
    }

    #[test]
    fn mirror_metal_reflects_about_the_normal() {
        let m = RenderMaterial { base: Vec3::ONE, metallic: 1.0, roughness: 0.02, emissive: Vec3::ZERO };
        let n = Vec3::Y;
        let wo = Vec3::new(1.0, 1.0, 0.0).normalize();
        let mut rng = Pcg::new(3, 9);
        let (wi, pdf, _) = sample_brdf(&m, n, wo, &mut rng);
        assert!(pdf > 0.0);
        assert!((wi - Vec3::new(-1.0, 1.0, 0.0).normalize()).length() < 0.15, "{wi}");
    }
}
