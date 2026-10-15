//! Evaluate a scene object's modifier stack.

use crate::Result;
use shape_geom::Mesh;
use shape_scene::{Modifier, ObjectData, ObjectId, Scene};
use std::borrow::Cow;

/// Apply one modifier that needs no other object. Boolean and Shrinkwrap come back unchanged
/// here; [`apply_in`] resolves their target.
pub fn apply(modifier: &Modifier, mesh: &Mesh) -> Result<Mesh> {
    apply_in(None, modifier, mesh)
}

/// Apply one modifier on behalf of object `id` in `scene`, so Boolean and Shrinkwrap can read
/// their target's base mesh and placement.
pub fn apply_in(context: Option<(&Scene, ObjectId)>, modifier: &Modifier, mesh: &Mesh) -> Result<Mesh> {
    match modifier {
        Modifier::Boolean { target: None, .. } | Modifier::Shrinkwrap { target: None, .. } | Modifier::Armature { object: None } => Ok(mesh.clone()),
        Modifier::Boolean { .. } | Modifier::Shrinkwrap { .. } | Modifier::Armature { .. } if context.is_none() => Ok(mesh.clone()),
        Modifier::Armature { object: Some(arm_id) } => {
            let (scene, id) = context.expect("checked above");
            let Some(shape_scene::ObjectData::Armature(arm)) = scene.object(*arm_id).map(|o| &o.data) else { return Ok(mesh.clone()) };
            let Some(shape_scene::ObjectData::Mesh { mesh: mesh_id, .. }) = scene.object(id).map(|o| &o.data) else { return Ok(mesh.clone()) };
            let Some(data) = scene.mesh(*mesh_id) else { return Ok(mesh.clone()) };
            // Weights are per base-mesh vertex: skinning only applies while the vertex count matches.
            if data.mesh.vertex_count() != mesh.vertex_count() {
                return Ok(mesh.clone());
            }
            let to_arm = scene.world_transform(*arm_id).inverse().mul_transform(&scene.world_transform(id));
            Ok(shape_scene::armature::skin(mesh, arm, &data.vertex_groups, &to_arm))
        }
        Modifier::Boolean { op, target: Some(target) } => {
            let (scene, id) = context.expect("checked above");
            let Some((_, other)) = scene.object_mesh(*target).filter(|_| *target != id) else { return Ok(mesh.clone()) };
            let to_self = scene.world_transform(id).inverse().mul_transform(&scene.world_transform(*target));
            crate::boolean(mesh, other, &to_self, bool_op(*op))
        }
        Modifier::Shrinkwrap { target: Some(target), offset } => {
            let (scene, id) = context.expect("checked above");
            let Some((_, other)) = scene.object_mesh(*target).filter(|_| *target != id) else { return Ok(mesh.clone()) };
            let to_self = scene.world_transform(id).inverse().mul_transform(&scene.world_transform(*target));
            crate::shrinkwrap(mesh, other, &to_self, *offset)
        }
        Modifier::Mirror { axis, merge_distance } => crate::mirror(mesh, (*axis).min(2) as usize, *merge_distance),
        Modifier::Array { count, offset, merge_distance } => crate::array(mesh, *count, *offset, *merge_distance),
        Modifier::Subdivision { levels, smooth } => {
            let mut m = mesh.clone();
            for _ in 0..(*levels).min(4) {
                m = if *smooth { crate::catmull_clark(&m)? } else { crate::subdivide_linear(&m)? };
            }
            Ok(m)
        }
        Modifier::Smooth { iterations, factor } => crate::smooth(mesh, *iterations, *factor),
        Modifier::Bevel { offset, segments } => {
            let edges: Vec<_> = mesh.edge_ids().collect();
            crate::bevel_edges_segments(mesh, &edges, *offset, *segments)
        }
        Modifier::Solidify { thickness, offset } => crate::solidify(mesh, *thickness, *offset),
        Modifier::Decimate { ratio } => crate::decimate(mesh, *ratio),
        Modifier::Wireframe { thickness } => crate::wireframe(mesh, *thickness),
        Modifier::Triangulate => crate::triangulate(mesh),
        Modifier::Screw { axis, angle, steps, screw } => crate::screw(mesh, axis_vec(*axis), *angle, *steps, *screw),
        Modifier::SimpleDeform { mode, axis, factor } => crate::simple_deform(mesh, deform_mode(*mode), (*axis).min(2) as usize, *factor),
        Modifier::Displace { strength, scale } => crate::displace(mesh, *strength, *scale),
        Modifier::Weld { distance } => crate::weld(mesh, *distance),
    }
}

