//! Edit mode the Blender way (stage P3): vertex / edge / face select modes (1 / 2 / 3), clicks
//! and box select per mode, Alt+click loops, L linked, Ctrl+Numpad +/- grow and shrink, F fill,
//! M merge menu, X delete menu, and the live flush that keeps the three selection sets agreeing.

use crate::ShapeApp;
use crate::keymap::Popup;
use shape_geom::{EdgeId, FaceId, Mesh, VertexId};
use shape_gpu::PickHit;
use shape_math::Vec2;
use shape_ops::MergeAt;
use shape_scene::SelectMode;
use std::collections::BTreeSet;

/// What Blender's edit-mode X menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditDelete {
    Vertices,
    Edges,
    Faces,
    OnlyFaces,
    DissolveEdges,
    DissolveFaces,
}

impl EditDelete {
    pub const ALL: [EditDelete; 6] =
        [EditDelete::Vertices, EditDelete::Edges, EditDelete::Faces, EditDelete::OnlyFaces, EditDelete::DissolveEdges, EditDelete::DissolveFaces];

    pub fn name(self) -> &'static str {
        match self {
            EditDelete::Vertices => "Vertices",
            EditDelete::Edges => "Edges",
            EditDelete::Faces => "Faces",
            EditDelete::OnlyFaces => "Only Faces",
            EditDelete::DissolveEdges => "Dissolve Edges",
            EditDelete::DissolveFaces => "Dissolve Faces",
        }
    }
}

impl ShapeApp {
    /// The edited mesh, cloned out so the selection can be edited while reading it.
    fn edit_mesh(&self) -> Option<(shape_scene::MeshId, Mesh)> {
        let edit = self.scene.edit.as_ref()?;
        let (id, mesh) = self.scene.object_mesh(edit.object)?;
        Some((id, mesh.clone()))
    }

    /// 1 / 2 / 3: switch the select mode, converting the selection.
    pub fn set_select_mode(&mut self, mode: SelectMode) {
        let Some((_, mesh)) = self.edit_mesh() else { return };
        if let Some(edit) = &mut self.scene.edit {
            edit.set_mode(mode, &mesh);
            self.status = format!("{} select", mode.name());
        }
    }

    /// A click in edit mode: pick the element of the current mode under the pointer.
    /// `shift` toggles, `alt` selects the loop through the element.
    pub(crate) fn edit_click(&mut self, hit: Option<PickHit>, shift: bool, alt: bool) {
        let Some((_, mesh)) = self.edit_mesh() else { return };
        let Some(edit) = &mut self.scene.edit else { return };
        let Some(h) = hit else {
            if !shift {
                edit.clear();
            }
            return;
        };
        if !shift {
            edit.clear();
        }
        match edit.mode {
            SelectMode::Vertex => {
                if let Some(v) = shape_gpu::nearest_vertex(&mesh, h.face, h.local_point)
                    && (shift && !edit.verts.remove(&v) || !shift)
                {
                    edit.verts.insert(v);
                }
            }
            SelectMode::Edge => {
                if let Some(e) = shape_gpu::nearest_edge(&mesh, h.face, h.local_point) {
                    if alt {
                        edit.edges.extend(shape_ops::edge_loop(&mesh, e));
                    } else if shift && !edit.edges.remove(&e) || !shift {
                        edit.edges.insert(e);
                    }
                }
            }
            SelectMode::Face => {
                if alt {
                    if let Some(e) = shape_gpu::nearest_edge(&mesh, h.face, h.local_point) {
                        edit.faces.extend(shape_ops::face_loop(&mesh, h.face, e));
                    }
                } else if shift && !edit.faces.remove(&h.face) || !shift {
                    edit.faces.insert(h.face);
                }
            }
        }
        edit.flush(&mesh);
    }

