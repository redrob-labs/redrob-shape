//! Evaluate a scene object's modifier stack.

use crate::Result;
use shape_geom::Mesh;
use shape_scene::{Modifier, ObjectData, ObjectId, Scene};
use std::borrow::Cow;

/// Apply one modifier.
pub fn apply(modifier: &Modifier, mesh: &Mesh) -> Result<Mesh> {
    match modifier {
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
    }
}

/// Apply a whole stack in order. A modifier that fails is skipped, so one bad setting never hides
/// the object.
pub fn evaluate<'a>(mesh: &'a Mesh, stack: &[Modifier]) -> Cow<'a, Mesh> {
    let mut current: Cow<Mesh> = Cow::Borrowed(mesh);
    for m in stack.iter().filter(|m| m.enabled()) {
        if let Ok(next) = apply(m, &current) {
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
    Some(evaluate(&data.mesh, modifiers))
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
