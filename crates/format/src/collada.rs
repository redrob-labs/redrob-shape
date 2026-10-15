//! COLLADA (`.dae`) import and export. Export writes effects/materials with the diffuse colour,
//! one geometry per object (positions + a polylist) and a visual scene of nodes with a matrix, in
//! Y_UP. Import reads geometries (positions through the VERTEX input, `polylist`, `triangles` or
//! `polygons`), node matrices or translate/rotate/scale, instance_geometry links, the material
//! diffuse colour, and converts Z_UP files (Blender's default) into our Y-up frame.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use quick_xml::Reader;
use quick_xml::events::Event;
use shape_math::{Mat4, Quat, Vec3};
use shape_scene::Scene;

use crate::interchange::{export_objects, floats, ints, scene_from_records, z_up_matrix_to_y_up, z_up_to_y_up};
use crate::{Result, parse_err};

pub fn save(scene: &Scene, path: &Path) -> Result<()> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    write(scene, &mut out)
}

pub fn write(scene: &Scene, out: &mut impl Write) -> Result<()> {
    let objects = export_objects(scene);
    writeln!(out, r#"<?xml version="1.0" encoding="utf-8"?>"#)?;
    writeln!(out, r#"<COLLADA xmlns="http://www.collada.org/2005/11/COLLADASchema" version="1.4.1">"#)?;
    writeln!(out, "  <asset>\n    <contributor><authoring_tool>Redrob Shape {}</authoring_tool></contributor>", env!("CARGO_PKG_VERSION"))?;
    writeln!(out, "    <unit name=\"meter\" meter=\"1\"/>\n    <up_axis>Y_UP</up_axis>\n  </asset>")?;
    // Materials.
    writeln!(out, "  <library_effects>")?;
    for o in objects.iter().filter(|o| o.color.is_some()) {
        let c = o.color.unwrap_or([0.8, 0.8, 0.8, 1.0]);
        writeln!(out, "    <effect id=\"{}-effect\"><profile_COMMON><technique sid=\"common\"><lambert>", o.name)?;
        writeln!(out, "      <diffuse><color sid=\"diffuse\">{} {} {} {}</color></diffuse>", c[0], c[1], c[2], c[3])?;
        writeln!(out, "    </lambert></technique></profile_COMMON></effect>")?;
    }
    writeln!(out, "  </library_effects>\n  <library_materials>")?;
    for o in objects.iter().filter(|o| o.color.is_some()) {
        writeln!(out, "    <material id=\"{0}-material\" name=\"{1}\"><instance_effect url=\"#{0}-effect\"/></material>", o.name, o.material_name.clone().unwrap_or_default())?;
    }
    writeln!(out, "  </library_materials>\n  <library_geometries>")?;
    for o in &objects {
        let id = format!("{}-mesh", o.name);
        writeln!(out, "    <geometry id=\"{id}\" name=\"{}\"><mesh>", o.name)?;
        write!(out, "      <source id=\"{id}-positions\"><float_array id=\"{id}-positions-array\" count=\"{}\">", o.positions.len() * 3)?;
        for p in &o.positions {
            write!(out, "{} {} {} ", p.x, p.y, p.z)?;
        }
        writeln!(out, "</float_array>")?;
        writeln!(out, "        <technique_common><accessor source=\"#{id}-positions-array\" count=\"{}\" stride=\"3\"><param name=\"X\" type=\"float\"/><param name=\"Y\" type=\"float\"/><param name=\"Z\" type=\"float\"/></accessor></technique_common>", o.positions.len())?;
        writeln!(out, "      </source>")?;
        writeln!(out, "      <vertices id=\"{id}-vertices\"><input semantic=\"POSITION\" source=\"#{id}-positions\"/></vertices>")?;
        let material_attr = if o.color.is_some() { format!(" material=\"{}-material\"", o.name) } else { String::new() };
        writeln!(out, "      <polylist{material_attr} count=\"{}\">", o.polygons.len())?;
        writeln!(out, "        <input semantic=\"VERTEX\" source=\"#{id}-vertices\" offset=\"0\"/>")?;
        write!(out, "        <vcount>")?;
        for poly in &o.polygons {
            write!(out, "{} ", poly.len())?;
        }
        writeln!(out, "</vcount>")?;
        write!(out, "        <p>")?;
        for poly in &o.polygons {
            for i in poly {
                write!(out, "{i} ")?;
            }
        }
        writeln!(out, "</p>\n      </polylist>\n    </mesh></geometry>")?;
    }
    writeln!(out, "  </library_geometries>\n  <library_visual_scenes>\n    <visual_scene id=\"Scene\" name=\"Scene\">")?;
    for o in &objects {
        writeln!(out, "      <node id=\"{0}\" name=\"{0}\" type=\"NODE\">", o.name)?;
        // COLLADA matrices are row-major.
        let m = o.world.to_cols_array_2d();
        write!(out, "        <matrix sid=\"transform\">")?;
        for row in 0..4 {
            for col in 0..4 {
                write!(out, "{} ", m[col][row])?;
            }
        }
        writeln!(out, "</matrix>")?;
        write!(out, "        <instance_geometry url=\"#{}-mesh\"", o.name)?;
        if o.color.is_some() {
            writeln!(out, ">")?;
            writeln!(out, "          <bind_material><technique_common><instance_material symbol=\"{0}-material\" target=\"#{0}-material\"/></technique_common></bind_material>", o.name)?;
            writeln!(out, "        </instance_geometry>")?;
        } else {
            writeln!(out, "/>")?;
        }
        writeln!(out, "      </node>")?;
    }
    writeln!(out, "    </visual_scene>\n  </library_visual_scenes>")?;
    writeln!(out, "  <scene><instance_visual_scene url=\"#Scene\"/></scene>\n</COLLADA>")?;
    Ok(())
}

/// A small DOM: enough to walk a COLLADA document.
#[derive(Debug, Default)]
struct Node {
    name: String,
    attrs: HashMap<String, String>,
    text: String,
    children: Vec<Node>,
}

impl Node {
    fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.get(k).map(String::as_str)
    }
    fn child(&self, name: &str) -> Option<&Node> {
        self.children.iter().find(|c| c.name == name)
    }
    fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Node> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    /// Depth-first search for the first descendant with this name.
    fn find(&self, name: &str) -> Option<&Node> {
        for c in &self.children {
            if c.name == name {
                return Some(c);
            }
            if let Some(f) = c.find(name) {
                return Some(f);
            }
        }
        None
    }
    fn find_all<'a>(&'a self, name: &str, out: &mut Vec<&'a Node>) {
        for c in &self.children {
            if c.name == name {
                out.push(c);
            }
            c.find_all(name, out);
        }
    }
}

