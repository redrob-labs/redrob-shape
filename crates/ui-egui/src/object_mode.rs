//! Object-mode conveniences of Blender's default screen (stage P5): the 3D cursor, pivot point
//! and transform orientation dropdowns, snapping, proportional editing, parenting, Set Origin,
//! and collections in the outliner.

use crate::ShapeApp;
use crate::keymap::Popup;
use crate::tools::{Orientation, PivotPoint};
use crate::viewport::ViewportInput;
use egui::{Color32, RichText, Stroke};
use shape_math::{Transform, Vec2, Vec3};
use shape_scene::{CollectionId, ObjectId};

/// Set Origin choices (Blender: Object > Set Origin).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SetOrigin {
    GeometryToOrigin,
    OriginToGeometry,
    OriginToCursor,
    CursorToSelected,
}

impl SetOrigin {
    pub const ALL: [SetOrigin; 4] =
        [SetOrigin::GeometryToOrigin, SetOrigin::OriginToGeometry, SetOrigin::OriginToCursor, SetOrigin::CursorToSelected];
    pub fn name(self) -> &'static str {
        match self {
            SetOrigin::GeometryToOrigin => "Geometry to Origin",
            SetOrigin::OriginToGeometry => "Origin to Geometry",
            SetOrigin::OriginToCursor => "Origin to 3D Cursor",
            SetOrigin::CursorToSelected => "Cursor to Selected",
        }
    }
}

impl ShapeApp {
    /// Shift+right click: put the 3D cursor on the surface under the pointer, or on the ground
    /// plane (y = 0) when nothing is hit.
    pub(crate) fn place_cursor(&mut self, input: &ViewportInput) -> bool {
        if !(input.secondary_clicked && input.shift) {
            return false;
        }
        let Some(p) = input.pointer else { return true };
        let size = input.size.max(Vec2::ONE);
        let at = match shape_gpu::pick_detail(&self.scene, &self.viewport.camera, p, size) {
            Some(hit) => self.scene.world_transform(hit.object).transform_point(hit.local_point),
            None => {
                let ray = self.viewport.camera.ray_pixel(p, size);
                if ray.direction.y.abs() < 1e-6 {
                    return true;
                }
                let t = -ray.origin.y / ray.direction.y;
                if t <= 0.0 {
                    return true;
                }
                ray.origin + ray.direction * t
            }
        };
        self.scene.cursor = at;
        self.status = format!("Cursor at ({:.2}, {:.2}, {:.2})", at.x, at.y, at.z);
        true
    }

