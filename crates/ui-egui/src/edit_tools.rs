//! Edit-mode tools that run while the pointer paints or clicks (stage P3b): C circle select, K
//! knife, plus Bridge Edge Loops, Spin and the Ctrl+E / Ctrl+V / Ctrl+F menus.

use crate::ShapeApp;
use crate::keymap::Popup;
use crate::viewport::ViewportInput;
use egui::{Color32, Stroke};
use shape_math::{Vec2, Vec3};
use shape_ops::EdgePoint;
use shape_scene::SelectMode;

/// Circle select in progress (Blender's C): radius in viewport pixels, wheel changes it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CircleSelect {
    pub radius: f32,
}

/// Knife in progress (Blender's K): the first edge point of the current cut, if placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Knife {
    pub face: Option<shape_geom::FaceId>,
    pub first: Option<EdgePoint>,
    /// Viewport pixel of the first point, for the overlay.
    pub first_px: Option<Vec2>,
}

impl ShapeApp {
    /// Circle select: paint-select under the brush while the left button is down, deselect with
    /// Shift; the wheel sizes the brush; Esc or right click ends. Returns true while active.
    pub(crate) fn circle_select_input(&mut self, input: &ViewportInput, escape: bool) -> bool {
        let Some(c) = &mut self.circle else { return false };
        if input.scroll != 0.0 {
            c.radius = (c.radius * if input.scroll > 0.0 { 1.15 } else { 1.0 / 1.15 }).clamp(4.0, 2000.0);
        }
        if escape || input.secondary_clicked {
            self.circle = None;
            self.status = "Circle select ended".into();
            return true;
        }
        if input.primary_down
            && let Some(p) = input.pointer
        {
            let r = c.radius;
            let size = input.size.max(Vec2::ONE);
            let inside = move |q: Vec2| (q - p).length() <= r;
            if input.shift {
                self.edit_circle_deselect(inside, size);
            } else {
                self.edit_box_select(inside, size, true);
            }
        }
        true
    }

    fn edit_circle_deselect(&mut self, inside: impl Fn(Vec2) -> bool, size: Vec2) {
        let Some(edit) = self.scene.edit.clone() else { return };
        let Some((_, mesh)) = self.scene.object_mesh(edit.object) else { return };
        let mesh = mesh.clone();
        let t = self.scene.world_transform(edit.object);
        let cam = self.viewport.camera.clone();
        let proj = |p: Vec3| cam.project(t.transform_point(p), size).is_some_and(&inside);
        let Some(edit) = &mut self.scene.edit else { return };
        match edit.mode {
            SelectMode::Vertex => edit.verts.retain(|&v| !proj(mesh.position(v))),
            SelectMode::Edge => edit.edges.retain(|&e| {
                let (a, b) = mesh.edge_vertices(e);
                !proj((mesh.position(a) + mesh.position(b)) * 0.5)
            }),
            SelectMode::Face => edit.faces.retain(|&f| !proj(mesh.face_centroid(f))),
        }
        edit.flush(&mesh);
    }

    /// Knife: each click snaps to the nearest edge of the face under the pointer; the second click
    /// on another edge of the same face cuts. Esc or right click ends. Returns true while active.
    pub(crate) fn knife_input(&mut self, input: &ViewportInput, escape: bool) -> bool {
        let Some(k) = self.knife else { return false };
        if escape || input.secondary_clicked {
            self.knife = None;
            self.status = "Knife ended".into();
            return true;
        }
        if !input.primary_clicked {
            return true;
        }
        let Some(p) = input.pointer else { return true };
        let size = input.size.max(Vec2::ONE);
        let Some(hit) = shape_gpu::pick_detail(&self.scene, &self.viewport.camera, p, size) else { return true };
        let Some((mesh_id, mesh)) = self.scene.object_mesh(hit.object) else { return true };
        let mesh = mesh.clone();
        let Some(edge) = shape_gpu::nearest_edge(&mesh, hit.face, hit.local_point) else { return true };
        let (a, b) = mesh.edge_vertices(edge);
        let (pa, pb) = (mesh.position(a), mesh.position(b));
        let t = ((hit.local_point - pa).dot(pb - pa) / (pb - pa).length_squared().max(1e-12)).clamp(0.0, 1.0);
        let point = EdgePoint { edge, t };
        match (k.face, k.first) {
            (Some(face), Some(first)) if face == hit.face && first.edge != edge => {
                match shape_ops::knife_cut(&mesh, face, first, point) {
                    Ok(m) => {
                        self.edit("Knife", |scene| {
                            if let Some(d) = scene.mesh_mut(mesh_id) {
                                d.set_mesh(m);
                            }
                            if let Some(e) = &mut scene.edit {
                                e.clear();
                            }
                        });
                        self.status = "Knife: cut".into();
                    }
                    Err(e) => self.status = format!("Knife failed: {e}"),
                }
                self.knife = Some(Knife { face: None, first: None, first_px: None });
            }
            _ => {
                self.knife = Some(Knife { face: Some(hit.face), first: Some(point), first_px: Some(p) });
                self.status = "Knife: click another edge of the same face".into();
            }
        }
        true
    }

