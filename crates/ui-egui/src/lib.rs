//! Editor panels, hotkeys and the egui shell.
//!
//! [`ShapeApp`] owns the [`Scene`], its undo [`History`] and the viewport. Stage 3 draws the scene
//! in a wgpu viewport with orbit/pan/zoom, click picking, an Add menu of primitives, an outliner
//! that selects, and a properties panel for the active object.

pub mod viewport;

use egui::RichText;
use shape_geom::primitives;
use shape_gpu::ShadingMode;
use shape_math::{Transform, Vec3};
use shape_scene::{Modifier, ObjectData, ObjectId, Scene};
use shape_undo::{History, Snapshot};
use std::path::PathBuf;
pub use viewport::{GpuContext, ViewportPanel};

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-ui-egui";

/// Version shown in the status bar; comes from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The whole editor window.
pub struct ShapeApp {
    pub scene: Scene,
    pub history: History<Scene>,
    pub viewport: ViewportPanel,
    status: String,
    file: Option<PathBuf>,
}

/// Primitives the Add menu offers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Primitive {
    Cube,
    Plane,
    Sphere,
    Torus,
    Cylinder,
}

impl Primitive {
    pub const ALL: [Primitive; 5] = [Primitive::Cube, Primitive::Plane, Primitive::Sphere, Primitive::Torus, Primitive::Cylinder];

    pub fn name(self) -> &'static str {
        match self {
            Primitive::Cube => "Cube",
            Primitive::Plane => "Plane",
            Primitive::Sphere => "UV Sphere",
            Primitive::Torus => "Torus",
            Primitive::Cylinder => "Cylinder",
        }
    }

    pub fn mesh(self) -> shape_geom::Mesh {
        match self {
            Primitive::Cube => primitives::cube(2.0),
            Primitive::Plane => primitives::plane(4.0, 4, 4),
            Primitive::Sphere => primitives::uv_sphere(1.0, 32, 16),
            Primitive::Torus => primitives::torus(1.0, 0.25, 48, 12),
            Primitive::Cylinder => primitives::cylinder(1.0, 2.0, 32),
        }
    }
}

impl Default for ShapeApp {
    fn default() -> Self {
        Self::with_gpu(None)
    }
}