    /// Blender's cursor: a red/white dashed ring with small axis ticks, drawn over the viewport.
    pub(crate) fn cursor_overlay(&self, ui: &egui::Ui) {
        let rect = self.viewport.last_rect;
        let ppp = self.viewport.last_ppp.max(0.01);
        let size = Vec2::new(rect.width() * ppp, rect.height() * ppp);
        let Some(px) = self.viewport.camera.project(self.scene.cursor, size) else { return };
        let c = egui::pos2(rect.left() + px.x / ppp, rect.top() + px.y / ppp);
        if !rect.contains(c) {
            return;
        }
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("cursor-3d")));
        let r = 7.0;
        let n = 16;
        for k in 0..n {
            let a0 = k as f32 / n as f32 * std::f32::consts::TAU;
            let a1 = (k + 1) as f32 / n as f32 * std::f32::consts::TAU;
            let col = if k % 2 == 0 { Color32::from_rgb(0xe0, 0x30, 0x30) } else { Color32::WHITE };
            painter.line_segment([c + egui::vec2(a0.cos(), a0.sin()) * r, c + egui::vec2(a1.cos(), a1.sin()) * r], Stroke::new(1.5, col));
        }
        for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let d = egui::vec2(dx, dy);
            painter.line_segment([c + d * (r + 3.0), c + d * (r + 9.0)], Stroke::new(1.0, Color32::from_gray(0x20)));
        }
    }

    /// Ctrl+P: parent the other selected objects to the active one, keeping their world placement.
    pub fn parent_to_active(&mut self) {
        let Some(active) = self.scene.selection.active else { return };
        let children: Vec<ObjectId> = self.scene.selection.objects.iter().copied().filter(|&id| id != active).collect();
        if children.is_empty() {
            self.status = "Parent needs an active object and at least one other selected".into();
            return;
        }
        self.edit("Make Parent", |scene| {
            let parent_world = scene.world_transform(active);
            for c in children {
                let world = scene.world_transform(c);
                if scene.set_parent(c, Some(active))
                    && let Some(o) = scene.object_mut(c)
                {
                    o.transform = parent_world.inverse().mul_transform(&world);
                }
            }
        });
    }

    /// Alt+P: clear the parent of the selected objects, keeping their world placement.
    pub fn clear_parent(&mut self) {
        let ids: Vec<ObjectId> =
            self.scene.selection.objects.iter().copied().filter(|&id| self.scene.object(id).is_some_and(|o| o.parent.is_some())).collect();
        if ids.is_empty() {
            return;
        }
        self.edit("Clear Parent", |scene| {
            for id in ids {
                let world = scene.world_transform(id);
                scene.set_parent(id, None);
                if let Some(o) = scene.object_mut(id) {
                    o.transform = world;
                }
            }
        });
    }

    /// Object > Set Origin.
    pub fn set_origin(&mut self, how: SetOrigin) {
        let ids: Vec<ObjectId> = self.scene.selection.objects.iter().copied().collect();
        if ids.is_empty() {
            return;
        }
        if how == SetOrigin::CursorToSelected {
            self.scene.cursor = self.scene.selection_center();
            return;
        }
        let cursor = self.scene.cursor;
        self.edit(how.name(), |scene| {
            for id in ids {
                let Some((mesh_id, mesh)) = scene.object_mesh(id) else { continue };
                let world = scene.world_transform(id);
                let (positions, polygons) = mesh.to_polygons();
                let centroid = positions.iter().copied().sum::<Vec3>() / positions.len().max(1) as f32;
                // Where the origin should land, in object-local coordinates.
                let new_origin_local = match how {
                    SetOrigin::GeometryToOrigin => {
                        // Move the geometry so its centre sits on the current origin.
                        let moved: Vec<Vec3> = positions.iter().map(|p| *p - centroid).collect();
                        if let Ok(m) = shape_geom::Mesh::from_polygons(moved, &polygons)
                            && let Some(d) = scene.mesh_mut(mesh_id)
                        {
                            d.set_mesh(m);
                        }
                        continue;
                    }
                    SetOrigin::OriginToGeometry => centroid,
                    SetOrigin::OriginToCursor => world.inverse().transform_point(cursor),
                    SetOrigin::CursorToSelected => unreachable!(),
                };
                let moved: Vec<Vec3> = positions.iter().map(|p| *p - new_origin_local).collect();
                if let Ok(m) = shape_geom::Mesh::from_polygons(moved, &polygons)
                    && let Some(d) = scene.mesh_mut(mesh_id)
                {
                    d.set_mesh(m);
                }
                // The origin moves in world space by the local offset carried through the transform.
                let shift_world = world.transform_vector(new_origin_local);
                let parent_world = scene.object(id).and_then(|o| o.parent).map(|p| scene.world_transform(p)).unwrap_or(Transform::IDENTITY);
                if let Some(o) = scene.object_mut(id) {
                    o.transform.translation += parent_world.inverse().transform_vector(shift_world);
                }
            }
        });
    }

    /// M in object mode: link the selection to a collection (or a new one).
    pub fn move_to_collection(&mut self, collection: Option<CollectionId>) {
        let ids: Vec<ObjectId> = self.scene.selection.objects.iter().copied().collect();
        if ids.is_empty() {
            return;
        }
        self.edit("Move to Collection", |scene| {
            for id in ids {
                scene.move_to_collection(id, collection);
            }
        });
    }

    pub fn new_collection_with_selection(&mut self) {
        let ids: Vec<ObjectId> = self.scene.selection.objects.iter().copied().collect();
        let n = self.scene.collections.len() + 1;
        self.edit("New Collection", |scene| {
            let c = scene.add_collection(format!("Collection {n}"));
            for id in ids {
                scene.move_to_collection(id, Some(c));
            }
        });
    }

    /// Keys of this stage: O (proportional), Ctrl+P / Alt+P, M (object mode), period (pivot menu).
    pub(crate) fn object_mode_keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() || self.modal.is_some() || self.popup.is_some() {
            return;
        }
        let editing = self.scene.edit.is_some();
        let [o, ctrl_p, alt_p, m] = ui.input_mut(|i| {
            [
                crate::key(i, egui::Modifiers::NONE, egui::Key::O),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::P),
                crate::key(i, egui::Modifiers::ALT, egui::Key::P),
                !editing && crate::key(i, egui::Modifiers::NONE, egui::Key::M),
            ]
        });
        if o {
            self.xf.proportional = !self.xf.proportional;
            self.status = format!("Proportional editing {}", if self.xf.proportional { "on" } else { "off" });
        }
        if ctrl_p && !editing {
            self.parent_to_active();
        }
        if alt_p && !editing {
            self.clear_parent();
        }
        if m && !self.scene.selection.is_empty() {
            self.popup = Some(Popup::MoveToCollection);
        }
    }

    /// Header widgets: pivot point, transform orientation, proportional toggle (Blender's header).
    pub(crate) fn transform_header(&mut self, ui: &mut egui::Ui) {
        egui::ComboBox::from_id_salt("orientation").selected_text(self.xf.orientation.name()).width(70.0).show_ui(ui, |ui| {
            for o in Orientation::ALL {
                ui.selectable_value(&mut self.xf.orientation, o, o.name());
            }
        });
        egui::ComboBox::from_id_salt("pivot").selected_text(self.xf.pivot.name()).width(120.0).show_ui(ui, |ui| {
            for p in PivotPoint::ALL {
                ui.selectable_value(&mut self.xf.pivot, p, p.name());
            }
        });
        if self.scene.edit.is_some() {
            let label = if self.xf.proportional { RichText::new("◎").color(crate::theme::ACCENT) } else { RichText::new("◎") };
            if ui
                .selectable_label(self.xf.proportional, label)
                .on_hover_text("Proportional editing (O); wheel sets the size during a transform")
                .clicked()
            {
                self.xf.proportional = !self.xf.proportional;
            }
        }
    }

    /// Outliner body with collections: the Scene Collection, then each collection as a header
    /// with an eye toggle and its objects under it.
    pub(crate) fn outliner_tree(&mut self, ui: &mut egui::Ui) {
        let shift = ui.input(|i| i.modifiers.shift);
        ui.label(RichText::new("Scene Collection").strong());
        let roots: Vec<ObjectId> = self.scene.roots().filter(|&id| self.scene.object(id).is_some_and(|o| o.collection.is_none())).collect();
        ui.indent("scene-collection", |ui| {
            for id in roots {
                self.outliner_row_eye(ui, id, shift);
            }
        });
        let count = self.scene.collections.len();
        for i in 0..count {
            let cid = CollectionId(i as u32);
            let (name, visible) = {
                let c = &self.scene.collections[i];
                (c.name.clone(), c.visible)
            };
            ui.horizontal(|ui| {
                let eye = if visible { "👁" } else { "―" };
                if ui.small_button(eye).on_hover_text("Hide / show the collection in the viewport").clicked()
                    && let Some(c) = self.scene.collection_mut(cid)
                {
                    c.visible = !c.visible;
                }
                ui.label(RichText::new(format!("▸ {name}")).strong());
            });
            let members: Vec<ObjectId> =
                self.scene.collection_members(Some(cid)).filter(|&id| self.scene.object(id).is_some_and(|o| o.parent.is_none())).collect();
            ui.indent(("collection", i), |ui| {
                for id in members {
                    self.outliner_row_eye(ui, id, shift);
                }
            });
        }
    }

    fn outliner_row_eye(&mut self, ui: &mut egui::Ui, id: ObjectId, shift: bool) {
        let Some(object) = self.scene.object(id) else { return };
        let visible = object.visible;
        ui.horizontal(|ui| {
            let eye = if visible { "👁" } else { "―" };
            if ui.small_button(eye).clicked()
                && let Some(o) = self.scene.object_mut(id)
            {
                o.visible = !o.visible;
            }
            self.outliner_row(ui, id, shift);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Primitive;

    #[test]
    fn parenting_keeps_world_placement_and_set_origin_moves_the_pivot() {
        let mut app = ShapeApp::default();
        let a = app.add_primitive(Primitive::Cube);
        let b = app.add_primitive(Primitive::Sphere);
        app.scene.object_mut(a).unwrap().transform.translation = Vec3::new(2.0, 0.0, 0.0);
        app.scene.object_mut(b).unwrap().transform.translation = Vec3::new(5.0, 1.0, 0.0);
        app.scene.selection.set(b);
        app.scene.selection.toggle(a);
        app.scene.selection.active = Some(a);
        app.parent_to_active();
        assert_eq!(app.scene.object(b).unwrap().parent, Some(a));
        let world = app.scene.world_transform(b).translation;
        assert!((world - Vec3::new(5.0, 1.0, 0.0)).length() < 1e-4, "{world}");
        assert!((app.scene.object(b).unwrap().transform.translation - Vec3::new(3.0, 1.0, 0.0)).length() < 1e-4);
        app.scene.selection.set(b);
        app.clear_parent();
        assert_eq!(app.scene.object(b).unwrap().parent, None);
        assert!((app.scene.object(b).unwrap().transform.translation - Vec3::new(5.0, 1.0, 0.0)).length() < 1e-4);
        // Origin to cursor: geometry stays put in world space, the origin lands on the cursor.
        app.scene.cursor = Vec3::new(5.0, 2.0, 0.0);
        app.set_origin(SetOrigin::OriginToCursor);
        let (_, mesh) = app.scene.object_mesh(b).unwrap();
        let world = app.scene.world_transform(b);
        let top = mesh.positions().map(|p| world.transform_point(p).y).fold(f32::NEG_INFINITY, f32::max);
        assert!((top - 2.0).abs() < 1e-3, "sphere of radius 1 at y=1 still tops out at 2: {top}");
        assert!((world.translation - Vec3::new(5.0, 2.0, 0.0)).length() < 1e-4);
        app.history.undo(&mut app.scene);
        app.history.undo(&mut app.scene);
        app.history.undo(&mut app.scene);
        assert_eq!(app.scene.object(b).unwrap().parent, None, "three undo steps back to before the parent");
    }

    #[test]
    fn collections_hide_their_members_and_cursor_places_new_objects() {
        let mut app = ShapeApp::default();
        let a = app.add_primitive(Primitive::Cube);
        app.scene.selection.set(a);
        app.new_collection_with_selection();
        assert_eq!(app.scene.collections.len(), 1);
        assert_eq!(app.scene.object(a).unwrap().collection, Some(CollectionId(0)));
        assert!(app.scene.is_visible(a));
        app.scene.collection_mut(CollectionId(0)).unwrap().visible = false;
        assert!(!app.scene.is_visible(a));
        assert!(shape_gpu::build_frame(&app.scene, &Default::default()).triangles.is_empty(), "hidden collection draws nothing");
        app.move_to_collection(None);
        assert_eq!(app.scene.object(a).unwrap().collection, None);
        app.scene.cursor = Vec3::new(1.0, 2.0, 3.0);
        let b = app.add_primitive(Primitive::Sphere);
        assert_eq!(app.scene.object(b).unwrap().transform.translation, Vec3::new(1.0, 2.0, 3.0));
    }
}