    /// Box select in edit mode: elements whose projected position falls inside.
    pub(crate) fn edit_box_select(&mut self, inside: impl Fn(Vec2) -> bool, size: Vec2, extend: bool) {
        let Some((_, mesh)) = self.edit_mesh() else { return };
        let Some(edit) = self.scene.edit.as_ref() else { return };
        let t = self.scene.world_transform(edit.object);
        let cam = self.viewport.camera.clone();
        let proj = |p: shape_math::Vec3| cam.project(t.transform_point(p), size).is_some_and(&inside);
        let Some(edit) = &mut self.scene.edit else { return };
        if !extend {
            edit.clear();
        }
        match edit.mode {
            SelectMode::Vertex => edit.verts.extend(mesh.vertex_ids().filter(|&v| proj(mesh.position(v)))),
            SelectMode::Edge => edit.edges.extend(mesh.edge_ids().filter(|&e| {
                let (a, b) = mesh.edge_vertices(e);
                proj((mesh.position(a) + mesh.position(b)) * 0.5)
            })),
            SelectMode::Face => edit.faces.extend(mesh.face_ids().filter(|&f| proj(mesh.face_centroid(f)))),
        }
        edit.flush(&mesh);
    }

    /// L: select everything connected to the selection (Ctrl+L in Blender; L uses the hovered
    /// element, which the pointer-less tests cannot provide, so both select from the selection).
    pub fn select_linked(&mut self) {
        let Some((_, mesh)) = self.edit_mesh() else { return };
        let Some(edit) = &mut self.scene.edit else { return };
        let mut seeds: BTreeSet<FaceId> = edit.verts.iter().flat_map(|&v| mesh.vertex_faces(v)).collect();
        seeds.extend(edit.faces.iter().copied());
        let linked = shape_ops::linked_faces(&mesh, &seeds);
        match edit.mode {
            SelectMode::Vertex => edit.verts.extend(linked.iter().flat_map(|&f| mesh.face_vertices(f))),
            SelectMode::Edge => edit.edges.extend(linked.iter().flat_map(|&f| mesh.face_halfedges(f).map(|h| mesh.halfedge(h).edge))),
            SelectMode::Face => edit.faces.extend(linked),
        }
        edit.flush(&mesh);
    }

    /// Ctrl+Numpad + / -: grow or shrink the selection by one ring of neighbours.
    pub fn select_more_less(&mut self, more: bool) {
        let Some((_, mesh)) = self.edit_mesh() else { return };
        let Some(edit) = &mut self.scene.edit else { return };
        let verts = if more { shape_ops::grow_vertices(&mesh, &edit.verts) } else { shape_ops::shrink_vertices(&mesh, &edit.verts) };
        match edit.mode {
            SelectMode::Vertex => edit.verts = verts,
            SelectMode::Edge => {
                edit.edges = mesh
                    .edge_ids()
                    .filter(|&e| {
                        let (a, b) = mesh.edge_vertices(e);
                        verts.contains(&a) && verts.contains(&b)
                    })
                    .collect();
            }
            SelectMode::Face => {
                edit.faces = if more {
                    mesh.face_ids().filter(|&f| mesh.face_vertices(f).any(|v| edit.verts.contains(&v))).collect()
                } else {
                    mesh.face_ids().filter(|&f| mesh.face_vertices(f).all(|v| verts.contains(&v))).collect()
                };
            }
        }
        edit.flush(&mesh);
    }

    /// F: fill a face through the selected vertices.
    pub fn fill_selected(&mut self) {
        let Some((mesh_id, mesh)) = self.edit_mesh() else { return };
        let Some(edit) = self.scene.edit.clone() else { return };
        if edit.verts.len() < 3 {
            self.status = "Fill needs at least three vertices".into();
            return;
        }
        match shape_ops::fill(&mesh, &edit.verts) {
            Ok(m) if m.face_count() > mesh.face_count() => {
                let new_face = FaceId(m.face_count() as u32 - 1);
                self.edit("Fill", |scene| {
                    if let Some(d) = scene.mesh_mut(mesh_id) {
                        d.set_mesh(m.clone());
                    }
                    if let Some(e) = &mut scene.edit {
                        e.faces.insert(new_face);
                        e.flush(&m);
                    }
                });
            }
            Ok(_) => self.status = "Fill: that face exists".into(),
            Err(e) => self.status = format!("Fill failed: {e}"),
        }
    }