    /// Bridge the selected edges' two loops.
    pub fn bridge_selected(&mut self) {
        let Some(edit) = self.scene.edit.clone() else { return };
        let Some((mesh_id, mesh)) = self.scene.object_mesh(edit.object) else { return };
        let loops = shape_ops::edge_loops(mesh, &edit.edges);
        if loops.len() != 2 || loops[0].len() != loops[1].len() {
            self.status = format!("Bridge needs two edge loops of the same length (found {})", loops.len());
            return;
        }
        match shape_ops::bridge_edge_loops(mesh, &edit.edges) {
            Ok(m) => self.edit("Bridge Edge Loops", |scene| {
                if let Some(d) = scene.mesh_mut(mesh_id) {
                    d.set_mesh(m);
                }
                if let Some(e) = &mut scene.edit {
                    e.clear();
                }
            }),
            Err(e) => self.status = format!("Bridge failed: {e}"),
        }
    }

    /// Spin the selected edges around the view's up axis through the object origin.
    pub fn spin_selected(&mut self, angle_deg: f32, steps: u32) {
        let Some(edit) = self.scene.edit.clone() else { return };
        if edit.edges.is_empty() {
            return;
        }
        let Some((mesh_id, mesh)) = self.scene.object_mesh(edit.object) else { return };
        let axis = Vec3::Y;
        match shape_ops::spin(mesh, &edit.edges, Vec3::ZERO, axis, angle_deg.to_radians(), steps) {
            Ok(m) => self.edit("Spin", |scene| {
                if let Some(d) = scene.mesh_mut(mesh_id) {
                    d.set_mesh(m);
                }
                if let Some(e) = &mut scene.edit {
                    e.clear();
                }
            }),
            Err(e) => self.status = format!("Spin failed: {e}"),
        }
    }

