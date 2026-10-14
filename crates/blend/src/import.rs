//! Walk a parsed file into a [`Scene`].

use crate::dna::View;
use crate::{BlendFile, Result};
use shape_geom::Mesh;
use shape_math::{Mat4, Quat, Transform, Vec3};
use shape_scene::{Camera, Light, LightKind, Material, MaterialId, MeshData, MeshId, Object, ObjectData, Scene};
use std::collections::HashMap;
use std::path::Path;

const OB_MESH: i32 = 1;
const OB_LAMP: i32 = 10;
const OB_CAMERA: i32 = 11;
const CD_MVERT: i32 = 0;
const CD_PROP_INT32: i32 = 11;
const CD_MPOLY: i32 = 25;
const CD_MLOOP: i32 = 26;
const CD_PROP_FLOAT3: i32 = 48;
// bke::AttrType / AttrDomain / AttrStorageType (Blender 5.0 AttributeStorage).
const ATTR_INT32: i32 = 3;
const ATTR_FLOAT3: i32 = 7;
const DOMAIN_POINT: i32 = 0;
const DOMAIN_CORNER: i32 = 3;
const STORAGE_ARRAY: i32 = 0;

/// Blender (Z up) -> Shape (Y up): x stays, Blender y -> -z, Blender z -> y.
fn to_y_up(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, -v.y)
}

/// The same change of basis for a whole matrix.
fn convert_matrix(m: Mat4) -> Mat4 {
    let c = Mat4::from_cols(Vec3::X.extend(0.0), Vec3::NEG_Z.extend(0.0), Vec3::Y.extend(0.0), shape_math::Vec4::W);
    c * m * c.inverse()
}

pub fn import(path: &Path) -> Result<Scene> {
    let file = BlendFile::open(path)?;
    import_file(&file, path.file_stem().and_then(|s| s.to_str()).unwrap_or("blend"))
}

pub fn import_file(file: &BlendFile, name: &str) -> Result<Scene> {
    let mut scene = Scene::new(name);
    let mut materials: HashMap<u64, MaterialId> = HashMap::new();
    for (bi, b) in file.ids(b"MA") {
        let Some(v) = file.view(bi, 0) else { continue };
        let m = Material {
            name: id_name(&v).unwrap_or("Material".into()),
            base_color: [v.f32("r").unwrap_or(0.8), v.f32("g").unwrap_or(0.8), v.f32("b").unwrap_or(0.8), v.f32("a").unwrap_or(1.0)],
            metallic: v.f32("metallic").unwrap_or(0.0),
            roughness: v.f32("roughness").unwrap_or(0.5),
            emissive: [0.0; 3],
        };
        materials.insert(b.old, scene.add_material(m));
    }

    let mut meshes: HashMap<u64, (MeshId, Option<MaterialId>)> = HashMap::new();
    for (bi, b) in file.ids(b"ME") {
        let Some(v) = file.view(bi, 0) else { continue };
        let name = id_name(&v).unwrap_or("Mesh".into());
        match read_mesh(file, bi, &v) {
            Ok(Some(mesh)) => {
                let material = first_material(file, bi, &v).and_then(|p| materials.get(&p).copied());
                let id = scene.add_mesh(MeshData::new(name, mesh));
                meshes.insert(b.old, (id, material));
            }
            Ok(None) => log::warn!("mesh {name}: no geometry layout this reader understands"),
            Err(e) => log::warn!("mesh {name}: {e}"),
        }
    }

    // Objects: world matrices from loc/rot/size and the parent chain, baked flat.
    let objects: Vec<(usize, u64)> = file.ids(b"OB").map(|(i, b)| (i, b.old)).collect();
    let mut local: HashMap<u64, (Mat4, u64, Mat4)> = HashMap::new(); // addr -> (local, parent addr, parentinv)
    for &(bi, addr) in &objects {
        let Some(v) = file.view(bi, 0) else { continue };
        let loc = vec3(&v, "loc");
        let size = v.f32s("size").map(|s| Vec3::new(s[0], s[1], s[2])).unwrap_or(Vec3::ONE);
        let rotmode = v.i32("rotmode").unwrap_or(1);
        let rot = if rotmode == 0 {
            let q = v.f32s("quat").unwrap_or(vec![1.0, 0.0, 0.0, 0.0]);
            Quat::from_xyzw(q[1], q[2], q[3], q[0]).normalize()
        } else {
            let r = vec3(&v, "rot");
            Quat::from_euler(shape_math::glam_euler(), r.x, r.y, r.z)
        };
        let m = Mat4::from_scale_rotation_translation(size, rot, loc);
        let parent = v.pointer("parent").unwrap_or(0);
        let parentinv =
            v.f32s("parentinv").filter(|m| m.len() == 16).map(|m| Mat4::from_cols_array(&m.try_into().expect("16"))).unwrap_or(Mat4::IDENTITY);
        local.insert(addr, (m, parent, parentinv));
    }
    fn world(addr: u64, local: &HashMap<u64, (Mat4, u64, Mat4)>, depth: u32) -> Mat4 {
        let Some((m, parent, parentinv)) = local.get(&addr) else { return Mat4::IDENTITY };
        if *parent == 0 || depth > 64 { *m } else { world(*parent, local, depth + 1) * *parentinv * *m }
    }
    for &(bi, addr) in &objects {
        let Some(v) = file.view(bi, 0) else { continue };
        let name = id_name(&v).unwrap_or("Object".into());
        let kind = v.i32("type").unwrap_or(0);
        let data_ptr = v.pointer("data").unwrap_or(0);
        let data = match kind {
            OB_MESH => match meshes.get(&data_ptr) {
                Some(&(mesh, material)) => {
                    // Materials may be linked to the object instead of the mesh (older files).
                    let material = material.or_else(|| first_material(file, bi, &v).and_then(|p| materials.get(&p).copied()));
                    ObjectData::Mesh { mesh, material, modifiers: Vec::new() }
                }
                None => ObjectData::Empty,
            },
            OB_LAMP => ObjectData::Light(read_light(file, data_ptr)),
            OB_CAMERA => ObjectData::Camera(read_camera(file, data_ptr)),
            _ => ObjectData::Empty,
        };
        let mut object = Object::new(name, data);
        object.transform = Transform::from_matrix(convert_matrix(world(addr, &local, 0)));
        scene.add_object(object);
    }
    Ok(scene)
}

