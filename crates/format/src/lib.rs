//! Native `.rshape` files plus OBJ, STL, glTF and PLY.
//!
//! Every importer produces a [`Scene`]; every exporter takes one. Importers that only carry
//! geometry (OBJ, STL, PLY) build one mesh object per group. Triangle soups (STL) are welded by
//! exact position, and if the welded result is not a manifold polygon mesh the triangles are kept
//! unwelded so the file still opens.

pub mod gltf_io;
pub mod obj;
pub mod ply;
pub mod rshape;
pub mod stl;

use shape_geom::{Mesh, MeshError};
use shape_math::Vec3;
use std::collections::HashMap;
use std::path::Path;

pub use shape_scene::Scene;

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-format";

#[derive(Debug, thiserror::Error)]
pub enum FormatError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("{format} line {line}: {message}")]
    Parse { format: &'static str, line: usize, message: String },
    #[error("mesh: {0}")]
    Mesh(#[from] MeshError),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("gltf: {0}")]
    Gltf(#[from] gltf::Error),
    #[error("unsupported file extension {0:?}")]
    UnknownExtension(String),
}

pub type Result<T> = std::result::Result<T, FormatError>;

fn parse_err(format: &'static str, line: usize, message: impl Into<String>) -> FormatError {
    FormatError::Parse { format, line, message: message.into() }
}

/// File formats by extension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Rshape,
    Obj,
    Stl,
    Ply,
    Gltf,
}

impl Format {
    pub fn from_path(path: &Path) -> Result<Self> {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        Ok(match ext.as_str() {
            "rshape" => Format::Rshape,
            "obj" => Format::Obj,
            "stl" => Format::Stl,
            "ply" => Format::Ply,
            "gltf" | "glb" => Format::Gltf,
            _ => return Err(FormatError::UnknownExtension(ext)),
        })
    }

    pub const ALL: [Format; 5] = [Format::Rshape, Format::Obj, Format::Stl, Format::Ply, Format::Gltf];

    pub fn extensions(self) -> &'static [&'static str] {
        match self {
            Format::Rshape => &["rshape"],
            Format::Obj => &["obj"],
            Format::Stl => &["stl"],
            Format::Ply => &["ply"],
            Format::Gltf => &["gltf", "glb"],
        }
    }
}

/// Open any supported file.
pub fn import(path: &Path) -> Result<Scene> {
    match Format::from_path(path)? {
        Format::Rshape => rshape::load(path),
        Format::Obj => obj::load(path),
        Format::Stl => stl::load(path),
        Format::Ply => ply::load(path),
        Format::Gltf => gltf_io::load(path),
    }
}

/// Write any supported file; the extension picks the format.
pub fn export(scene: &Scene, path: &Path) -> Result<()> {
    match Format::from_path(path)? {
        Format::Rshape => rshape::save(scene, path),
        Format::Obj => obj::save(scene, path),
        Format::Stl => stl::save(scene, path),
        Format::Ply => ply::save(scene, path),
        Format::Gltf => gltf_io::save(scene, path),
    }
}

