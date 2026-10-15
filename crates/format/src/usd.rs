//! USD (`.usda`, the ASCII encoding) import and export. Export writes one `Xform` per object with
//! a `xformOp:transform` matrix and a child `Mesh` (points, faceVertexCounts, faceVertexIndices,
//! displayColor), with `upAxis = "Y"`. Import walks `def` blocks, reads those attributes and the
//! Xform's transform (or translate / rotateXYZ / scale ops), and converts `upAxis = "Z"` files
//! (Blender's) into our frame. Binary `.usdc` and `.usdz` are not read.

use std::io::Write;
use std::path::Path;

use shape_math::{Mat4, Quat, Vec3};
use shape_scene::Scene;

use crate::interchange::{export_objects, floats, ints, scene_from_records, z_up_matrix_to_y_up, z_up_to_y_up};
use crate::{Result, parse_err};

pub fn save(scene: &Scene, path: &Path) -> Result<()> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    write(scene, &mut out)
}

pub fn write(scene: &Scene, out: &mut impl Write) -> Result<()> {
    writeln!(
        out,
        "#usda 1.0\n(\n    defaultPrim = \"root\"\n    doc = \"Redrob Shape {}\"\n    metersPerUnit = 1\n    upAxis = \"Y\"\n)\n",
        env!("CARGO_PKG_VERSION")
    )?;
    writeln!(out, "def Xform \"root\"\n{{")?;
    for o in export_objects(scene) {
        writeln!(out, "    def Xform \"{}\"\n    {{", o.name)?;
        let m = o.world.to_cols_array_2d();
        // USD matrices are row vectors: rows are the basis vectors, translation in the last row.
        write!(out, "        matrix4d xformOp:transform = ( ")?;
        for (col, c) in m.iter().enumerate() {
            write!(out, "({}, {}, {}, {})", c[0], c[1], c[2], c[3])?;
            if col < 3 {
                write!(out, ", ")?;
            }
        }
        writeln!(out, " )")?;
        writeln!(out, "        uniform token[] xformOpOrder = [\"xformOp:transform\"]\n")?;
        writeln!(out, "        def Mesh \"{}_mesh\"\n        {{", o.name)?;
        write!(out, "            int[] faceVertexCounts = [")?;
        for (i, poly) in o.polygons.iter().enumerate() {
            write!(out, "{}{}", if i > 0 { ", " } else { "" }, poly.len())?;
        }
        writeln!(out, "]")?;
        write!(out, "            int[] faceVertexIndices = [")?;
        let mut first = true;
        for poly in &o.polygons {
            for i in poly {
                write!(out, "{}{}", if first { "" } else { ", " }, i)?;
                first = false;
            }
        }
        writeln!(out, "]")?;
        write!(out, "            point3f[] points = [")?;
        for (i, p) in o.positions.iter().enumerate() {
            write!(out, "{}({}, {}, {})", if i > 0 { ", " } else { "" }, p.x, p.y, p.z)?;
        }
        writeln!(out, "]")?;
        if let Some(c) = o.color {
            writeln!(out, "            color3f[] primvars:displayColor = [({}, {}, {})]", c[0], c[1], c[2])?;
        }
        writeln!(out, "            uniform token subdivisionScheme = \"none\"\n        }}\n    }}")?;
    }
    writeln!(out, "}}")?;
    Ok(())
}

/// One `def Type "Name" { ... }` block with its raw body text and nested blocks.
#[derive(Debug, Default)]
struct Prim {
    ty: String,
    name: String,
    body: String,
    children: Vec<Prim>,
}

