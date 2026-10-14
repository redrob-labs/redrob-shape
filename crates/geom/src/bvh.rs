//! Bounding volume hierarchy over a mesh's triangles, for picking and ray casting.
//!
//! Built once from [`Mesh::triangles`]; rebuild after the mesh changes. Binary tree, median split on
//! the longest axis, leaves hold up to [`Bvh::LEAF_SIZE`] triangles.

use crate::{FaceId, Mesh, VertexId};
use shape_math::{Aabb, Ray, Vec3};

/// One triangle hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    /// Ray parameter; the point is `ray.at(t)`.
    pub t: f32,
    pub face: FaceId,
    /// Index into the triangle list the BVH was built from.
    pub triangle: usize,
    pub barycentric: (f32, f32),
}

#[derive(Clone, Debug)]
enum Node {
    Leaf { aabb: Aabb, first: usize, count: usize },
    Inner { aabb: Aabb, left: usize, right: usize },
}

impl Node {
    fn aabb(&self) -> &Aabb {
        match self {
            Node::Leaf { aabb, .. } | Node::Inner { aabb, .. } => aabb,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Bvh {
    nodes: Vec<Node>,
    /// Triangle indices, permuted so each leaf's triangles are contiguous.
    order: Vec<usize>,
    triangles: Vec<[Vec3; 3]>,
    faces: Vec<FaceId>,
}

impl Bvh {
    pub const LEAF_SIZE: usize = 4;

    pub fn build(mesh: &Mesh) -> Self {
        let tris: Vec<[VertexId; 3]> = mesh.triangles();
        let triangles: Vec<[Vec3; 3]> = tris.iter().map(|t| [mesh.position(t[0]), mesh.position(t[1]), mesh.position(t[2])]).collect();
        let faces = mesh.triangle_faces();
        let centroids: Vec<Vec3> = triangles.iter().map(|t| (t[0] + t[1] + t[2]) / 3.0).collect();
        let mut order: Vec<usize> = (0..triangles.len()).collect();
        let mut bvh = Bvh { nodes: Vec::new(), order: Vec::new(), triangles, faces };
        if !bvh.triangles.is_empty() {
            bvh.build_node(&mut order, 0, bvh.triangles.len(), &centroids);
        }
        bvh.order = order;
        bvh
    }

    fn build_node(&mut self, order: &mut [usize], first: usize, count: usize, centroids: &[Vec3]) -> usize {
        let aabb = order[first..first + count].iter().fold(Aabb::EMPTY, |b, &i| {
            let t = &self.triangles[i];
            b.with_point(t[0]).with_point(t[1]).with_point(t[2])
        });
        let id = self.nodes.len();
        if count <= Self::LEAF_SIZE {
            self.nodes.push(Node::Leaf { aabb, first, count });
            return id;
        }
        let cbox = order[first..first + count].iter().fold(Aabb::EMPTY, |b, &i| b.with_point(centroids[i]));
        let axis = cbox.longest_axis();
        let slice = &mut order[first..first + count];
        slice.sort_by(|&a, &b| centroids[a][axis].total_cmp(&centroids[b][axis]));
        let mid = count / 2;
        self.nodes.push(Node::Inner { aabb, left: 0, right: 0 }); // patched after children exist
        let left = self.build_node(order, first, mid, centroids);
        let right = self.build_node(order, first + mid, count - mid, centroids);
        self.nodes[id] = Node::Inner { aabb, left, right };
        id
    }

    pub fn aabb(&self) -> Aabb {
        self.nodes.first().map(|n| *n.aabb()).unwrap_or(Aabb::EMPTY)
    }

    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }

    /// Nearest hit along the ray, if any.
    pub fn intersect(&self, ray: &Ray) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        if self.nodes.is_empty() {
            return None;
        }
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n];
            let Some((tmin, _)) = ray.intersect_aabb(node.aabb()) else { continue };
            if best.is_some_and(|h| tmin > h.t) {
                continue;
            }
            match node {
                Node::Leaf { first, count, .. } => {
                    for &i in &self.order[*first..*first + *count] {
                        let t = &self.triangles[i];
                        if let Some((d, u, v)) = ray.intersect_triangle(t[0], t[1], t[2])
                            && best.is_none_or(|h| d < h.t)
                        {
                            best = Some(Hit { t: d, face: self.faces[i], triangle: i, barycentric: (u, v) });
                        }
                    }
                }
                Node::Inner { left, right, .. } => {
                    stack.push(*right);
                    stack.push(*left);
                }
            }
        }
        best
    }

    /// Brute-force reference used by tests and as a fallback for tiny meshes.
    pub fn intersect_linear(&self, ray: &Ray) -> Option<Hit> {
        let mut best: Option<Hit> = None;
        for (i, t) in self.triangles.iter().enumerate() {
            if let Some((d, u, v)) = ray.intersect_triangle(t[0], t[1], t[2])
                && best.is_none_or(|h| d < h.t)
            {
                best = Some(Hit { t: d, face: self.faces[i], triangle: i, barycentric: (u, v) });
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitives::{cube, uv_sphere};

    #[test]
    fn ray_hits_cube_front_face() {
        let m = cube(2.0);
        let bvh = Bvh::build(&m);
        assert_eq!(bvh.triangle_count(), 12);
        let hit = bvh.intersect(&Ray::new(Vec3::new(0.1, 0.2, -5.0), Vec3::Z)).unwrap();
        assert!((hit.t - 4.0).abs() < 1e-5);
        assert!((m.face_normal(hit.face) + Vec3::Z).length() < 1e-5, "nearest face is -Z");
        assert!(bvh.intersect(&Ray::new(Vec3::new(5.0, 0.0, -5.0), Vec3::Z)).is_none());
    }

    #[test]
    fn bvh_matches_brute_force_on_sphere() {
        let m = uv_sphere(1.0, 32, 16);
        let bvh = Bvh::build(&m);
        assert!(bvh.nodes.len() > 1);
        let mut hits = 0;
        for i in 0..200 {
            let a = i as f32 * 0.37;
            let b = i as f32 * 0.11;
            let origin = Vec3::new(a.sin() * 3.0, b.cos() * 3.0, a.cos() * 3.0);
            let target = Vec3::new((b * 1.7).sin() * 0.8, (a * 0.9).cos() * 0.8, (b * 0.5).sin() * 0.8);
            let ray = Ray::new(origin, (target - origin).normalize());
            let fast = bvh.intersect(&ray);
            let slow = bvh.intersect_linear(&ray);
            match (fast, slow) {
                (Some(f), Some(s)) => {
                    assert!((f.t - s.t).abs() < 1e-5, "ray {i}: {f:?} vs {s:?}");
                    hits += 1;
                }
                (None, None) => {}
                other => panic!("ray {i}: {other:?}"),
            }
        }
        assert!(hits > 150, "only {hits} rays hit");
    }

    #[test]
    fn empty_mesh_has_empty_bvh() {
        let bvh = Bvh::build(&Mesh::default());
        assert!(bvh.aabb().is_empty());
        assert!(bvh.intersect(&Ray::new(Vec3::ZERO, Vec3::Z)).is_none());
    }
}
