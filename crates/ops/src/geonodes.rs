//! Evaluate a geometry node tree: pull geometry from the `Group Output` back through the links.

use std::collections::HashMap;

use shape_geom::Mesh;
use shape_geom::primitives::{cube, cylinder, plane, uv_sphere};
use shape_math::{Mat4, Quat, Transform, Vec3, glam_euler};
use shape_scene::{GeoNodeKind, GeoNodeTree};

use crate::Result;

/// Run `tree` with `input` on the Group Input. An unlinked Group Output gives an empty mesh;
/// an unlinked geometry input anywhere else is empty geometry, as in Blender.
pub fn evaluate(tree: &GeoNodeTree, input: &Mesh) -> Result<Mesh> {
    let Some(out) = tree.output_node() else { return Ok(input.clone()) };
    let mut cache: HashMap<usize, Mesh> = HashMap::new();
    socket(tree, input, out, 0, &mut cache, 0)
}

fn empty() -> Mesh {
    Mesh::from_polygons(Vec::new(), &[]).unwrap_or_default()
}

/// The geometry arriving at `node.input[index]`.
fn socket(tree: &GeoNodeTree, input: &Mesh, node: usize, index: usize, cache: &mut HashMap<usize, Mesh>, depth: u32) -> Result<Mesh> {
    let Some(link) = tree.input_link(node, index) else { return Ok(empty()) };
    node_output(tree, input, link.from_node, cache, depth + 1)
}

fn node_output(tree: &GeoNodeTree, input: &Mesh, node: usize, cache: &mut HashMap<usize, Mesh>, depth: u32) -> Result<Mesh> {
    if depth > 256 {
        return Ok(empty());
    }
    if let Some(m) = cache.get(&node) {
        return Ok(m.clone());
    }
    let Some(n) = tree.nodes.get(node) else { return Ok(empty()) };
    let mesh = match &n.kind {
        GeoNodeKind::GroupInput => input.clone(),
        GeoNodeKind::GroupOutput => empty(),
        GeoNodeKind::Transform { translation, rotation, scale } => {
            let g = socket(tree, input, node, 0, cache, depth)?;
            let rot = Quat::from_euler(glam_euler(), rotation.x, rotation.y, rotation.z);
            transform(&g, Mat4::from_scale_rotation_translation(*scale, rot, *translation))
        }
        GeoNodeKind::JoinGeometry => {
            let a = socket(tree, input, node, 0, cache, depth)?;
            let b = socket(tree, input, node, 1, cache, depth)?;
            join(&[a, b])?
        }
        GeoNodeKind::SubdivideMesh { level } => {
            let mut g = socket(tree, input, node, 0, cache, depth)?;
            for _ in 0..(*level).min(6) {
                g = crate::subdivide_linear(&g)?;
            }
            g
        }
        GeoNodeKind::SubdivisionSurface { level } => {
            let mut g = socket(tree, input, node, 0, cache, depth)?;
            for _ in 0..(*level).min(6) {
                g = crate::catmull_clark(&g)?;
            }
            g
        }
        GeoNodeKind::SetPosition { offset } => {
            let mut g = socket(tree, input, node, 0, cache, depth)?;
            for v in g.vertex_ids().collect::<Vec<_>>() {
                let p = g.position(v);
                g.set_position(v, p + *offset);
            }
            g
        }
        GeoNodeKind::MeshCube { size } => transform(&cube(1.0), Mat4::from_scale(*size)),
        GeoNodeKind::MeshUvSphere { radius, segments, rings } => uv_sphere(*radius, (*segments).max(3), (*rings).max(2)),
        GeoNodeKind::MeshCylinder { radius, depth: d, vertices } => cylinder(*radius, *d, (*vertices).max(3)),
        GeoNodeKind::MeshGrid { size_x, size_y, vertices_x, vertices_y } => {
            let g = plane(1.0, (*vertices_x).max(2) - 1, (*vertices_y).max(2) - 1);
            transform(&g, Mat4::from_scale(Vec3::new(*size_x, 1.0, *size_y)))
        }
        GeoNodeKind::InstanceOnPoints { scale } => {
            let points = socket(tree, input, node, 0, cache, depth)?;
            let instance = socket(tree, input, node, 1, cache, depth)?;
            let copies: Vec<Mesh> =
                points.positions().take(4096).map(|p| transform(&instance, Mat4::from_scale_rotation_translation(Vec3::splat(*scale), Quat::IDENTITY, p))).collect();
            join(&copies)?
        }
        GeoNodeKind::MeshBoolean { op } => {
            let a = socket(tree, input, node, 0, cache, depth)?;
            let b = socket(tree, input, node, 1, cache, depth)?;
            if a.face_count() == 0 {
                b
            } else if b.face_count() == 0 {
                a
            } else {
                crate::boolean(&a, &b, &Transform::IDENTITY, *op)?
            }
        }
    };
    cache.insert(node, mesh.clone());
    Ok(mesh)
}

