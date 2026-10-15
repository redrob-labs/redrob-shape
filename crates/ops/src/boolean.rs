//! Boolean (union / difference / intersect) on two meshes through BSP clipping of their triangle
//! soups, and Shrinkwrap (nearest point on another mesh's surface). The BSP approach is the
//! classic one: each solid's polygons are split by the other's planes, the parts inside/outside
//! are kept per operation, and the surviving polygons are stitched back by position.

use crate::{PolyMesh, Result};
use shape_geom::Mesh;
use shape_math::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BooleanOp {
    Union,
    Difference,
    Intersect,
}

#[derive(Clone, Debug)]
struct Poly {
    verts: Vec<Vec3>,
    normal: Vec3,
    w: f32,
}

const EPS: f32 = 1e-5;

impl Poly {
    fn new(verts: Vec<Vec3>) -> Option<Self> {
        if verts.len() < 3 {
            return None;
        }
        let mut n = Vec3::ZERO;
        for i in 0..verts.len() {
            let a = verts[i];
            let b = verts[(i + 1) % verts.len()];
            n += Vec3::new((a.y - b.y) * (a.z + b.z), (a.z - b.z) * (a.x + b.x), (a.x - b.x) * (a.y + b.y));
        }
        if n.length_squared() < 1e-14 {
            return None;
        }
        let normal = n.normalize();
        let w = normal.dot(verts[0]);
        Some(Self { verts, normal, w })
    }

    fn flip(&mut self) {
        self.verts.reverse();
        self.normal = -self.normal;
        self.w = -self.w;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Coplanar = 0,
    Front = 1,
    Back = 2,
    Spanning = 3,
}

/// Split `poly` by the plane (normal, w) into the four buckets.
fn split(normal: Vec3, w: f32, poly: &Poly, cf: &mut Vec<Poly>, cb: &mut Vec<Poly>, front: &mut Vec<Poly>, back: &mut Vec<Poly>) {
    let mut kind = 0u8;
    let mut sides = Vec::with_capacity(poly.verts.len());
    for v in &poly.verts {
        let t = normal.dot(*v) - w;
        let s = if t < -EPS {
            Side::Back
        } else if t > EPS {
            Side::Front
        } else {
            Side::Coplanar
        };
        kind |= s as u8;
        sides.push(s);
    }
    match kind {
        0 => {
            if normal.dot(poly.normal) > 0.0 {
                cf.push(poly.clone());
            } else {
                cb.push(poly.clone());
            }
        }
        1 => front.push(poly.clone()),
        2 => back.push(poly.clone()),
        _ => {
            let mut f = Vec::new();
            let mut b = Vec::new();
            let n = poly.verts.len();
            for i in 0..n {
                let j = (i + 1) % n;
                let (si, sj) = (sides[i], sides[j]);
                let (vi, vj) = (poly.verts[i], poly.verts[j]);
                if si != Side::Back {
                    f.push(vi);
                }
                if si != Side::Front {
                    b.push(vi);
                }
                if (si as u8 | sj as u8) == Side::Spanning as u8 {
                    let t = (w - normal.dot(vi)) / normal.dot(vj - vi);
                    let v = vi + (vj - vi) * t;
                    f.push(v);
                    b.push(v);
                }
            }
            if let Some(p) = Poly::new(f) {
                front.push(p);
            }
            if let Some(p) = Poly::new(b) {
                back.push(p);
            }
        }
    }
}

#[derive(Default)]
struct Node {
    plane: Option<(Vec3, f32)>,
    front: Option<Box<Node>>,
    back: Option<Box<Node>>,
    polys: Vec<Poly>,
}

impl Node {
    fn new(polys: Vec<Poly>) -> Self {
        let mut n = Node::default();
        n.build(polys);
        n
    }

    fn build(&mut self, polys: Vec<Poly>) {
        if polys.is_empty() {
            return;
        }
        if self.plane.is_none() {
            self.plane = Some((polys[0].normal, polys[0].w));
        }
        let (normal, w) = self.plane.unwrap();
        let mut front = Vec::new();
        let mut back = Vec::new();
        for p in &polys {
            let mut cf = Vec::new();
            let mut cb = Vec::new();
            split(normal, w, p, &mut cf, &mut cb, &mut front, &mut back);
            self.polys.extend(cf);
            self.polys.extend(cb);
        }
        if !front.is_empty() {
            self.front.get_or_insert_with(Default::default).build(front);
        }
        if !back.is_empty() {
            self.back.get_or_insert_with(Default::default).build(back);
        }
    }