/// Parse nested `def` blocks. Braces inside strings are not expected in the attributes we read.
fn parse_prims(text: &str) -> Vec<Prim> {
    let bytes = text.as_bytes();
    let mut prims = Vec::new();
    let mut i = 0;
    while let Some(rel) = text[i..].find("def ") {
        let start = i + rel;
        // Must be at a line start (or after whitespace), not inside a word.
        if start > 0 && !text.as_bytes()[start - 1].is_ascii_whitespace() {
            i = start + 4;
            continue;
        }
        let Some(brace_rel) = text[start..].find('{') else { break };
        let header = &text[start + 4..start + brace_rel];
        let mut depth = 0i32;
        let mut end = None;
        for (k, &b) in bytes.iter().enumerate().skip(start + brace_rel) {
            match b {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(k);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else { break };
        let inner = &text[start + brace_rel + 1..end];
        let mut words = header.split_whitespace();
        let ty = words.next().unwrap_or("").to_string();
        let name = header.split('"').nth(1).unwrap_or("").to_string();
        let children = parse_prims(inner);
        // The body without nested defs (attributes of this prim).
        let body = strip_defs(inner);
        prims.push(Prim { ty, name, body, children });
        i = end + 1;
    }
    prims
}

fn strip_defs(inner: &str) -> String {
    let mut out = String::new();
    let mut depth = 0i32;
    let mut skipping = false;
    let mut rest = inner;
    while !rest.is_empty() {
        if !skipping
            && let Some(k) = rest.find("def ")
            && (k == 0 || rest.as_bytes()[k - 1].is_ascii_whitespace())
        {
            out.push_str(&rest[..k]);
            rest = &rest[k..];
            skipping = true;
            depth = 0;
            continue;
        }
        if skipping {
            let mut consumed = 0;
            for (k, ch) in rest.char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            consumed = k + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            if consumed == 0 {
                break;
            }
            rest = &rest[consumed..];
            skipping = false;
        } else {
            out.push_str(rest);
            break;
        }
    }
    out
}

/// The text between `name =` and the end of its bracketed value (or line).
fn attribute<'a>(body: &'a str, name: &str) -> Option<&'a str> {
    let mut search = 0;
    while let Some(rel) = body[search..].find(name) {
        let at = search + rel;
        let after = &body[at + name.len()..];
        let trimmed = after.trim_start();
        if !trimmed.starts_with('=') || (at > 0 && body.as_bytes()[at - 1].is_ascii_alphanumeric()) {
            search = at + name.len();
            continue;
        }
        let value = trimmed[1..].trim_start();
        return Some(match value.chars().next() {
            Some('[') => bracketed(value, '[', ']'),
            Some('(') => bracketed(value, '(', ')'),
            _ => value.lines().next().unwrap_or(""),
        });
    }
    None
}

fn bracketed(s: &str, open: char, close: char) -> &str {
    let mut depth = 0i32;
    for (k, ch) in s.char_indices() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
            if depth == 0 {
                return &s[..=k];
            }
        }
    }
    s
}

fn numbers(s: &str) -> Vec<f32> {
    floats(&s.replace(['(', ')', '[', ']'], " "))
}

fn xform_matrix(body: &str) -> Mat4 {
    if let Some(v) = attribute(body, "xformOp:transform") {
        let n = numbers(v);
        if n.len() == 16 {
            // Row-vector convention: our column c is their row c.
            let mut cols = [0.0f32; 16];
            for row in 0..4 {
                for col in 0..4 {
                    cols[row * 4 + col] = n[row * 4 + col];
                }
            }
            return Mat4::from_cols_array(&cols);
        }
    }
    let mut m = Mat4::IDENTITY;
    if let Some(v) = attribute(body, "xformOp:translate") {
        let n = numbers(v);
        if n.len() == 3 {
            m *= Mat4::from_translation(Vec3::new(n[0], n[1], n[2]));
        }
    }
    if let Some(v) = attribute(body, "xformOp:rotateXYZ") {
        let n = numbers(v);
        if n.len() == 3 {
            m *= Mat4::from_quat(Quat::from_euler(shape_math::glam_euler(), n[0].to_radians(), n[1].to_radians(), n[2].to_radians()));
        }
    }
    if let Some(v) = attribute(body, "xformOp:scale") {
        let n = numbers(v);
        if n.len() == 3 {
            m *= Mat4::from_scale(Vec3::new(n[0], n[1], n[2]));
        }
    }
    m
}

pub fn load(path: &Path) -> Result<Scene> {
    let text = std::fs::read_to_string(path)?;
    read(&text, path.file_stem().and_then(|s| s.to_str()).unwrap_or("usd"))
}

