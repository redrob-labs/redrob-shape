//! Shared pieces for the interchange formats of P13: the per-object records an exporter writes
//! (local mesh, world/local transform, material colour) and the Z-up <-> Y-up swap Blender's
//! exporters need (Blender is Z-up; Shape, glTF and FBX are Y-up).

use shape_math::{Mat4, Vec3};
use shape_scene::{ObjectData, Scene};

/// One exportable object: name, local-space polygons, its world matrix and material colour.
pub(crate) struct ExportObject {
    pub name: String,
    pub positions: Vec<Vec3>,
    pub polygons: Vec<Vec<u32>>,
    pub world: Mat4,
    /// Linear RGBA of the material, when the object has one.
    pub color: Option<[f32; 4]>,
    pub material_name: Option<String>,
}

pub(crate) fn export_objects(scene: &Scene) -> Vec<ExportObject> {
    let mut out = Vec::new();
    for (id, o) in scene.objects() {
        if !scene.is_visible(id) {
            continue;
        }
        let Some(mesh) = shape_ops::evaluated_mesh(scene, id) else { continue };
        let (positions, polygons) = mesh.to_polygons();
        let material = match &o.data {
            ObjectData::Mesh { material: Some(m), .. } => scene.material(*m),
            _ => None,
        };
        out.push(ExportObject {
            name: safe_name(&o.name),
            positions,
            polygons,
            world: scene.world_transform(id).to_matrix(),
            color: material.map(|m| m.base_color),
            material_name: material.map(|m| safe_name(&m.name)),
        });
    }
    out
}

/// Names usable as XML ids / USD prim names / FBX names: letters, digits and underscores.
pub(crate) fn safe_name(s: &str) -> String {
    let mut out: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
    if out.is_empty() || out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

/// Blender's Z-up frame to our Y-up frame: (x, y, z) -> (x, z, -y).
pub(crate) fn z_up_to_y_up(p: Vec3) -> Vec3 {
    Vec3::new(p.x, p.z, -p.y)
}

/// The same change of basis for a matrix: C * M * C^-1 with C the axis swap above.
pub(crate) fn z_up_matrix_to_y_up(m: Mat4) -> Mat4 {
    let c = Mat4::from_cols_array(&[1.0, 0.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    c * m * c.inverse()
}

/// Parse a run of whitespace-separated floats.
pub(crate) fn floats(text: &str) -> Vec<f32> {
    text.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).filter_map(|t| t.parse::<f32>().ok()).collect()
}

pub(crate) fn ints(text: &str) -> Vec<i64> {
    text.split(|c: char| c.is_whitespace() || c == ',').filter(|t| !t.is_empty()).filter_map(|t| t.parse::<i64>().ok()).collect()
}

/// Build the scene objects from imported records (name, positions, polygons, world matrix).
pub(crate) fn scene_from_records(name: &str, records: Vec<(String, Vec<Vec3>, Vec<Vec<u32>>, Mat4, Option<[f32; 4]>)>) -> crate::Result<Scene> {
    let mut scene = Scene::new(name);
    for (obj_name, positions, polygons, world, color) in records {
        if polygons.is_empty() {
            continue;
        }
        let mesh = crate::mesh_from_soup(&positions, &polygons)?;
        let material =
            color.map(|c| scene.add_material(shape_scene::Material { name: format!("{obj_name}_mat"), base_color: c, ..Default::default() }));
        let id = scene.add_mesh_object(obj_name, mesh, material);
        if let Some(o) = scene.object_mut(id) {
            o.transform = shape_math::Transform::from_matrix(world);
        }
    }
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_swap_round_trips_points_and_matrices() {
        let p = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(z_up_to_y_up(p), Vec3::new(1.0, 3.0, -2.0));
        let m = Mat4::from_translation(p);
        let t = z_up_matrix_to_y_up(m).transform_point3(Vec3::ZERO);
        assert!((t - z_up_to_y_up(p)).length() < 1e-6, "{t}");
        assert_eq!(safe_name("Cube.001"), "Cube_001");
        assert_eq!(safe_name("1abc"), "_1abc");
        assert_eq!(floats("1 2.5, -3"), vec![1.0, 2.5, -3.0]);
    }
}
