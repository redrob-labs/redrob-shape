//! CPU side of a frame: turn the scene into vertex lists, and pick objects with the BVH.

use crate::{ColorMode, FrameGeometry, GpuLight, OrbitCamera, SolidOptions, Vertex, ViewOptions};
use shape_geom::FaceId;
use shape_geom::bvh::Bvh;
use shape_math::{Vec2, Vec3};
use shape_scene::{LightKind, ObjectData, ObjectId, Scene};
use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ShadingMode {
    Wireframe,
    /// Blender's Solid: one studio grey for every surface, no materials.
    #[default]
    Solid,
    /// Solid with every edge drawn on top.
    SolidWire,
    /// Blender's Material Preview: materials, image textures and the scene's lights.
    MaterialPreview,
}

impl ShadingMode {
    pub const ALL: [ShadingMode; 4] = [ShadingMode::Wireframe, ShadingMode::Solid, ShadingMode::SolidWire, ShadingMode::MaterialPreview];
    pub fn name(self) -> &'static str {
        match self {
            ShadingMode::Wireframe => "Wireframe",
            ShadingMode::Solid => "Solid",
            ShadingMode::SolidWire => "Solid + Wireframe",
            ShadingMode::MaterialPreview => "Material Preview",
        }
    }
    /// Surfaces take their material colour (else Blender's solid grey).
    pub fn uses_materials(self) -> bool {
        self == ShadingMode::MaterialPreview
    }
}

/// Solid-mode surface colour per Blender's shading popover "Color" setting.
fn solid_color(scene: &Scene, id: ObjectId, data: &ObjectData, solid: &SolidOptions) -> [f32; 4] {
    match solid.color {
        ColorMode::Material => match data {
            ObjectData::Mesh { material: Some(m), .. } => scene.material(*m).map(|m| m.viewport_color).unwrap_or(DEFAULT_SURFACE),
            _ => DEFAULT_SURFACE,
        },
        ColorMode::Object => DEFAULT_SURFACE,
        ColorMode::Random => {
            // A stable pastel per object id (golden-ratio hue walk).
            let hue = (id.0 as f32 * 0.618_034) % 1.0;
            let (r, g, b) = hsv(hue, 0.45, 0.85);
            [r, g, b, 1.0]
        }
        ColorMode::Single => solid.single_color,
    }
}

fn hsv(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    match i as i32 % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
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
// Blender's selected face: the surface tinted orange (measured #9B7D66..#A58B77 lit in its theme).
const SELECTED_FACE: [f32; 4] = [0.55, 0.33, 0.18, 1.0];
const GIZMO: [f32; 4] = [0.95, 0.85, 0.40, 1.0];
const CAMERA_GIZMO: [f32; 4] = [0.70, 0.75, 0.85, 1.0];
/// Power at which a point light's uploaded radiance is 1 at one metre.
const POINT_POWER_SCALE: f32 = 1.0 / 80.0;
const SUN_SCALE: f32 = 0.35;
const EDIT_WIRE: [f32; 4] = [0.10, 0.10, 0.12, 1.0];
/// Blender's seam edge colour.
const SEAM: [f32; 4] = [0.86, 0.16, 0.16, 1.0];
/// Blender's edit-mode selection orange for edges and vertices, and its black vertex dots.
const EDIT_SELECTED: [f32; 4] = [0.79, 0.29, 0.03, 1.0]; // #E89330 in Blender's dark theme
const EDIT_VERTEX: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

/// The mesh an object is drawn with: the base mesh while it is being edited, otherwise the
/// evaluated one.
pub fn display_mesh(scene: &Scene, id: ObjectId) -> Option<Cow<'_, shape_geom::Mesh>> {
    if scene.edit.as_ref().is_some_and(|e| e.object == id) {
        scene.object_mesh(id).map(|(_, m)| Cow::Borrowed(m))
    } else {
        shape_ops::evaluated_mesh(scene, id)
    }
}