fn transform(mesh: &Mesh, m: Mat4) -> Mesh {
    let (mut pos, polys) = mesh.to_polygons();
    for p in &mut pos {
        *p = m.transform_point3(*p);
    }
    Mesh::from_polygons(pos, &polys).unwrap_or_else(|_| mesh.clone())
}

/// Concatenate meshes as separate shells.
pub fn join(meshes: &[Mesh]) -> Result<Mesh> {
    let mut positions = Vec::new();
    let mut polygons: Vec<Vec<u32>> = Vec::new();
    for m in meshes {
        let (pos, polys) = m.to_polygons();
        let offset = positions.len() as u32;
        positions.extend(pos);
        polygons.extend(polys.into_iter().map(|poly| poly.into_iter().map(|i| i + offset).collect()));
    }
    Ok(Mesh::from_polygons(positions, &polygons)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_scene::BooleanOp;

    #[test]
    fn default_tree_passes_the_input_through() {
        let tree = GeoNodeTree::default_tree();
        let out = evaluate(&tree, &cube(1.0)).unwrap();
        assert_eq!(out.face_count(), 6);
    }

    #[test]
    fn primitives_join_subdivide_and_instance() {
        let mut t = GeoNodeTree::default_tree();
        let c = t.add_node(GeoNodeKind::MeshCube { size: Vec3::ONE }, [0.0; 2]);
        let sub = t.add_node(GeoNodeKind::SubdivideMesh { level: 1 }, [0.0; 2]);
        let j = t.add_node(GeoNodeKind::JoinGeometry, [0.0; 2]);
        assert!(t.link(c, 0, sub, 0));
        assert!(t.link(sub, 0, j, 0));
        assert!(t.link(0, 0, j, 1));
        assert!(t.link(j, 0, 1, 0));
        let out = evaluate(&t, &cube(1.0)).unwrap();
        assert_eq!(out.face_count(), 24 + 6, "subdivided cube joined with the input cube");

        let mut t2 = GeoNodeTree::default_tree();
        let grid = t2.add_node(GeoNodeKind::MeshGrid { size_x: 2.0, size_y: 2.0, vertices_x: 3, vertices_y: 3 }, [0.0; 2]);
        let inst = t2.add_node(GeoNodeKind::InstanceOnPoints { scale: 0.1 }, [0.0; 2]);
        assert!(t2.link(grid, 0, inst, 0));
        assert!(t2.link(0, 0, inst, 1));
        assert!(t2.link(inst, 0, 1, 0));
        let out = evaluate(&t2, &cube(1.0)).unwrap();
        assert_eq!(out.face_count(), 9 * 6, "one cube per grid vertex");
    }

    #[test]
    fn set_position_and_boolean() {
        let mut t = GeoNodeTree::default_tree();
        let sp = t.add_node(GeoNodeKind::SetPosition { offset: Vec3::new(0.0, 3.0, 0.0) }, [0.0; 2]);
        assert!(t.link(0, 0, sp, 0));
        assert!(t.link(sp, 0, 1, 0));
        let out = evaluate(&t, &cube(1.0)).unwrap();
        assert!(out.positions().all(|p| p.y > 2.0));

        let mut t = GeoNodeTree::default_tree();
        let b = t.add_node(GeoNodeKind::MeshBoolean { op: BooleanOp::Union }, [0.0; 2]);
        let s = t.add_node(GeoNodeKind::MeshUvSphere { radius: 0.3, segments: 8, rings: 4 }, [0.0; 2]);
        assert!(t.link(0, 0, b, 0));
        assert!(t.link(s, 0, b, 1));
        assert!(t.link(b, 0, 1, 0));
        let out = evaluate(&t, &cube(1.0)).unwrap();
        assert!(out.face_count() >= 6);
    }
}
