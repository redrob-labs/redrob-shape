//! FBX import and export in the ASCII encoding (version 7.4). Export writes one `Geometry` per
//! object (Vertices, PolygonVertexIndex with FBX's negated last index per polygon) and a `Model`
//! carrying Lcl Translation / Rotation / Scaling, connected OO; materials become `Material`
//! nodes with a DiffuseColor. Import parses the generic ASCII node tree and rebuilds the same.
//! Binary FBX (what Blender writes by default) is not read; Blender imports ASCII FBX only from
//! its own exporter's binary, so the round trip with Blender is export-only for now.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use shape_math::{Mat4, Quat, Vec3, glam_euler};
use shape_scene::Scene;

use crate::interchange::{export_objects, scene_from_records};
use crate::{Result, parse_err};

pub fn save(scene: &Scene, path: &Path) -> Result<()> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    write(scene, &mut out)
}

pub fn write(scene: &Scene, out: &mut impl Write) -> Result<()> {
    let objects = export_objects(scene);
    writeln!(out, "; FBX 7.4.0 project file\n; Redrob Shape {}\nFBXHeaderExtension:  {{\n\tFBXHeaderVersion: 1003\n\tFBXVersion: 7400\n\tCreator: \"Redrob Shape\"\n}}", env!("CARGO_PKG_VERSION"))?;
    writeln!(out, "GlobalSettings:  {{\n\tVersion: 1000\n\tProperties70:  {{\n\t\tP: \"UpAxis\", \"int\", \"Integer\", \"\",1\n\t\tP: \"UpAxisSign\", \"int\", \"Integer\", \"\",1\n\t\tP: \"FrontAxis\", \"int\", \"Integer\", \"\",2\n\t\tP: \"CoordAxis\", \"int\", \"Integer\", \"\",0\n\t\tP: \"UnitScaleFactor\", \"double\", \"Number\", \"\",100\n\t}}\n}}")?;
    let n_mat = objects.iter().filter(|o| o.color.is_some()).count();
    writeln!(out, "Definitions:  {{\n\tVersion: 100\n\tCount: {}\n\tObjectType: \"Geometry\" {{\n\t\tCount: {}\n\t}}\n\tObjectType: \"Model\" {{\n\t\tCount: {}\n\t}}\n\tObjectType: \"Material\" {{\n\t\tCount: {n_mat}\n\t}}\n}}", objects.len() * 2 + n_mat, objects.len(), objects.len())?;
    writeln!(out, "Objects:  {{")?;
    for (i, o) in objects.iter().enumerate() {
        let geo_id = 1_000_000 + i as u64;
        writeln!(out, "\tGeometry: {geo_id}, \"Geometry::{}\", \"Mesh\" {{", o.name)?;
        write!(out, "\t\tVertices: *{} {{\n\t\t\ta: ", o.positions.len() * 3)?;
        for (k, p) in o.positions.iter().enumerate() {
            write!(out, "{}{},{},{}", if k > 0 { "," } else { "" }, p.x, p.y, p.z)?;
        }
        writeln!(out, "\n\t\t}}")?;
        let n_idx: usize = o.polygons.iter().map(Vec::len).sum();
        write!(out, "\t\tPolygonVertexIndex: *{n_idx} {{\n\t\t\ta: ")?;
        let mut first = true;
        for poly in &o.polygons {
            for (k, &v) in poly.iter().enumerate() {
                let v = if k + 1 == poly.len() { -(v as i64) - 1 } else { v as i64 };
                write!(out, "{}{}", if first { "" } else { "," }, v)?;
                first = false;
            }
        }
        writeln!(out, "\n\t\t}}\n\t\tGeometryVersion: 124\n\t}}")?;
        let model_id = 2_000_000 + i as u64;
        let (scale, rotation, translation) = o.world.to_scale_rotation_translation();
        let (rx, ry, rz) = rotation.to_euler(glam_euler());
        writeln!(out, "\tModel: {model_id}, \"Model::{}\", \"Mesh\" {{\n\t\tVersion: 232\n\t\tProperties70:  {{", o.name)?;
        writeln!(out, "\t\t\tP: \"Lcl Translation\", \"Lcl Translation\", \"\", \"A\",{},{},{}", translation.x, translation.y, translation.z)?;
        writeln!(out, "\t\t\tP: \"Lcl Rotation\", \"Lcl Rotation\", \"\", \"A\",{},{},{}", rx.to_degrees(), ry.to_degrees(), rz.to_degrees())?;
        writeln!(out, "\t\t\tP: \"Lcl Scaling\", \"Lcl Scaling\", \"\", \"A\",{},{},{}", scale.x, scale.y, scale.z)?;
        writeln!(out, "\t\t}}\n\t\tShading: T\n\t\tCulling: \"CullingOff\"\n\t}}")?;
        if let Some(c) = o.color {
            let mat_id = 3_000_000 + i as u64;
            writeln!(out, "\tMaterial: {mat_id}, \"Material::{}\", \"\" {{\n\t\tVersion: 102\n\t\tShadingModel: \"lambert\"\n\t\tProperties70:  {{", o.material_name.clone().unwrap_or_default())?;
            writeln!(out, "\t\t\tP: \"DiffuseColor\", \"Color\", \"\", \"A\",{},{},{}\n\t\t}}\n\t}}", c[0], c[1], c[2])?;
        }
    }
    writeln!(out, "}}\nConnections:  {{")?;
    for (i, o) in objects.iter().enumerate() {
        let (geo_id, model_id, mat_id) = (1_000_000 + i as u64, 2_000_000 + i as u64, 3_000_000 + i as u64);
        writeln!(out, "\tC: \"OO\",{model_id},0\n\tC: \"OO\",{geo_id},{model_id}")?;
        if o.color.is_some() {
            writeln!(out, "\tC: \"OO\",{mat_id},{model_id}")?;
        }
    }
    writeln!(out, "}}")?;
    Ok(())
}