/// Vertex lists for one frame of the scene.
pub fn build_frame(scene: &Scene, options: &ViewOptions) -> FrameGeometry {
    let mut out = FrameGeometry::default();
    if options.show_grid {
        grid(&mut out.lines);
    }
    out.lights = scene_lights(scene);
    for (id, object) in scene.objects() {
        if !scene.is_visible(id) {
            continue;
        }
        let world = scene.world_transform(id);
        let selected = scene.selection.contains(id);
        match &object.data {
            ObjectData::Light(light) => {
                let c = if selected { SELECTED_WIRE } else { GIZMO };
                light_gizmo(&mut out.lines, &world, light, c);
                continue;
            }
            ObjectData::Camera(cam) => {
                let c = if selected { SELECTED_WIRE } else { CAMERA_GIZMO };
                camera_gizmo(&mut out.lines, &world, cam, c);
                continue;
            }
            ObjectData::Empty => {
                let c = if selected { SELECTED_WIRE } else { GIZMO };
                axes_gizmo(&mut out.lines, world.translation, 0.3, c);
                continue;
            }
            ObjectData::Armature(arm) => {
                armature_gizmo(&mut out.lines, &world, arm, selected);
                continue;
            }
            ObjectData::Mesh { .. } => {}
        }
        let Some(mesh) = display_mesh(scene, id) else { continue };
        let mesh = mesh.as_ref();
        let editing = scene.edit.as_ref().filter(|e| e.object == id);
        let material = match &object.data {
            ObjectData::Mesh { material: Some(m), .. } if options.shading.uses_materials() => scene.material(*m),
            _ => None,
        };
        let base = if options.shading.uses_materials() {
            material.map(|m| m.base_color).unwrap_or(DEFAULT_SURFACE)
        } else {
            solid_color(scene, id, &object.data, &options.solid)
        };
        let (metallic, roughness, emissive) = material.map(|m| (m.metallic, m.roughness, m.emissive)).unwrap_or((0.0, 0.5, [0.0; 3]));
        // Image texture: only with UVs on the base mesh (modifiers drop them), in Material Preview.
        let uvs = match (&object.data, material.and_then(|m| m.base_color_texture)) {
            (ObjectData::Mesh { mesh: mid, modifiers, .. }, Some(tex)) if modifiers.iter().all(|m| !m.enabled()) => {
                scene.mesh(*mid).and_then(|d| d.uvs.as_ref()).filter(|u| u.len() == mesh.face_count()).map(|u| (tex, u))
            }
            _ => None,
        };
        let positions: Vec<Vec3> = mesh.positions().map(|p| world.transform_point(p)).collect();
        let active = scene.selection.active == Some(id);

        if options.shading != ShadingMode::Wireframe {
            let mut textured_tris: Vec<Vertex> = Vec::new();
            for f in mesh.face_ids() {
                let vs: Vec<usize> = mesh.face_vertices(f).map(|v| v.index()).collect();
                let n = world.transform_vector(mesh.face_normal(f)).normalize_or_zero();
                let selected_face = editing.is_some_and(|e| e.faces.contains(&f));
                let color = if selected_face { SELECTED_FACE } else { base };
                let face_uv = uvs.as_ref().map(|(_, u)| &u[f.index()]).filter(|u| u.len() == vs.len() && !selected_face);
                for i in 1..vs.len() - 1 {
                    for &c in &[0, i, i + 1] {
                        let v = Vertex::new(positions[vs[c]], n, color).with_material(metallic, roughness, emissive);
                        match face_uv {
                            Some(u) => textured_tris.push(v.with_uv(u[c])),
                            None => out.triangles.push(v),
                        }
                    }
                }
            }
            if let (Some((tex, _)), false) = (&uvs, textured_tris.is_empty()) {
                match out.textured.iter_mut().find(|(t, _)| t == tex) {
                    Some((_, list)) => list.extend(textured_tris),
                    None => out.textured.push((*tex, textured_tris)),
                }
            }
        }
        let wire_color = if editing.is_some() {
            Some(EDIT_WIRE)
        } else if active {
            Some(ACTIVE_WIRE)
        } else if selected {
            Some(SELECTED_WIRE)
        } else if options.shading != ShadingMode::Solid {
            Some(WIRE)
        } else {
            None
        };
        if let Some(c) = wire_color {
            let seams = match &object.data {
                ObjectData::Mesh { mesh: mid, .. } if editing.is_some() => scene.mesh(*mid).map(|d| &d.seams),
                _ => None,
            };
            for e in mesh.edge_ids() {
                let (a, b) = mesh.edge_vertices(e);
                let is_seam = seams.is_some_and(|s| s.contains(&shape_ops::unwrap::seam_key(a, b)));
                let c = if editing.is_some_and(|ed| ed.edges.contains(&e)) {
                    EDIT_SELECTED
                } else if is_seam {
                    SEAM
                } else {
                    c
                };
                out.lines.push(Vertex::new(positions[a.index()], Vec3::Y, c));
                out.lines.push(Vertex::new(positions[b.index()], Vec3::Y, c));
            }
        }
        // Vertex dots in vertex select mode, selected ones orange.
        if let Some(ed) = editing
            && ed.mode == shape_scene::SelectMode::Vertex
        {
            for v in mesh.vertex_ids() {
                let c = if ed.verts.contains(&v) { EDIT_SELECTED } else { EDIT_VERTEX };
                out.points.push(Vertex::new(positions[v.index()], Vec3::Y, c));
            }
        }
    }
    out
}