fn bool_op(op: shape_scene::BooleanOp) -> crate::BooleanOp {
    match op {
        shape_scene::BooleanOp::Union => crate::BooleanOp::Union,
        shape_scene::BooleanOp::Difference => crate::BooleanOp::Difference,
        shape_scene::BooleanOp::Intersect => crate::BooleanOp::Intersect,
    }
}

fn axis_vec(axis: u8) -> shape_math::Vec3 {
    match axis {
        0 => shape_math::Vec3::X,
        2 => shape_math::Vec3::Z,
        _ => shape_math::Vec3::Y,
    }
}

fn deform_mode(m: shape_scene::DeformMode) -> crate::DeformMode {
    match m {
        shape_scene::DeformMode::Twist => crate::DeformMode::Twist,
        shape_scene::DeformMode::Bend => crate::DeformMode::Bend,
        shape_scene::DeformMode::Taper => crate::DeformMode::Taper,
        shape_scene::DeformMode::Stretch => crate::DeformMode::Stretch,
    }
}

/// Apply a whole stack in order. A modifier that fails is skipped, so one bad setting never hides
/// the object.
pub fn evaluate<'a>(mesh: &'a Mesh, stack: &[Modifier]) -> Cow<'a, Mesh> {
    evaluate_in(None, mesh, stack)
}

/// [`evaluate`] with the scene at hand, so target modifiers resolve.
pub fn evaluate_in<'a>(context: Option<(&Scene, ObjectId)>, mesh: &'a Mesh, stack: &[Modifier]) -> Cow<'a, Mesh> {
    let mut current: Cow<Mesh> = Cow::Borrowed(mesh);
    for m in stack.iter().filter(|m| m.enabled()) {
        if let Ok(next) = apply_in(context, m, &current) {
            current = Cow::Owned(next);
        }
    }
    current
}

/// The mesh an object shows: its data with modifiers applied. `None` for non-mesh objects.
pub fn evaluated_mesh(scene: &Scene, id: ObjectId) -> Option<Cow<'_, Mesh>> {
    let object = scene.object(id)?;
    let ObjectData::Mesh { mesh, modifiers, .. } = &object.data else { return None };
    let data = scene.mesh(*mesh)?;
    Some(evaluate_in(Some((scene, id)), &data.mesh, modifiers))
}

