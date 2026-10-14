//! Flatten a scene for rendering.

use crate::bvh::{Bvh, Tri};
use crate::camera::RenderCamera;
use crate::integrator::Settings;
use shape_math::Vec3;
use shape_scene::{LightKind, ObjectData, Scene};

#[derive(Clone, Copy, Debug)]
pub struct RenderMaterial {
    pub base: Vec3,
    pub metallic: f32,
    pub roughness: f32,
    pub emissive: Vec3,
}

#[derive(Clone, Copy, Debug)]
pub enum RenderLight {
    Point {
        position: Vec3,
        intensity: Vec3,
    },
    /// `direction` points towards the light.
    Sun {
        direction: Vec3,
        irradiance: Vec3,
    },
    Spot {
        position: Vec3,
        intensity: Vec3,
        direction: Vec3,
        cos_inner: f32,
        cos_outer: f32,
    },
}

pub struct RenderScene {
    pub bvh: Bvh,
    pub materials: Vec<RenderMaterial>,
    pub lights: Vec<RenderLight>,
    pub camera: RenderCamera,
    pub sky: Vec3,
}

/// Radiant intensity of a point light of `power` watts, isotropic: W / (4 pi) per steradian, with
/// the same scale the viewport uses so the two roughly agree.
fn point_intensity(power: f32, color: [f32; 3]) -> Vec3 {
    Vec3::from_array(color) * (power / (4.0 * std::f32::consts::PI)) * 0.25
}

impl RenderScene {
    pub fn build(scene: &Scene, camera: RenderCamera, settings: &Settings) -> Self {
        let mut materials = vec![RenderMaterial { base: Vec3::splat(0.62), metallic: 0.0, roughness: 0.5, emissive: Vec3::ZERO }];
        let mut material_index = std::collections::HashMap::new();
        for (id, m) in scene.materials() {
            material_index.insert(id, materials.len() as u32);
            materials.push(RenderMaterial {
                base: Vec3::new(m.base_color[0], m.base_color[1], m.base_color[2]),
                metallic: m.metallic.clamp(0.0, 1.0),
                roughness: m.roughness.clamp(0.02, 1.0),
                emissive: Vec3::from_array(m.emissive),
            });
        }
        let mut tris = Vec::new();
        let mut lights = Vec::new();
        for (id, object) in scene.objects() {
            if !object.visible {
                continue;
            }
            let world = scene.world_transform(id);
            match &object.data {
                ObjectData::Mesh { material, .. } => {
                    let Some(mesh) = shape_ops::evaluated_mesh(scene, id) else { continue };
                    let mi = material.and_then(|m| material_index.get(&m).copied()).unwrap_or(0);
                    let positions: Vec<Vec3> = mesh.positions().map(|p| world.transform_point(p)).collect();
                    for f in mesh.face_ids() {
                        let vs: Vec<usize> = mesh.face_vertices(f).map(|v| v.index()).collect();
                        let n = world.transform_vector(mesh.face_normal(f)).normalize_or_zero();
                        for i in 1..vs.len() - 1 {
                            tris.push(Tri { a: positions[vs[0]], b: positions[vs[i]], c: positions[vs[i + 1]], normal: n, material: mi });
                        }
                    }
                }
                ObjectData::Light(light) => {
                    let shine = world.transform_vector(-Vec3::Z).normalize_or_zero();
                    lights.push(match light.kind {
                        LightKind::Point => RenderLight::Point { position: world.translation, intensity: point_intensity(light.power, light.color) },
                        LightKind::Sun => RenderLight::Sun { direction: -shine, irradiance: Vec3::from_array(light.color) * light.power },
                        LightKind::Spot { inner_angle, outer_angle } => RenderLight::Spot {
                            position: world.translation,
                            intensity: point_intensity(light.power, light.color),
                            direction: shine,
                            cos_inner: inner_angle.cos(),
                            cos_outer: outer_angle.max(inner_angle + 1e-3).cos(),
                        },
                    });
                }
                _ => {}
            }
        }
        Self { bvh: Bvh::build(tris), materials, lights, camera, sky: settings.sky }
    }

    /// The scene's first camera object, as a render camera, if any.
    pub fn scene_camera(scene: &Scene) -> Option<RenderCamera> {
        scene.objects().find_map(|(id, o)| match &o.data {
            ObjectData::Camera(c) if o.visible => Some(RenderCamera::from_matrix(scene.world_transform(id).to_matrix(), c.fov_y)),
            _ => None,
        })
    }
}
