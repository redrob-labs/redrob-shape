//! Armatures, after Blender's: a chain of bones, each a segment from head to tail with a roll
//! about its axis; the rest pose is where the bones were built, the pose is a per-bone local
//! transform on top of it. A mesh follows an armature through the Armature modifier, which skins
//! each vertex by its vertex-group weights; `automatic_weights` fills those groups from bone
//! distance, as Parent > With Automatic Weights does.

use serde::{Deserialize, Serialize};
use shape_geom::Mesh;
use shape_math::{Mat4, Quat, Transform, Vec3};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bone {
    pub name: String,
    /// Rest position in armature space.
    pub head: Vec3,
    pub tail: Vec3,
    /// Rotation about the head-to-tail axis, radians.
    pub roll: f32,
    pub parent: Option<usize>,
    /// Pose transform in the bone's own rest space (identity = rest).
    #[serde(default)]
    pub pose: Transform,
}

impl Bone {
    pub fn new(name: impl Into<String>, head: Vec3, tail: Vec3) -> Self {
        Self { name: name.into(), head, tail, roll: 0.0, parent: None, pose: Transform::IDENTITY }
    }

    pub fn length(&self) -> f32 {
        self.head.distance(self.tail)
    }

    /// The rest matrix: translation to the head, Y along the bone, rolled about it.
    pub fn rest_matrix(&self) -> Mat4 {
        let dir = (self.tail - self.head).normalize_or_zero();
        let dir = if dir.length_squared() < 0.5 { Vec3::Y } else { dir };
        let align = Quat::from_rotation_arc(Vec3::Y, dir);
        let roll = Quat::from_axis_angle(dir, self.roll);
        Mat4::from_rotation_translation(roll * align, self.head)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Armature {
    pub bones: Vec<Bone>,
    /// Bones selected in Pose / Edit mode (indices). Not saved.
    #[serde(skip)]
    pub selected: Vec<usize>,
    #[serde(skip)]
    pub active: Option<usize>,
}

impl Armature {
    /// Blender's default: one bone from the origin, one unit long, pointing up.
    pub fn single_bone() -> Self {
        Self { bones: vec![Bone::new("Bone", Vec3::ZERO, Vec3::Y)], selected: Vec::new(), active: None }
    }

    /// Add a bone as the child of `parent`, starting at the parent's tail.
    pub fn add_child(&mut self, parent: usize, tail: Vec3, name: impl Into<String>) -> usize {
        let head = self.bones.get(parent).map(|b| b.tail).unwrap_or(Vec3::ZERO);
        let mut b = Bone::new(name, head, tail);
        b.parent = Some(parent);
        self.bones.push(b);
        self.bones.len() - 1
    }

    /// Rest matrices in armature space.
    pub fn rest_matrices(&self) -> Vec<Mat4> {
        self.bones.iter().map(Bone::rest_matrix).collect()
    }

    /// Posed matrices in armature space: each bone's pose applies in its rest frame and parents
    /// carry their children along.
    pub fn pose_matrices(&self) -> Vec<Mat4> {
        let rest = self.rest_matrices();
        let mut out = vec![Mat4::IDENTITY; self.bones.len()];
        // Parents have lower indices than their children by construction (add_child), but walk
        // by dependency anyway so hand-built chains are fine.
        let mut done = vec![false; self.bones.len()];
        for _ in 0..self.bones.len() {
            for (i, b) in self.bones.iter().enumerate() {
                if done[i] {
                    continue;
                }
                let local = rest[i] * b.pose.to_matrix();
                match b.parent {
                    None => {
                        out[i] = local;
                        done[i] = true;
                    }
                    Some(p) if done[p] => {
                        out[i] = out[p] * rest[p].inverse() * local;
                        done[i] = true;
                    }
                    Some(_) => {}
                }
            }
        }
        out
    }

    /// Head and tail of every bone in the current pose, armature space.
    pub fn posed_segments(&self) -> Vec<(Vec3, Vec3)> {
        let pose = self.pose_matrices();
        self.bones.iter().zip(&pose).map(|(b, m)| (m.transform_point3(Vec3::ZERO), m.transform_point3(Vec3::Y * b.length()))).collect()
    }

    /// Per-bone skinning matrices: rest space to posed space.
    pub fn skin_matrices(&self) -> Vec<Mat4> {
        self.pose_matrices().iter().zip(self.rest_matrices()).map(|(p, r)| *p * r.inverse()).collect()
    }

    pub fn clear_pose(&mut self) {
        for b in &mut self.bones {
            b.pose = Transform::IDENTITY;
        }
    }
}

/// Weights of one bone over a mesh's vertices (Blender's vertex group, named after the bone).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VertexGroup {
    pub name: String,
    /// (vertex index, weight) pairs, weight in 0..1.
    pub weights: Vec<(u32, f32)>,
}

impl VertexGroup {
    pub fn weight(&self, v: u32) -> f32 {
        self.weights.iter().find(|(i, _)| *i == v).map(|(_, w)| *w).unwrap_or(0.0)
    }
}

fn point_segment_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let t = if ab.length_squared() < 1e-12 { 0.0 } else { ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0) };
    p.distance(a + ab * t)
}

