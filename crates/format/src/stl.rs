//! STL, binary and ASCII. A triangle soup: reading welds vertices, writing triangulates.

use crate::{Result, Scene, mesh_from_soup, parse_err, world_polygons};
use shape_math::Vec3;
use std::io::Write;
use std::path::Path;

pub fn load(path: &Path) -> Result<Scene> {
    let bytes = std::fs::read(path)?;
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("stl");
    let triangles = if is_ascii(&bytes) { parse_ascii(&bytes)? } else { parse_binary(&bytes)? };
    let mut positions = Vec::with_capacity(triangles.len() * 3);
    let mut polys = Vec::with_capacity(triangles.len());
    for t in triangles {
        let s = positions.len() as u32;
        positions.extend(t);
        polys.push(vec![s, s + 1, s + 2]);
    }
    let mut scene = Scene::new(name);
    scene.add_mesh_object(name, mesh_from_soup(&positions, &polys)?, None);
    Ok(scene)
}

fn is_ascii(bytes: &[u8]) -> bool {
    // Binary files can start with "solid" too; trust the size check first.
    if bytes.len() >= 84 {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if bytes.len() == 84 + n * 50 {
            return false;
        }
    }
    bytes.starts_with(b"solid")
}

fn parse_binary(bytes: &[u8]) -> Result<Vec<[Vec3; 3]>> {
    if bytes.len() < 84 {
        return Err(parse_err("stl", 0, "binary STL shorter than its 84-byte header"));
    }
    let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
    if bytes.len() < 84 + n * 50 {
        return Err(parse_err("stl", 0, format!("header promises {n} triangles but the file is too short")));
    }
    let f = |o: usize| f32::from_le_bytes([bytes[o], bytes[o + 1], bytes[o + 2], bytes[o + 3]]);
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let base = 84 + i * 50 + 12; // skip the normal
        let v = |k: usize| Vec3::new(f(base + k * 12), f(base + k * 12 + 4), f(base + k * 12 + 8));
        out.push([v(0), v(1), v(2)]);
    }
    Ok(out)
}

fn parse_ascii(bytes: &[u8]) -> Result<Vec<[Vec3; 3]>> {
    let text = String::from_utf8_lossy(bytes);
    let mut out = Vec::new();
    let mut current: Vec<Vec3> = Vec::new();
    for (ln, line) in text.lines().enumerate() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("vertex") => {
                let mut c = [0f32; 3];
                for v in &mut c {
                    *v = it.next().and_then(|s| s.parse().ok()).ok_or_else(|| parse_err("stl", ln + 1, "vertex needs 3 numbers"))?;
                }
                current.push(Vec3::from_array(c));
            }
            Some("endfacet") => {
                if current.len() != 3 {
                    return Err(parse_err("stl", ln + 1, format!("facet has {} vertices", current.len())));
                }
                out.push([current[0], current[1], current[2]]);
                current.clear();
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Binary STL of every visible mesh object, in world space.
pub fn save(scene: &Scene, path: &Path) -> Result<()> {
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    write_binary(scene, &mut out)
}

pub fn write_binary(scene: &Scene, out: &mut impl Write) -> Result<()> {
    let mut tris: Vec<[Vec3; 3]> = Vec::new();
    for (_, pos, polys) in world_polygons(scene) {
        for poly in polys {
            for i in 1..poly.len() - 1 {
                tris.push([pos[poly[0] as usize], pos[poly[i] as usize], pos[poly[i + 1] as usize]]);
            }
        }
    }
    let mut header = [0u8; 80];
    let tag = b"Redrob Shape binary STL";
    header[..tag.len()].copy_from_slice(tag);
    out.write_all(&header)?;
    out.write_all(&(tris.len() as u32).to_le_bytes())?;
    for t in tris {
        let n = (t[1] - t[0]).cross(t[2] - t[0]).normalize_or_zero();
        for v in [n, t[0], t[1], t[2]] {
            for c in v.to_array() {
                out.write_all(&c.to_le_bytes())?;
            }
        }
        out.write_all(&[0, 0])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{sample_scene, scratch};
    use shape_geom::primitives::cube;

    #[test]
    fn binary_round_trip_welds_back_to_a_closed_cube() {
        let mut scene = Scene::new("c");
        scene.add_mesh_object("Cube", cube(2.0), None);
        let path = scratch("cube.stl");
        save(&scene, &path).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 84 + 12 * 50);
        let back = load(&path).unwrap();
        let (id, _) = back.objects().next().unwrap();
        let (_, m) = back.object_mesh(id).unwrap();
        assert_eq!((m.vertex_count(), m.face_count()), (8, 12));
        assert!(m.is_closed());
        assert_eq!(m.euler_characteristic(), 2);
    }

    #[test]
    fn ascii_parses() {
        let text =
            "solid t\n facet normal 0 0 1\n  outer loop\n   vertex 0 0 0\n   vertex 1 0 0\n   vertex 0 1 0\n  endloop\n endfacet\nendsolid t\n";
        let path = scratch("tri.stl");
        std::fs::write(&path, text).unwrap();
        let back = load(&path).unwrap();
        let (id, _) = back.objects().next().unwrap();
        assert_eq!(back.object_mesh(id).unwrap().1.face_count(), 1);
    }

    #[test]
    fn whole_sample_scene_merges_into_one_object() {
        let path = scratch("sample.stl");
        save(&sample_scene(), &path).unwrap();
        let back = load(&path).unwrap();
        assert_eq!(back.object_count(), 1);
    }
}