/// The base (unmodified) mesh of a mesh object.
pub fn scene_mesh(scene: &Scene, id: ObjectId) -> Option<&Mesh> {
    scene.object_mesh(id).map(|(_, m)| m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::cube;
    use shape_math::Vec3;

    #[test]
    fn stack_applies_in_order_and_skips_disabled() {
        let mut scene = Scene::new("t");
        let id = scene.add_mesh_object("c", cube(1.0), None);
        if let ObjectData::Mesh { modifiers, .. } = &mut scene.object_mut(id).unwrap().data {
            modifiers.push(Modifier::Array { count: 2, offset: Vec3::new(2.0, 0.0, 0.0), merge_distance: 0.0 });
            modifiers.push(Modifier::Subdivision { levels: 1, smooth: true });
            modifiers.push(Modifier::Smooth { iterations: 0, factor: 0.5 });
        }
        let m = evaluated_mesh(&scene, id).unwrap();
        assert_eq!(m.face_count(), 2 * 24);
        assert_eq!(scene_mesh(&scene, id).unwrap().face_count(), 6);
    }

    #[test]
    fn borrowed_when_no_modifiers() {
        let c = cube(1.0);
        assert!(matches!(evaluate(&c, &[]), Cow::Borrowed(_)));
    }
}

#[cfg(test)]
mod stack_tests {
    use super::*;
    use shape_geom::primitives;

    #[test]
    fn every_modifier_kind_evaluates_on_a_cube() {
        let cube = primitives::cube(1.0);
        let kinds = [
            Modifier::Bevel { offset: 0.1, segments: 3 },
            Modifier::Solidify { thickness: 0.1, offset: -1.0 },
            Modifier::Decimate { ratio: 0.5 },
            Modifier::Wireframe { thickness: 0.05 },
            Modifier::Triangulate,
            Modifier::Screw { axis: 1, angle: 1.0, steps: 4, screw: 0.5 },
            Modifier::SimpleDeform { mode: shape_scene::DeformMode::Twist, axis: 1, factor: 0.5 },
            Modifier::Displace { strength: 0.05, scale: 2.0 },
            Modifier::Weld { distance: 0.001 },
        ];
        for m in kinds {
            let out = apply(&m, &cube).unwrap_or_else(|e| panic!("{}: {e}", m.name()));
            if m == (Modifier::Screw { axis: 1, angle: 1.0, steps: 4, screw: 0.5 }) {
                assert_eq!(out.face_count(), 6, "a closed cube has no profile to screw");
                // An open profile does screw.
                let plane = primitives::plane(1.0, 1, 1);
                let s = apply(&m, &plane).unwrap();
                assert_eq!(s.face_count(), 1 + 4 * 4);
            }
            out.check().unwrap_or_else(|e| panic!("{}: {e}", m.name()));
            assert!(out.face_count() > 0, "{}", m.name());
        }
        // A stack: bevel then solidify stays closed.
        let stack = [Modifier::Bevel { offset: 0.1, segments: 2 }, Modifier::Solidify { thickness: 0.05, offset: -1.0 }];
        let out = evaluate(&cube, &stack);
        assert!(out.is_closed());
    }
}

#[cfg(test)]
mod target_tests {
    use super::*;
    use shape_geom::primitives;
    use shape_math::Transform;

    #[test]
    fn boolean_and_shrinkwrap_resolve_their_target_in_the_scene() {
        let mut scene = Scene::new("t");
        let a = scene.add_mesh_object("A", primitives::cube(2.0), None);
        let b = scene.add_mesh_object("B", primitives::cube(2.0), None);
        scene.object_mut(b).unwrap().transform = Transform::from_translation(shape_math::Vec3::new(1.0, 1.0, 1.0));
        scene.modifiers_mut(a).unwrap().push(Modifier::Boolean { op: shape_scene::BooleanOp::Difference, target: Some(b) });
        let out = evaluated_mesh(&scene, a).unwrap();
        assert!(out.face_count() > 6, "the corner was cut out: {} faces", out.face_count());
        // Without a scene the modifier is a no-op.
        let base = primitives::cube(2.0);
        let plain = evaluate(&base, scene.modifiers(a));
        assert_eq!(plain.face_count(), 6);
        // Shrinkwrap a small cube onto the big one.
        let c = scene.add_mesh_object("C", primitives::cube(0.5), None);
        scene.modifiers_mut(c).unwrap().push(Modifier::Shrinkwrap { target: Some(b), offset: 0.0 });
        let wrapped = evaluated_mesh(&scene, c).unwrap();
        for p in wrapped.positions() {
            // On B's surface: some coordinate equals one of B's faces (B spans 0..2 on each axis).
            let on = [p.x, p.y, p.z].iter().any(|&v| (v - 0.0).abs() < 1e-3 || (v - 2.0).abs() < 1e-3);
            assert!(on, "{p}");
        }
    }
}
