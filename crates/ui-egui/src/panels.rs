//! Menu bar, status bar, outliner and properties panels.

use crate::{Primitive, ShapeApp, VERSION};
use egui::RichText;
use shape_gpu::ShadingMode;
use shape_math::Vec3;
use shape_scene::{Camera, Light, Material, Modifier, ObjectData, ObjectId, Scene};
use std::path::PathBuf;

impl ShapeApp {
    pub(crate) fn file_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("File", |ui| {
            if ui.button("New").clicked() {
                self.scene = Scene::new("Scene");
                self.history.clear();
                self.file = None;
                self.status = "New scene".into();
            }
            if ui.button("Open…").clicked()
                && let Some(path) = file_dialog(true, &self.file)
            {
                self.open(path);
            }
            ui.separator();
            if ui.button("Save").clicked() {
                match self.file.clone() {
                    Some(path) => self.save_as(path),
                    None => {
                        if let Some(path) = file_dialog(false, &None) {
                            self.save_as(path);
                        }
                    }
                }
            }
            if ui.button("Save As…").clicked()
                && let Some(path) = file_dialog(false, &self.file)
            {
                self.save_as(path);
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        });
    }

    pub(crate) fn edit_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Edit", |ui| {
            if ui.button("Operator Search…    F3").clicked() {
                self.popup = Some(crate::Popup::Search { query: String::new() });
            }
            ui.separator();
            let undo_label = self.history.undo_name().map(|n| format!("Undo {n}")).unwrap_or("Undo".into());
            if ui.add_enabled(self.history.can_undo(), egui::Button::new(undo_label)).clicked() {
                self.undo();
            }
            let redo_label = self.history.redo_name().map(|n| format!("Redo {n}")).unwrap_or("Redo".into());
            if ui.add_enabled(self.history.can_redo(), egui::Button::new(redo_label)).clicked() {
                self.redo();
            }
            ui.separator();
            if ui.add_enabled(!self.scene.selection.is_empty(), egui::Button::new("Delete")).clicked() {
                self.delete_selected();
            }
            ui.separator();
            let has_mesh = self.scene.selection.active.is_some_and(|id| self.scene.object_mesh(id).is_some());
            ui.add_enabled_ui(has_mesh, |ui| {
                ui.menu_button("Mesh", |ui| {
                    if ui.button("Subdivide (Catmull-Clark)").clicked() {
                        self.apply_mesh_op("Subdivide", shape_ops::catmull_clark);
                    }
                    if ui.button("Subdivide (Linear)").clicked() {
                        self.apply_mesh_op("Subdivide Linear", shape_ops::subdivide_linear);
                    }
                    if ui.button("Extrude All Faces 0.5").clicked() {
                        self.apply_mesh_op("Extrude", |m| shape_ops::extrude_faces(m, &shape_ops::all_faces(m), 0.5));
                    }
                    if ui.button("Inset All Faces 0.2").clicked() {
                        self.apply_mesh_op("Inset", |m| shape_ops::inset_faces(m, &shape_ops::all_faces(m), 0.2));
                    }
                    if ui.button("Bevel All Edges 0.1").clicked() {
                        self.apply_mesh_op("Bevel", |m| shape_ops::bevel_all(m, 0.1));
                    }
                    if ui.button("Smooth").clicked() {
                        self.apply_mesh_op("Smooth", |m| shape_ops::smooth(m, 2, 0.5));
                    }
                    if ui.button("Triangulate").clicked() {
                        self.apply_mesh_op("Triangulate", shape_ops::triangulate);
                    }
                    if ui.button("Flip Normals").clicked() {
                        self.apply_mesh_op("Flip Normals", shape_ops::flip_normals);
                    }
                    ui.separator();
                    ui.menu_button("UV", |ui| {
                        if ui.button("Box Projection").clicked() {
                            self.generate_uvs("UV Box", shape_ops::uv_box);
                        }
                        if ui.button("Planar (top)").clicked() {
                            self.generate_uvs("UV Planar", |m| shape_ops::uv_planar(m, 1));
                        }
                        if ui.button("Sphere Projection").clicked() {
                            self.generate_uvs("UV Sphere", shape_ops::uv_sphere);
                        }
                    });
                    ui.separator();
                    if ui.button("Apply Modifiers").clicked() {
                        self.apply_modifiers();
                    }
                });
            });
        });
    }

    pub(crate) fn add_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Add", |ui| {
            for p in Primitive::ALL {
                if ui.button(p.name()).clicked() {
                    self.add_primitive(p);
                }
            }
            ui.separator();
            if ui.button("Point Light").clicked() {
                self.add_object("Point", ObjectData::Light(Light::point(1000.0)), Vec3::new(2.0, 4.0, 2.0));
            }
            if ui.button("Sun").clicked() {
                self.add_object("Sun", ObjectData::Light(Light::sun(3.0)), Vec3::new(0.0, 5.0, 0.0));
            }
            if ui.button("Spot Light").clicked() {
                self.add_object("Spot", ObjectData::Light(Light::spot(1500.0)), Vec3::new(0.0, 5.0, 0.0));
            }
            if ui.button("Camera").clicked() {
                self.add_object("Camera", ObjectData::Camera(Camera::default()), Vec3::new(6.0, 4.0, 6.0));
            }
            if ui.button("Empty").clicked() {
                self.add_object("Empty", ObjectData::Empty, Vec3::ZERO);
            }
        });
    }

    pub(crate) fn view_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("View", |ui| {
            for (mode, label) in ShadingMode::ALL.map(|m| (m, m.name())) {
                if ui.radio(self.viewport.options.shading == mode, label).clicked() {
                    self.viewport.options.shading = mode;
                }
            }
            ui.separator();
            ui.checkbox(&mut self.viewport.options.show_grid, "Grid");
            ui.checkbox(&mut self.regions.agent, "Agent panel");
            if ui.button("Frame All").clicked() {
                self.viewport.frame_scene(&self.scene);
            }
        });
    }

    pub(crate) fn render_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Render", |ui| {
            if ui.button("Render Image (F12)").clicked() {
                self.start_render(ui.ctx());
            }
            if ui.button("Render Animation (Ctrl+F12)").clicked() {
                self.start_render_animation(ui.ctx());
            }
            if ui.button("Show Render Window").clicked() {
                self.render.open = true;
            }
        });
    }

    pub(crate) fn help_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Help", |ui| {
            ui.label(format!("Redrob Shape {VERSION}"));
        });
    }

    pub(crate) fn outliner(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Outliner").color(crate::theme::TEXT_WEAK).size(12.0));
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            if self.scene.object_count() == 0 {
                ui.label(RichText::new("Scene Collection is empty").weak());
                return;
            }
            self.outliner_tree(ui);
        });
    }

    pub(crate) fn outliner_row(&mut self, ui: &mut egui::Ui, id: ObjectId, shift: bool) {
        let Some(object) = self.scene.object(id) else { return };
        let selected = self.scene.selection.contains(id);
        let icon = match object.data {
            ObjectData::Mesh { .. } => "▲",
            ObjectData::Light(_) => "☀",
            ObjectData::Camera(_) => "🎥",
            ObjectData::Empty => "○",
            ObjectData::Armature(_) => "🦴",
            ObjectData::Curve(_) => "∿",
            ObjectData::Text { .. } => "A",
            ObjectData::Metaball { .. } => "◍",
        };
        let eye = if object.visible { "" } else { "  (hidden)" };
        let text = format!("{icon} {}{eye}", object.name);
        let text = if self.scene.selection.active == Some(id) { RichText::new(text).strong() } else { RichText::new(text) };
        let visible = object.visible;
        // One row: eye toggle + name. Children follow, indented (so the row itself must not be
        // inside a horizontal layout).
        ui.horizontal(|ui| {
            let eye = if visible { "👁" } else { "―" };
            if ui.small_button(eye).clicked()
                && let Some(o) = self.scene.object_mut(id)
            {
                o.visible = !o.visible;
            }
            if ui.selectable_label(selected, text).clicked() {
                if shift {
                    self.scene.selection.toggle(id);
                } else {
                    self.scene.selection.set(id);
                }
            }
        });
        let children: Vec<ObjectId> = self.scene.children(Some(id)).collect();
        if !children.is_empty() {
            ui.indent(id.0, |ui| {
                for c in children {
                    self.outliner_row(ui, c, shift);
                }
            });
        }
    }

    /// Material slot of a mesh object: pick, create, and edit the assigned material.
    pub(crate) fn material_panel(&mut self, ui: &mut egui::Ui, id: ObjectId) {
        let Some(ObjectData::Mesh { material, .. }) = self.scene.object(id).map(|o| o.data.clone()) else { return };
        ui.horizontal(|ui| {
            ui.label(RichText::new("Material").strong());
            let current = material.and_then(|m| self.scene.material(m)).map(|m| m.name.clone()).unwrap_or("None".into());
            let mut pick: Option<Option<shape_scene::MaterialId>> = None;
            let mut new = false;
            egui::ComboBox::from_id_salt("material").selected_text(current).show_ui(ui, |ui| {
                if ui.selectable_label(material.is_none(), "None").clicked() {
                    pick = Some(None);
                }
                for (mid, m) in self.scene.materials() {
                    if ui.selectable_label(material == Some(mid), &m.name).clicked() {
                        pick = Some(Some(mid));
                    }
                }
                ui.separator();
                if ui.button("New material").clicked() {
                    new = true;
                }
            });
            if new {
                let n = self.scene.materials().count();
                self.edit("New Material", |scene| {
                    let mid = scene.add_material(Material { name: format!("Material.{n:03}"), ..Default::default() });
                    if let Some(ObjectData::Mesh { material, .. }) = scene.object_mut(id).map(|o| &mut o.data) {
                        *material = Some(mid);
                    }
                });
            } else if let Some(choice) = pick {
                self.edit("Assign Material", |scene| {
                    if let Some(ObjectData::Mesh { material, .. }) = scene.object_mut(id).map(|o| &mut o.data) {
                        *material = choice;
                    }
                });
            }
        });
        let Some(mid) = material else { return };
        let Some(original) = self.scene.material(mid).cloned() else { return };
        let mut m = original.clone();
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut m.name);
        });
        ui.horizontal(|ui| {
            ui.label("Base");
            ui.color_edit_button_rgba_unmultiplied(&mut m.base_color);
            ui.label("Emissive");
            ui.color_edit_button_rgb(&mut m.emissive);
        });
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut m.metallic).speed(0.01).range(0.0..=1.0).prefix("metallic "));
            ui.add(egui::DragValue::new(&mut m.roughness).speed(0.01).range(0.0..=1.0).prefix("roughness "));
        });
        // Base Color image texture (Blender: Image Texture node into Base Color). Needs UVs on the mesh.
        ui.horizontal(|ui| {
            ui.label("Image");
            let current = m.base_color_texture.and_then(|t| self.scene.texture(t)).map(|t| t.name.clone()).unwrap_or("none".into());
            ui.label(RichText::new(current).weak());
            if ui.button("Open…").clicked()
                && let Some(path) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg"]).pick_file()
            {
                match shape_scene::Texture::load(&path) {
                    Ok(tex) => {
                        let tid = self.scene.add_texture(tex);
                        m.base_color_texture = Some(tid);
                        m.set_image_texture(Some(tid));
                        if self.scene.object_mesh(id).is_some_and(|(mid, _)| self.scene.mesh(mid).is_none_or(|d| d.uvs.is_none())) {
                            self.generate_uvs("UV Box", shape_ops::uv_box);
                            self.status = "Image texture set; box UVs generated (Data > UV Maps to change)".into();
                        }
                    }
                    Err(e) => self.status = format!("Image failed: {e}"),
                }
            }
            if m.base_color_texture.is_some() && ui.button("✕").clicked() {
                m.base_color_texture = None;
                m.set_image_texture(None);
            }
        });
        ui.horizontal(|ui| {
            ui.label("Viewport Display");
            ui.color_edit_button_rgba_unmultiplied(&mut m.viewport_color).on_hover_text("Colour in Solid shading");
        });
        ui.horizontal(|ui| {
            if m.nodes.is_none() {
                if ui.button("Use Nodes").clicked() {
                    m.nodes = Some(shape_scene::NodeTree::from_material(&m));
                    self.set_workspace(crate::layout::Workspace::Shading);
                }
            } else {
                ui.label(RichText::new("Nodes: edit in the Shading workspace").weak());
            }
        });
        if m != original {
            // With nodes, the panel writes the BSDF's constants; linked inputs stay node-driven.
            if let Some(mut t) = m.nodes.take() {
                t.set_from_material(&m);
                m.nodes = Some(t);
                m.apply_nodes();
            }
            if self.property_before.is_none() {
                self.property_before = Some(self.scene.clone());
            }
            if let Some(slot) = self.scene.material_mut(mid) {
                *slot = m;
            }
        }
    }

    pub(crate) fn modifier_panel(&mut self, ui: &mut egui::Ui, id: ObjectId) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Modifiers").strong());
            ui.menu_button("Add", |ui| {
                if ui.button("Mirror X").clicked() {
                    self.add_modifier(Modifier::Mirror { axis: 0, merge_distance: 1e-4 });
                }
                if ui.button("Array").clicked() {
                    self.add_modifier(Modifier::Array { count: 2, offset: Vec3::new(2.5, 0.0, 0.0), merge_distance: 0.0 });
                }
                if ui.button("Subdivision").clicked() {
                    self.add_modifier(Modifier::Subdivision { levels: 1, smooth: true });
                }
                if ui.button("Smooth").clicked() {
                    self.add_modifier(Modifier::Smooth { iterations: 1, factor: 0.5 });
                }
                if ui.button("Bevel").clicked() {
                    self.add_modifier(Modifier::Bevel { offset: 0.05, segments: 1 });
                }
                ui.separator();
                if ui.button("Solidify").clicked() {
                    self.add_modifier(Modifier::Solidify { thickness: 0.1, offset: -1.0 });
                }
                if ui.button("Decimate").clicked() {
                    self.add_modifier(Modifier::Decimate { ratio: 0.5 });
                }
                if ui.button("Wireframe").clicked() {
                    self.add_modifier(Modifier::Wireframe { thickness: 0.05 });
                }
                if ui.button("Triangulate").clicked() {
                    self.add_modifier(Modifier::Triangulate);
                }
                if ui.button("Screw").clicked() {
                    self.add_modifier(Modifier::Screw { axis: 1, angle: std::f32::consts::TAU, steps: 16, screw: 0.0 });
                }
                if ui.button("Simple Deform").clicked() {
                    self.add_modifier(Modifier::SimpleDeform { mode: shape_scene::DeformMode::Twist, axis: 1, factor: 0.785 });
                }
                if ui.button("Displace").clicked() {
                    self.add_modifier(Modifier::Displace { strength: 0.1, scale: 1.0 });
                }
                if ui.button("Weld").clicked() {
                    self.add_modifier(Modifier::Weld { distance: 0.001 });
                }
                ui.separator();
                if ui.button("Boolean").clicked() {
                    self.add_modifier(Modifier::Boolean { op: shape_scene::BooleanOp::Difference, target: None });
                }
                if ui.button("Shrinkwrap").clicked() {
                    self.add_modifier(Modifier::Shrinkwrap { target: None, offset: 0.0 });
                }
                if ui.button("Armature").clicked() {
                    self.add_modifier(Modifier::Armature { object: None });
                }
                ui.separator();
                if ui.button("Geometry Nodes").clicked() {
                    self.add_modifier(Modifier::GeometryNodes { tree: shape_scene::GeoNodeTree::default_tree() });
                }
            });
        });
        let mut stack = self.scene.modifiers(id).to_vec();
        let before = stack.clone();
        let mut remove: Option<usize> = None;
        // Other mesh objects, for the target pickers.
        let others: Vec<(ObjectId, String)> = self
            .scene
            .objects()
            .filter(|(oid, o)| *oid != id && matches!(o.data, ObjectData::Mesh { .. }))
            .map(|(oid, o)| (oid, o.name.clone()))
            .collect();
        let armatures: Vec<(ObjectId, String)> =
            self.scene.objects().filter(|(_, o)| matches!(o.data, ObjectData::Armature(_))).map(|(oid, o)| (oid, o.name.clone())).collect();
        let target_picker = |ui: &mut egui::Ui, salt: &str, target: &mut Option<ObjectId>| {
            let current = target.and_then(|t| others.iter().find(|(o, _)| *o == t)).map(|(_, n)| n.clone()).unwrap_or("Object: none".into());
            egui::ComboBox::from_id_salt(salt).selected_text(current).show_ui(ui, |ui| {
                if ui.selectable_label(target.is_none(), "none").clicked() {
                    *target = None;
                }
                for (oid, name) in &others {
                    if ui.selectable_label(*target == Some(*oid), name).clicked() {
                        *target = Some(*oid);
                    }
                }
            });
        };
        for (i, m) in stack.iter_mut().enumerate() {
            ui.push_id(i, |ui| {
                ui.horizontal(|ui| {
                    ui.label(m.name());
                    if ui.small_button("✕").clicked() {
                        remove = Some(i);
                    }
                });
                match m {
                    Modifier::Mirror { axis, merge_distance } => {
                        ui.horizontal(|ui| {
                            for (a, label) in [(0u8, "X"), (1, "Y"), (2, "Z")] {
                                ui.radio_value(axis, a, label);
                            }
                            ui.add(egui::DragValue::new(merge_distance).speed(0.0001).prefix("merge ").max_decimals(4));
                        });
                    }
                    Modifier::Array { count, offset, .. } => {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(count).range(1..=64).prefix("count "));
                        });
                        vec3_row(ui, "Offset", offset, 0.05);
                    }
                    Modifier::Subdivision { levels, smooth } => {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(levels).range(0..=4).prefix("levels "));
                            ui.checkbox(smooth, "smooth");
                        });
                    }
                    Modifier::Smooth { iterations, factor } => {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(iterations).range(0..=50).prefix("iterations "));
                            ui.add(egui::DragValue::new(factor).range(0.0..=1.0).speed(0.01).prefix("factor "));
                        });
                    }
                    Modifier::Bevel { offset, segments } => {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(offset).range(0.0..=10.0).speed(0.005).prefix("Amount "));
                            ui.add(egui::DragValue::new(segments).range(1..=32).prefix("Segments "));
                        });
                    }
                    Modifier::Solidify { thickness, offset } => {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(thickness).range(-10.0..=10.0).speed(0.005).prefix("Thickness "));
                            ui.add(egui::DragValue::new(offset).range(-1.0..=1.0).speed(0.01).prefix("Offset "));
                        });
                    }
                    Modifier::Decimate { ratio } => {
                        ui.add(egui::DragValue::new(ratio).range(0.0..=1.0).speed(0.005).prefix("Ratio "));
                    }
                    Modifier::Wireframe { thickness } => {
                        ui.add(egui::DragValue::new(thickness).range(0.0..=10.0).speed(0.002).prefix("Thickness "));
                    }
                    Modifier::Triangulate => {}
                    Modifier::Screw { axis, angle, steps, screw } => {
                        ui.horizontal(|ui| {
                            for (i, l) in ["X", "Y", "Z"].iter().enumerate() {
                                ui.selectable_value(axis, i as u8, *l);
                            }
                            let mut deg = angle.to_degrees();
                            if ui.add(egui::DragValue::new(&mut deg).range(0.0..=720.0).prefix("Angle ° ")).changed() {
                                *angle = deg.to_radians();
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(steps).range(1..=256).prefix("Steps "));
                            ui.add(egui::DragValue::new(screw).range(-100.0..=100.0).speed(0.01).prefix("Screw "));
                        });
                    }
                    Modifier::SimpleDeform { mode, axis, factor } => {
                        ui.horizontal(|ui| {
                            for m in shape_scene::DeformMode::ALL {
                                ui.selectable_value(mode, m, m.name());
                            }
                        });
                        ui.horizontal(|ui| {
                            for (i, l) in ["X", "Y", "Z"].iter().enumerate() {
                                ui.selectable_value(axis, i as u8, *l);
                            }
                            let angular = matches!(mode, shape_scene::DeformMode::Twist | shape_scene::DeformMode::Bend);
                            if angular {
                                let mut deg = factor.to_degrees();
                                if ui.add(egui::DragValue::new(&mut deg).range(-720.0..=720.0).prefix("Angle ° ")).changed() {
                                    *factor = deg.to_radians();
                                }
                            } else {
                                ui.add(egui::DragValue::new(factor).range(-10.0..=10.0).speed(0.01).prefix("Factor "));
                            }
                        });
                    }
                    Modifier::Displace { strength, scale } => {
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(strength).range(-10.0..=10.0).speed(0.005).prefix("Strength "));
                            ui.add(egui::DragValue::new(scale).range(0.01..=100.0).speed(0.01).prefix("Scale "));
                        });
                    }
                    Modifier::Weld { distance } => {
                        ui.add(egui::DragValue::new(distance).range(0.0..=10.0).speed(0.0005).prefix("Distance "));
                    }
                    Modifier::Boolean { op, target } => {
                        ui.horizontal(|ui| {
                            for o in shape_scene::BooleanOp::ALL {
                                ui.selectable_value(op, o, o.name());
                            }
                        });
                        target_picker(ui, "boolean-target", target);
                    }
                    Modifier::Shrinkwrap { target, offset } => {
                        target_picker(ui, "shrinkwrap-target", target);
                        ui.add(egui::DragValue::new(offset).range(-10.0..=10.0).speed(0.005).prefix("Offset "));
                    }
                    Modifier::GeometryNodes { tree } => {
                        ui.label(RichText::new(format!("{} nodes · edit in the Geometry Nodes workspace", tree.nodes.len())).weak());
                    }
                    Modifier::Armature { object } => {
                        let current =
                            object.and_then(|t| armatures.iter().find(|(o, _)| *o == t)).map(|(_, n)| n.clone()).unwrap_or("Object: none".into());
                        egui::ComboBox::from_id_salt("armature-object").selected_text(current).show_ui(ui, |ui| {
                            if ui.selectable_label(object.is_none(), "none").clicked() {
                                *object = None;
                            }
                            for (oid, name) in &armatures {
                                if ui.selectable_label(*object == Some(*oid), name).clicked() {
                                    *object = Some(*oid);
                                }
                            }
                        });
                    }
                }
            });
        }
        if let Some(i) = remove {
            stack.remove(i);
            let name = format!("Remove {}", before[i].name());
            self.edit(&name, |scene| {
                if let Some(s) = scene.modifiers_mut(id) {
                    *s = stack.clone();
                }
            });
        } else if stack != before {
            if self.property_before.is_none() {
                self.property_before = Some(self.scene.clone());
            }
            if let Some(s) = self.scene.modifiers_mut(id) {
                *s = stack;
            }
        }
    }
}

/// Native open/save dialog filtered to the formats `shape-format` handles.
fn file_dialog(open: bool, current: &Option<PathBuf>) -> Option<PathBuf> {
    let mut dialog = rfd::FileDialog::new();
    let all: Vec<&str> = shape_format::Format::ALL.iter().filter(|f| open || f.can_export()).flat_map(|f| f.extensions().iter().copied()).collect();
    dialog = dialog.add_filter("3D scenes", &all).add_filter("Redrob Shape", &["rshape"]);
    if let Some(dir) = current.as_ref().and_then(|p| p.parent()) {
        dialog = dialog.set_directory(dir);
    }
    if open { dialog.pick_file() } else { dialog.set_file_name("scene.rshape").save_file() }
}

/// Three drag values on one row. Returns true when any changed.
pub(crate) fn vec3_row(ui: &mut egui::Ui, label: &str, v: &mut Vec3, speed: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
            changed |= ui.add(egui::DragValue::new(&mut v[i]).speed(speed).prefix(format!("{axis} ")).max_decimals(3)).changed();
        }
    });
    changed
}