/// Merge vertices at identical positions (to ~1e-6) and build a mesh. If the welded polygons are
/// not a valid manifold mesh, fall back to one vertex per corner so nothing is lost.
pub fn mesh_from_soup(positions: &[Vec3], polygons: &[Vec<u32>]) -> Result<Mesh> {
    // Only positions a polygon references survive, so a shared vertex pool (OBJ) does not leak
    // other groups' vertices into this mesh.
    let mut key_to_index: HashMap<[i64; 3], u32> = HashMap::new();
    let mut welded = Vec::new();
    let mut weld = |p: Vec3| -> u32 {
        let key = [(p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64, (p.z * 1e6).round() as i64];
        *key_to_index.entry(key).or_insert_with(|| {
            welded.push(p);
            welded.len() as u32 - 1
        })
    };
    let mut welded_polys: Vec<Vec<u32>> = Vec::with_capacity(polygons.len());
    for poly in polygons {
        let mut q: Vec<u32> = poly.iter().map(|&i| weld(positions[i as usize])).collect();
        q.dedup();
        if q.len() > 1 && q[0] == q[q.len() - 1] {
            q.pop();
        }
        if q.len() >= 3 {
            welded_polys.push(q);
        }
    }
    match Mesh::from_polygons(welded, &welded_polys) {
        Ok(m) => Ok(m),
        Err(MeshError::NonManifoldEdge { .. }) => {
            // Unwelded: every polygon gets its own vertices.
            let mut pos = Vec::new();
            let mut polys = Vec::new();
            for poly in polygons {
                if poly.len() < 3 {
                    continue;
                }
                let start = pos.len() as u32;
                pos.extend(poly.iter().map(|&i| positions[i as usize]));
                polys.push((start..start + poly.len() as u32).collect());
            }
            Ok(Mesh::from_polygons(pos, &polys)?)
        }
        Err(e) => Err(e.into()),
    }
}

/// Positions and polygons of every visible mesh object in world space (modifiers applied), with
/// the object's name.
pub(crate) fn world_polygons(scene: &Scene) -> Vec<(String, Vec<Vec3>, Vec<Vec<u32>>)> {
    let mut out = Vec::new();
    for (id, o) in scene.objects() {
        if !o.visible {
            continue;
        }
        if let Some(mesh) = shape_ops::evaluated_mesh(scene, id) {
            let t = scene.world_transform(id);
            let (mut pos, polys) = mesh.to_polygons();
            for p in &mut pos {
                *p = t.transform_point(*p);
            }
            out.push((o.name.clone(), pos, polys));
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod testutil {
    use super::*;
    use shape_geom::primitives::{cube, uv_sphere};
    use shape_math::{Quat, Transform};
    use shape_scene::Material;

    pub fn sample_scene() -> Scene {
        let mut s = Scene::new("sample");
        let mat = s.add_material(Material { name: "red".into(), base_color: [0.8, 0.1, 0.1, 1.0], ..Default::default() });
        let c = s.add_mesh_object("Cube", cube(1.0), Some(mat));
        s.object_mut(c).unwrap().transform = Transform::from_translation(Vec3::new(2.0, 0.0, 0.0));
        let sp = s.add_mesh_object("Sphere", uv_sphere(0.5, 8, 4), None);
        s.object_mut(sp).unwrap().transform = Transform::from_rotation(Quat::from_rotation_z(0.3));
        s
    }

    pub fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::var_os("KIROCREW_SCRATCH").map(std::path::PathBuf::from).unwrap_or_else(std::env::temp_dir);
        let dir = dir.join("shape-format-tests");
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    /// Geometry-only equality: same object names, same vertex/face counts, same world bounds.
    pub fn assert_same_geometry(a: &Scene, b: &Scene) {
        let (pa, pb) = (world_polygons(a), world_polygons(b));
        assert_eq!(pa.len(), pb.len(), "object count");
        for ((na, va, fa), (nb, vb, fb)) in pa.iter().zip(&pb) {
            assert_eq!(na, nb);
            assert_eq!(va.len(), vb.len(), "{na}: vertex count");
            let tris = |f: &Vec<Vec<u32>>| f.iter().map(|p| p.len() - 2).sum::<usize>();
            assert_eq!(tris(fa), tris(fb), "{na}: triangle count");
            let (ba, bb) = (shape_math::Aabb::from_points(va.iter().copied()), shape_math::Aabb::from_points(vb.iter().copied()));
            assert!((ba.min - bb.min).length() < 1e-4 && (ba.max - bb.max).length() < 1e-4, "{na}: bounds {ba:?} vs {bb:?}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::cube;

    #[test]
    fn crate_name() {
        assert_eq!(CRATE, "shape-format");
    }

    #[test]
    fn format_from_extension() {
        assert_eq!(Format::from_path(Path::new("a/b.OBJ")).unwrap(), Format::Obj);
        assert_eq!(Format::from_path(Path::new("x.glb")).unwrap(), Format::Gltf);
        assert!(matches!(Format::from_path(Path::new("x.blend")), Err(FormatError::UnknownExtension(_))));
    }

    #[test]
    fn soup_welds_a_cube() {
        let c = cube(1.0);
        let mut pos = Vec::new();
        let mut polys = Vec::new();
        for [a, b, d] in c.triangles() {
            let s = pos.len() as u32;
            pos.extend([c.position(a), c.position(b), c.position(d)]);
            polys.push(vec![s, s + 1, s + 2]);
        }
        let m = mesh_from_soup(&pos, &polys).unwrap();
        assert_eq!((m.vertex_count(), m.face_count()), (8, 12));
        assert!(m.is_closed());
    }

    #[test]
    fn soup_falls_back_when_not_manifold() {
        // Two triangles with the same winding on a shared edge: non-orientable.
        let pos = vec![Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];
        let polys = vec![vec![0, 1, 2], vec![0, 1, 3]];
        let m = mesh_from_soup(&pos, &polys).unwrap();
        assert_eq!((m.vertex_count(), m.face_count()), (6, 2));
    }
}