/// Blender's "With Automatic Weights", approximated by distance: every vertex is weighted to the
/// bones by an inverse-square falloff of its distance to each bone segment, normalised; bones
/// below a tenth of the strongest are dropped so joints blend between neighbours only.
/// `mesh_to_armature` maps mesh space into armature space.
pub fn automatic_weights(mesh: &Mesh, armature: &Armature, mesh_to_armature: &Transform) -> Vec<VertexGroup> {
    let mut groups: Vec<VertexGroup> = armature.bones.iter().map(|b| VertexGroup { name: b.name.clone(), weights: Vec::new() }).collect();
    for (vi, p) in mesh.positions().enumerate() {
        let p = mesh_to_armature.transform_point(p);
        let raw: Vec<f32> = armature
            .bones
            .iter()
            .map(|b| {
                let d = point_segment_distance(p, b.head, b.tail).max(1e-3 * b.length().max(1e-3));
                1.0 / (d * d)
            })
            .collect();
        let best = raw.iter().cloned().fold(0.0f32, f32::max);
        let kept: Vec<f32> = raw.iter().map(|&w| if w >= best * 0.1 { w } else { 0.0 }).collect();
        let sum: f32 = kept.iter().sum();
        if sum <= 0.0 {
            continue;
        }
        for (g, w) in groups.iter_mut().zip(kept) {
            if w > 0.0 {
                g.weights.push((vi as u32, w / sum));
            }
        }
    }
    groups
}

/// Deform `mesh` by the armature: each vertex moves by the weighted sum of its bones' skinning
/// matrices. `mesh_to_armature` maps mesh space into armature space (and back).
pub fn skin(mesh: &Mesh, armature: &Armature, groups: &[VertexGroup], mesh_to_armature: &Transform) -> Mesh {
    let skins = armature.skin_matrices();
    let to_arm = mesh_to_armature.to_matrix();
    let from_arm = to_arm.inverse();
    // Vertex -> (bone, weight) lists.
    let n = mesh.vertex_count();
    let mut per_vertex: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];
    for g in groups {
        let Some(bone) = armature.bones.iter().position(|b| b.name == g.name) else { continue };
        for &(v, w) in &g.weights {
            if let Some(list) = per_vertex.get_mut(v as usize) {
                list.push((bone, w));
            }
        }
    }
    let mut out = mesh.clone();
    for (i, v) in mesh.vertex_ids().enumerate() {
        if per_vertex[i].is_empty() {
            continue;
        }
        let p = to_arm.transform_point3(mesh.position(v));
        let total: f32 = per_vertex[i].iter().map(|(_, w)| *w).sum();
        let mut q = Vec3::ZERO;
        for &(bone, w) in &per_vertex[i] {
            q += skins[bone].transform_point3(p) * (w / total.max(1e-9));
        }
        // Unweighted remainder (total < 1) stays put, as in Blender.
        let q = if total < 1.0 { p.lerp(q, total.clamp(0.0, 1.0)) } else { q };
        out.set_position(v, from_arm.transform_point3(q));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives::cylinder;

    fn two_bone_arm() -> Armature {
        let mut a = Armature::single_bone();
        a.add_child(0, Vec3::new(0.0, 2.0, 0.0), "Bone.001");
        a
    }

    #[test]
    fn rest_and_pose_matrices_chain() {
        let mut a = two_bone_arm();
        let segs = a.posed_segments();
        assert!(segs[1].0.distance(Vec3::Y) < 1e-5 && segs[1].1.distance(Vec3::Y * 2.0) < 1e-5);
        // Bend the root 90 degrees about Z: the child swings with it, its head staying on the
        // root's tail.
        a.bones[0].pose.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let segs = a.posed_segments();
        assert!(segs[0].1.distance(Vec3::new(-1.0, 0.0, 0.0)) < 1e-4, "{:?}", segs[0]);
        assert!(segs[1].0.distance(segs[0].1) < 1e-4, "child head follows parent tail");
        assert!(segs[1].1.distance(Vec3::new(-2.0, 0.0, 0.0)) < 1e-4, "{:?}", segs[1]);
    }

    #[test]
    fn automatic_weights_favour_the_nearest_bone_and_sum_to_one() {
        let a = two_bone_arm();
        let m = cylinder(0.3, 2.0, 12);
        // Cylinder spans y = -1..1; shift it to 0..2 to cover both bones.
        let t = Transform::from_translation(Vec3::Y);
        let groups = automatic_weights(&m, &a, &t);
        assert_eq!(groups.len(), 2);
        for (vi, p) in m.positions().enumerate() {
            let w0 = groups[0].weight(vi as u32);
            let w1 = groups[1].weight(vi as u32);
            assert!((w0 + w1 - 1.0).abs() < 1e-4, "sum {}", w0 + w1);
            if p.y < -0.5 {
                assert!(w0 > 0.9, "bottom belongs to the root: {w0}");
            }
            if p.y > 0.5 {
                assert!(w1 > 0.9, "top belongs to the child: {w1}");
            }
        }
    }

    #[test]
    fn skinning_bends_the_top_of_the_cylinder_only() {
        let mut a = two_bone_arm();
        let m = cylinder(0.3, 2.0, 12);
        let t = Transform::from_translation(Vec3::Y);
        let groups = automatic_weights(&m, &a, &t);
        a.bones[1].pose.rotation = Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
        let bent = skin(&m, &a, &groups, &t);
        for (v, p) in m.vertex_ids().zip(m.positions()) {
            let q = bent.position(v);
            if p.y < -0.5 {
                assert!(p.distance(q) < 1e-3, "bottom unchanged: {p} -> {q}");
            }
            if p.y > 0.9 {
                // The top ring (armature y ~ 2) swings to x ~ -1 at armature height 1.
                assert!(q.x < -0.5 && (q.y - 0.0).abs() < 0.5, "top bent: {p} -> {q}");
            }
        }
        a.clear_pose();
        let back = skin(&m, &a, &groups, &t);
        assert!(m.positions().zip(back.positions()).all(|(p, q)| p.distance(q) < 1e-4), "rest pose is identity");
    }
}