fn id_name(v: &View) -> Option<String> {
    // ID.name is "OBCube": two-letter code then the name.
    let id = v.sub("id")?;
    let s = id.string("name")?;
    Some(s.get(2..).unwrap_or(&s).to_owned())
}

fn vec3(v: &View, name: &str) -> Vec3 {
    v.f32s(name).filter(|a| a.len() >= 3).map(|a| Vec3::new(a[0], a[1], a[2])).unwrap_or(Vec3::ZERO)
}

/// First slot of a `Material **mat` array on a mesh or an object.
fn first_material(file: &BlendFile, mesh_block: usize, v: &View) -> Option<u64> {
    let totcol = v.i32("totcol").unwrap_or(0);
    if totcol <= 0 {
        return None;
    }
    let ptr = v.pointer("mat")?;
    let block = file.resolve(ptr, mesh_block)?;
    let data = file.data(block);
    (data.len() >= file.header.pointer_size).then(|| file.endian().pointer(data, file.header.pointer_size)).filter(|&p| p != 0)
}

/// Find a CustomData layer by type (and name when given) and return its data block.
fn layer_block(file: &BlendFile, mesh_block: usize, cd: &View, cd_type: i32, name: Option<&str>) -> Option<usize> {
    let layers_ptr = cd.pointer("layers")?;
    let total = cd.i32("totlayer").unwrap_or(0).max(0) as usize;
    let block = file.resolve(layers_ptr, mesh_block)?;
    for layer in file.views_as(block, "CustomDataLayer").into_iter().take(total) {
        if layer.i32("type") != Some(cd_type) {
            continue;
        }
        if let Some(want) = name
            && layer.string("name").as_deref() != Some(want)
        {
            continue;
        }
        let data = layer.pointer("data")?;
        return file.resolve(data, mesh_block);
    }
    None
}

/// Blender 5.0+: `Mesh.attribute_storage.dna_attributes` is an array of `Attribute`, each naming a
/// `raw_data` string block and an `AttributeArray` whose `data` block holds the values.
fn attribute_block(file: &BlendFile, mesh_block: usize, v: &View, name: &str, data_type: i32, domain: i32) -> Option<usize> {
    let storage = v.sub("attribute_storage")?;
    let ptr = storage.pointer("dna_attributes")?;
    let n = storage.i32("dna_attributes_num").unwrap_or(0).max(0) as usize;
    let block = file.resolve(ptr, mesh_block)?;
    for attr in file.views_as(block, "Attribute").into_iter().take(n) {
        if attr.i32("data_type") != Some(data_type) || attr.i32("domain") != Some(domain) || attr.i32("storage_type") != Some(STORAGE_ARRAY) {
            continue;
        }
        let name_block = attr.pointer("name").and_then(|p| file.resolve(p, mesh_block))?;
        let d = file.data(name_block);
        let attr_name = String::from_utf8_lossy(&d[..d.iter().position(|&c| c == 0).unwrap_or(d.len())]);
        if attr_name != name {
            continue;
        }
        let array_block = attr.pointer("data").and_then(|p| file.resolve(p, mesh_block))?;
        let array = file.views_as(array_block, "AttributeArray").into_iter().next()?;
        return array.pointer("data").and_then(|p| file.resolve(p, mesh_block));
    }
    None
}

