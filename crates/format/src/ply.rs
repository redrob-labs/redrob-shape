//! PLY, ASCII only: `vertex` with x/y/z and `face` with a vertex index list. Other properties are
//! read past and dropped. Binary PLY is a later stage.

use crate::{Result, Scene, mesh_from_soup, parse_err, world_polygons};
use shape_math::Vec3;
use std::io::Write;
use std::path::Path;

pub fn load(path: &Path) -> Result<Scene> {
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("ply");
    let text = std::fs::read_to_string(path)?;
    let (positions, polys) = parse(&text)?;
    let mut scene = Scene::new(name);
    scene.add_mesh_object(name, mesh_from_soup(&positions, &polys)?, None);
    Ok(scene)
}

struct Element {
    name: String,
    count: usize,
    props: Vec<String>, // property names in order; list properties are prefixed "list:"
}

pub fn parse(text: &str) -> Result<(Vec<Vec3>, Vec<Vec<u32>>)> {
    let mut lines = text.lines().enumerate();
    let (_, magic) = lines.next().ok_or_else(|| parse_err("ply", 1, "empty file"))?;
    if magic.trim() != "ply" {
        return Err(parse_err("ply", 1, "missing 'ply' magic"));
    }
    let mut elements: Vec<Element> = Vec::new();
    let mut header_end = 0;
    for (ln, line) in lines.by_ref() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("format") => {
                if it.next() != Some("ascii") {
                    return Err(parse_err("ply", ln + 1, "only ascii PLY is supported"));
                }
            }
            Some("element") => {
                let name = it.next().unwrap_or("").to_owned();
                let count = it.next().and_then(|s| s.parse().ok()).ok_or_else(|| parse_err("ply", ln + 1, "element needs a count"))?;
                elements.push(Element { name, count, props: Vec::new() });
            }
            Some("property") => {
                let el = elements.last_mut().ok_or_else(|| parse_err("ply", ln + 1, "property before element"))?;
                let kind = it.next().unwrap_or("");
                if kind == "list" {
                    let name = it.nth(2).unwrap_or("");
                    el.props.push(format!("list:{name}"));
                } else {
                    el.props.push(it.next().unwrap_or("").to_owned());
                }
            }
            Some("end_header") => {
                header_end = ln + 1;
                break;
            }
            _ => {}
        }
    }
    if header_end == 0 {
        return Err(parse_err("ply", 0, "no end_header"));
    }
    let mut positions = Vec::new();
    let mut polys = Vec::new();
    for el in &elements {
        let (xi, yi, zi) = (el.props.iter().position(|p| p == "x"), el.props.iter().position(|p| p == "y"), el.props.iter().position(|p| p == "z"));
        let list_i = el.props.iter().position(|p| p.starts_with("list:"));
        for _ in 0..el.count {
            let (ln, line) = lines.next().ok_or_else(|| parse_err("ply", 0, format!("file ends inside element {}", el.name)))?;
            let toks: Vec<&str> = line.split_whitespace().collect();
            if el.name == "vertex" {
                let (Some(xi), Some(yi), Some(zi)) = (xi, yi, zi) else {
                    return Err(parse_err("ply", ln + 1, "vertex element lacks x y z"));
                };
                let get = |i: usize| toks.get(i).and_then(|s| s.parse::<f32>().ok()).ok_or_else(|| parse_err("ply", ln + 1, "bad vertex"));
                positions.push(Vec3::new(get(xi)?, get(yi)?, get(zi)?));
            } else if el.name == "face" {
                let li = list_i.ok_or_else(|| parse_err("ply", ln + 1, "face element lacks a list property"))?;
                let n: usize = toks.get(li).and_then(|s| s.parse().ok()).ok_or_else(|| parse_err("ply", ln + 1, "bad face count"))?;
                let mut poly = Vec::with_capacity(n);
                for k in 0..n {
                    let idx: u32 = toks.get(li + 1 + k).and_then(|s| s.parse().ok()).ok_or_else(|| parse_err("ply", ln + 1, "bad face index"))?;
                    if idx as usize >= positions.len() {
                        return Err(parse_err("ply", ln + 1, format!("face index {idx} out of range")));
                    }
                    poly.push(idx);
                }
                if poly.len() >= 3 {
                    polys.push(poly);
                }
            }
        }
    }
    Ok((positions, polys))
}

pub fn save(scene: &Scene, path: &Path) -> Result<()> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    write(scene, &mut out)
}

pub fn write(scene: &Scene, out: &mut impl Write) -> Result<()> {
    let groups = world_polygons(scene);
    let nv: usize = groups.iter().map(|g| g.1.len()).sum();
    let nf: usize = groups.iter().map(|g| g.2.len()).sum();
    writeln!(out, "ply\nformat ascii 1.0\ncomment Redrob Shape {}", env!("CARGO_PKG_VERSION"))?;
    writeln!(out, "element vertex {nv}\nproperty float x\nproperty float y\nproperty float z")?;
    writeln!(out, "element face {nf}\nproperty list uchar uint vertex_indices\nend_header")?;
    for (_, pos, _) in &groups {
        for p in pos {
            writeln!(out, "{} {} {}", p.x, p.y, p.z)?;
        }
    }
    let mut offset = 0u32;
    for (_, pos, polys) in &groups {
        for poly in polys {
            write!(out, "{}", poly.len())?;
            for &i in poly {
                write!(out, " {}", i + offset)?;
            }
            writeln!(out)?;
        }
        offset += pos.len() as u32;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{sample_scene, scratch};

    #[test]
    fn round_trip_counts() {
        let scene = sample_scene();
        let path = scratch("sample.ply");
        save(&scene, &path).unwrap();
        let back = load(&path).unwrap();
        let (id, _) = back.objects().next().unwrap();
        let (_, m) = back.object_mesh(id).unwrap();
        let expected_faces: usize = scene.meshes().map(|(_, d)| d.mesh.face_count()).sum();
        assert_eq!(m.face_count(), expected_faces);
        assert_eq!(m.vertex_count(), 8 + 26);
    }

    #[test]
    fn skips_extra_properties() {
        let text = "ply\nformat ascii 1.0\nelement vertex 3\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nelement face 1\nproperty list uchar int vertex_indices\nend_header\n0 0 0 255\n1 0 0 255\n0 1 0 255\n3 0 1 2\n";
        let (p, f) = parse(text).unwrap();
        assert_eq!((p.len(), f.len()), (3, 1));
    }
}