/// A generic ASCII FBX node: `Name: prop, prop, ... { children }`.
#[derive(Debug, Default)]
struct FbxNode {
    name: String,
    props: Vec<String>,
    children: Vec<FbxNode>,
}

impl FbxNode {
    fn child(&self, name: &str) -> Option<&FbxNode> {
        self.children.iter().find(|c| c.name == name)
    }
    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a FbxNode> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    /// Numbers in this node's props plus any `a:` array child.
    fn numbers(&self) -> Vec<f64> {
        let mut out: Vec<f64> = self.props.iter().filter_map(|p| p.parse::<f64>().ok()).collect();
        if let Some(a) = self.child("a") {
            out.extend(a.props.iter().filter_map(|p| p.parse::<f64>().ok()));
        }
        out
    }
    fn id(&self) -> Option<i64> {
        self.props.first().and_then(|p| p.parse::<i64>().ok())
    }
}

/// Split a property list on commas outside quotes, trimming each item and stripping quotes.
fn split_props(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    for ch in s.chars() {
        match ch {
            '"' => in_quote = !in_quote,
            ',' if !in_quote => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn parse_tree(text: &str) -> Result<FbxNode> {
    if text.starts_with("Kaydara FBX Binary") {
        return Err(parse_err("fbx", 1, "binary FBX is not supported; export ASCII FBX (Blender: not available; other tools: 'ASCII' option)"));
    }
    let mut root = FbxNode { name: "#root".into(), ..Default::default() };
    let mut stack: Vec<FbxNode> = Vec::new();
    for (line_no, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        if line == "}" {
            let node = stack.pop().ok_or_else(|| parse_err("fbx", line_no + 1, "unbalanced '}'"))?;
            match stack.last_mut() {
                Some(parent) => parent.children.push(node),
                None => root.children.push(node),
            }
            continue;
        }
        let Some(colon) = line.find(':') else {
            // A continuation line of a multi-line `a:` array.
            if let Some(parent) = stack.last_mut()
                && let Some(last) = parent.children.last_mut()
                && last.name == "a"
            {
                last.props.extend(split_props(line));
            }
            continue;
        };
        let name = line[..colon].trim().to_string();
        let mut rest = line[colon + 1..].trim();
        let opens = rest.ends_with('{');
        if opens {
            rest = rest[..rest.len() - 1].trim();
        }
        // Array headers like `*24 {` carry only the count; the values follow on `a:` lines.
        let props = if rest.starts_with('*') { Vec::new() } else { split_props(rest) };
        let node = FbxNode { name, props, children: Vec::new() };
        if opens {
            stack.push(node);
        } else {
            match stack.last_mut() {
                Some(parent) => {
                    // Continuation of a multi-line `a:` array: append to the existing child.
                    if node.name == "a"
                        && let Some(last) = parent.children.last_mut()
                        && last.name == "a"
                    {
                        last.props.extend(node.props);
                    } else {
                        parent.children.push(node);
                    }
                }
                None => root.children.push(node),
            }
        }
    }
    while let Some(node) = stack.pop() {
        match stack.last_mut() {
            Some(parent) => parent.children.push(node),
            None => root.children.push(node),
        }
    }
    Ok(root)
}

pub fn load(path: &Path) -> Result<Scene> {
    let text = std::fs::read_to_string(path).map_err(|_| parse_err("fbx", 0, "not a text file (binary FBX is not supported)"))?;
    read(&text, path.file_stem().and_then(|s| s.to_str()).unwrap_or("fbx"))
}

pub fn read(text: &str, name: &str) -> Result<Scene> {
    let root = parse_tree(text)?;
    let objects = root.child("Objects").ok_or_else(|| parse_err("fbx", 0, "no Objects section"))?;
    let unit_scale = root
        .child("GlobalSettings")
        .and_then(|g| g.child("Properties70"))
        .and_then(|p| p.children_named("P").find(|c| c.props.first().map(String::as_str) == Some("UnitScaleFactor")))
        .and_then(|c| c.props.get(4))
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(100.0);
    // UnitScaleFactor is centimetres per unit: 100 means the file is in metres (what we write).
    let to_metres = unit_scale / 100.0;
    // Geometry id -> (positions, polygons)
    let mut geometries: HashMap<i64, (Vec<Vec3>, Vec<Vec<u32>>)> = HashMap::new();
    for g in objects.children_named("Geometry") {
        let Some(id) = g.id() else { continue };
        let verts: Vec<f64> = g.child("Vertices").map(FbxNode::numbers).unwrap_or_default();
        let positions: Vec<Vec3> = verts.chunks(3).filter(|c| c.len() == 3).map(|c| Vec3::new(c[0] as f32, c[1] as f32, c[2] as f32) * to_metres).collect();
        let idx: Vec<f64> = g.child("PolygonVertexIndex").map(FbxNode::numbers).unwrap_or_default();
        let mut polygons = Vec::new();
        let mut poly: Vec<u32> = Vec::new();
        for v in idx {
            let v = v as i64;
            if v < 0 {
                poly.push((-v - 1) as u32);
                if poly.len() >= 3 {
                    polygons.push(std::mem::take(&mut poly));
                } else {
                    poly.clear();
                }
            } else {
                poly.push(v as u32);
            }
        }
        geometries.insert(id, (positions, polygons));
    }
    // Material id -> colour
    let mut materials: HashMap<i64, [f32; 4]> = HashMap::new();
    for m in objects.children_named("Material") {
        let Some(id) = m.id() else { continue };
        if let Some(c) = m.child("Properties70").and_then(|p| p.children_named("P").find(|c| c.props.first().map(String::as_str) == Some("DiffuseColor"))) {
            let v: Vec<f32> = c.props.iter().skip(4).filter_map(|s| s.parse::<f32>().ok()).collect();
            if v.len() >= 3 {
                materials.insert(id, [v[0], v[1], v[2], 1.0]);
            }
        }
    }
    // Models: id -> (name, local matrix, parent)
    struct Model {
        name: String,
        local: Mat4,
    }
    let mut models: HashMap<i64, Model> = HashMap::new();
    for m in objects.children_named("Model") {
        let Some(id) = m.id() else { continue };
        let name = m.props.get(1).map(|s| s.trim_start_matches("Model::").to_string()).unwrap_or_else(|| format!("Model{id}"));
        let (mut t, mut r, mut s) = (Vec3::ZERO, Vec3::ZERO, Vec3::ONE);
        if let Some(props) = m.child("Properties70") {
            for p in props.children_named("P") {
                let vals: Vec<f32> = p.props.iter().skip(4).filter_map(|v| v.parse::<f32>().ok()).collect();
                if vals.len() < 3 {
                    continue;
                }
                let v = Vec3::new(vals[0], vals[1], vals[2]);
                match p.props.first().map(String::as_str) {
                    Some("Lcl Translation") => t = v * to_metres,
                    Some("Lcl Rotation") => r = v,
                    Some("Lcl Scaling") => s = v,
                    _ => {}
                }
            }
        }
        let rot = Quat::from_euler(glam_euler(), r.x.to_radians(), r.y.to_radians(), r.z.to_radians());
        models.insert(id, Model { name, local: Mat4::from_scale_rotation_translation(s, rot, t) });
    }
    // Connections: child -> parent.
    let mut parent_of: HashMap<i64, i64> = HashMap::new();
    if let Some(conns) = root.child("Connections") {
        for c in conns.children_named("C") {
            if c.props.first().map(String::as_str) == Some("OO")
                && let (Some(child), Some(parent)) = (c.props.get(1).and_then(|v| v.parse::<i64>().ok()), c.props.get(2).and_then(|v| v.parse::<i64>().ok()))
            {
                parent_of.insert(child, parent);
            }
        }
    }
    fn world_of(id: i64, models: &HashMap<i64, Model>, parent_of: &HashMap<i64, i64>, depth: u32) -> Mat4 {
        let Some(m) = models.get(&id) else { return Mat4::IDENTITY };
        match parent_of.get(&id) {
            Some(&p) if p != 0 && depth < 64 => world_of(p, models, parent_of, depth + 1) * m.local,
            _ => m.local,
        }
    }
    let mut records = Vec::new();
    for (&gid, (positions, polygons)) in &geometries {
        let Some(&model_id) = parent_of.get(&gid) else { continue };
        let Some(model) = models.get(&model_id) else { continue };
        let world = world_of(model_id, &models, &parent_of, 0);
        let color = parent_of.iter().find(|(child, parent)| **parent == model_id && materials.contains_key(child)).and_then(|(child, _)| materials.get(child)).copied();
        records.push((model.name.clone(), positions.clone(), polygons.clone(), world, color));
    }
    records.sort_by(|a, b| a.0.cmp(&b.0));
    scene_from_records(name, records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{sample_scene, scratch};

    #[test]
    fn ascii_round_trip_keeps_objects_transforms_and_colour() {
        let scene = sample_scene();
        let path = scratch("sample.fbx");
        save(&scene, &path).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back.object_count(), scene.object_count());
        let (id, cube) = back.objects().find(|(_, o)| o.name == "Cube").unwrap();
        assert!((cube.transform.translation - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-4, "{}", cube.transform.translation);
        assert_eq!(back.object_mesh(id).unwrap().1.face_count(), 6);
        let color = match &cube.data {
            shape_scene::ObjectData::Mesh { material: Some(m), .. } => back.material(*m).map(|m| m.base_color),
            _ => None,
        };
        assert_eq!(color.map(|c| (c[0] * 10.0).round()), Some(8.0));
        let (_, sphere) = back.objects().find(|(_, o)| o.name == "Sphere").unwrap();
        assert!(sphere.transform.rotation.angle_between(Quat::from_rotation_z(0.3)) < 1e-3);
    }

    #[test]
    fn binary_files_are_refused_with_a_clear_message() {
        let err = read("Kaydara FBX Binary  \0", "t").unwrap_err().to_string();
        assert!(err.contains("binary FBX"), "{err}");
    }
}
