//! UV editing, after Blender's UV Editing workspace: the U menu in edit mode (Unwrap, Smart UV
//! Project, Cube / Cylinder / Sphere Projection, Pack Islands), Mark / Clear Seam, and a UV editor
//! that draws the active mesh's islands over the 0..1 grid with the selected faces highlighted.

use std::collections::BTreeSet;

use egui::{Color32, RichText, Stroke, pos2, vec2};
use shape_ops::unwrap::{self, seam_key};
use shape_scene::ObjectData;

use crate::{ShapeApp, theme};

/// What the U menu offers (Blender's UV Mapping menu).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UvOp {
    Unwrap,
    SmartProject,
    CubeProjection,
    CylinderProjection,
    SphereProjection,
    PackIslands,
    MarkSeam,
    ClearSeam,
}

impl UvOp {
    pub const MENU: [UvOp; 6] =
        [UvOp::Unwrap, UvOp::SmartProject, UvOp::CubeProjection, UvOp::CylinderProjection, UvOp::SphereProjection, UvOp::PackIslands];
    pub fn name(self) -> &'static str {
        match self {
            UvOp::Unwrap => "Unwrap",
            UvOp::SmartProject => "Smart UV Project",
            UvOp::CubeProjection => "Cube Projection",
            UvOp::CylinderProjection => "Cylinder Projection",
            UvOp::SphereProjection => "Sphere Projection",
            UvOp::PackIslands => "Pack Islands",
            UvOp::MarkSeam => "Mark Seam",
            UvOp::ClearSeam => "Clear Seam",
        }
    }
}

impl ShapeApp {
    /// Run a U-menu operator on the mesh in edit mode (or the active mesh).
    pub fn uv_op(&mut self, op: UvOp) {
        let Some(id) = self.scene.edit.as_ref().map(|e| e.object).or(self.scene.selection.active) else { return };
        let Some((mesh_id, mesh)) = self.scene.object_mesh(id) else { return };
        let mesh = mesh.clone();
        let Some(data) = self.scene.mesh(mesh_id) else { return };
        let seams = data.seams.clone();
        let name = format!("UV {}", op.name());
        match op {
            UvOp::MarkSeam | UvOp::ClearSeam => {
                let Some(edit) = self.scene.edit.clone() else { return };
                // Selected edges, plus the edges of selected faces.
                let mut keys: BTreeSet<(u32, u32)> = edit.edges.iter().map(|&e| mesh.edge_vertices(e)).map(|(a, b)| seam_key(a, b)).collect();
                for &f in &edit.faces {
                    for h in mesh.face_halfedges(f) {
                        keys.insert(seam_key(mesh.from(h), mesh.halfedge(h).to));
                    }
                }
                if keys.is_empty() {
                    self.status = "Select edges first".into();
                    return;
                }
                let mark = op == UvOp::MarkSeam;
                self.edit(&name, |scene| {
                    if let Some(d) = scene.mesh_mut(mesh_id) {
                        for k in &keys {
                            if mark {
                                d.seams.insert(*k);
                            } else {
                                d.seams.remove(k);
                            }
                        }
                    }
                });
            }
            UvOp::PackIslands => {
                let Some(mut uvs) = data.uvs.clone() else {
                    self.status = "No UVs to pack: unwrap first".into();
                    return;
                };
                let groups = unwrap::islands(&mesh, &seams);
                unwrap::pack_islands(&mut uvs, &groups, 0.01);
                self.edit(&name, |scene| {
                    if let Some(d) = scene.mesh_mut(mesh_id) {
                        d.uvs = Some(uvs);
                    }
                });
            }
            _ => {
                let uvs = match op {
                    UvOp::Unwrap => unwrap::unwrap(&mesh, &seams),
                    UvOp::SmartProject => unwrap::smart_project(&mesh, 66f32.to_radians()),
                    UvOp::CubeProjection => shape_ops::uv_box(&mesh),
                    UvOp::CylinderProjection => unwrap::uv_cylinder(&mesh),
                    _ => shape_ops::uv_sphere(&mesh),
                };
                self.edit(&name, |scene| {
                    if let Some(d) = scene.mesh_mut(mesh_id) {
                        d.uvs = Some(uvs);
                    }
                });
            }
        }
    }

