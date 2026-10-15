//! Text objects, after Blender's: a string set in a font becomes filled glyph outlines lying in
//! the ground plane (Blender's XY = our XZ), optionally extruded to a solid. Outlines come from
//! the font's TrueType tables, curves are flattened, and each glyph's contours are triangulated
//! with ear clipping (holes included).

use std::collections::HashMap;

use shape_geom::Mesh;
use shape_math::Vec3;

/// The bundled font (Ubuntu Light, UFL), the one the editor's own UI uses.
pub fn default_font() -> &'static [u8] {
    epaint_default_fonts::UBUNTU_LIGHT
}

#[derive(Default)]
struct Outline {
    contours: Vec<Vec<[f32; 2]>>,
    cur: Vec<[f32; 2]>,
}

impl Outline {
    fn last(&self) -> [f32; 2] {
        self.cur.last().copied().unwrap_or([0.0, 0.0])
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.cur.push([x, y]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.cur.push([x, y]);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let p0 = self.last();
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let u = 1.0 - t;
            self.cur.push([u * u * p0[0] + 2.0 * u * t * x1 + t * t * x, u * u * p0[1] + 2.0 * u * t * y1 + t * t * y]);
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let p0 = self.last();
        for i in 1..=8 {
            let t = i as f32 / 8.0;
            let u = 1.0 - t;
            let (a, b, c, d) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            self.cur.push([a * p0[0] + b * x1 + c * x2 + d * x, a * p0[1] + b * y1 + c * y2 + d * y]);
        }
    }
    fn close(&mut self) {
        if self.cur.len() >= 3 {
            // Drop a duplicated closing point.
            if self.cur.first() == self.cur.last() {
                self.cur.pop();
            }
            if self.cur.len() >= 3 {
                self.contours.push(std::mem::take(&mut self.cur));
                return;
            }
        }
        self.cur.clear();
    }
}

fn signed_area(c: &[[f32; 2]]) -> f32 {
    let n = c.len();
    (0..n)
        .map(|i| {
            let (a, b) = (c[i], c[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        * 0.5
}

fn point_in(c: &[[f32; 2]], p: [f32; 2]) -> bool {
    let n = c.len();
    let mut inside = false;
    for i in 0..n {
        let (a, b) = (c[i], c[(i + 1) % n]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                inside = !inside;
            }
        }
    }
    inside
}

/// One glyph's filled polygons: (outer contour, holes), all in font units scaled by `scale`
/// and shifted by `offset`.
fn glyph_polygons(face: &ttf_parser::Face, ch: char, scale: f32, offset: f32) -> Vec<(Vec<[f32; 2]>, Vec<Vec<[f32; 2]>>)> {
    let Some(gid) = face.glyph_index(ch) else { return Vec::new() };
    let mut outline = Outline::default();
    if face.outline_glyph(gid, &mut outline).is_none() {
        return Vec::new();
    }
    ttf_parser::OutlineBuilder::close(&mut outline);
    let mut contours: Vec<(Vec<[f32; 2]>, f32)> = outline
        .contours
        .into_iter()
        .map(|c| {
            let c: Vec<[f32; 2]> = c.into_iter().map(|p| [p[0] * scale + offset, p[1] * scale]).collect();
            let a = signed_area(&c);
            (c, a)
        })
        .filter(|(_, a)| a.abs() > 1e-9)
        .collect();
    contours.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap_or(std::cmp::Ordering::Equal));
    let Some(outer_sign) = contours.first().map(|(_, a)| a.signum()) else { return Vec::new() };
    let mut polys: Vec<(Vec<[f32; 2]>, Vec<Vec<[f32; 2]>>)> = Vec::new();
    for (c, a) in contours {
        if a.signum() == outer_sign {
            polys.push((c, Vec::new()));
        } else {
            // A hole belongs to the outer contour that contains it.
            let p = c[0];
            if let Some(poly) = polys.iter_mut().find(|(outer, _)| point_in(outer, p)) {
                poly.1.push(c);
            }
        }
    }
    polys
}

/// Build the mesh for `body` at cap height `size` (world units per em), extruded `extrude` both
/// ways along +Y / -Y (Blender's Extrude), lying in the XZ plane with the baseline on X.
pub fn text_mesh(body: &str, size: f32, extrude: f32) -> Mesh {
    let Ok(face) = ttf_parser::Face::parse(default_font(), 0) else { return empty() };
    let scale = size / face.units_per_em() as f32;
    let mut positions: Vec<Vec3> = Vec::new();
    let mut faces: Vec<Vec<u32>> = Vec::new();
    let mut pen = 0.0f32;
    let mut earcut = earcut::Earcut::new();
    // Glyph (x, y) with y up -> world (x, 0, -y): Blender's text lies in its XY plane.
    let to_world = |p: [f32; 2], y: f32| Vec3::new(p[0], y, -p[1]);
    for (line_no, line) in body.split('\n').enumerate() {
        pen = 0.0;
        let line_y = -(line_no as f32) * size * 1.2;
        for ch in line.chars() {
            if ch == ' ' {
                pen += face.glyph_index(' ').and_then(|g| face.glyph_hor_advance(g)).unwrap_or(500) as f32 * scale;
                continue;
            }
            for (outer, holes) in glyph_polygons(&face, ch, scale, pen) {
                let shift = |p: [f32; 2]| [p[0], p[1] + line_y];
                let mut ring: Vec<[f32; 2]> = outer.iter().map(|p| shift(*p)).collect();
                let mut hole_starts: Vec<u32> = Vec::new();
                for h in &holes {
                    hole_starts.push(ring.len() as u32);
                    ring.extend(h.iter().map(|p| shift(*p)));
                }
                let mut tris: Vec<u32> = Vec::new();
                earcut.earcut(ring.iter().copied(), &hole_starts, &mut tris);
                if tris.is_empty() {
                    continue;
                }
                let base_front = positions.len() as u32;
                positions.extend(ring.iter().map(|p| to_world(*p, extrude)));
                // Front cap (+Y): earcut's winding mapped through (x, y) -> (x, 0, -y) already
                // faces up; the back cap is the reverse.
                for t in tris.chunks(3) {
                    faces.push(vec![base_front + t[0], base_front + t[1], base_front + t[2]]);
                }
                if extrude > 0.0 {
                    let base_back = positions.len() as u32;
                    positions.extend(ring.iter().map(|p| to_world(*p, -extrude)));
                    for t in tris.chunks(3) {
                        faces.push(vec![base_back + t[0], base_back + t[2], base_back + t[1]]);
                    }
                    // Side walls along the cap's boundary edges, wound against the front
                    // triangles so every edge has exactly two opposite half-edges.
                    let directed: std::collections::BTreeSet<(u32, u32)> =
                        tris.chunks(3).flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])]).collect();
                    for &(u, v) in &directed {
                        if !directed.contains(&(v, u)) {
                            faces.push(vec![base_front + v, base_front + u, base_back + u, base_back + v]);
                        }
                    }
                }
            }
            pen += face.glyph_index(ch).and_then(|g| face.glyph_hor_advance(g)).unwrap_or(0) as f32 * scale;
        }
    }
    let _ = pen;
    // Triangles sharing a wall edge must share vertices: weld exact duplicates.
    let mut map: HashMap<[i64; 3], u32> = HashMap::new();
    let mut remap = Vec::with_capacity(positions.len());
    let mut welded: Vec<Vec3> = Vec::new();
    for p in &positions {
        let key = [(p.x * 1e5).round() as i64, (p.y * 1e5).round() as i64, (p.z * 1e5).round() as i64];
        let idx = *map.entry(key).or_insert_with(|| {
            welded.push(*p);
            welded.len() as u32 - 1
        });
        remap.push(idx);
    }
    let faces: Vec<Vec<u32>> = faces
        .into_iter()
        .map(|f| {
            let mut out: Vec<u32> = Vec::with_capacity(f.len());
            for i in f {
                let v = remap[i as usize];
                if out.last() != Some(&v) && out.first() != Some(&v) {
                    out.push(v);
                }
            }
            out
        })
        .filter(|f| f.len() >= 3)
        .collect();
    Mesh::from_polygons(welded, &faces).unwrap_or_else(|_| empty())
}

