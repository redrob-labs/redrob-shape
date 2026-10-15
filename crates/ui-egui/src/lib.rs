//! Editor panels, hotkeys and the egui shell.
//!
//! [`ShapeApp`] owns the [`Scene`], its undo [`History`], the viewport, the active [`Tool`] and any
//! running [`Modal`]. Panels live in `panels`, modal operators in `tools`, the 3D view in
//! `viewport`.

pub mod agent;
mod panels;
pub mod render_panel;
pub mod tools;
pub mod viewport;

pub use agent::{AgentFactory, AgentPanel};
use egui::RichText;
pub use render_panel::RenderPanel;
pub use shape_agent::ControlServer;
use shape_geom::primitives;
use shape_gpu::ShadingMode;
use shape_math::{Transform, Vec2};
use shape_scene::{EditSelection, Modifier, ObjectId, Scene};
use shape_undo::{History, Snapshot};
use std::path::PathBuf;
pub use tools::{Modal, ModalEnd, ModalKind, Tool};
pub use viewport::{GpuContext, ViewportInput, ViewportPanel};

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-ui-egui";

/// Version shown in the status bar; comes from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The whole editor window.
pub struct ShapeApp {
    pub scene: Scene,
    pub history: History<Scene>,
    pub viewport: ViewportPanel,
    pub tool: Tool,
    pub modal: Option<Modal>,
    status: String,
    file: Option<PathBuf>,
    /// Scene before the property drag in progress, turned into one undo step on release.
    property_before: Option<Scene>,
    /// Pointer over the viewport last frame, in viewport pixels.
    last_pointer: Option<Vec2>,
    pub agent: AgentPanel,
    /// Tool calls from the MCP server, run on the scene each frame.
    control: Option<ControlServer>,
    pub show_agent: bool,
    pub render: RenderPanel,
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
        Self {
            scene: Scene::new("Scene"),
            history: History::default(),
            viewport: ViewportPanel::new(gpu),
            tool: Tool::Select,
            modal: None,
            status: String::new(),
            file: None,
            property_before: None,
            last_pointer: None,
            agent: AgentPanel::new(None),
            control: None,
            show_agent: true,
            render: RenderPanel::new(),
        }
    }

    /// Camera for a render: the scene's first camera object, else the viewport's view.
    pub fn render_camera(&self) -> shape_render::RenderCamera {
        shape_render::RenderScene::scene_camera(&self.scene).unwrap_or_else(|| {
            let c = &self.viewport.camera;
            shape_render::RenderCamera::look_at(c.position(), c.target, c.fov_y)
        })
    }

    /// Start (or restart) a render of the current scene.
    pub fn start_render(&mut self, ctx: &egui::Context) {
        let camera = self.render_camera();
        self.render.start(&self.scene, camera, ctx.clone());
        self.status = "Rendering…".into();
    }

    /// Attach the agent process factory and the control channel its tools arrive on.
    pub fn with_agent(mut self, factory: Option<AgentFactory>, control: Option<ControlServer>) -> Self {
        self.agent = AgentPanel::new(factory);
        self.control = control;
        self
    }

    /// Run one tool call from the agent on the scene, as one undo step. Public so the control
    /// loop and tests share it.
    pub fn run_tool(&mut self, name: &str, params: &serde_json::Value) -> Result<serde_json::Value, String> {
        if shape_agent::tools::mutates(name) {
            let before = self.scene.clone();
            match shape_agent::tools::execute(&mut self.scene, name, params) {
                Ok(v) => {
                    self.record_from(&format!("Agent: {name}"), before);
                    self.status = format!("Agent ran {name}");
                    Ok(v)
                }
                Err(e) => {
                    self.scene = before;
                    Err(e)
                }
            }
        } else {
            shape_agent::tools::execute(&mut self.scene, name, params)
        }
    }

    fn drain_control(&mut self) {
        let Some(control) = &self.control else { return };
        let pending: Vec<shape_agent::ControlRequest> = control.requests.try_iter().collect();
        for req in pending {
            let (method, params) = (req.method.clone(), req.params.clone());
            match self.run_tool(&method, &params) {
                Ok(v) => req.ok(v),
                Err(e) => req.err(e),
            }
        }
    }

    /// Apply an undoable edit to the scene.
    pub fn edit(&mut self, name: &str, edit: impl FnOnce(&mut Scene)) {
        let cmd = Snapshot::new(name, &self.scene, edit);
        self.history.push(Box::new(cmd), &mut self.scene);
        self.status = name.to_owned();
    }

    /// Record the current scene as the result of `name`, with `before` as the undo state.
    fn record_from(&mut self, name: &str, before: Scene) {
        let after = std::mem::replace(&mut self.scene, before);
        let cmd = Snapshot::new(name, &self.scene, move |s| *s = after);
        self.history.push(Box::new(cmd), &mut self.scene);
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

    /// Add any object at a position, undoably, and make it active.
    pub fn add_object(&mut self, name: &str, data: shape_scene::ObjectData, at: shape_math::Vec3) -> ObjectId {
        let n = self.scene.object_count();
        let mut id = ObjectId(0);
        let label = format!("Add {name}");
        let name = if n == 0 { name.to_owned() } else { format!("{name}.{n:03}") };
        self.edit(&label, |scene| {
            let mut object = shape_scene::Object::new(name, data);
            object.transform = Transform::from_translation(at);
            // Lights and cameras added above the origin look at it.
            if at.length_squared() > 1e-6 && !matches!(object.data, shape_scene::ObjectData::Mesh { .. } | shape_scene::ObjectData::Empty) {
                // Straight above the origin, Y is parallel to the view: pick another up vector.
                let dir = at.normalize();
                let up = if dir.y.abs() > 0.99 { shape_math::Vec3::Z } else { shape_math::Vec3::Y };
                let look = shape_math::Mat4::look_at_rh(at, shape_math::Vec3::ZERO, up).inverse();
                object.transform = Transform::from_matrix(look);
            }
            id = scene.add_object(object);
            scene.selection.set(id);
        });
        id
    }

    /// Store generated UVs on the active object's mesh, undoably.
    pub fn generate_uvs(&mut self, name: &str, generator: impl Fn(&shape_geom::Mesh) -> Vec<Vec<shape_math::Vec2>>) {
        let Some(id) = self.scene.selection.active else { return };
        let Some((mesh_id, mesh)) = self.scene.object_mesh(id) else { return };
        let uvs = generator(mesh);
        self.edit(name, |scene| {
            if let Some(data) = scene.mesh_mut(mesh_id) {
                data.uvs = Some(uvs);
            }
        });
    }

    /// Replace the active object's mesh with `op(mesh)`, undoably. Modifiers stay in place.
    pub fn apply_mesh_op(&mut self, name: &str, op: impl Fn(&shape_geom::Mesh) -> shape_ops::Result<shape_geom::Mesh>) {
        let Some(id) = self.scene.selection.active else { return };
        let Some((mesh_id, mesh)) = self.scene.object_mesh(id) else { return };
        match op(mesh) {
            Ok(new_mesh) => self.edit(name, |scene| {
                if let Some(data) = scene.mesh_mut(mesh_id) {
                    data.set_mesh(new_mesh);
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
                data.set_mesh(evaluated);
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
                self.modal = None;
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

    /// Enter edit mode on the active mesh object, or leave it.
    pub fn toggle_edit_mode(&mut self) {
        if self.scene.edit.is_some() {
            self.scene.edit = None;
            self.status = "Object Mode".into();
            return;
        }
        if let Some(id) = self.scene.selection.active
            && self.scene.object_mesh(id).is_some()
        {
            self.scene.edit = Some(EditSelection::new(id));
            self.status = "Edit Mode".into();
        }
    }

    /// Select every face (edit mode) or every object (object mode); again to deselect all.
    pub fn select_all(&mut self) {
        if let Some(object) = self.scene.edit.as_ref().map(|e| e.object) {
            let Some((_, mesh)) = self.scene.object_mesh(object) else { return };
            let all: std::collections::BTreeSet<_> = mesh.face_ids().collect();
            let Some(edit) = &mut self.scene.edit else { return };
            if edit.faces == all {
                edit.faces.clear();
            } else {
                edit.faces = all;
            }
        } else {
            let all: Vec<ObjectId> = self.scene.objects().map(|(id, _)| id).collect();
            if self.scene.selection.objects.len() == all.len() {
                self.scene.selection.clear();
            } else {
                for id in all {
                    self.scene.selection.objects.insert(id);
                }
                if self.scene.selection.active.is_none() {
                    self.scene.selection.active = self.scene.selection.objects.iter().next_back().copied();
                }
            }
        }
    }

    /// Loop cut through the edge nearest the pointer on the hovered face (edit mode).
    pub fn loop_cut_at_pointer(&mut self) {
        let Some(p) = self.last_pointer else { return };
        let size = self.viewport.size_px();
        let Some(hit) = shape_gpu::pick_detail(&self.scene, &self.viewport.camera, p, size) else { return };
        let Some((mesh_id, mesh)) = self.scene.object_mesh(hit.object) else { return };
        let Some(edge) = shape_gpu::nearest_edge(mesh, hit.face, hit.local_point) else { return };
        match shape_ops::loop_cut(mesh, edge) {
            Ok(new_mesh) => self.edit("Loop Cut", |scene| {
                if let Some(data) = scene.mesh_mut(mesh_id) {
                    data.set_mesh(new_mesh);
                }
                if let Some(e) = &mut scene.edit {
                    e.faces.clear();
                }
            }),
            Err(e) => self.status = format!("Loop cut failed: {e}"),
        }
    }

    /// Keys that start or steer modals; read before the viewport so a modal sees them this frame.
    /// Returns `(enter, escape)` for the running modal.
    fn modal_keys(&mut self, ui: &egui::Ui) -> (bool, bool) {
        if ui.ctx().egui_wants_keyboard_input() {
            return (false, false);
        }
        let modal_running = self.modal.is_some();
        let (enter, escape, g, r, sc, e, i, loop_cut, bevel, tab, a, x, y, z) = ui.input_mut(|inp| {
            (
                inp.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                inp.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                inp.consume_key(egui::Modifiers::NONE, egui::Key::G),
                inp.consume_key(egui::Modifiers::NONE, egui::Key::R),
                inp.consume_key(egui::Modifiers::NONE, egui::Key::S),
                inp.consume_key(egui::Modifiers::NONE, egui::Key::E),
                inp.consume_key(egui::Modifiers::NONE, egui::Key::I),
                inp.consume_key(egui::Modifiers::COMMAND, egui::Key::R),
                inp.consume_key(egui::Modifiers::COMMAND, egui::Key::B),
                inp.consume_key(egui::Modifiers::NONE, egui::Key::Tab),
                inp.consume_key(egui::Modifiers::NONE, egui::Key::A),
                modal_running && inp.consume_key(egui::Modifiers::NONE, egui::Key::X),
                modal_running && inp.consume_key(egui::Modifiers::NONE, egui::Key::Y),
                modal_running && inp.consume_key(egui::Modifiers::NONE, egui::Key::Z),
            )
        });
        if let Some(m) = &mut self.modal {
            if x {
                m.set_axis(0);
            }
            if y {
                m.set_axis(1);
            }
            if z {
                m.set_axis(2);
            }
            return (enter, escape);
        }
        let pointer = self.last_pointer.unwrap_or(self.viewport.size_px() * 0.5);
        if tab {
            self.toggle_edit_mode();
        }
        if a {
            self.select_all();
        }
        if self.scene.edit.is_some() {
            if e {
                self.modal = Modal::faces(ModalKind::Extrude, &self.scene, pointer);
            }
            if i {
                self.modal = Modal::faces(ModalKind::Inset, &self.scene, pointer);
            }
            if loop_cut {
                self.loop_cut_at_pointer();
            }
            if bevel {
                self.modal = Modal::faces(ModalKind::Bevel, &self.scene, pointer);
            }
        } else {
            let kind = if g {
                Some(ModalKind::Move)
            } else if r {
                Some(ModalKind::Rotate)
            } else if sc {
                Some(ModalKind::Scale)
            } else {
                None
            };
            if let Some(kind) = kind {
                self.modal = Modal::transform(kind, &self.scene, pointer, false);
            }
        }
        (enter, escape)
    }

    fn hotkeys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() || self.modal.is_some() {
            return;
        }
        let f12 = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::F12));
        if f12 {
            self.start_render(ui.ctx());
        }
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
        self.drain_control();
        let (enter, escape) = self.modal_keys(ui);
        self.hotkeys(ui);
        self.menu_bar(ui);
        self.status_bar(ui);
        self.outliner(ui);
        self.properties(ui);
        if self.show_agent {
            egui::Panel::bottom("agent").default_size(200.0).resizable(true).show(ui, |ui| self.agent.ui(ui));
        }
        self.viewport.camera_enabled = self.modal.is_none();
        let overlay = self.modal.as_ref().map(|m| m.status()).unwrap_or_default();
        let input = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ui, |ui| {
                self.tool_bar(ui);
                self.viewport.ui(ui, &self.scene, &overlay)
            })
            .inner;
        self.last_pointer = input.pointer.or(self.last_pointer);
        self.handle_viewport_input(&input, enter, escape);
        match self.render.ui(ui.ctx()) {
            Some(render_panel::RenderAction::Start) => self.start_render(ui.ctx()),
            Some(render_panel::RenderAction::Save) => {
                if let Some(path) = rfd::FileDialog::new().add_filter("PNG", &["png"]).set_file_name("render.png").save_file() {
                    self.status = match self.render.save_png(&path) {
                        Ok(()) => format!("Saved {}", path.display()),
                        Err(e) => format!("Save failed: {e}"),
                    };
                }
            }
            None => {}
        }
        if self.modal.is_some() {
            ui.ctx().request_repaint();
        }
    }

    fn tool_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("tools").show(ui, |ui| {
            ui.horizontal(|ui| {
                for t in Tool::ALL {
                    if ui.selectable_label(self.tool == t, t.name()).clicked() {
                        self.tool = t;
                    }
                }
                ui.separator();
                let editing = self.scene.edit.is_some();
                if ui.selectable_label(editing, "Edit Mode (Tab)").clicked() {
                    self.toggle_edit_mode();
                }
                let hint = if editing {
                    "click face · shift add · E extrude · I inset · Ctrl+B bevel · Ctrl+R loop cut · A all"
                } else {
                    "G move · R rotate · S scale · X delete · Home frame"
                };
                ui.label(RichText::new(hint).weak());
            });
        });
    }

    /// Run the modal, or pick, from this frame's viewport input.
    pub fn handle_viewport_input(&mut self, input: &ViewportInput, enter: bool, escape: bool) {
        let size = input.size.max(Vec2::ONE);
        if let Some(modal) = &mut self.modal {
            if let Some(p) = input.pointer {
                modal.update(&mut self.scene, &self.viewport.camera, p, size);
            }
            if let Some(end) = modal.end_for(input, enter, escape) {
                let modal = self.modal.take().expect("modal present");
                match end {
                    ModalEnd::Confirmed => {
                        self.record_from(modal.kind.name(), modal.before);
                        self.status = format!("{} confirmed", modal.kind.name());
                    }
                    ModalEnd::Cancelled => {
                        self.scene = modal.before;
                        self.status = format!("{} cancelled", modal.kind.name());
                    }
                }
            }
            return;
        }
        // A tool drag on a selected object starts a modal that confirms on release.
        if input.primary_pressed
            && let Some(kind) = self.tool.modal()
            && self.scene.edit.is_none()
            && let Some(p) = input.pointer
        {
            let over_selected = shape_gpu::pick(&self.scene, &self.viewport.camera, p, size).is_some_and(|(id, _)| self.scene.selection.contains(id));
            if over_selected {
                self.modal = Modal::transform(kind, &self.scene, p, true);
                return;
            }
        }
        if input.primary_clicked
            && let Some(p) = input.pointer
        {
            let hit = shape_gpu::pick_detail(&self.scene, &self.viewport.camera, p, size);
            if let Some(edit) = &mut self.scene.edit {
                match hit {
                    Some(h) if input.shift => {
                        if !edit.faces.remove(&h.face) {
                            edit.faces.insert(h.face);
                        }
                    }
                    Some(h) => {
                        edit.faces.clear();
                        edit.faces.insert(h.face);
                    }
                    None => edit.faces.clear(),
                }
            } else {
                match hit {
                    Some(h) if input.shift => self.scene.selection.toggle(h.object),
                    Some(h) => self.scene.selection.set(h.object),
                    None => self.scene.selection.clear(),
                }
            }
        }
    }
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
    use shape_math::Vec3;

    #[test]
    fn crate_name() {
        assert_eq!(CRATE, "shape-ui-egui");
    }

    #[test]
    fn shell_shows_every_region() {
        let mut app = ShapeApp::default();
        let mut harness = Harness::new_ui(|ui| app.ui(ui));
        harness.run();
        for label in ["Outliner", "Properties", "Ready", "File", "Add", "View", "Select", "Move", "Edit Mode (Tab)"] {
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
    fn edit_mode_select_all_and_modal_confirm_records_one_undo() {
        let mut app = ShapeApp::default();
        let id = app.add_primitive(Primitive::Cube);
        let undo_before = app.history.len();
        app.toggle_edit_mode();
        assert!(app.scene.edit.is_some());
        app.select_all();
        assert_eq!(app.scene.edit.as_ref().unwrap().faces.len(), 6);
        app.toggle_edit_mode();
        assert!(app.scene.edit.is_none());

        let size = Vec2::new(800.0, 600.0);
        app.modal = Modal::transform(ModalKind::Move, &app.scene, size * 0.5, false);
        let input = ViewportInput { pointer: Some(size * 0.5 + Vec2::new(120.0, 0.0)), size, ..Default::default() };
        app.handle_viewport_input(&input, false, false);
        assert!(app.scene.object(id).unwrap().transform.translation.length() > 0.1);
        app.handle_viewport_input(&input, true, false);
        assert!(app.modal.is_none());
        assert_eq!(app.history.len(), undo_before + 1);
        assert_eq!(app.history.undo_name(), Some("Move"));
        app.history.undo(&mut app.scene);
        assert!(app.scene.object(id).unwrap().transform.translation.length() < 1e-6);

        app.modal = Modal::transform(ModalKind::Scale, &app.scene, size * 0.5 + Vec2::new(50.0, 0.0), false);
        let input = ViewportInput { pointer: Some(size * 0.5 + Vec2::new(200.0, 0.0)), size, ..Default::default() };
        app.handle_viewport_input(&input, false, false);
        app.handle_viewport_input(&input, false, true);
        assert_eq!(app.scene.object(id).unwrap().transform.scale, Vec3::ONE, "cancel restores");
        assert_eq!(app.history.len(), undo_before, "cancel records nothing (Move sits in redo)");
        assert_eq!(app.history.redo_name(), Some("Move"));
    }

    #[test]
    fn click_picks_in_object_and_edit_mode() {
        let mut app = ShapeApp::default();
        let id = app.add_primitive(Primitive::Cube);
        app.scene.selection.clear();
        app.viewport.camera = shape_gpu::OrbitCamera { yaw: 0.0, pitch: 0.0, distance: 8.0, ..Default::default() };
        let size = Vec2::new(800.0, 600.0);
        let click = ViewportInput { pointer: Some(size * 0.5), size, primary_clicked: true, ..Default::default() };
        app.handle_viewport_input(&click, false, false);
        assert_eq!(app.scene.selection.active, Some(id));
        app.toggle_edit_mode();
        app.handle_viewport_input(&click, false, false);
        assert_eq!(app.scene.edit.as_ref().unwrap().faces.len(), 1);
        let miss = ViewportInput { pointer: Some(Vec2::new(5.0, 5.0)), size, primary_clicked: true, ..Default::default() };
        app.handle_viewport_input(&miss, false, false);
        assert!(app.scene.edit.as_ref().unwrap().faces.is_empty());
    }

    #[test]
    fn lights_cameras_and_uvs_are_added_undoably() {
        let mut app = ShapeApp::default();
        let cube = app.add_primitive(Primitive::Cube);
        app.generate_uvs("UV Box", shape_ops::uv_box);
        let (mesh_id, _) = app.scene.object_mesh(cube).unwrap();
        assert_eq!(app.scene.mesh(mesh_id).unwrap().uvs.as_ref().unwrap().len(), 6);
        app.apply_mesh_op("Subdivide", shape_ops::catmull_clark);
        assert!(app.scene.mesh(mesh_id).unwrap().uvs.is_none(), "rebuilt mesh drops stale UVs");
        let sun = app.add_object("Sun", shape_scene::ObjectData::Light(shape_scene::Light::sun(3.0)), shape_math::Vec3::new(0.0, 5.0, 0.0));
        let lights = shape_gpu::scene_lights(&app.scene);
        assert_eq!(lights.len(), 1);
        assert!(lights[0].position[1] > 0.99, "sun above the origin shines down: {:?}", lights[0].position);
        app.add_object("Camera", shape_scene::ObjectData::Camera(Default::default()), shape_math::Vec3::new(6.0, 4.0, 6.0));
        assert_eq!(app.scene.object_count(), 3);
        app.history.undo(&mut app.scene);
        app.history.undo(&mut app.scene);
        assert!(app.scene.object(sun).is_none());
        let mut harness = Harness::new_ui(|ui| app.ui(ui));
        harness.run();
        harness.get_by_label("Material");
    }

    #[test]
    fn agent_tools_run_as_undo_steps() {
        let mut app = ShapeApp::default();
        let r = app.run_tool("add_primitive", &serde_json::json!({"kind": "torus", "name": "Ring"})).unwrap();
        assert_eq!(r["name"], "Ring");
        assert_eq!(app.history.undo_name(), Some("Agent: add_primitive"));
        assert!(app.run_tool("mesh_op", &serde_json::json!({"object": "Nope", "op": "subdivide"})).is_err());
        assert_eq!(app.history.len(), 1, "a failed tool records nothing");
        let s = app.run_tool("scene_summary", &serde_json::json!({})).unwrap();
        assert_eq!(s["objects"][0]["name"], "Ring");
        assert_eq!(app.history.len(), 1, "a read-only tool records nothing");
        app.history.undo(&mut app.scene);
        assert_eq!(app.scene.object_count(), 0);
        let mut harness = Harness::new_ui(|ui| app.ui(ui));
        harness.run();
        harness.get_by_label("Agent");
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
