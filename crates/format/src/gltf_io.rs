//! glTF 2.0: `.gltf` and `.glb` import through the `gltf` crate; export as a single `.gltf` with the
//! buffer embedded as a data URI so one file carries everything.

use crate::{Result, Scene, mesh_from_soup, world_polygons};
use base64::Engine;
use shape_math::{Mat4, Quat, Transform, Vec3};
use shape_scene::{Material, MaterialId, MeshData, MeshId, Object, ObjectData};
use std::collections::HashMap;
use std::path::Path;

pub fn load(path: &Path) -> Result<Scene> {
    let (doc, buffers, _images) = gltf::import(path)?;
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("gltf");
    let mut scene = Scene::new(name);

    let materials: Vec<MaterialId> = doc
        .materials()
        .map(|m| {
            let pbr = m.pbr_metallic_roughness();
            let e = m.emissive_factor();
            scene.add_material(Material {
                name: m.name().unwrap_or("Material").to_owned(),
                base_color: pbr.base_color_factor(),
                metallic: pbr.metallic_factor(),
                roughness: pbr.roughness_factor(),
                emissive: e,
                base_color_texture: None,
                nodes: None,
                viewport_color: [0.8, 0.8, 0.8, 1.0],
            })
        })
        .collect();

    // One MeshData per glTF mesh; primitives are merged into one polygon soup.
    let mut mesh_ids: HashMap<usize, (MeshId, Option<MaterialId>)> = HashMap::new();
    for m in doc.meshes() {
        let mut positions: Vec<Vec3> = Vec::new();
        let mut polys: Vec<Vec<u32>> = Vec::new();
        let mut material = None;
        for prim in m.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = prim.reader(|b| buffers.get(b.index()).map(|d| d.0.as_slice()));
            let Some(pos) = reader.read_positions() else { continue };
            let base = positions.len() as u32;
            positions.extend(pos.map(Vec3::from_array));
            let count = positions.len() as u32 - base;
            let indices: Vec<u32> = match reader.read_indices() {
                Some(idx) => idx.into_u32().collect(),
                None => (0..count).collect(),
            };
            for tri in indices.as_chunks::<3>().0 {
                polys.push(vec![base + tri[0], base + tri[1], base + tri[2]]);
            }
            if material.is_none() {
                material = prim.material().index().and_then(|i| materials.get(i).copied());
            }
        }
        if polys.is_empty() {
            continue;
        }
        let mesh = mesh_from_soup(&positions, &polys)?;
        let id = scene.add_mesh(MeshData::new(m.name().unwrap_or("Mesh"), mesh));
        mesh_ids.insert(m.index(), (id, material));
    }

    // Nodes: depth-first so parents exist before children.
    let root_nodes: Vec<gltf::Node> = match doc.default_scene().or_else(|| doc.scenes().next()) {
        Some(s) => s.nodes().collect(),
        None => doc.nodes().collect(),
    };
    for node in root_nodes {
        add_node(&mut scene, &node, None, &mesh_ids);
    }
    Ok(scene)
}

fn add_node(scene: &mut Scene, node: &gltf::Node, parent: Option<shape_scene::ObjectId>, meshes: &HashMap<usize, (MeshId, Option<MaterialId>)>) {
    let data = match node.mesh().and_then(|m| meshes.get(&m.index())) {
        Some(&(mesh, material)) => ObjectData::Mesh { mesh, material, modifiers: Vec::new() },
        None => ObjectData::Empty,
    };
    let mut object = Object::new(node.name().unwrap_or("Node"), data);
    object.transform = match node.transform() {
        gltf::scene::Transform::Matrix { matrix } => Transform::from_matrix(Mat4::from_cols_array_2d(&matrix)),
        gltf::scene::Transform::Decomposed { translation, rotation, scale } => {
            Transform { translation: Vec3::from_array(translation), rotation: Quat::from_array(rotation), scale: Vec3::from_array(scale) }
        }
    };
    object.parent = parent;
    let id = scene.add_object(object);
    for child in node.children() {
        add_node(scene, &child, Some(id), meshes);
    }
}

/// Write a `.gltf` with one node per visible mesh object (world transforms baked, as the other
/// exporters do) and the geometry in an embedded base64 buffer.
pub fn save(scene: &Scene, path: &Path) -> Result<()> {
    std::fs::write(path, to_json(scene)?)?;
    Ok(())
}