    /// Keys for this stage: C, K, Alt+S, Ctrl+E / Ctrl+V / Ctrl+F.
    pub(crate) fn edit_tool_keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() || self.modal.is_some() || self.scene.edit.is_none() || self.popup.is_some() {
            return;
        }
        let [c, k, alt_s, ctrl_e, ctrl_v, ctrl_f] = ui.input_mut(|i| {
            [
                crate::key(i, egui::Modifiers::NONE, egui::Key::C),
                crate::key(i, egui::Modifiers::NONE, egui::Key::K),
                crate::key(i, egui::Modifiers::ALT, egui::Key::S),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::E),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::V),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::F),
            ]
        });
        if c {
            self.circle = Some(CircleSelect { radius: 40.0 });
            self.knife = None;
            self.status = "Circle select: paint with the left button, Shift deselects, wheel sizes, Esc ends".into();
        }
        if k {
            self.knife = Some(Knife { face: None, first: None, first_px: None });
            self.circle = None;
            self.status = "Knife: click two edges of one face, Esc ends".into();
        }
        if alt_s {
            self.start_face_modal(crate::tools::ModalKind::ShrinkFatten);
        }
        if ctrl_e {
            self.popup = Some(Popup::EdgeMenu);
        }
        if ctrl_v {
            self.popup = Some(Popup::VertexMenu);
        }
        if ctrl_f {
            self.popup = Some(Popup::FaceMenu);
        }
    }

    /// Brush circle and knife point over the viewport.
    pub(crate) fn edit_tool_overlay(&self, ui: &egui::Ui) {
        let rect = self.viewport.last_rect;
        let ppp = self.viewport.last_ppp.max(0.01);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("edit-tools")));
        if let Some(c) = &self.circle
            && let Some(p) = self.last_pointer
        {
            let centre = egui::pos2(rect.left() + p.x / ppp, rect.top() + p.y / ppp);
            painter.circle_stroke(centre, c.radius / ppp, Stroke::new(1.0, Color32::WHITE));
        }
        if let Some(k) = &self.knife
            && let Some(p) = k.first_px
        {
            let c = egui::pos2(rect.left() + p.x / ppp, rect.top() + p.y / ppp);
            painter.circle_filled(c, 4.0, Color32::from_rgb(0xe8, 0x93, 0x30));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Primitive;
    use std::collections::BTreeSet;

    #[test]
    fn circle_select_paints_and_shift_erases() {
        let mut app = ShapeApp::default();
        let id = app.add_primitive(Primitive::Cube);
        app.scene.selection.set(id);
        app.toggle_edit_mode();
        app.set_select_mode(SelectMode::Vertex);
        let size = Vec2::new(800.0, 600.0);
        let centre = app.viewport.camera.project(Vec3::ZERO, size).unwrap();
        app.circle = Some(CircleSelect { radius: 400.0 });
        let input = ViewportInput { size, pointer: Some(centre), primary_down: true, ..Default::default() };
        assert!(app.circle_select_input(&input, false));
        assert_eq!(app.scene.edit.as_ref().unwrap().verts.len(), 8);
        let input = ViewportInput { size, pointer: Some(centre), primary_down: true, shift: true, ..Default::default() };
        assert!(app.circle_select_input(&input, false));
        assert!(app.scene.edit.as_ref().unwrap().verts.is_empty());
        assert!(app.circle_select_input(&ViewportInput { size, ..Default::default() }, true));
        assert!(app.circle.is_none());
    }

    #[test]
    fn bridge_and_spin_from_the_selection() {
        let mut app = ShapeApp::default();
        let id = app.add_primitive(Primitive::Cylinder);
        app.scene.selection.set(id);
        app.toggle_edit_mode();
        let mesh = app.scene.object_mesh(id).unwrap().1.clone();
        // Delete the caps, then bridge the two rims: Blender users do this to make a tube a torus-ish band.
        let caps: BTreeSet<_> = mesh.face_ids().filter(|&f| mesh.face_normal(f).y.abs() > 0.9).collect();
        app.set_select_mode(SelectMode::Face);
        app.scene.edit.as_mut().unwrap().faces = caps;
        app.scene.edit.as_mut().unwrap().flush(&mesh);
        app.edit_delete(crate::EditDelete::Faces);
        let open = app.scene.object_mesh(id).unwrap().1.clone();
        assert_eq!(open.boundary_loops().len(), 2);
        // Spin the bottom rim 90 degrees around Y: 32 new quads.
        app.set_select_mode(SelectMode::Edge);
        app.scene.edit.as_mut().unwrap().edges = open
            .edge_ids()
            .filter(|&e| {
                let (a, b) = open.edge_vertices(e);
                open.is_boundary_edge(e) && open.position(a).y < 0.0 && open.position(b).y < 0.0
            })
            .collect();
        app.scene.edit.as_mut().unwrap().flush(&open);
        assert_eq!(app.scene.edit.as_ref().unwrap().edges.len(), 32);
        app.spin_selected(90.0, 4);
        let spun = app.scene.object_mesh(id).unwrap().1.clone();
        assert_eq!(spun.face_count(), 32 + 32 * 4);
        app.history.undo(&mut app.scene);
        app.history.undo(&mut app.scene);
        assert_eq!(app.scene.object_mesh(id).unwrap().1.face_count(), 34);
    }
}