    fn invert(&mut self) {
        for p in &mut self.polys {
            p.flip();
        }
        if let Some((n, w)) = &mut self.plane {
            *n = -*n;
            *w = -*w;
        }
        if let Some(f) = &mut self.front {
            f.invert();
        }
        if let Some(b) = &mut self.back {
            b.invert();
        }
        std::mem::swap(&mut self.front, &mut self.back);
    }

    /// Remove the parts of `polys` inside this solid.
    fn clip_polys(&self, polys: Vec<Poly>) -> Vec<Poly> {
        let Some((normal, w)) = self.plane else { return polys };
        let mut front = Vec::new();
        let mut back = Vec::new();
        for p in &polys {
            let (mut cf, mut cb) = (Vec::new(), Vec::new());
            split(normal, w, p, &mut cf, &mut cb, &mut front, &mut back);
            front.extend(cf);
            back.extend(cb);
        }
        let mut front = match &self.front {
            Some(f) => f.clip_polys(front),
            None => front,
        };
        let back = match &self.back {
            Some(b) => b.clip_polys(back),
            None => Vec::new(),
        };
        front.extend(back);
        front
    }

    fn clip_to(&mut self, other: &Node) {
        self.polys = other.clip_polys(std::mem::take(&mut self.polys));
        if let Some(f) = &mut self.front {
            f.clip_to(other);
        }
        if let Some(b) = &mut self.back {
            b.clip_to(other);
        }
    }

    fn all_polys(&self) -> Vec<Poly> {
        let mut out = self.polys.clone();
        if let Some(f) = &self.front {
            out.extend(f.all_polys());
        }
        if let Some(b) = &self.back {
            out.extend(b.all_polys());
        }
        out
    }
}

fn polys_of(mesh: &Mesh, transform: &shape_math::Transform) -> Vec<Poly> {
    mesh.face_ids().filter_map(|f| Poly::new(mesh.face_vertices(f).map(|v| transform.transform_point(mesh.position(v))).collect())).collect()
}

/// Boolean of `a` (in its own space) with `b` placed by `b_to_a` (b's space into a's). Both
/// should be closed; the result is stitched by position and may carry T-junctions where the two
/// surfaces met, which show as open edges rather than errors.
pub fn boolean(a: &Mesh, b: &Mesh, b_to_a: &shape_math::Transform, op: BooleanOp) -> Result<Mesh> {
    let mut na = Node::new(polys_of(a, &shape_math::Transform::IDENTITY));
    let mut nb = Node::new(polys_of(b, b_to_a));
    match op {
        BooleanOp::Union => {
            na.clip_to(&nb);
            nb.clip_to(&na);
            nb.invert();
            nb.clip_to(&na);
            nb.invert();
            na.build(nb.all_polys());
        }
        BooleanOp::Difference => {
            na.invert();
            na.clip_to(&nb);
            nb.clip_to(&na);
            nb.invert();
            nb.clip_to(&na);
            nb.invert();
            na.build(nb.all_polys());
            na.invert();
        }
        BooleanOp::Intersect => {
            na.invert();
            nb.clip_to(&na);
            nb.invert();
            na.clip_to(&nb);
            nb.clip_to(&na);
            na.build(nb.all_polys());
            na.invert();
        }
    }
    let polys = na.all_polys();
    let mut pm = PolyMesh::default();
    for p in polys {
        let idx: Vec<u32> = p.verts.iter().map(|v| pm.push_vertex(*v)).collect();
        pm.polygons.push(idx);
    }
    pm.merge_by_distance(1e-4);
    // Drop exact duplicate polygons the clipping can leave on coplanar faces.
    let mut seen = std::collections::HashSet::new();
    pm.polygons.retain(|poly| {
        let mut key = poly.clone();
        key.sort_unstable();
        seen.insert(key)
    });
    match pm.clone().build() {
        Ok(m) => Ok(m),
        Err(_) => {
            // Coplanar leftovers can still pair up the same directed edge twice: fall back to a
            // triangle soup with duplicated vertices, which always builds (as open triangles).
            let mut soup = PolyMesh::default();
            for poly in &pm.polygons {
                for i in 1..poly.len() - 1 {
                    let tri = [poly[0], poly[i], poly[i + 1]];
                    let idx: Vec<u32> = tri.iter().map(|&v| soup.push_vertex(pm.positions[v as usize])).collect();
                    soup.polygons.push(idx);
                }
            }
            soup.build()
        }
    }
}

/// Shrinkwrap: move every vertex to the nearest point on `target`'s surface (target placed by
/// `target_to_self`), then `offset` along the direction it came from.
pub fn shrinkwrap(mesh: &Mesh, target: &Mesh, target_to_self: &shape_math::Transform, offset: f32) -> Result<Mesh> {
    let tris: Vec<[Vec3; 3]> = target.triangles().iter().map(|t| [0, 1, 2].map(|i| target_to_self.transform_point(target.position(t[i])))).collect();
    if tris.is_empty() {
        return Ok(mesh.clone());
    }
    let mut pm = PolyMesh::from_mesh(mesh);
    for p in &mut pm.positions {
        let mut best = (f32::INFINITY, *p);
        for t in &tris {
            let q = closest_point_on_triangle(*p, t);
            let d = (q - *p).length_squared();
            if d < best.0 {
                best = (d, q);
            }
        }
        let dir = (*p - best.1).normalize_or_zero();
        *p = best.1 + dir * offset;
    }
    pm.build()
}

/// Closest point on a triangle to `p` (Ericson, Real-Time Collision Detection 5.1.5).
fn closest_point_on_triangle(p: Vec3, t: &[Vec3; 3]) -> Vec3 {
    let [a, b, c] = *t;
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return a + ab * v;
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return a + ac * w;
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return b + (c - b) * w;
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    a + ab * v + ac * w
}

#[cfg(test)]
mod tests {
    use super::*;
    use shape_geom::primitives;
    use shape_math::Transform;