    /// M menu: merge the selected vertices.
    pub fn merge_selected(&mut self, at: MergeAt) {
        let Some((mesh_id, mesh)) = self.edit_mesh() else { return };
        let Some(edit) = self.scene.edit.clone() else { return };
        let verts: Vec<VertexId> = edit.verts.iter().copied().collect();
        if verts.len() < 2 {
            return;
        }
        match shape_ops::merge_vertices(&mesh, &verts, at) {
            Ok(m) => self.edit("Merge", |scene| {
                if let Some(d) = scene.mesh_mut(mesh_id) {
                    d.set_mesh(m.clone());
                }
                if let Some(e) = &mut scene.edit {
                    e.clear();
                }
            }),
            Err(e) => self.status = format!("Merge failed: {e}"),
        }
    }

    /// X menu in edit mode.
    pub fn edit_delete(&mut self, what: EditDelete) {
        let Some((mesh_id, mesh)) = self.edit_mesh() else { return };
        let Some(edit) = self.scene.edit.clone() else { return };
        let edges: BTreeSet<EdgeId> = edit.edges.clone();
        let result = match what {
            EditDelete::Vertices => shape_ops::delete_vertices(&mesh, &edit.verts),
            EditDelete::Edges => shape_ops::delete_edges(&mesh, &edges),
            EditDelete::Faces | EditDelete::OnlyFaces => shape_ops::delete_faces(&mesh, &edit.faces),
            EditDelete::DissolveEdges => shape_ops::dissolve_edges(&mesh, &edges),
            EditDelete::DissolveFaces => shape_ops::dissolve_faces(&mesh, &edit.faces),
        };
        match result {
            Ok(m) => self.edit(what.name(), |scene| {
                if let Some(d) = scene.mesh_mut(mesh_id) {
                    d.set_mesh(m);
                }
                if let Some(e) = &mut scene.edit {
                    e.clear();
                }
            }),
            Err(e) => self.status = format!("{} failed: {e}", what.name()),
        }
    }