fn parse_dom(xml: &str) -> Result<Node> {
    let mut reader = Reader::from_str(xml);
    let mut stack: Vec<Node> = vec![Node { name: "#root".into(), ..Default::default() }];
    loop {
        let event = reader.read_event().map_err(|e| parse_err("dae", 0, e.to_string()))?;
        match event {
            Event::Start(e) => stack.push(make_node(&e)),
            Event::Empty(e) => {
                let node = make_node(&e);
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(node);
                }
            }
            Event::Text(t) => {
                if let Some(cur) = stack.last_mut() {
                    cur.text.push_str(&t.decode().map(|s| s.to_string()).unwrap_or_default());
                }
            }
            Event::End(_) => {
                if stack.len() > 1 {
                    let node = stack.pop().expect("non-empty");
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(node);
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    stack.into_iter().next().ok_or_else(|| parse_err("dae", 0, "empty document"))
}

fn make_node(e: &quick_xml::events::BytesStart) -> Node {
    let mut node = Node { name: local_name(&String::from_utf8_lossy(e.name().as_ref())), ..Default::default() };
    for a in e.attributes().flatten() {
        let k = local_name(&String::from_utf8_lossy(a.key.as_ref()));
        let v = a.unescape_value().map(|v| v.to_string()).unwrap_or_default();
        node.attrs.insert(k, v);
    }
    node
}

fn local_name(s: &str) -> String {
    s.rsplit(':').next().unwrap_or(s).to_string()
}

pub fn load(path: &Path) -> Result<Scene> {
    let xml = std::fs::read_to_string(path)?;
    let scene = read(&xml, path.file_stem().and_then(|s| s.to_str()).unwrap_or("collada"))?;
    Ok(scene)
}

pub fn read(xml: &str, name: &str) -> Result<Scene> {
    let root = parse_dom(xml)?;
    let collada = root.child("COLLADA").ok_or_else(|| parse_err("dae", 0, "no COLLADA element"))?;
    let z_up = collada.find("up_axis").map(|n| n.text.trim() == "Z_UP").unwrap_or(false);
    // Materials: id -> diffuse colour, through material -> effect.
    let mut effects: HashMap<String, [f32; 4]> = HashMap::new();
    let mut effect_nodes = Vec::new();
    collada.find_all("effect", &mut effect_nodes);
    for e in effect_nodes {
        if let (Some(id), Some(diffuse)) = (e.attr("id"), e.find("diffuse").and_then(|d| d.child("color"))) {
            let c = floats(&diffuse.text);
            if c.len() >= 3 {
                effects.insert(id.to_string(), [c[0], c[1], c[2], c.get(3).copied().unwrap_or(1.0)]);
            }
        }
    }
    let mut materials: HashMap<String, [f32; 4]> = HashMap::new();
    let mut material_nodes = Vec::new();
    collada.find_all("material", &mut material_nodes);
    for m in material_nodes {
        if let (Some(id), Some(url)) = (m.attr("id"), m.child("instance_effect").and_then(|i| i.attr("url")))
            && let Some(c) = effects.get(url.trim_start_matches('#'))
        {
            materials.insert(id.to_string(), *c);
        }
    }
    // Geometries: id -> (positions, polygons, material symbol).
    let mut geometries: HashMap<String, (Vec<Vec3>, Vec<Vec<u32>>, Option<String>)> = HashMap::new();
    let mut geometry_nodes = Vec::new();
    collada.find_all("geometry", &mut geometry_nodes);
    for g in geometry_nodes {
        let Some(id) = g.attr("id") else { continue };
        let Some(mesh) = g.child("mesh") else { continue };
        let sources: HashMap<String, Vec<f32>> = mesh
            .children_named("source")
            .filter_map(|s| Some((s.attr("id")?.to_string(), floats(&s.child("float_array")?.text))))
            .collect();
        let Some(vertices) = mesh.child("vertices") else { continue };
        let position_source = vertices
            .children_named("input")
            .find(|i| i.attr("semantic") == Some("POSITION"))
            .and_then(|i| i.attr("source"))
            .map(|s| s.trim_start_matches('#').to_string());
        let Some(pos) = position_source.and_then(|s| sources.get(&s)) else { continue };
        let mut positions: Vec<Vec3> = pos.chunks(3).filter(|c| c.len() == 3).map(|c| Vec3::new(c[0], c[1], c[2])).collect();
        if z_up {
            for p in &mut positions {
                *p = z_up_to_y_up(*p);
            }
        }
        let mut polygons: Vec<Vec<u32>> = Vec::new();
        let mut symbol = None;
        for prim in mesh.children.iter().filter(|c| matches!(c.name.as_str(), "polylist" | "triangles" | "polygons")) {
            if symbol.is_none() {
                symbol = prim.attr("material").map(str::to_string);
            }
            let inputs: Vec<&Node> = prim.children_named("input").collect();
            let stride = inputs.iter().filter_map(|i| i.attr("offset")?.parse::<usize>().ok()).max().map(|m| m + 1).unwrap_or(1);
            let vertex_offset = inputs.iter().find(|i| i.attr("semantic") == Some("VERTEX")).and_then(|i| i.attr("offset")?.parse::<usize>().ok()).unwrap_or(0);
            match prim.name.as_str() {
                "polylist" => {
                    let counts = prim.child("vcount").map(|v| ints(&v.text)).unwrap_or_default();
                    let p = prim.child("p").map(|v| ints(&v.text)).unwrap_or_default();
                    let mut cursor = 0usize;
                    for n in counts {
                        let n = n.max(0) as usize;
                        let mut poly = Vec::with_capacity(n);
                        for k in 0..n {
                            let idx = (cursor + k) * stride + vertex_offset;
                            if let Some(v) = p.get(idx) {
                                poly.push(*v as u32);
                            }
                        }
                        cursor += n;
                        if poly.len() >= 3 {
                            polygons.push(poly);
                        }
                    }
                }
                "triangles" => {
                    let p = prim.child("p").map(|v| ints(&v.text)).unwrap_or_default();
                    for tri in p.chunks(3 * stride) {
                        if tri.len() == 3 * stride {
                            polygons.push((0..3).map(|k| tri[k * stride + vertex_offset] as u32).collect());
                        }
                    }
                }
                _ => {
                    for p in prim.children_named("p") {
                        let v = ints(&p.text);
                        let poly: Vec<u32> = v.chunks(stride).filter_map(|c| c.get(vertex_offset)).map(|i| *i as u32).collect();
                        if poly.len() >= 3 {
                            polygons.push(poly);
                        }
                    }
                }
            }
        }
        geometries.insert(id.to_string(), (positions, polygons, symbol));
    }
    // Visual scene nodes.
    let mut records = Vec::new();
    let mut node_nodes = Vec::new();
    collada.find_all("node", &mut node_nodes);
    fn walk(n: &Node, parent: Mat4, z_up: bool, geometries: &HashMap<String, (Vec<Vec3>, Vec<Vec<u32>>, Option<String>)>, materials: &HashMap<String, [f32; 4]>, records: &mut Vec<(String, Vec<Vec3>, Vec<Vec<u32>>, Mat4, Option<[f32; 4]>)>) {
        let local = node_matrix(n);
        let world = parent * local;
        for inst in n.children_named("instance_geometry") {
            let Some(url) = inst.attr("url") else { continue };
            let Some((positions, polygons, symbol)) = geometries.get(url.trim_start_matches('#')) else { continue };
            let bound = inst.find("instance_material").and_then(|m| m.attr("target")).map(|t| t.trim_start_matches('#').to_string());
            let color = bound.or_else(|| symbol.clone()).and_then(|m| materials.get(&m).copied());
            let world_y = if z_up { z_up_matrix_to_y_up(world) } else { world };
            let name = n.attr("name").or(n.attr("id")).unwrap_or("Object").to_string();
            records.push((name, positions.clone(), polygons.clone(), world_y, color));
        }
        for c in n.children_named("node") {
            walk(c, world, z_up, geometries, materials, records);
        }
    }
    if let Some(vs) = collada.find("visual_scene") {
        for n in vs.children_named("node") {
            walk(n, Mat4::IDENTITY, z_up, &geometries, &materials, &mut records);
        }
    } else {
        for n in node_nodes {
            walk(n, Mat4::IDENTITY, z_up, &geometries, &materials, &mut records);
        }
    }
    scene_from_records(name, records)
}

/// A node's local transform: `<matrix>` (row-major) or translate / rotate / scale elements in order.
fn node_matrix(n: &Node) -> Mat4 {
    let mut m = Mat4::IDENTITY;
    for c in &n.children {
        match c.name.as_str() {
            "matrix" => {
                let v = floats(&c.text);
                if v.len() == 16 {
                    let mut cols = [0.0f32; 16];
                    for row in 0..4 {
                        for col in 0..4 {
                            cols[col * 4 + row] = v[row * 4 + col];
                        }
                    }
                    m *= Mat4::from_cols_array(&cols);
                }
            }
            "translate" => {
                let v = floats(&c.text);
                if v.len() == 3 {
                    m *= Mat4::from_translation(Vec3::new(v[0], v[1], v[2]));
                }
            }
            "rotate" => {
                let v = floats(&c.text);
                if v.len() == 4 {
                    let axis = Vec3::new(v[0], v[1], v[2]).normalize_or_zero();
                    if axis.length_squared() > 0.5 {
                        m *= Mat4::from_quat(Quat::from_axis_angle(axis, v[3].to_radians()));
                    }
                }
            }
            "scale" => {
                let v = floats(&c.text);
                if v.len() == 3 {
                    m *= Mat4::from_scale(Vec3::new(v[0], v[1], v[2]));
                }
            }
            _ => {}
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{sample_scene, scratch};

    #[test]
    fn round_trip_keeps_objects_transforms_and_colour() {
        let scene = sample_scene();
        let path = scratch("sample.dae");
        save(&scene, &path).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back.object_count(), scene.object_count());
        let (_, cube) = back.objects().find(|(_, o)| o.name == "Cube").unwrap();
        assert!((cube.transform.translation - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-4);
        let color = match &cube.data {
            shape_scene::ObjectData::Mesh { material: Some(m), .. } => back.material(*m).map(|m| m.base_color),
            _ => None,
        };
        assert_eq!(color.map(|c| (c[0] * 10.0).round()), Some(8.0));
        let total_faces: usize = back.objects().filter_map(|(id, _)| back.object_mesh(id)).map(|(_, m)| m.face_count()).sum();
        let expect: usize = scene.objects().filter_map(|(id, _)| scene.object_mesh(id)).map(|(_, m)| m.face_count()).sum();
        assert_eq!(total_faces, expect);
    }

    #[test]
    fn z_up_file_with_triangles_is_converted() {
        let xml = r#"<?xml version="1.0"?><COLLADA version="1.4.1"><asset><up_axis>Z_UP</up_axis></asset>
<library_geometries><geometry id="g"><mesh>
<source id="g-pos"><float_array id="g-pos-array" count="12">0 0 0 1 0 0 1 1 0 0 1 0</float_array></source>
<vertices id="g-v"><input semantic="POSITION" source="#g-pos"/></vertices>
<triangles count="2"><input semantic="VERTEX" source="#g-v" offset="0"/><input semantic="NORMAL" source="#g-pos" offset="1"/><p>0 0 1 0 2 0 0 0 2 0 3 0</p></triangles>
</mesh></geometry></library_geometries>
<library_visual_scenes><visual_scene id="S"><node id="n" name="Quad"><translate>0 0 5</translate><instance_geometry url="#g"/></node></visual_scene></library_visual_scenes>
</COLLADA>"#;
        let scene = read(xml, "t").unwrap();
        let (id, o) = scene.objects().next().unwrap();
        assert_eq!(o.name, "Quad");
        // Blender's +Z becomes our +Y.
        assert!((o.transform.translation - Vec3::new(0.0, 5.0, 0.0)).length() < 1e-4, "{}", o.transform.translation);
        let (_, mesh) = scene.object_mesh(id).unwrap();
        assert_eq!(mesh.face_count(), 2);
        assert!(mesh.positions().all(|p| p.y.abs() < 1e-6), "the quad lies in our ground plane");
    }
}
