//! CPU side of a frame: turn the scene into vertex lists, and pick objects with the BVH.

use crate::{FrameGeometry, OrbitCamera, Vertex, ViewOptions};
use shape_geom::bvh::Bvh;
use shape_math::{Vec2, Vec3};
use shape_scene::{ObjectData, ObjectId, Scene};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShadingMode {
    Wireframe,
    #[default]
    Solid,
    /// Solid with every edge drawn on top.
    SolidWire,
}

const GRID_HALF: i32 = 20;
const GRID_COLOR: [f32; 4] = [0.22, 0.24, 0.28, 1.0];
const GRID_MAJOR: [f32; 4] = [0.30, 0.33, 0.38, 1.0];
const AXIS_X: [f32; 4] = [0.75, 0.25, 0.25, 1.0];
const AXIS_Z: [f32; 4] = [0.25, 0.40, 0.85, 1.0];
const WIRE: [f32; 4] = [0.05, 0.05, 0.06, 1.0];
const SELECTED_WIRE: [f32; 4] = [1.0, 0.60, 0.15, 1.0];
const ACTIVE_WIRE: [f32; 4] = [1.0, 0.80, 0.35, 1.0];
const DEFAULT_SURFACE: [f32; 4] = [0.62, 0.63, 0.66, 1.0];

/// Vertex lists for one frame of the scene.
pub fn build_frame(scene: &Scene, options: &ViewOptions) -> FrameGeometry {
    let mut out = FrameGeometry::default();
    if options.show_grid {
        grid(&mut out.lines);
    }
    for (id, object) in scene.objects() {
        if !object.visible {
            continue;
        }
        let Some((_, mesh)) = scene.object_mesh(id) else { continue };
        let material = match &object.data {
            ObjectData::Mesh { material: Some(m), .. } => scene.material(*m),
            _ => None,
        };
        let base = material.map(|m| m.base_color).unwrap_or(DEFAULT_SURFACE);
        let world = scene.world_transform(id);
        let positions: Vec<Vec3> = mesh.positions().map(|p| world.transform_point(p)).collect();
        let selected = scene.selection.contains(id);
        let active = scene.selection.active == Some(id);

        if options.shading != ShadingMode::Wireframe {
            for f in mesh.face_ids() {
                let vs: Vec<usize> = mesh.face_vertices(f).map(|v| v.index()).collect();
                let n = world.transform_vector(mesh.face_normal(f)).normalize_or_zero();
                for i in 1..vs.len() - 1 {
                    for &k in &[vs[0], vs[i], vs[i + 1]] {
                        out.triangles.push(Vertex::new(positions[k], n, base));
                    }
                }
            }
        }
        let wire_color = if active {
            Some(ACTIVE_WIRE)
        } else if selected {
            Some(SELECTED_WIRE)
        } else if options.shading != ShadingMode::Solid {
            Some(WIRE)
        } else {
            None
        };
        if let Some(c) = wire_color {
            for e in mesh.edge_ids() {
                let (a, b) = mesh.edge_vertices(e);
                out.lines.push(Vertex::new(positions[a.index()], Vec3::Y, c));
                out.lines.push(Vertex::new(positions[b.index()], Vec3::Y, c));
            }
        }
    }
    out
}

fn grid(lines: &mut Vec<Vertex>) {
    let h = GRID_HALF as f32;
    for i in -GRID_HALF..=GRID_HALF {
        let t = i as f32;
        let (cx, cz) = match i {
            0 => (AXIS_Z, AXIS_X),
            _ if i % 10 == 0 => (GRID_MAJOR, GRID_MAJOR),
            _ => (GRID_COLOR, GRID_COLOR),
        };
        // line along Z at x = t
        lines.push(Vertex::new(Vec3::new(t, 0.0, -h), Vec3::ZERO, cx));
        lines.push(Vertex::new(Vec3::new(t, 0.0, h), Vec3::ZERO, cx));
        // line along X at z = t
        lines.push(Vertex::new(Vec3::new(-h, 0.0, t), Vec3::ZERO, cz));
        lines.push(Vertex::new(Vec3::new(h, 0.0, t), Vec3::ZERO, cz));
    }
}

/// Object under a pixel, nearest first. `pixel` has y down; `size` is the viewport in pixels.
pub fn pick(scene: &Scene, camera: &OrbitCamera, pixel: Vec2, size: Vec2) -> Option<(ObjectId, f32)> {
    let ray = camera.ray_pixel(pixel, size);
    let mut best: Option<(ObjectId, f32)> = None;
    for (id, object) in scene.objects() {
        if !object.visible {
            continue;
        }
        let Some((_, mesh)) = scene.object_mesh(id) else { continue };
        // Cast in object space: transform the ray by the inverse world transform.
        let inv = scene.world_transform(id).inverse();
        let local = shape_math::Ray::new(inv.transform_point(ray.origin), inv.transform_vector(ray.direction));
        let Some(hit) = Bvh::build(mesh).intersect(&local) else { continue };
        // Distance in world units: re-project the local hit point.
        let world_hit = scene.world_transform(id).transform_point(local.at(hit.t));
        let t = (world_hit - ray.origin).dot(ray.direction);
        if best.is_none_or(|(_, bt)| t < bt) {
            best = Some((id, t));
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::cube;
    use shape_math::Transform;

    #[test]
    fn frame_has_grid_cube_and_wire_only_when_asked() {
        let mut scene = Scene::new("t");
        let id = scene.add_mesh_object("Cube", cube(1.0), None);
        let solid = build_frame(&scene, &ViewOptions::default());
        assert_eq!(solid.triangles.len(), 12 * 3);
        assert_eq!(solid.lines.len(), (GRID_HALF as usize * 2 + 1) * 4);
        let wire = build_frame(&scene, &ViewOptions { shading: ShadingMode::Wireframe, show_grid: false, ..Default::default() });
        assert!(wire.triangles.is_empty());
        assert_eq!(wire.lines.len(), 12 * 2);
        scene.selection.set(id);
        let sel = build_frame(&scene, &ViewOptions { show_grid: false, ..Default::default() });
        assert_eq!(sel.lines.len(), 24);
        assert_eq!(sel.lines[0].color, ACTIVE_WIRE);
    }

    #[test]
    fn pick_finds_nearest_object() {
        let mut scene = Scene::new("t");
        let near = scene.add_mesh_object("near", cube(1.0), None);
        let far = scene.add_mesh_object("far", cube(1.0), None);
        scene.object_mut(far).unwrap().transform = Transform::from_translation(Vec3::new(0.0, 0.0, -5.0));
        // camera on +Z looking at the origin: the near cube is in front
        let cam = OrbitCamera { yaw: 0.0, pitch: 0.0, distance: 6.0, ..Default::default() };
        let size = Vec2::new(400.0, 300.0);
        let (hit, t) = pick(&scene, &cam, size * 0.5, size).unwrap();
        assert_eq!(hit, near);
        // the ray starts on the near plane, so the cube face at z = 0.5 is 6 - 0.5 - near away
        assert!((t - (5.5 - cam.near)).abs() < 1e-3, "t = {t}");
        assert!(pick(&scene, &cam, Vec2::new(5.0, 5.0), size).is_none());
        // from behind, the far cube comes first
        let cam2 = OrbitCamera { yaw: std::f32::consts::PI, pitch: 0.0, distance: 12.0, ..Default::default() };
        assert_eq!(pick(&scene, &cam2, size * 0.5, size).unwrap().0, far);
    }
}
