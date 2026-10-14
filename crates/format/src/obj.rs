//! Wavefront OBJ: `v`, `f` (any polygon, negative indices, `v/vt/vn` triplets) and `o`/`g` groups.
//! `vt` is written when a mesh carries UVs; on read, texture coordinates and normals are skipped.

use crate::{Result, Scene, mesh_from_soup, parse_err, world_polygons};
use shape_math::Vec3;
use std::io::Write;
use std::path::Path;

pub fn load(path: &Path) -> Result<Scene> {
    parse(&std::fs::read_to_string(path)?, path.file_stem().and_then(|s| s.to_str()).unwrap_or("obj"))
}

pub fn parse(text: &str, default_name: &str) -> Result<Scene> {
    let mut scene = Scene::new(default_name);
    let mut positions: Vec<Vec3> = Vec::new();
    let mut group_name = default_name.to_owned();
    let mut group_polys: Vec<Vec<u32>> = Vec::new();

    fn flush(scene: &mut Scene, positions: &[Vec3], name: &str, polys: &mut Vec<Vec<u32>>) -> Result<()> {
        if polys.is_empty() {
            return Ok(());
        }
        let mesh = mesh_from_soup(positions, polys)?;
        scene.add_mesh_object(name, mesh, None);
        polys.clear();
        Ok(())
    }

    for (ln, raw) in text.lines().enumerate() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        match key {
            "v" => {
                let mut c = [0f32; 3];
                for v in &mut c {
                    *v = it.next().and_then(|s| s.parse().ok()).ok_or_else(|| parse_err("obj", ln + 1, "vertex needs 3 numbers"))?;
                }
                positions.push(Vec3::from_array(c));
            }
            "f" => {
                let mut poly = Vec::new();
                for tok in it {
                    let idx_str = tok.split('/').next().unwrap_or("");
                    let idx: i64 = idx_str.parse().map_err(|_| parse_err("obj", ln + 1, format!("bad face index {tok:?}")))?;
                    let n = positions.len() as i64;
                    let resolved = if idx < 0 { n + idx } else { idx - 1 };
                    if resolved < 0 || resolved >= n {
                        return Err(parse_err("obj", ln + 1, format!("face index {idx} out of range")));
                    }
                    poly.push(resolved as u32);
                }
                if poly.len() < 3 {
                    return Err(parse_err("obj", ln + 1, "face needs 3 vertices"));
                }
                group_polys.push(poly);
            }
            "o" | "g" => {
                flush(&mut scene, &positions, &group_name, &mut group_polys)?;
                group_name = it.collect::<Vec<_>>().join(" ");
                if group_name.is_empty() {
                    group_name = default_name.to_owned();
                }
            }
            _ => {} // vt, vn, usemtl, mtllib, s: ignored
        }
    }
    flush(&mut scene, &positions, &group_name, &mut group_polys)?;
    Ok(scene)
}

pub fn save(scene: &Scene, path: &Path) -> Result<()> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    write(scene, &mut out)
}

pub fn write(scene: &Scene, out: &mut impl Write) -> Result<()> {
    writeln!(out, "# Redrob Shape {}", env!("CARGO_PKG_VERSION"))?;
    let mut offset = 1u32;
    let mut vt_offset = 1u32;
    let uv_sets = crate::world_uvs(scene);
    for ((name, pos, polys), uvs) in world_polygons(scene).into_iter().zip(uv_sets) {
        writeln!(out, "o {}", name.replace(char::is_whitespace, "_"))?;
        for p in &pos {
            writeln!(out, "v {} {} {}", p.x, p.y, p.z)?;
        }
        // UVs are per corner: write them in face order and reference them in the same order.
        let uvs = uvs.filter(|u| u.len() == polys.len() && u.iter().zip(&polys).all(|(a, b)| a.len() == b.len()));
        if let Some(uvs) = &uvs {
            for corner in uvs.iter().flatten() {
                writeln!(out, "vt {} {}", corner.x, corner.y)?;
            }
        }
        let mut vt = vt_offset;
        for poly in &polys {
            write!(out, "f")?;
            for &i in poly {
                if uvs.is_some() {
                    write!(out, " {}/{}", i + offset, vt)?;
                    vt += 1;
                } else {
                    write!(out, " {}", i + offset)?;
                }
            }
            writeln!(out)?;
        }
        if let Some(uvs) = &uvs {
            vt_offset += uvs.iter().map(|f| f.len() as u32).sum::<u32>();
        }
        offset += pos.len() as u32;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{assert_same_geometry, sample_scene, scratch};

    #[test]
    fn round_trip_keeps_geometry() {
        let scene = sample_scene();
        let path = scratch("sample.obj");
        save(&scene, &path).unwrap();
        let back = load(&path).unwrap();
        assert_same_geometry(&scene, &back);
        assert_eq!(back.objects().map(|(_, o)| o.name.as_str()).collect::<Vec<_>>(), ["Cube", "Sphere"]);
    }

    #[test]
    fn writes_vt_when_the_mesh_has_uvs() {
        let mut scene = Scene::new("uv");
        let id = scene.add_mesh_object("Plane", shape_geom::primitives::plane(2.0, 1, 1), None);
        let (mesh_id, mesh) = scene.object_mesh(id).unwrap();
        let uvs = shape_ops::uv_planar(mesh, 1);
        scene.mesh_mut(mesh_id).unwrap().uvs = Some(uvs);
        let mut out = Vec::new();
        write(&scene, &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().filter(|l| l.starts_with("vt ")).count(), 4);
        assert!(text.lines().any(|l| l.starts_with("f 1/1 ")), "{text}");
        let back = parse(&text, "t").unwrap();
        assert_eq!(back.object_count(), 1);
    }

    #[test]
    fn parses_triplets_negative_indices_and_comments() {
        let text = "# tri\nv 0 0 0\nv 1 0 0\nv 0 1 0\nvt 0 0\nvn 0 0 1\nf 1/1/1 2/1/1 -1/1/1 # face\n";
        let scene = parse(text, "t").unwrap();
        let (id, _) = scene.objects().next().unwrap();
        let (_, m) = scene.object_mesh(id).unwrap();
        assert_eq!((m.vertex_count(), m.face_count()), (3, 1));
    }

    #[test]
    fn reports_bad_index_with_line() {
        let err = parse("v 0 0 0\nv 1 0 0\nf 1 2 9\n", "t").unwrap_err();
        assert!(err.to_string().contains("line 3"), "{err}");
    }
}
