//! Menu bar, status bar, outliner and properties panels.

use crate::{Primitive, ShapeApp, VERSION};
use egui::RichText;
use shape_gpu::ShadingMode;
use shape_math::Vec3;
use shape_scene::{Camera, Light, LightKind, Material, Modifier, ObjectData, ObjectId, Scene};
use shape_undo::Snapshot;
use std::path::PathBuf;

impl ShapeApp {
    pub(crate) fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
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
                ui.menu_button("Edit", |ui| {
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
                ui.menu_button("View", |ui| {
                    for (mode, label) in
                        [(ShadingMode::Wireframe, "Wireframe"), (ShadingMode::Solid, "Solid"), (ShadingMode::SolidWire, "Solid + Wire")]
                    {
                        if ui.radio(self.viewport.options.shading == mode, label).clicked() {
                            self.viewport.options.shading = mode;
                        }
                    }
                    ui.separator();
                    ui.checkbox(&mut self.viewport.options.show_grid, "Grid");
                    if ui.button("Frame All").clicked() {
                        self.viewport.frame_scene(&self.scene);
                    }
                });
                ui.menu_button("Render", |_ui| {});
                ui.menu_button("Help", |ui| {
                    ui.label(format!("Redrob Shape {VERSION}"));
                });
            });
        });
    }

    pub(crate) fn status_bar(&self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(if self.status.is_empty() { "Ready" } else { &self.status });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("Redrob Shape {VERSION}")).weak());
                });
            });
        });
    }

    pub(crate) fn outliner(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("outliner").default_size(220.0).show(ui, |ui| {
            ui.heading("Outliner");
            ui.separator();
            if self.scene.object_count() == 0 {
                ui.label(RichText::new("Scene is empty").weak());
                return;
            }
            let shift = ui.input(|i| i.modifiers.shift);
            let roots: Vec<ObjectId> = self.scene.roots().collect();
            for id in roots {
                self.outliner_row(ui, id, shift);
            }
        });
    }

    fn outliner_row(&mut self, ui: &mut egui::Ui, id: ObjectId, shift: bool) {
        let Some(object) = self.scene.object(id) else { return };
        let selected = self.scene.selection.contains(id);
        let icon = match object.data {
            ObjectData::Mesh { .. } => "▲",
            ObjectData::Light(_) => "☀",
            ObjectData::Camera(_) => "🎥",
            ObjectData::Empty => "○",
        };
        let text = format!("{icon} {}", object.name);
        let text = if self.scene.selection.active == Some(id) { RichText::new(text).strong() } else { RichText::new(text) };
        if ui.selectable_label(selected, text).clicked() {
            if shift {
                self.scene.selection.toggle(id);
            } else {
                self.scene.selection.set(id);
            }
        }
        let children: Vec<ObjectId> = self.scene.children(Some(id)).collect();
        if !children.is_empty() {
            ui.indent(id.0, |ui| {
                for c in children {
                    self.outliner_row(ui, c, shift);
                }
            });
        }
    }

    pub(crate) fn properties(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("properties").default_size(260.0).show(ui, |ui| {
            ui.heading("Properties");
            ui.separator();
            let Some(id) = self.scene.selection.active else {
                ui.label(RichText::new("Nothing selected").weak());
                return;
            };
            let Some(object) = self.scene.object(id).cloned() else { return };
            let mut edited = object.clone();
            ui.horizontal(|ui| {
                ui.label("Name");
                ui.text_edit_singleline(&mut edited.name);
            });
            ui.checkbox(&mut edited.visible, "Visible");
            ui.separator();
            ui.label(RichText::new("Transform").strong());
            vec3_row(ui, "Location", &mut edited.transform.translation, 0.01);
            let (mut rx, mut ry, mut rz) = edited.transform.rotation.to_euler(shape_math::glam_euler());
            let mut deg = Vec3::new(rx.to_degrees(), ry.to_degrees(), rz.to_degrees());
            if vec3_row(ui, "Rotation", &mut deg, 0.5) {
                (rx, ry, rz) = (deg.x.to_radians(), deg.y.to_radians(), deg.z.to_radians());
                edited.transform.rotation = shape_math::Quat::from_euler(shape_math::glam_euler(), rx, ry, rz);
            }
            vec3_row(ui, "Scale", &mut edited.transform.scale, 0.01);
            if let ObjectData::Mesh { mesh, .. } = &object.data
                && let Some(data) = self.scene.mesh(*mesh)
            {
                ui.separator();
                ui.label(RichText::new("Mesh").strong());
                ui.label(format!("{} vertices, {} edges, {} faces", data.mesh.vertex_count(), data.mesh.edge_count(), data.mesh.face_count()));
                if data.uvs.is_some() {
                    ui.label(RichText::new("has UVs").weak());
                }
                if !self.scene.modifiers(id).is_empty()
                    && let Some(ev) = shape_ops::evaluated_mesh(&self.scene, id)
                {
                    ui.label(RichText::new(format!("evaluated: {} vertices, {} faces", ev.vertex_count(), ev.face_count())).weak());
                }
            }
            match &mut edited.data {
                ObjectData::Light(light) => {
                    ui.separator();
                    ui.label(RichText::new("Light").strong());
                    ui.horizontal(|ui| {
                        for (kind, label) in [(LightKind::Point, "Point"), (LightKind::Sun, "Sun"), (Light::spot(0.0).kind, "Spot")] {
                            let same = std::mem::discriminant(&light.kind) == std::mem::discriminant(&kind);
                            if ui.radio(same, label).clicked() && !same {
                                light.kind = kind;
                            }
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("Color");
                        ui.color_edit_button_rgb(&mut light.color);
                        let label = if matches!(light.kind, LightKind::Sun) { "Strength" } else { "Power (W)" };
                        ui.add(egui::DragValue::new(&mut light.power).speed(1.0).range(0.0..=1.0e6).prefix(format!("{label} ")));
                    });
                    if let LightKind::Spot { inner_angle, outer_angle } = &mut light.kind {
                        let (mut inner, mut outer) = (inner_angle.to_degrees(), outer_angle.to_degrees());
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut inner).range(1.0..=89.0).prefix("inner ° "));
                            ui.add(egui::DragValue::new(&mut outer).range(1.0..=90.0).prefix("outer ° "));
                        });
                        *inner_angle = inner.to_radians();
                        *outer_angle = outer.max(inner + 0.5).to_radians();
                    }
                }
                ObjectData::Camera(cam) => {
                    ui.separator();
                    ui.label(RichText::new("Camera").strong());
                    let mut fov = cam.fov_y.to_degrees();
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut fov).range(5.0..=170.0).prefix("FOV ° "));
                        ui.add(egui::DragValue::new(&mut cam.near).speed(0.01).range(0.001..=100.0).prefix("near "));
                        ui.add(egui::DragValue::new(&mut cam.far).speed(1.0).range(1.0..=1.0e5).prefix("far "));
                    });
                    cam.fov_y = fov.to_radians();
                }
                _ => {}
            }
            if edited != object {
                if self.property_before.is_none() {
                    self.property_before = Some(self.scene.clone());
                }
                if let Some(o) = self.scene.object_mut(id) {
                    *o = edited;
                }
            }
            if matches!(object.data, ObjectData::Mesh { .. }) {
                ui.separator();
                self.material_panel(ui, id);
                ui.separator();
                self.modifier_panel(ui, id);
            }
            // One undo step per drag/typing session, recorded when pointer and keyboard let go.
            let idle = ui.input(|i| !i.pointer.any_down()) && !ui.ctx().egui_wants_keyboard_input();
            if idle && let Some(before) = self.property_before.take() {
                let after = std::mem::replace(&mut self.scene, before);
                let cmd = Snapshot::new("Edit Properties", &self.scene, move |s| *s = after);
                self.history.push(Box::new(cmd), &mut self.scene);
            }
        });
    }

    /// Material slot of a mesh object: pick, create, and edit the assigned material.
    fn material_panel(&mut self, ui: &mut egui::Ui, id: ObjectId) {
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
        if m != original {
            if self.property_before.is_none() {
                self.property_before = Some(self.scene.clone());
            }
            if let Some(slot) = self.scene.material_mut(mid) {
                *slot = m;
            }
        }
    }

    fn modifier_panel(&mut self, ui: &mut egui::Ui, id: ObjectId) {
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
            });
        });
        let mut stack = self.scene.modifiers(id).to_vec();
        let before = stack.clone();
        let mut remove: Option<usize> = None;
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
    let all: Vec<&str> = shape_format::Format::ALL.iter().flat_map(|f| f.extensions().iter().copied()).collect();
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