fn empty() -> Mesh {
    Mesh::from_polygons(Vec::new(), &[]).unwrap_or_else(|_| shape_geom::primitives::cube(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn letters_become_faces_and_extrusion_closes_them() {
        let flat = text_mesh("Hi", 1.0, 0.0);
        assert!(flat.face_count() > 10, "triangulated glyphs: {}", flat.face_count());
        // Flat text lies at y = 0 facing up.
        assert!(flat.positions().all(|p| p.y.abs() < 1e-6));
        let n: Vec3 = flat.face_ids().map(|f| flat.face_normal(f)).sum::<Vec3>().normalize();
        assert!(n.y > 0.99, "front faces point +Y: {n}");
        let solid = text_mesh("Hi", 1.0, 0.1);
        assert!(solid.face_count() > flat.face_count() * 2, "solid {} flat {}", solid.face_count(), flat.face_count());
        assert!(solid.is_closed(), "extruded letters are watertight");
        let volume: f32 = solid.face_ids().map(|f| solid.face_centroid(f).dot(solid.face_normal(f)) * solid.face_area(f) / 3.0).sum();
        assert!(volume > 0.0, "faces point outwards: {volume}");
        let b = solid.aabb();
        assert!((b.max.y - 0.1).abs() < 1e-5 && (b.min.y + 0.1).abs() < 1e-5);
        assert!(b.max.x > 0.8, "two letters span the em: {}", b.max.x);
    }

    #[test]
    fn a_letter_with_a_hole_keeps_it() {
        // "O": the inner contour must not be filled, so the face area is well below the outer area.
        let o = text_mesh("O", 1.0, 0.0);
        let area: f32 = o.face_ids().map(|f| o.face_area(f)).sum();
        let b = o.aabb();
        let bbox_area = (b.max.x - b.min.x) * (b.max.z - b.min.z);
        assert!(area < bbox_area * 0.6, "ring area {area} vs bbox {bbox_area}");
        assert!(area > bbox_area * 0.15);
    }
}