pub fn read(text: &str, name: &str) -> Result<Scene> {
    if !text.trim_start().starts_with("#usda") {
        return Err(parse_err("usda", 1, "not an ASCII USD file (#usda 1.0 header missing); binary usdc/usdz is not supported"));
    }
    let header_end = text.find("\ndef ").unwrap_or(0);
    let z_up = text[..header_end].contains("upAxis = \"Z\"");
    let prims = parse_prims(text);
    let mut records = Vec::new();
    fn walk(p: &Prim, parent: Mat4, z_up: bool, records: &mut Vec<(String, Vec<Vec3>, Vec<Vec<u32>>, Mat4, Option<[f32; 4]>)>) {
        let world = parent * xform_matrix(&p.body);
        if p.ty == "Mesh" {
            let counts = attribute(&p.body, "faceVertexCounts").map(|v| ints(&v.replace(['[', ']'], " "))).unwrap_or_default();
            let indices = attribute(&p.body, "faceVertexIndices").map(|v| ints(&v.replace(['[', ']'], " "))).unwrap_or_default();
            let pts = attribute(&p.body, "points").map(numbers).unwrap_or_default();
            let mut positions: Vec<Vec3> = pts.chunks(3).filter(|c| c.len() == 3).map(|c| Vec3::new(c[0], c[1], c[2])).collect();
            if z_up {
                for q in &mut positions {
                    *q = z_up_to_y_up(*q);
                }
            }
            let mut polygons = Vec::new();
            let mut cursor = 0usize;
            for n in counts {
                let n = n.max(0) as usize;
                if cursor + n <= indices.len() && n >= 3 {
                    polygons.push(indices[cursor..cursor + n].iter().map(|i| *i as u32).collect());
                }
                cursor += n;
            }
            let color = attribute(&p.body, "primvars:displayColor").map(numbers).filter(|c| c.len() >= 3).map(|c| [c[0], c[1], c[2], 1.0]);
            let world_y = if z_up { z_up_matrix_to_y_up(world) } else { world };
            let name = p.name.trim_end_matches("_mesh").to_string();
            records.push((name, positions, polygons, world_y, color));
        }
        for c in &p.children {
            walk(c, world, z_up, records);
        }
    }
    for p in &prims {
        walk(p, Mat4::IDENTITY, z_up, &mut records);
    }
    scene_from_records(name, records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{sample_scene, scratch};

    #[test]
    fn round_trip_keeps_objects_and_transforms() {
        let scene = sample_scene();
        let path = scratch("sample.usda");
        save(&scene, &path).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back.object_count(), scene.object_count());
        let (id, cube) = back.objects().find(|(_, o)| o.name == "Cube").unwrap();
        assert!((cube.transform.translation - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-4, "{}", cube.transform.translation);
        assert_eq!(back.object_mesh(id).unwrap().1.face_count(), 6);
        let (_, sphere) = back.objects().find(|(_, o)| o.name == "Sphere").unwrap();
        let angle = sphere.transform.rotation.angle_between(Quat::from_rotation_z(0.3));
        assert!(angle < 1e-3, "rotation kept: {angle}");
    }

    #[test]
    fn blender_style_z_up_file_with_translate_op() {
        let text = r#"#usda 1.0
(
    upAxis = "Z"
    metersPerUnit = 1
)

def Xform "Plane"
{
    double3 xformOp:translate = (0, 0, 3)
    uniform token[] xformOpOrder = ["xformOp:translate"]

    def Mesh "Plane"
    {
        int[] faceVertexCounts = [4]
        int[] faceVertexIndices = [0, 1, 3, 2]
        point3f[] points = [(-1, -1, 0), (1, -1, 0), (-1, 1, 0), (1, 1, 0)]
        color3f[] primvars:displayColor = [(0.8, 0.1, 0.1)]
    }
}
"#;
        let scene = read(text, "t").unwrap();
        let (id, o) = scene.objects().next().unwrap();
        assert_eq!(o.name, "Plane");
        assert!((o.transform.translation - Vec3::new(0.0, 3.0, 0.0)).length() < 1e-4, "{}", o.transform.translation);
        let (_, mesh) = scene.object_mesh(id).unwrap();
        assert_eq!(mesh.face_count(), 1);
        assert!(mesh.positions().all(|p| p.y.abs() < 1e-6));
    }
}
