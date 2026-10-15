//! `cargo run -p shape-blend --example inspect -- file.blend`: print what the importer sees.
fn main() {
    let path = std::env::args().nth(1).expect("path to a .blend");
    let file = shape_blend::BlendFile::open(std::path::Path::new(&path)).expect("parse");
    println!("version {} blocks {} structs {}", file.version(), file.blocks.len(), file.sdna.structs.len());
    let scene = shape_blend::import(std::path::Path::new(&path)).expect("import");
    for (_, o) in scene.objects() {
        let kind = match &o.data {
            shape_scene::ObjectData::Mesh { mesh, material, .. } => {
                let m = scene.mesh(*mesh).unwrap();
                format!(
                    "mesh v={} e={} f={} closed={} mat={:?}",
                    m.mesh.vertex_count(),
                    m.mesh.edge_count(),
                    m.mesh.face_count(),
                    m.mesh.is_closed(),
                    material.and_then(|id| scene.material(id)).map(|m| &m.name)
                )
            }
            shape_scene::ObjectData::Light(l) => format!("light {} {}W", l.kind_name(), l.power),
            shape_scene::ObjectData::Camera(c) => format!("camera fov {:.1}", c.fov_y.to_degrees()),
            shape_scene::ObjectData::Empty => "empty".into(),
            shape_scene::ObjectData::Armature(a) => format!("armature {} bones", a.bones.len()),
        };
        println!("{:<24} at {:?}  {}", o.name, o.transform.translation, kind);
    }
}