impl ShapeApp {
    /// Title of the main window.
    pub const WINDOW_TITLE: &str = "Redrob Shape";

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        Self::with_gpu(GpuContext::from_eframe(cc))
    }

    pub fn with_gpu(gpu: Option<GpuContext>) -> Self {
        Self { scene: Scene::new("Scene"), history: History::default(), viewport: ViewportPanel::new(gpu), status: String::new(), file: None }
    }

    /// Apply an undoable edit to the scene.
    pub fn edit(&mut self, name: &str, edit: impl FnOnce(&mut Scene)) {
        let cmd = Snapshot::new(name, &self.scene, edit);
        self.history.push(Box::new(cmd), &mut self.scene);
        self.status = name.to_owned();
    }

    pub fn add_primitive(&mut self, p: Primitive) -> ObjectId {
        let n = self.scene.object_count();
        let at = self.scene.selection_center();
        let mut id = ObjectId(0);
        self.edit(&format!("Add {}", p.name()), |scene| {
            let name = if n == 0 { p.name().to_owned() } else { format!("{}.{:03}", p.name(), n) };
            id = scene.add_mesh_object(name, p.mesh(), None);
            scene.object_mut(id).unwrap().transform = Transform::from_translation(at);
            scene.selection.set(id);
        });
        id
    }

    /// Replace the active object's mesh with `op(mesh)`, undoably. Modifiers stay in place.
    pub fn apply_mesh_op(&mut self, name: &str, op: impl Fn(&shape_geom::Mesh) -> shape_ops::Result<shape_geom::Mesh>) {
        let Some(id) = self.scene.selection.active else { return };
        let Some((mesh_id, mesh)) = self.scene.object_mesh(id) else { return };
        match op(mesh) {
            Ok(new_mesh) => self.edit(name, |scene| {
                if let Some(data) = scene.mesh_mut(mesh_id) {
                    data.mesh = new_mesh;
                }
            }),
            Err(e) => self.status = format!("{name} failed: {e}"),
        }
    }

    /// Bake the active object's modifier stack into its mesh.
    pub fn apply_modifiers(&mut self) {
        let Some(id) = self.scene.selection.active else { return };
        let Some(evaluated) = shape_ops::evaluated_mesh(&self.scene, id).map(|m| m.into_owned()) else { return };
        let Some((mesh_id, _)) = self.scene.object_mesh(id) else { return };
        self.edit("Apply Modifiers", |scene| {
            if let Some(data) = scene.mesh_mut(mesh_id) {
                data.mesh = evaluated;
            }
            if let Some(stack) = scene.modifiers_mut(id) {
                stack.clear();
            }
        });
    }

    pub fn add_modifier(&mut self, modifier: Modifier) {
        let Some(id) = self.scene.selection.active else { return };
        let name = format!("Add {}", modifier.name());
        self.edit(&name, |scene| {
            if let Some(stack) = scene.modifiers_mut(id) {
                stack.push(modifier);
            }
        });
    }

    pub fn delete_selected(&mut self) {
        if self.scene.selection.is_empty() {
            return;
        }
        self.edit("Delete", |scene| {
            let ids: Vec<ObjectId> = scene.selection.objects.iter().copied().collect();
            for id in ids {
                scene.remove_object(id);
            }
            scene.prune_meshes();
        });
    }

    pub fn open(&mut self, path: PathBuf) {
        match shape_format::import(&path) {
            Ok(scene) => {
                self.scene = scene;
                self.history.clear();
                self.viewport.frame_scene(&self.scene);
                self.status = format!("Opened {}", path.display());
                self.file = Some(path);
            }
            Err(e) => self.status = format!("Open failed: {e}"),
        }
    }

    pub fn save_as(&mut self, path: PathBuf) {
        match shape_format::export(&self.scene, &path) {
            Ok(()) => {
                self.status = format!("Saved {}", path.display());
                if path.extension().is_some_and(|e| e == "rshape") {
                    self.file = Some(path);
                }
            }
            Err(e) => self.status = format!("Save failed: {e}"),
        }
    }

    fn undo(&mut self) {
        if let Some(name) = self.history.undo(&mut self.scene) {
            self.status = format!("Undo {name}");
        }
    }

    fn redo(&mut self) {
        if let Some(name) = self.history.redo(&mut self.scene) {
            self.status = format!("Redo {name}");
        }
    }

    fn hotkeys(&mut self, ui: &egui::Ui) {
        let (undo, redo, del, frame, wire, solid) = ui.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z),
                i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z)
                    || i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Delete) || i.consume_key(egui::Modifiers::NONE, egui::Key::X),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Home),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Num1),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Num2),
            )
        });
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        if redo {
            self.redo();
        } else if undo {
            self.undo();
        }
        if del {
            self.delete_selected();
        }
        if frame {
            self.viewport.frame_scene(&self.scene);
        }
        if wire {
            self.viewport.options.shading = ShadingMode::Wireframe;
        }
        if solid {
            self.viewport.options.shading = ShadingMode::Solid;
        }
    }

    /// Draw one frame. Split out of `eframe::App::ui` so `egui_kittest` can drive it.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        self.hotkeys(ui);
        self.menu_bar(ui);
        self.status_bar(ui);
        self.outliner(ui);
        self.properties(ui);
        let events = egui::CentralPanel::default().frame(egui::Frame::NONE).show(ui, |ui| self.viewport.ui(ui, &self.scene)).inner;
        if let Some(hit) = events.clicked {
            match hit {
                Some(id) if events.shift => self.scene.selection.toggle(id),
                Some(id) => self.scene.selection.set(id),
                None => self.scene.selection.clear(),
            }
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New").clicked() {
                        self.scene = Scene::new("Scene");
                        self.history.clear();
                        self.file = None;
                        self.status = "New scene".into();
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

    fn status_bar(&self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(if self.status.is_empty() { "Ready" } else { &self.status });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("Redrob Shape {VERSION}")).weak());
                });
            });
        });
    }

    fn outliner(&mut self, ui: &mut egui::Ui) {
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

    fn properties(&mut self, ui: &mut egui::Ui) {
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
                if !self.scene.modifiers(id).is_empty()
                    && let Some(ev) = shape_ops::evaluated_mesh(&self.scene, id)
                {
                    ui.label(RichText::new(format!("evaluated: {} vertices, {} faces", ev.vertex_count(), ev.face_count())).weak());
                }
            }
            if edited != object {
                // Live edits are not undoable individually; one snapshot per drag is a later stage.
                if let Some(o) = self.scene.object_mut(id) {
                    *o = edited;
                }
            }
            if matches!(object.data, ObjectData::Mesh { .. }) {
                ui.separator();
                self.modifier_panel(ui, id);
            }
        });
    }
}

