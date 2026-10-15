//! Flat triangle BVH for the whole scene in world space.

use shape_math::{Aabb, Ray, Vec3};

#[derive(Clone, Copy, Debug)]
pub struct Tri {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
    pub normal: Vec3,
    pub material: u32,
    /// Texture coordinates at a, b, c (zero when the mesh has none).
    pub uv: [shape_math::Vec2; 3],
}

impl Tri {
    /// Barycentric interpolation of the corner UVs at point `p` on the triangle.
    pub fn uv_at(&self, p: Vec3) -> shape_math::Vec2 {
        let v0 = self.b - self.a;
        let v1 = self.c - self.a;
        let v2 = p - self.a;
        let d00 = v0.dot(v0);
        let d01 = v0.dot(v1);
        let d11 = v1.dot(v1);
        let d20 = v2.dot(v0);
        let d21 = v2.dot(v1);
        let denom = (d00 * d11 - d01 * d01).max(1e-12);
        let v = (d11 * d20 - d01 * d21) / denom;
        let w = (d00 * d21 - d01 * d20) / denom;
        let u = 1.0 - v - w;
        self.uv[0] * u + self.uv[1] * v + self.uv[2] * w
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Hit {
    pub t: f32,
    pub tri: usize,
}

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

pub struct Bvh {
    nodes: Vec<Node>,
    order: Vec<usize>,
    pub tris: Vec<Tri>,
}

impl Bvh {
    const LEAF: usize = 4;

    pub fn build(tris: Vec<Tri>) -> Self {
        let centroids: Vec<Vec3> = tris.iter().map(|t| (t.a + t.b + t.c) / 3.0).collect();
        let mut order: Vec<usize> = (0..tris.len()).collect();
        let mut bvh = Bvh { nodes: Vec::new(), order: Vec::new(), tris };
        if !bvh.tris.is_empty() {
            bvh.node(&mut order, 0, bvh.tris.len(), &centroids);
        }
        bvh.order = order;
        bvh
    }

    fn node(&mut self, order: &mut [usize], first: usize, count: usize, centroids: &[Vec3]) -> usize {
        let aabb = order[first..first + count].iter().fold(Aabb::EMPTY, |b, &i| {
            let t = &self.tris[i];
            b.with_point(t.a).with_point(t.b).with_point(t.c)
        });
        let id = self.nodes.len();
        if count <= Self::LEAF {
            self.nodes.push(Node::Leaf { aabb, first, count });
            return id;
        }
        let cbox = order[first..first + count].iter().fold(Aabb::EMPTY, |b, &i| b.with_point(centroids[i]));
        let axis = cbox.longest_axis();
        order[first..first + count].sort_by(|&a, &b| centroids[a][axis].total_cmp(&centroids[b][axis]));
        let mid = count / 2;
        self.nodes.push(Node::Inner { aabb, left: 0, right: 0 });
        let left = self.node(order, first, mid, centroids);
        let right = self.node(order, first + mid, count - mid, centroids);
        self.nodes[id] = Node::Inner { aabb, left, right };
        id
    }

    /// Nearest hit with `t` in `(t_min, t_max)`.
    pub fn intersect(&self, ray: &Ray, t_min: f32, t_max: f32) -> Option<Hit> {
        if self.nodes.is_empty() {
            return None;
        }
        let mut best: Option<Hit> = None;
        let mut limit = t_max;
        let mut stack = [0usize; 64];
        let mut sp = 1;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp]];
            let Some((entry, _)) = ray.intersect_aabb(node.aabb()) else { continue };
            if entry > limit {
                continue;
            }
            match node {
                Node::Leaf { first, count, .. } => {
                    for &i in &self.order[*first..*first + *count] {
                        let t = &self.tris[i];
                        if let Some((d, _, _)) = ray.intersect_triangle(t.a, t.b, t.c)
                            && d > t_min
                            && d < limit
                        {
                            limit = d;
                            best = Some(Hit { t: d, tri: i });
                        }
                    }
                }
                Node::Inner { left, right, .. } => {
                    if sp + 2 <= stack.len() {
                        stack[sp] = *right;
                        stack[sp + 1] = *left;
                        sp += 2;
                    }
                }
            }
        }
        best
    }

    /// Any hit closer than `t_max` (shadow rays).
    pub fn occluded(&self, ray: &Ray, t_min: f32, t_max: f32) -> bool {
        self.intersect(ray, t_min, t_max).is_some()
    }
}