    fn volume(mesh: &Mesh) -> f32 {
        // Signed volume from the triangle fan of every face.
        let mut v = 0.0;
        for f in mesh.face_ids() {
            let vs: Vec<Vec3> = mesh.face_vertices(f).map(|x| mesh.position(x)).collect();
            for i in 1..vs.len() - 1 {
                v += vs[0].dot(vs[i].cross(vs[i + 1])) / 6.0;
            }
        }
        v
    }

    #[test]
    fn booleans_of_two_offset_cubes_have_the_right_volumes() {
        let a = primitives::cube(2.0);
        let b = primitives::cube(2.0);
        let shift = Transform::from_translation(Vec3::new(1.0, 1.0, 1.0));
        // Overlap is a unit cube: union 8+8-1 = 15, difference 7, intersect 1.
        for (op, want) in [(BooleanOp::Union, 15.0), (BooleanOp::Difference, 7.0), (BooleanOp::Intersect, 1.0)] {
            let out = boolean(&a, &b, &shift, op).unwrap();
            let vol = volume(&out);
            assert!((vol - want).abs() < 0.05, "{op:?}: volume {vol}, want {want}");
            for f in out.face_ids() {
                assert!(out.face_sides(f) >= 3);
            }
        }
    }

    #[test]
    fn shrinkwrap_puts_a_cube_onto_a_sphere() {
        let cube = primitives::cube(1.0);
        let sphere = primitives::uv_sphere(2.0, 32, 16);
        let out = shrinkwrap(&cube, &sphere, &Transform::IDENTITY, 0.0).unwrap();
        for p in out.positions() {
            assert!((p.length() - 2.0).abs() < 0.05, "{p} not on the sphere");
        }
        let lifted = shrinkwrap(&cube, &sphere, &Transform::IDENTITY, 0.1).unwrap();
        for p in lifted.positions() {
            assert!((p.length() - 1.9).abs() < 0.06, "{p}");
        }
    }
}