/// Visible scene lights in world space, as the shader wants them.
pub fn scene_lights(scene: &Scene) -> Vec<GpuLight> {
    let mut out = Vec::new();
    for (id, object) in scene.objects() {
        let ObjectData::Light(light) = &object.data else { continue };
        if !scene.is_visible(id) {
            continue;
        }
        let world = scene.world_transform(id);
        let shine = world.transform_vector(-Vec3::Z).normalize_or_zero();
        let (kind, position, radiance, cone) = match light.kind {
            LightKind::Point => (0.0, world.translation, light.power * POINT_POWER_SCALE, [0.0, 0.0]),
            LightKind::Sun => (1.0, -shine, light.power * SUN_SCALE, [0.0, 0.0]),
            LightKind::Spot { inner_angle, outer_angle } => {
                (2.0, world.translation, light.power * POINT_POWER_SCALE, [inner_angle.cos(), outer_angle.max(inner_angle + 1e-3).cos()])
            }
        };
        out.push(GpuLight {
            position: position.extend(kind).to_array(),
            radiance: [light.color[0] * radiance, light.color[1] * radiance, light.color[2] * radiance, 0.0],
            direction: shine.extend(0.0).to_array(),
            cone: [cone[0], cone[1], 0.0, 0.0],
        });
    }
    out
}

fn seg(lines: &mut Vec<Vertex>, a: Vec3, b: Vec3, c: [f32; 4]) {
    lines.push(Vertex::new(a, Vec3::Y, c));
    lines.push(Vertex::new(b, Vec3::Y, c));
}

fn axes_gizmo(lines: &mut Vec<Vertex>, at: Vec3, r: f32, c: [f32; 4]) {
    for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
        seg(lines, at - axis * r, at + axis * r, c);
    }
}

/// Blender's octahedral bones: a thin diamond from head to tail, selected bones in the pose-mode
/// blue, the active one lighter.
fn armature_gizmo(lines: &mut Vec<Vertex>, world: &shape_math::Transform, arm: &shape_scene::Armature, object_selected: bool) {
    const BONE: [f32; 4] = [0.62, 0.62, 0.65, 1.0];
    const BONE_SELECTED: [f32; 4] = [0.35, 0.55, 0.85, 1.0];
    const BONE_ACTIVE: [f32; 4] = [0.55, 0.75, 1.0, 1.0];
    for (i, (head, tail)) in arm.display_segments().into_iter().enumerate() {
        let c = if arm.active == Some(i) {
            BONE_ACTIVE
        } else if arm.selected.contains(&i) || (object_selected && arm.selected.is_empty()) {
            BONE_SELECTED
        } else {
            BONE
        };
        let axis = tail - head;
        let len = axis.length().max(1e-6);
        let dir = axis / len;
        let side = dir.any_orthonormal_vector();
        let up = dir.cross(side);
        let mid = head + dir * (len * 0.1);
        let r = len * 0.1;
        let ring = [mid + side * r, mid + up * r, mid - side * r, mid - up * r].map(|p| world.transform_point(p));
        let (h, t) = (world.transform_point(head), world.transform_point(tail));
        for k in 0..4 {
            for (a, b) in [(h, ring[k]), (ring[k], t), (ring[k], ring[(k + 1) % 4])] {
                lines.push(Vertex::new(a, Vec3::Y, c));
                lines.push(Vertex::new(b, Vec3::Y, c));
            }
        }
    }
}