fn f32_array(file: &BlendFile, block: usize, count: usize) -> Vec<f32> {
    let d = file.data(block);
    (0..count.min(d.len() / 4)).map(|i| file.endian().f32(&d[i * 4..])).collect()
}

fn i32_array(file: &BlendFile, block: usize, count: usize) -> Vec<i32> {
    let d = file.data(block);
    (0..count.min(d.len() / 4)).map(|i| file.endian().i32(&d[i * 4..])).collect()
}

fn read_mesh(file: &BlendFile, bi: usize, v: &View) -> Result<Option<Mesh>> {
    let verts_num = v.i32("verts_num").or_else(|| v.i32("totvert")).unwrap_or(0).max(0) as usize;
    let faces_num = v.i32("faces_num").or_else(|| v.i32("totpoly")).unwrap_or(0).max(0) as usize;
    let corners_num = v.i32("corners_num").or_else(|| v.i32("totloop")).unwrap_or(0).max(0) as usize;
    if verts_num == 0 {
        return Ok(None);
    }

    // ---- positions ----
    let vdata = v.sub("vert_data").or_else(|| v.sub("vdata"));
    let mut positions: Option<Vec<Vec3>> = None;
    let position_block = attribute_block(file, bi, v, "position", ATTR_FLOAT3, DOMAIN_POINT)
        .or_else(|| vdata.as_ref().and_then(|cd| layer_block(file, bi, cd, CD_PROP_FLOAT3, Some("position"))));
    if let Some(block) = position_block {
        let f = f32_array(file, block, verts_num * 3);
        positions = Some(f.as_chunks::<3>().0.iter().map(|c| to_y_up(Vec3::new(c[0], c[1], c[2]))).collect());
    }
    if positions.is_none() {
        let mvert_block =
            vdata.as_ref().and_then(|cd| layer_block(file, bi, cd, CD_MVERT, None)).or_else(|| v.pointer("mvert").and_then(|p| file.resolve(p, bi)));
        if let Some(block) = mvert_block {
            let verts = file.views_as(block, "MVert");
            positions = Some(verts.iter().take(verts_num).map(|mv| to_y_up(vec3(mv, "co"))).collect());
        }
    }
    let Some(positions) = positions else { return Ok(None) };

    // ---- polygons ----
    let mut polygons: Vec<Vec<u32>> = Vec::new();
    let ldata = v.sub("corner_data").or_else(|| v.sub("ldata"));
    let pdata = v.sub("face_data").or_else(|| v.sub("pdata"));
    let corner_verts: Option<Vec<i32>> = attribute_block(file, bi, v, ".corner_vert", ATTR_INT32, DOMAIN_CORNER)
        .or_else(|| ldata.as_ref().and_then(|cd| layer_block(file, bi, cd, CD_PROP_INT32, Some(".corner_vert"))))
        .map(|b| i32_array(file, b, corners_num))
        .or_else(|| {
            let block = ldata
                .as_ref()
                .and_then(|cd| layer_block(file, bi, cd, CD_MLOOP, None))
                .or_else(|| v.pointer("mloop").and_then(|p| file.resolve(p, bi)))?;
            Some(file.views_as(block, "MLoop").iter().take(corners_num).map(|l| l.i32("v").unwrap_or(0)).collect())
        });
    if let Some(corner_verts) = corner_verts {
        if let Some(block) = v.pointer("face_offset_indices").or_else(|| v.pointer("poly_offset_indices")).and_then(|p| file.resolve(p, bi)) {
            let offsets = i32_array(file, block, faces_num + 1);
            for w in offsets.windows(2) {
                let (a, b) = (w[0].max(0) as usize, w[1].max(0) as usize);
                if b > a && b <= corner_verts.len() {
                    polygons.push(corner_verts[a..b].iter().map(|&i| i as u32).collect());
                }
            }
        } else if let Some(block) =
            pdata.as_ref().and_then(|cd| layer_block(file, bi, cd, CD_MPOLY, None)).or_else(|| v.pointer("mpoly").and_then(|p| file.resolve(p, bi)))
        {
            for poly in file.views_as(block, "MPoly").iter().take(faces_num) {
                let start = poly.i32("loopstart").unwrap_or(0).max(0) as usize;
                let n = poly.i32("totloop").unwrap_or(0).max(0) as usize;
                if start + n <= corner_verts.len() && n >= 3 {
                    polygons.push(corner_verts[start..start + n].iter().map(|&i| i as u32).collect());
                }
            }
        }
    }
    if polygons.is_empty() {
        // Legacy MFace (before 2.63): quads with v4 == 0 meaning a triangle.
        let totface = v.i32("totface").or_else(|| v.i32("totface_legacy")).unwrap_or(0).max(0) as usize;
        if let Some(block) = v.pointer("mface").and_then(|p| file.resolve(p, bi)) {
            for f in file.views_as(block, "MFace").iter().take(totface) {
                let (v1, v2, v3, v4) = (f.i32("v1").unwrap_or(0), f.i32("v2").unwrap_or(0), f.i32("v3").unwrap_or(0), f.i32("v4").unwrap_or(0));
                let mut poly = vec![v1 as u32, v2 as u32, v3 as u32];
                if v4 != 0 {
                    poly.push(v4 as u32);
                }
                polygons.push(poly);
            }
        }
    }
    if polygons.is_empty() {
        return Ok(None);
    }
    // Blender's winding is counter-clockwise in Z-up; the Y-up conversion is a rotation, so it
    // survives. Drop faces that reference missing vertices rather than failing the whole mesh.
    polygons.retain(|p| p.iter().all(|&i| (i as usize) < positions.len()));
    Ok(Some(lenient_build(positions, polygons)?))
}