    /// Edit-mode keys: 1/2/3, L, Ctrl+Numpad +/-, F, M (menu), X (menu).
    pub(crate) fn edit_keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() || self.modal.is_some() || self.scene.edit.is_none() || self.popup.is_some() {
            return;
        }
        let keys = ui.input_mut(|i| {
            [
                crate::key(i, egui::Modifiers::NONE, egui::Key::Num1),
                crate::key(i, egui::Modifiers::NONE, egui::Key::Num2),
                crate::key(i, egui::Modifiers::NONE, egui::Key::Num3),
                crate::key(i, egui::Modifiers::NONE, egui::Key::L),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::L),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::Plus) || crate::key(i, egui::Modifiers::COMMAND, egui::Key::Equals),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::Minus),
                crate::key(i, egui::Modifiers::NONE, egui::Key::F),
                crate::key(i, egui::Modifiers::NONE, egui::Key::M),
                crate::key(i, egui::Modifiers::NONE, egui::Key::X),
                crate::key(i, egui::Modifiers::NONE, egui::Key::U),
            ]
        });
        let [k1, k2, k3, l, ctrl_l, more, less, f, m, x, u] = keys;
        if u {
            self.popup = Some(Popup::UvMenu);
        }
        if k1 {
            self.set_select_mode(SelectMode::Vertex);
        }
        if k2 {
            self.set_select_mode(SelectMode::Edge);
        }
        if k3 {
            self.set_select_mode(SelectMode::Face);
        }
        if l || ctrl_l {
            self.select_linked();
        }
        if more {
            self.select_more_less(true);
        }
        if less {
            self.select_more_less(false);
        }
        if f {
            self.fill_selected();
        }
        if m {
            self.popup = Some(Popup::Merge);
        }
        if x && self.scene.edit.as_ref().is_some_and(|e| !e.is_empty()) {
            self.popup = Some(Popup::EditDelete);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Primitive;

    fn cube_in_edit() -> (ShapeApp, Mesh) {
        let mut app = ShapeApp::default();
        let id = app.add_primitive(Primitive::Cube);
        app.scene.selection.set(id);
        app.toggle_edit_mode();
        let mesh = app.scene.object_mesh(id).unwrap().1.clone();
        (app, mesh)
    }

    #[test]
    fn modes_convert_the_selection_both_ways() {
        let (mut app, mesh) = cube_in_edit();
        let top = mesh.face_ids().find(|&f| mesh.face_normal(f).y > 0.9).unwrap();
        app.scene.edit.as_mut().unwrap().faces.insert(top);
        app.scene.edit.as_mut().unwrap().flush(&mesh);
        assert_eq!(app.scene.edit.as_ref().unwrap().verts.len(), 4);
        assert_eq!(app.scene.edit.as_ref().unwrap().edges.len(), 4);
        app.set_select_mode(SelectMode::Vertex);
        assert_eq!(app.scene.edit.as_ref().unwrap().verts.len(), 4);
        assert_eq!(app.scene.edit.as_ref().unwrap().faces.len(), 1, "the top face stays fully selected");
        // Grow: all 8 vertices, every face.
        app.select_more_less(true);
        assert_eq!(app.scene.edit.as_ref().unwrap().verts.len(), 8);
        assert_eq!(app.scene.edit.as_ref().unwrap().faces.len(), 6);
        app.select_more_less(false);
        assert_eq!(app.scene.edit.as_ref().unwrap().verts.len(), 8, "a closed cube has no unselected neighbour to shrink from");
        app.scene.edit.as_mut().unwrap().clear();
        app.scene.edit.as_mut().unwrap().verts.insert(VertexId(0));
        app.select_linked();
        assert_eq!(app.scene.edit.as_ref().unwrap().verts.len(), 8);
    }

    #[test]
    fn delete_fill_and_merge_are_undo_steps() {
        let (mut app, mesh) = cube_in_edit();
        let top = mesh.face_ids().find(|&f| mesh.face_normal(f).y > 0.9).unwrap();
        app.scene.edit.as_mut().unwrap().faces.insert(top);
        app.scene.edit.as_mut().unwrap().flush(&mesh);
        app.edit_delete(EditDelete::Faces);
        let (_, open) = app.scene.object_mesh(app.scene.edit.as_ref().unwrap().object).unwrap();
        assert_eq!(open.face_count(), 5);
        let rim: BTreeSet<VertexId> = open.boundary_loops()[0].iter().copied().collect();
        app.set_select_mode(SelectMode::Vertex);
        app.scene.edit.as_mut().unwrap().verts = rim;
        app.fill_selected();
        let (_, closed) = app.scene.object_mesh(app.scene.edit.as_ref().unwrap().object).unwrap();
        assert_eq!(closed.face_count(), 6);
        assert_eq!(app.scene.edit.as_ref().unwrap().faces.len(), 1, "the new face is selected");
        app.merge_selected(MergeAt::Center);
        let (_, pyramid) = app.scene.object_mesh(app.scene.edit.as_ref().unwrap().object).unwrap();
        assert_eq!(pyramid.vertex_count(), 5);
        for _ in 0..3 {
            app.history.undo(&mut app.scene);
        }
        let (_, back) = app.scene.object_mesh(app.scene.selection.active.unwrap()).unwrap();
        assert_eq!((back.vertex_count(), back.face_count()), (8, 6));
    }

    #[test]
    fn alt_click_selects_a_face_loop_on_a_cylinder() {
        let mut app = ShapeApp::default();
        let id = app.add_primitive(Primitive::Cylinder);
        app.scene.selection.set(id);
        app.toggle_edit_mode();
        let (_, mesh) = app.scene.object_mesh(id).unwrap();
        let mesh = mesh.clone();
        let side = mesh.face_ids().find(|&f| mesh.face_normal(f).y.abs() < 0.1).unwrap();
        // A point near the middle of a vertical edge of that face.
        let vertical =
            mesh.face_halfedges(side).find(|&h| (mesh.position(mesh.from(h)).y - mesh.position(mesh.halfedge(h).to).y).abs() > 1.0).unwrap();
        let p = (mesh.position(mesh.from(vertical)) + mesh.position(mesh.halfedge(vertical).to)) * 0.5;
        let hit = PickHit { object: id, t: 1.0, face: side, local_point: p };
        app.edit_click(Some(hit), false, true);
        assert_eq!(app.scene.edit.as_ref().unwrap().faces.len(), 32, "every side quad of the barrel");
        app.set_select_mode(SelectMode::Edge);
        app.scene.edit.as_mut().unwrap().clear();
        app.edit_click(Some(hit), false, false);
        assert_eq!(app.scene.edit.as_ref().unwrap().edges.len(), 1);
        app.edit_click(Some(hit), true, false);
        assert!(app.scene.edit.as_ref().unwrap().edges.is_empty(), "shift+click toggles off");
    }
}