pub fn to_json(scene: &Scene) -> Result<String> {
    use serde_json::{Value, json};
    let mut bin: Vec<u8> = Vec::new();
    let mut buffer_views = Vec::new();
    let mut accessors = Vec::new();
    let mut meshes = Vec::new();
    let mut nodes = Vec::new();

    let mut materials: Vec<Value> = Vec::new();
    let mut material_index: HashMap<MaterialId, usize> = HashMap::new();
    for (id, m) in scene.materials() {
        material_index.insert(id, materials.len());
        materials.push(json!({
            "name": m.name,
            "pbrMetallicRoughness": {"baseColorFactor": m.base_color, "metallicFactor": m.metallic, "roughnessFactor": m.roughness},
            "emissiveFactor": m.emissive,
        }));
    }

    let object_materials: Vec<Option<MaterialId>> = scene
        .objects()
        .filter(|(id, o)| o.visible && scene.object_mesh(*id).is_some())
        .map(|(_, o)| if let ObjectData::Mesh { material, .. } = &o.data { *material } else { None })
        .collect();

    for ((name, pos, polys), material) in world_polygons(scene).into_iter().zip(object_materials) {
        let mut indices: Vec<u32> = Vec::new();
        for poly in &polys {
            for i in 1..poly.len() - 1 {
                indices.extend([poly[0], poly[i], poly[i + 1]]);
            }
        }
        let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        let pos_offset = bin.len();
        for p in &pos {
            for k in 0..3 {
                min[k] = min[k].min(p[k]);
                max[k] = max[k].max(p[k]);
                bin.extend_from_slice(&p[k].to_le_bytes());
            }
        }
        let pos_view = buffer_views.len();
        buffer_views.push(json!({"buffer": 0, "byteOffset": pos_offset, "byteLength": pos.len() * 12, "target": 34962}));
        let idx_offset = bin.len();
        for i in &indices {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        let idx_view = buffer_views.len();
        buffer_views.push(json!({"buffer": 0, "byteOffset": idx_offset, "byteLength": indices.len() * 4, "target": 34963}));
        let pos_acc = accessors.len();
        accessors.push(json!({"bufferView": pos_view, "componentType": 5126, "count": pos.len(), "type": "VEC3", "min": min, "max": max}));
        let idx_acc = accessors.len();
        accessors.push(json!({"bufferView": idx_view, "componentType": 5125, "count": indices.len(), "type": "SCALAR"}));
        let mut prim = json!({"attributes": {"POSITION": pos_acc}, "indices": idx_acc, "mode": 4});
        if let Some(mi) = material.and_then(|m| material_index.get(&m)) {
            prim["material"] = json!(mi);
        }
        let mesh_i = meshes.len();
        meshes.push(json!({"name": name, "primitives": [prim]}));
        nodes.push(json!({"name": name, "mesh": mesh_i}));
    }

    let uri = format!("data:application/octet-stream;base64,{}", base64::engine::general_purpose::STANDARD.encode(&bin));
    let doc = json!({
        "asset": {"version": "2.0", "generator": format!("Redrob Shape {}", env!("CARGO_PKG_VERSION"))},
        "scene": 0,
        "scenes": [{"name": scene.name, "nodes": (0..nodes.len()).collect::<Vec<_>>()}],
        "nodes": nodes,
        "meshes": meshes,
        "materials": materials,
        "accessors": accessors,
        "bufferViews": buffer_views,
        "buffers": [{"byteLength": bin.len(), "uri": uri}],
    });
    Ok(serde_json::to_string_pretty(&doc)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{assert_same_geometry, sample_scene, scratch};

    #[test]
    fn round_trip_keeps_geometry_and_material() {
        let scene = sample_scene();
        let path = scratch("sample.gltf");
        save(&scene, &path).unwrap();
        let back = load(&path).unwrap();
        assert_same_geometry(&scene, &back);
        let cube = back.objects().find(|(_, o)| o.name == "Cube").unwrap().1;
        let ObjectData::Mesh { material: Some(m), .. } = &cube.data else { panic!("cube lost its material") };
        let m = *m;
        assert_eq!(back.material(m).unwrap().base_color, [0.8, 0.1, 0.1, 1.0]);
        // Faces survive as triangles; the welded cube is closed again.
        let (id, _) = back.objects().find(|(_, o)| o.name == "Cube").unwrap();
        let (_, mesh) = back.object_mesh(id).unwrap();
        assert!(mesh.is_closed());
        assert_eq!(mesh.face_count(), 12);
    }
}