/// Build the mesh; if the polygons are not a manifold set, fall back to unwelded faces.
fn lenient_build(positions: Vec<Vec3>, polygons: Vec<Vec<u32>>) -> Result<Mesh> {
    match Mesh::from_polygons(positions.clone(), &polygons) {
        Ok(m) => Ok(m),
        Err(shape_geom::MeshError::NonManifoldEdge { .. }) | Err(shape_geom::MeshError::RepeatedVertex { .. }) => {
            let mut pos = Vec::new();
            let mut polys = Vec::new();
            for poly in &polygons {
                let mut q = poly.clone();
                q.dedup();
                if q.len() < 3 {
                    continue;
                }
                let start = pos.len() as u32;
                pos.extend(q.iter().map(|&i| positions[i as usize]));
                polys.push((start..start + q.len() as u32).collect());
            }
            Ok(Mesh::from_polygons(pos, &polys)?)
        }
        Err(e) => Err(e.into()),
    }
}

fn read_light(file: &BlendFile, ptr: u64) -> Light {
    let Some(block) = file.resolve(ptr, 0) else { return Light::default() };
    let Some(v) = file.view(block, 0) else { return Light::default() };
    let kind = match v.i32("type").unwrap_or(0) {
        1 => LightKind::Sun,
        2 => {
            let size = v.f32("spotsize").unwrap_or(45f32.to_radians());
            let blend = v.f32("spotblend").unwrap_or(0.15);
            LightKind::Spot { inner_angle: size * 0.5 * (1.0 - blend), outer_angle: size * 0.5 }
        }
        _ => LightKind::Point,
    };
    Light { kind, color: [v.f32("r").unwrap_or(1.0), v.f32("g").unwrap_or(1.0), v.f32("b").unwrap_or(1.0)], power: v.f32("energy").unwrap_or(10.0) }
}

fn read_camera(file: &BlendFile, ptr: u64) -> Camera {
    let Some(block) = file.resolve(ptr, 0) else { return Camera::default() };
    let Some(v) = file.view(block, 0) else { return Camera::default() };
    let lens = v.f32("lens").unwrap_or(50.0);
    let sensor = v.f32("sensor_x").unwrap_or(36.0);
    // Blender fits the sensor to the wider side by default; treat it as horizontal 16:9 here.
    let fov_x = 2.0 * (sensor * 0.5 / lens).atan();
    let fov_y = 2.0 * ((fov_x * 0.5).tan() * 9.0 / 16.0).atan();
    Camera {
        fov_y,
        near: v.f32("clip_start").or_else(|| v.f32("clipsta")).unwrap_or(0.1),
        far: v.f32("clip_end").or_else(|| v.f32("clipend")).unwrap_or(1000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_conversion_is_a_rotation() {
        assert_eq!(to_y_up(Vec3::new(1.0, 2.0, 3.0)), Vec3::new(1.0, 3.0, -2.0));
        let m = Mat4::from_translation(Vec3::new(1.0, 2.0, 3.0));
        let c = convert_matrix(m);
        assert!((c.transform_point3(Vec3::ZERO) - Vec3::new(1.0, 3.0, -2.0)).length() < 1e-6);
        let r = Mat4::from_rotation_z(0.5);
        let cr = convert_matrix(r);
        // a rotation about Blender Z becomes a rotation about our Y
        let p = cr.transform_point3(Vec3::X);
        assert!(p.y.abs() < 1e-6 && (p.x - 0.5f32.cos()).abs() < 1e-6, "{p}");
    }
}