fn light_gizmo(lines: &mut Vec<Vertex>, world: &shape_math::Transform, light: &shape_scene::Light, c: [f32; 4]) {
    let at = world.translation;
    axes_gizmo(lines, at, 0.25, c);
    // a small circle in the XZ plane
    let n = 16;
    for i in 0..n {
        let (a0, a1) = (i as f32 / n as f32 * std::f32::consts::TAU, (i + 1) as f32 / n as f32 * std::f32::consts::TAU);
        seg(lines, at + Vec3::new(a0.cos(), 0.0, a0.sin()) * 0.35, at + Vec3::new(a1.cos(), 0.0, a1.sin()) * 0.35, c);
    }
    let shine = world.transform_vector(-Vec3::Z).normalize_or_zero();
    match light.kind {
        LightKind::Sun => seg(lines, at, at + shine * 2.0, c),
        LightKind::Spot { outer_angle, .. } => {
            let len = 2.0;
            let r = outer_angle.tan() * len;
            let right = shine.cross(Vec3::Y).normalize_or_zero();
            let right = if right.length_squared() < 1e-6 { Vec3::X } else { right };
            let up = right.cross(shine);
            let tip = at + shine * len;
            for i in 0..4 {
                let a = i as f32 / 4.0 * std::f32::consts::TAU;
                seg(lines, at, tip + (right * a.cos() + up * a.sin()) * r, c);
            }
            for i in 0..n {
                let (a0, a1) = (i as f32 / n as f32 * std::f32::consts::TAU, (i + 1) as f32 / n as f32 * std::f32::consts::TAU);
                seg(lines, tip + (right * a0.cos() + up * a0.sin()) * r, tip + (right * a1.cos() + up * a1.sin()) * r, c);
            }
        }
        LightKind::Point => {}
    }
}

fn camera_gizmo(lines: &mut Vec<Vertex>, world: &shape_math::Transform, cam: &shape_scene::Camera, c: [f32; 4]) {
    let depth = 1.0;
    let h = (cam.fov_y * 0.5).tan() * depth;
    let w = h * 16.0 / 9.0;
    let p = |x: f32, y: f32, z: f32| world.transform_point(Vec3::new(x, y, z));
    let eye = p(0.0, 0.0, 0.0);
    let corners = [p(-w, -h, -depth), p(w, -h, -depth), p(w, h, -depth), p(-w, h, -depth)];
    for i in 0..4 {
        seg(lines, eye, corners[i], c);
        seg(lines, corners[i], corners[(i + 1) % 4], c);
    }
    // up marker: a triangle above the frame
    seg(lines, p(-w * 0.4, h * 1.05, -depth), p(w * 0.4, h * 1.05, -depth), c);
    seg(lines, p(-w * 0.4, h * 1.05, -depth), p(0.0, h * 1.5, -depth), c);
    seg(lines, p(w * 0.4, h * 1.05, -depth), p(0.0, h * 1.5, -depth), c);
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

/// What a pick ray hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickHit {
    pub object: ObjectId,
    /// Distance along the pick ray in world units.
    pub t: f32,
    /// Face of the displayed mesh (base mesh in edit mode, evaluated otherwise).
    pub face: FaceId,
    /// Hit point in the object's local space.
    pub local_point: Vec3,
}

/// Object under a pixel, nearest first. `pixel` has y down; `size` is the viewport in pixels.
pub fn pick(scene: &Scene, camera: &OrbitCamera, pixel: Vec2, size: Vec2) -> Option<(ObjectId, f32)> {
    pick_detail(scene, camera, pixel, size).map(|h| (h.object, h.t))
}

/// Like [`pick`] but with the face and hit point. In edit mode only the edited object is tested.
pub fn pick_detail(scene: &Scene, camera: &OrbitCamera, pixel: Vec2, size: Vec2) -> Option<PickHit> {
    let ray = camera.ray_pixel(pixel, size);
    let mut best: Option<PickHit> = None;
    for (id, _object) in scene.objects() {
        if !scene.is_visible(id) {
            continue;
        }
        if let Some(e) = &scene.edit
            && e.object != id
        {
            continue;
        }
        let Some(mesh) = display_mesh(scene, id) else { continue };
        // Cast in object space: transform the ray by the inverse world transform.
        let inv = scene.world_transform(id).inverse();
        let local = shape_math::Ray::new(inv.transform_point(ray.origin), inv.transform_vector(ray.direction));
        let Some(hit) = Bvh::build(&mesh).intersect(&local) else { continue };
        // Distance in world units: re-project the local hit point.
        let local_point = local.at(hit.t);
        let world_hit = scene.world_transform(id).transform_point(local_point);
        let t = (world_hit - ray.origin).dot(ray.direction);
        if best.is_none_or(|b| t < b.t) {
            best = Some(PickHit { object: id, t, face: hit.face, local_point });
        }
    }
    best
}

/// Edge of `face` nearest to a local-space point: for loop cut on the hovered edge.
/// The vertex of `face` nearest to a local-space point (vertex picking).
pub fn nearest_vertex(mesh: &shape_geom::Mesh, face: FaceId, local_point: Vec3) -> Option<shape_geom::VertexId> {
    mesh.face_vertices(face).min_by(|&a, &b| {
        let da = (mesh.position(a) - local_point).length_squared();
        let db = (mesh.position(b) - local_point).length_squared();
        da.total_cmp(&db)
    })
}