impl ShapeApp {
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
            // Live parameter edits are applied directly; a per-drag snapshot comes with edit mode.
            if let Some(s) = self.scene.modifiers_mut(id) {
                *s = stack;
            }
        }
    }
}

/// Three drag values on one row. Returns true when any changed.
fn vec3_row(ui: &mut egui::Ui, label: &str, v: &mut Vec3, speed: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
            changed |= ui.add(egui::DragValue::new(&mut v[i]).speed(speed).prefix(format!("{axis} ")).max_decimals(3)).changed();
        }
    });
    changed
}

impl eframe::App for ShapeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui(ui);
    }
}

impl std::fmt::Debug for ShapeApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShapeApp").field("objects", &self.scene.object_count()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    #[test]
    fn crate_name() {
        assert_eq!(CRATE, "shape-ui-egui");
    }

    #[test]
    fn shell_shows_every_region() {
        let mut app = ShapeApp::default();
        let mut harness = Harness::new_ui(|ui| app.ui(ui));
        harness.run();
        for label in ["Outliner", "Properties", "Ready", "File", "Add", "View"] {
            harness.get_by_label(label);
        }
    }

    #[test]
    fn add_select_delete_undo() {
        let mut app = ShapeApp::default();
        let a = app.add_primitive(Primitive::Cube);
        let b = app.add_primitive(Primitive::Sphere);
        assert_eq!(app.scene.object_count(), 2);
        assert_eq!(app.scene.selection.active, Some(b));
        assert_eq!(app.scene.object(b).unwrap().name, "UV Sphere.001");
        app.scene.selection.set(a);
        app.delete_selected();
        assert_eq!(app.scene.object_count(), 1);
        assert!(app.scene.object(a).is_none());
        app.history.undo(&mut app.scene);
        assert_eq!(app.scene.object_count(), 2);
        assert_eq!(app.scene.object(a).unwrap().name, "Cube");
        assert_eq!(app.history.redo_name(), Some("Delete"));
    }

    #[test]
    fn mesh_ops_and_modifiers_are_undoable() {
        let mut app = ShapeApp::default();
        let id = app.add_primitive(Primitive::Cube);
        app.apply_mesh_op("Subdivide", shape_ops::catmull_clark);
        assert_eq!(app.scene.object_mesh(id).unwrap().1.face_count(), 24);
        app.history.undo(&mut app.scene);
        assert_eq!(app.scene.object_mesh(id).unwrap().1.face_count(), 6);
        app.add_modifier(Modifier::Subdivision { levels: 2, smooth: true });
        assert_eq!(app.scene.modifiers(id).len(), 1);
        assert_eq!(shape_ops::evaluated_mesh(&app.scene, id).unwrap().face_count(), 96);
        app.apply_modifiers();
        assert!(app.scene.modifiers(id).is_empty());
        assert_eq!(app.scene.object_mesh(id).unwrap().1.face_count(), 96);
        app.history.undo(&mut app.scene);
        assert_eq!(app.scene.object_mesh(id).unwrap().1.face_count(), 6);
        assert_eq!(app.scene.modifiers(id).len(), 1);
    }

    #[test]
    fn outliner_lists_objects_and_properties_show_active() {
        let mut app = ShapeApp::default();
        app.add_primitive(Primitive::Torus);
        let mut harness = Harness::new_ui(|ui| app.ui(ui));
        harness.run();
        assert!(harness.query_all_by_label_contains("Torus").count() >= 2, "outliner row and status");
        harness.get_by_label("Location");
        harness.get_by_label_contains("vertices");
    }
}