    /// The UV editor region: the 0..1 grid with the active mesh's UV faces.
    pub(crate) fn uv_editor_panel(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("uv-editor").default_size(480.0).resizable(true).frame(theme::region_frame(theme::TIMELINE)).show(ui, |ui| {
            egui::Panel::top("uv-editor-header").exact_size(theme::HEADER_H).frame(theme::region_frame(theme::HEADER)).show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.label(RichText::new("UV Editor").strong());
                    ui.separator();
                    ui.menu_button("UV", |ui| {
                        for op in UvOp::MENU {
                            if ui.button(op.name()).clicked() {
                                self.uv_op(op);
                            }
                        }
                    });
                    ui.label(RichText::new("U: unwrap menu · Ctrl+E: seams").color(theme::TEXT_WEAK).small());
                });
            });
            ui.set_min_width(300.0);
            let size = ui.available_size();
            let (resp, painter) = ui.allocate_painter(size, egui::Sense::hover());
            let rect = resp.rect;
            painter.rect_filled(rect, 0.0, Color32::from_rgb(0x30, 0x30, 0x30));
            // The unit square, as large as fits, centred.
            let side = (rect.width().min(rect.height()) - 24.0).max(10.0);
            let origin = pos2(rect.center().x - side / 2.0, rect.center().y + side / 2.0);
            let to_screen = |u: f32, v: f32| pos2(origin.x + u * side, origin.y - v * side);
            painter.rect_filled(egui::Rect::from_min_max(to_screen(0.0, 1.0), to_screen(1.0, 0.0)), 0.0, Color32::from_rgb(0x3d, 0x3d, 0x3d));
            for i in 0..=8 {
                let t = i as f32 / 8.0;
                let c = if i % 4 == 0 { Color32::from_rgb(0x5a, 0x5a, 0x5a) } else { Color32::from_rgb(0x48, 0x48, 0x48) };
                painter.line_segment([to_screen(t, 0.0), to_screen(t, 1.0)], Stroke::new(1.0, c));
                painter.line_segment([to_screen(0.0, t), to_screen(1.0, t)], Stroke::new(1.0, c));
            }
            let Some(id) = self.scene.edit.as_ref().map(|e| e.object).or(self.scene.selection.active) else { return };
            let Some(ObjectData::Mesh { mesh: mesh_id, .. }) = self.scene.object(id).map(|o| &o.data) else { return };
            let Some(data) = self.scene.mesh(*mesh_id) else { return };
            let Some(uvs) = &data.uvs else {
                painter.text(rect.center(), egui::Align2::CENTER_CENTER, "No UV map: U > Unwrap", egui::FontId::proportional(13.0), theme::TEXT_WEAK);
                return;
            };
            let selected = self.scene.edit.as_ref().map(|e| e.faces.clone()).unwrap_or_default();
            let face_fill = Color32::from_rgba_unmultiplied(0xc0, 0xc0, 0xc0, 40);
            let sel_fill = Color32::from_rgba_unmultiplied(0xff, 0xa0, 0x40, 90);
            for (f, corners) in data.mesh.face_ids().zip(uvs) {
                let pts: Vec<egui::Pos2> = corners.iter().map(|c| to_screen(c.x, c.y)).collect();
                if pts.len() < 3 {
                    continue;
                }
                let sel = selected.contains(&f);
                painter.add(egui::Shape::convex_polygon(pts.clone(), if sel { sel_fill } else { face_fill }, Stroke::NONE));
                let stroke = Stroke::new(1.0, if sel { Color32::from_rgb(0xff, 0xa0, 0x28) } else { Color32::from_rgb(0xe0, 0xe0, 0xe0) });
                painter.add(egui::Shape::closed_line(pts, stroke));
            }
            let n = unwrap::islands(&data.mesh, &data.seams).len();
            painter.text(
                rect.left_top() + vec2(8.0, 6.0),
                egui::Align2::LEFT_TOP,
                format!("{} faces · {n} islands · {} seams", uvs.len(), data.seams.len()),
                egui::FontId::proportional(12.0),
                theme::TEXT_WEAK,
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Primitive;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn uv_workspace_unwraps_and_marks_seams() {
        let mut app = ShapeApp::default();
        let id = app.add_primitive(Primitive::Cube);
        app.scene.selection.set(id);
        app.set_workspace(crate::layout::Workspace::UvEditing);
        assert!(app.scene.edit.is_some(), "UV Editing enters edit mode");
        // All faces selected: Mark Seam seams every edge, so Unwrap gives six islands.
        app.uv_op(UvOp::MarkSeam);
        let (mesh_id, _) = app.scene.object_mesh(id).unwrap();
        assert_eq!(app.scene.mesh(mesh_id).unwrap().seams.len(), 12);
        let mut harness = Harness::new_ui(|ui| app.ui(ui));
        harness.run();
        harness.get_by_label("UV Editor");
        harness.key_press(egui::Key::U);
        harness.run();
        harness.get_by_label("Smart UV Project");
        harness.get_by_label("Unwrap").click();
        harness.run();
        harness.run();
        drop(harness);
        let data = app.scene.mesh(mesh_id).unwrap();
        let uvs = data.uvs.as_ref().expect("Unwrap wrote UVs");
        assert_eq!(uvs.len(), 6);
        assert_eq!(unwrap::islands(&data.mesh, &data.seams).len(), 6);
        assert_eq!(app.history.undo_name(), Some("UV Unwrap"));
        app.uv_op(UvOp::ClearSeam);
        assert!(app.scene.mesh(mesh_id).unwrap().seams.is_empty());
    }
}