pub fn nearest_edge(mesh: &shape_geom::Mesh, face: FaceId, local_point: Vec3) -> Option<shape_geom::EdgeId> {
    let mut best: Option<(f32, shape_geom::EdgeId)> = None;
    for h in mesh.face_halfedges(face) {
        let (a, b) = (mesh.position(mesh.from(h)), mesh.position(mesh.halfedge(h).to));
        let ab = b - a;
        let t = ((local_point - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
        let d = (a + ab * t - local_point).length();
        if best.is_none_or(|(bd, _)| d < bd) {
            best = Some((d, mesh.halfedge(h).edge));
        }
    }
    best.map(|(_, e)| e)
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

#[cfg(test)]
mod edit_tests {
    use super::*;
    use shape_geom::primitives::cube;
    use shape_scene::EditSelection;

    #[test]
    fn edit_mode_highlights_faces_and_picks_them() {
        let mut scene = Scene::new("t");
        let id = scene.add_mesh_object("Cube", cube(2.0), None);
        let cam = OrbitCamera { yaw: 0.0, pitch: 0.0, distance: 6.0, ..Default::default() };
        let size = Vec2::new(400.0, 300.0);
        let hit = pick_detail(&scene, &cam, size * 0.5, size).unwrap();
        let mesh = &scene.object_mesh(id).unwrap().1.clone();
        assert!(mesh.face_normal(hit.face).z > 0.9, "front (+Z) face");
        assert!((hit.local_point.z - 1.0).abs() < 1e-4);
        let edge = nearest_edge(mesh, hit.face, Vec3::new(0.9, 0.0, 1.0)).unwrap();
        let (a, b) = mesh.edge_vertices(edge);
        assert!(mesh.position(a).x > 0.9 && mesh.position(b).x > 0.9, "the x = 1 edge");

        let mut e = EditSelection::new(id);
        e.faces.insert(hit.face);
        scene.edit = Some(e);
        let frame = build_frame(&scene, &ViewOptions { show_grid: false, ..Default::default() });
        let orange = frame.triangles.iter().filter(|v| v.color == SELECTED_FACE).count();
        assert_eq!(orange, 6, "two triangles of the selected quad");
        assert_eq!(frame.lines.len(), 24, "edit wire on every edge");
        assert!(frame.points.is_empty(), "no vertex dots in face mode");
        let mut e = scene.edit.take().unwrap();
        e.flush(mesh);
        e.set_mode(shape_scene::SelectMode::Vertex, mesh);
        scene.edit = Some(e);
        let frame = build_frame(&scene, &ViewOptions { show_grid: false, ..Default::default() });
        assert_eq!(frame.points.len(), 8, "a dot per vertex in vertex mode");
        assert_eq!(frame.points.iter().filter(|p| p.color == EDIT_SELECTED).count(), 4, "the four selected corners are orange");
        assert_eq!(frame.lines.iter().filter(|p| p.color == EDIT_SELECTED).count(), 8, "the four selected edges, two ends each");
        assert_eq!(crate::expand_points(&frame.points).len(), 48);
    }
}

#[cfg(test)]
mod light_tests {
    use super::*;
    use shape_math::{Quat, Transform};
    use shape_scene::{Light, Object};

    #[test]
    fn lights_and_gizmos_reach_the_frame() {
        let mut scene = Scene::new("t");
        let mut sun = Object::new("Sun", ObjectData::Light(Light::sun(2.0)));
        sun.transform = Transform::from_rotation(Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2)); // shines down
        scene.add_object(sun);
        let mut point = Object::new("Lamp", ObjectData::Light(Light::point(800.0)));
        point.transform = Transform::from_translation(Vec3::new(0.0, 3.0, 0.0));
        scene.add_object(point);
        scene.add_object(Object::new("Camera", ObjectData::Camera(Default::default())));
        let lights = scene_lights(&scene);
        assert_eq!(lights.len(), 2);
        assert_eq!(lights[0].position[3], 1.0);
        assert!(lights[0].position[1] > 0.99, "sun direction points up (towards the light): {:?}", lights[0].position);
        assert_eq!(lights[1].position[3], 0.0);
        assert!((lights[1].radiance[0] - 10.0).abs() < 1e-4);
        let frame = build_frame(&scene, &ViewOptions { show_grid: false, ..Default::default() });
        assert!(frame.triangles.is_empty());
        assert!(frame.lines.len() > 40, "gizmo lines: {}", frame.lines.len());
        assert_eq!(frame.lights.len(), 2);
    }
}
