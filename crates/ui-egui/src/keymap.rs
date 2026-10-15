//! Blender's object-mode keymap, stage P2: the operator registry behind F3 search, the popups that
//! open at the pointer (Shift+A add, Z shading, X delete), box select (B / Select tool drag),
//! numpad views, Ctrl+J join and P separate.

use crate::tools::Tool;
use crate::{Primitive, ShapeApp};
use egui::{Color32, RichText, Stroke};
use shape_gpu::{AxisView, ShadingMode};
use shape_math::{Vec2, Vec3};
use shape_scene::{Camera, Light, ObjectData, ObjectId};

/// Every operator a key or F3 can run. One list, so the search, the menus and the keymap table
/// agree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Add(Primitive),
    AddPointLight,
    AddSun,
    AddSpot,
    AddCamera,
    AddEmpty,
    Delete,
    Duplicate,
    Hide,
    Reveal,
    SelectAll,
    SelectNone,
    SelectInvert,
    ToggleEditMode,
    Join,
    Separate,
    FrameAll,
    FrameSelected,
    View(AxisView),
    TogglePerspective,
    Shading(ShadingMode),
    ToggleWireframe,
    ToggleXray,
    RenderImage,
    ApplyModifiers,
    Subdivide,
    SubdivideLinear,
    SmoothVertices,
    Triangulate,
    FlipNormals,
    BevelAll,
    ToggleToolbar,
    ToggleSidebar,
    ToggleMaximize,
}

impl Op {
    pub const ALL: &'static [Op] = &[
        Op::Add(Primitive::Cube),
        Op::Add(Primitive::Plane),
        Op::Add(Primitive::Sphere),
        Op::Add(Primitive::Torus),
        Op::Add(Primitive::Cylinder),
        Op::AddPointLight,
        Op::AddSun,
        Op::AddSpot,
        Op::AddCamera,
        Op::AddEmpty,
        Op::Delete,
        Op::Duplicate,
        Op::Hide,
        Op::Reveal,
        Op::SelectAll,
        Op::SelectNone,
        Op::SelectInvert,
        Op::ToggleEditMode,
        Op::Join,
        Op::Separate,
        Op::FrameAll,
        Op::FrameSelected,
        Op::View(AxisView::Front),
        Op::View(AxisView::Back),
        Op::View(AxisView::Right),
        Op::View(AxisView::Left),
        Op::View(AxisView::Top),
        Op::View(AxisView::Bottom),
        Op::TogglePerspective,
        Op::Shading(ShadingMode::Wireframe),
        Op::Shading(ShadingMode::Solid),
        Op::Shading(ShadingMode::SolidWire),
        Op::Shading(ShadingMode::MaterialPreview),
        Op::ToggleWireframe,
        Op::ToggleXray,
        Op::RenderImage,
        Op::ApplyModifiers,
        Op::Subdivide,
        Op::SubdivideLinear,
        Op::SmoothVertices,
        Op::Triangulate,
        Op::FlipNormals,
        Op::BevelAll,
        Op::ToggleToolbar,
        Op::ToggleSidebar,
        Op::ToggleMaximize,
    ];

    /// Name as Blender's search shows it ("Menu > Item").
    pub fn name(self) -> String {
        match self {
            Op::Add(p) => format!("Add > Mesh > {}", p.name()),
            Op::AddPointLight => "Add > Light > Point".into(),
            Op::AddSun => "Add > Light > Sun".into(),
            Op::AddSpot => "Add > Light > Spot".into(),
            Op::AddCamera => "Add > Camera".into(),
            Op::AddEmpty => "Add > Empty".into(),
            Op::Delete => "Object > Delete".into(),
            Op::Duplicate => "Object > Duplicate Objects".into(),
            Op::Hide => "Object > Show/Hide > Hide Selected".into(),
            Op::Reveal => "Object > Show/Hide > Show Hidden Objects".into(),
            Op::SelectAll => "Select > All".into(),
            Op::SelectNone => "Select > None".into(),
            Op::SelectInvert => "Select > Invert".into(),
            Op::ToggleEditMode => "Object > Toggle Edit Mode".into(),
            Op::Join => "Object > Join".into(),
            Op::Separate => "Mesh > Separate > Selection".into(),
            Op::FrameAll => "View > Frame All".into(),
            Op::FrameSelected => "View > Frame Selected".into(),
            Op::View(v) => format!("View > Viewpoint > {}", v.name()),
            Op::TogglePerspective => "View > Perspective/Orthographic".into(),
            Op::Shading(ShadingMode::Wireframe) => "Viewport Shading > Wireframe".into(),
            Op::Shading(ShadingMode::Solid) => "Viewport Shading > Solid".into(),
            Op::Shading(ShadingMode::SolidWire) => "Viewport Shading > Solid + Wireframe".into(),
            Op::Shading(ShadingMode::MaterialPreview) => "Viewport Shading > Material Preview".into(),
            Op::ToggleWireframe => "Viewport Shading > Toggle Wireframe".into(),
            Op::ToggleXray => "Viewport Shading > Toggle X-Ray".into(),
            Op::RenderImage => "Render > Render Image".into(),
            Op::ApplyModifiers => "Object > Apply > Visual Geometry to Mesh".into(),
            Op::Subdivide => "Mesh > Subdivide (Catmull-Clark)".into(),
            Op::SubdivideLinear => "Mesh > Subdivide (Linear)".into(),
            Op::SmoothVertices => "Mesh > Smooth Vertices".into(),
            Op::Triangulate => "Mesh > Triangulate Faces".into(),
            Op::FlipNormals => "Mesh > Normals > Flip".into(),
            Op::BevelAll => "Mesh > Bevel All Edges".into(),
            Op::ToggleToolbar => "View > Toolbar".into(),
            Op::ToggleSidebar => "View > Sidebar".into(),
            Op::ToggleMaximize => "View > Area > Toggle Maximize Area".into(),
        }
    }

    /// Blender's default key, or empty.
    pub fn key(self) -> &'static str {
        match self {
            Op::Add(_) | Op::AddPointLight | Op::AddSun | Op::AddSpot | Op::AddCamera | Op::AddEmpty => "Shift+A",
            Op::Delete => "X / Delete",
            Op::Duplicate => "Shift+D",
            Op::Hide => "H",
            Op::Reveal => "Alt+H",
            Op::SelectAll => "A",
            Op::SelectNone => "Alt+A",
            Op::SelectInvert => "Ctrl+I",
            Op::ToggleEditMode => "Tab / Ctrl+Tab",
            Op::Join => "Ctrl+J",
            Op::Separate => "P",
            Op::FrameAll => "Home / Shift+C",
            Op::FrameSelected => "Numpad .",
            Op::View(v) => v.key(),
            Op::TogglePerspective => "Numpad 5",
            Op::Shading(_) => "Z",
            Op::ToggleWireframe => "Shift+Z",
            Op::ToggleXray => "Alt+Z",
            Op::RenderImage => "F12",
            Op::ApplyModifiers => "",
            Op::Subdivide | Op::SubdivideLinear | Op::SmoothVertices | Op::Triangulate | Op::FlipNormals | Op::BevelAll => "",
            Op::ToggleToolbar => "T",
            Op::ToggleSidebar => "N",
            Op::ToggleMaximize => "Ctrl+Space",
        }
    }
}

/// A popup opened at the pointer, Blender style (menus and pies open where the pointer is).
#[derive(Clone, Debug, PartialEq)]
pub enum Popup {
    Add,
    Shading,
    /// Ctrl+Tab: Object / Edit / Sculpt.
    Mode,
    DeleteConfirm,
    Search {
        query: String,
    },
    /// Edit mode M: where to merge.
    Merge,
    /// Edit mode X: what to delete or dissolve.
    EditDelete,
    /// Ctrl+V / Ctrl+E / Ctrl+F.
    VertexMenu,
    /// Edit mode U: UV Mapping.
    UvMenu,
    /// Object mode I: Insert Keyframe.
    InsertKeyframe,
    EdgeMenu,
    FaceMenu,
    /// M in object mode.
    MoveToCollection,
}

/// What an edit-mode menu item does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EditAction {
    Fill,
    Bridge,
    Spin,
    LoopCut,
    Extrude,
    Inset,
    Bevel,
    ShrinkFatten,
}

/// Box select in progress: start pixel, current pixel (viewport pixels).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxSelect {
    pub start: Vec2,
    pub current: Vec2,
}

impl ShapeApp {
    /// Run an operator by name (F3, menus, tests).
    pub fn run_op(&mut self, op: Op) {
        match op {
            Op::Add(p) => {
                self.add_primitive(p);
            }
            Op::AddPointLight => {
                self.add_object("Point", ObjectData::Light(Light::point(1000.0)), Vec3::new(2.0, 4.0, 2.0));
            }
            Op::AddSun => {
                self.add_object("Sun", ObjectData::Light(Light::sun(3.0)), Vec3::new(0.0, 5.0, 0.0));
            }
            Op::AddSpot => {
                self.add_object("Spot", ObjectData::Light(Light::spot(1500.0)), Vec3::new(0.0, 5.0, 0.0));
            }
            Op::AddCamera => {
                self.add_object("Camera", ObjectData::Camera(Camera::default()), Vec3::new(6.0, 4.0, 6.0));
            }
            Op::AddEmpty => {
                self.add_object("Empty", ObjectData::Empty, Vec3::ZERO);
            }
            Op::Delete => self.delete_selected(),
            Op::Duplicate => self.duplicate_selected(),
            Op::Hide => self.hide_selected(true),
            Op::Reveal => self.reveal_all(),
            Op::SelectAll => self.select_all(),
            Op::SelectNone => self.select_none(),
            Op::SelectInvert => self.select_invert(),
            Op::ToggleEditMode => self.toggle_edit_mode(),
            Op::Join => self.join_selected(),
            Op::Separate => self.separate_selection(),
            Op::FrameAll => self.viewport.frame_scene(&self.scene),
            Op::FrameSelected => self.frame_selected(),
            Op::View(v) => self.viewport.camera.set_axis_view(v),
            Op::TogglePerspective => self.viewport.camera.ortho = !self.viewport.camera.ortho,
            Op::Shading(m) => self.viewport.options.shading = m,
            Op::ToggleXray => self.viewport.options.solid.xray = !self.viewport.options.solid.xray,
            Op::ToggleWireframe => {
                self.viewport.options.shading =
                    if self.viewport.options.shading == ShadingMode::Wireframe { ShadingMode::Solid } else { ShadingMode::Wireframe };
            }
            Op::RenderImage => self.render_requested = true,
            Op::ApplyModifiers => self.apply_modifiers(),
            Op::Subdivide => self.apply_mesh_op("Subdivide", shape_ops::catmull_clark),
            Op::SubdivideLinear => self.apply_mesh_op("Subdivide Linear", shape_ops::subdivide_linear),
            Op::SmoothVertices => self.apply_mesh_op("Smooth", |m| shape_ops::smooth(m, 2, 0.5)),
            Op::Triangulate => self.apply_mesh_op("Triangulate", shape_ops::triangulate),
            Op::FlipNormals => self.apply_mesh_op("Flip Normals", shape_ops::flip_normals),
            Op::BevelAll => self.apply_mesh_op("Bevel", |m| shape_ops::bevel_all(m, 0.1)),
            Op::ToggleToolbar => self.regions.toolbar = !self.regions.toolbar,
            Op::ToggleSidebar => self.regions.sidebar = !self.regions.sidebar,
            Op::ToggleMaximize => self.regions.maximized = !self.regions.maximized,
        }
    }

    /// Operators whose name contains every word of `query`, best (shortest) first.
    pub fn search_ops(query: &str) -> Vec<Op> {
        let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
        let mut hits: Vec<(usize, Op)> = Op::ALL
            .iter()
            .copied()
            .filter(|op| {
                let name = op.name().to_lowercase();
                words.iter().all(|w| name.contains(w.as_str()))
            })
            .map(|op| (op.name().len(), op))
            .collect();
        hits.sort_by_key(|(len, _)| *len);
        hits.into_iter().map(|(_, op)| op).collect()
    }

    /// Numpad .: frame the selection (objects, or the selected faces in edit mode).
    pub fn frame_selected(&mut self) {
        let mut b = shape_math::Aabb::EMPTY;
        if let Some(edit) = &self.scene.edit {
            if let Some((_, mesh)) = self.scene.object_mesh(edit.object) {
                let t = self.scene.world_transform(edit.object);
                for &f in &edit.faces {
                    for v in mesh.face_vertices(f) {
                        b = b.with_point(t.transform_point(mesh.position(v)));
                    }
                }
            }
        } else {
            for &id in &self.scene.selection.objects {
                let t = self.scene.world_transform(id);
                match self.scene.object_mesh(id) {
                    Some((_, mesh)) => {
                        for p in mesh.positions() {
                            b = b.with_point(t.transform_point(p));
                        }
                    }
                    None => b = b.with_point(t.translation),
                }
            }
        }
        if b.is_empty() {
            self.viewport.frame_scene(&self.scene);
        } else {
            let pad = Vec3::splat(0.5);
            self.viewport.camera.frame(b.min - pad, b.max + pad);
        }
    }

    /// Ctrl+J: merge every selected mesh into the active one, in the active object's space.
    pub fn join_selected(&mut self) {
        if self.scene.edit.is_some() {
            return;
        }
        let Some(active) = self.scene.selection.active else { return };
        let others: Vec<ObjectId> =
            self.scene.selection.objects.iter().copied().filter(|&id| id != active && self.scene.object_mesh(id).is_some()).collect();
        if others.is_empty() || self.scene.object_mesh(active).is_none() {
            return;
        }
        self.edit("Join", |scene| {
            let Some((active_mesh, base)) = scene.object_mesh(active) else { return };
            let to_active = scene.world_transform(active).inverse();
            let mut pm = shape_ops::PolyMesh::from_mesh(base);
            for id in &others {
                let Some((_, mesh)) = scene.object_mesh(*id) else { continue };
                let t = scene.world_transform(*id);
                let mut other = shape_ops::PolyMesh::from_mesh(mesh);
                for p in &mut other.positions {
                    *p = to_active.transform_point(t.transform_point(*p));
                }
                pm.append(&other);
            }
            if let Ok(joined) = pm.build()
                && let Some(data) = scene.mesh_mut(active_mesh)
            {
                data.set_mesh(joined);
            }
            for id in others {
                scene.remove_object(id);
            }
            scene.prune_meshes();
            scene.selection.set(active);
        });
    }

    /// P: move the selected faces into a new object with the same transform (edit mode).
    pub fn separate_selection(&mut self) {
        let Some(edit) = self.scene.edit.clone() else { return };
        if edit.faces.is_empty() {
            return;
        }
        let Some((mesh_id, mesh)) = self.scene.object_mesh(edit.object) else { return };
        let all: std::collections::BTreeSet<_> = mesh.face_ids().collect();
        if edit.faces.len() == all.len() {
            return;
        }
        let (positions, polygons) = mesh.to_polygons();
        let split = |keep: &dyn Fn(usize) -> bool| -> Option<shape_geom::Mesh> {
            let mut used = vec![u32::MAX; positions.len()];
            let mut pos = Vec::new();
            let mut polys = Vec::new();
            for (i, poly) in polygons.iter().enumerate() {
                if !keep(i) {
                    continue;
                }
                let mut q = Vec::with_capacity(poly.len());
                for &v in poly {
                    if used[v as usize] == u32::MAX {
                        used[v as usize] = pos.len() as u32;
                        pos.push(positions[v as usize]);
                    }
                    q.push(used[v as usize]);
                }
                polys.push(q);
            }
            shape_geom::Mesh::from_polygons(pos, &polys).ok()
        };
        let selected = |i: usize| edit.faces.contains(&shape_geom::FaceId(i as u32));
        let (Some(kept), Some(taken)) = (split(&|i| !selected(i)), split(&selected)) else { return };
        let object = edit.object;
        self.edit("Separate", |scene| {
            if let Some(data) = scene.mesh_mut(mesh_id) {
                data.set_mesh(kept);
            }
            let Some(source) = scene.object(object).cloned() else { return };
            let name = format!("{}.{:03}", source.name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&source.name), scene.object_count());
            let new_mesh = scene.add_mesh(shape_scene::MeshData::new(name.clone(), taken));
            let material = match &source.data {
                ObjectData::Mesh { material, .. } => *material,
                _ => None,
            };
            let mut new = shape_scene::Object::new(name, ObjectData::Mesh { mesh: new_mesh, material, modifiers: Vec::new() });
            new.transform = source.transform;
            new.parent = source.parent;
            let id = scene.add_object(new);
            if let Some(edit) = &mut scene.edit {
                edit.faces.clear();
            }
            scene.selection.objects.insert(id);
        });
    }

    /// The mode keys that stay live in Sculpt Mode: Ctrl+Tab (mode menu) and Escape.
    pub(crate) fn mode_keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() {
            return;
        }
        let (mode, escape) =
            ui.input_mut(|i| (crate::key(i, egui::Modifiers::COMMAND, egui::Key::Tab), crate::key(i, egui::Modifiers::NONE, egui::Key::Escape)));
        if mode {
            self.popup = Some(Popup::Mode);
        }
        if escape {
            self.popup = None;
        }
    }

    /// Object-mode keys that open popups or drive the view. Runs after the modal keys so a modal
    /// owns X/Y/Z while it is active.
    pub(crate) fn object_keys(&mut self, ui: &egui::Ui) {
        if ui.ctx().egui_wants_keyboard_input() || self.modal.is_some() {
            return;
        }
        let editing = self.scene.edit.is_some();
        let keys = ui.input_mut(|i| {
            [
                crate::key(i, egui::Modifiers::SHIFT, egui::Key::A),
                crate::key(i, egui::Modifiers::NONE, egui::Key::Z),
                crate::key(i, egui::Modifiers::SHIFT, egui::Key::Z),
                crate::key(i, egui::Modifiers::ALT, egui::Key::Z),
                crate::key(i, egui::Modifiers::NONE, egui::Key::F3),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::Tab),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::J),
                crate::key(i, egui::Modifiers::NONE, egui::Key::P),
                crate::key(i, egui::Modifiers::SHIFT, egui::Key::C),
                crate::key(i, egui::Modifiers::NONE, egui::Key::B),
                !editing && crate::key(i, egui::Modifiers::NONE, egui::Key::Num1),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::Num1),
                !editing && crate::key(i, egui::Modifiers::NONE, egui::Key::Num3),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::Num3),
                crate::key(i, egui::Modifiers::NONE, egui::Key::Num7),
                crate::key(i, egui::Modifiers::COMMAND, egui::Key::Num7),
                crate::key(i, egui::Modifiers::NONE, egui::Key::Num5),
                crate::key(i, egui::Modifiers::NONE, egui::Key::Period),
                crate::key(i, egui::Modifiers::NONE, egui::Key::Escape),
            ]
        });
        let [add, shading, wire, xray, search, mode, join, separate, center, boxkey, n1, c1, n3, c3, n7, c7, n5, period, escape] = keys;
        if escape {
            self.popup = None;
            self.box_pending = false;
        }
        if add {
            self.popup = Some(Popup::Add);
        }
        if shading {
            self.popup = Some(Popup::Shading);
        }
        if wire {
            self.run_op(Op::ToggleWireframe);
        }
        if xray {
            self.run_op(Op::ToggleXray);
        }
        if search {
            self.popup = Some(Popup::Search { query: String::new() });
        }
        if mode {
            self.popup = Some(Popup::Mode);
        }
        if join {
            self.run_op(Op::Join);
        }
        if separate && editing {
            self.run_op(Op::Separate);
        }
        if center {
            self.run_op(Op::FrameAll);
        }
        if boxkey {
            self.box_pending = true;
            self.status = "Box select: drag in the viewport".into();
        }
        // egui cannot tell the numpad from the digit row, and in edit mode 1 / 2 / 3 are Blender's
        // select modes; the plain-digit views are object mode only (Ctrl+digit works in both).
        for (hit, view) in
            [(n1, AxisView::Front), (c1, AxisView::Back), (n3, AxisView::Right), (c3, AxisView::Left), (n7, AxisView::Top), (c7, AxisView::Bottom)]
        {
            if hit {
                self.run_op(Op::View(view));
                self.status = format!("{} {}", view.name(), if self.viewport.camera.ortho { "Orthographic" } else { "Perspective" });
            }
        }
        if n5 {
            self.run_op(Op::TogglePerspective);
        }
        if period {
            self.run_op(Op::FrameSelected);
        }
    }

    /// Draw the open popup at the pointer and run what it picks.
    pub(crate) fn popup_ui(&mut self, ui: &mut egui::Ui) {
        let Some(popup) = self.popup.clone() else { return };
        let pos = self.popup_pos.unwrap_or_else(|| ui.ctx().pointer_latest_pos().unwrap_or(ui.max_rect().center()));
        self.popup_pos = Some(pos);
        let mut close = false;
        let mut run: Option<Op> = None;
        let mut merge: Option<shape_ops::MergeAt> = None;
        let mut edit_delete: Option<crate::EditDelete> = None;
        let mut edit_action: Option<EditAction> = None;
        let mut collection_target: Option<Option<shape_scene::CollectionId>> = None;
        let mut new_collection = false;
        let mut uv: Option<crate::UvOp> = None;
        let mut keying: Option<shape_scene::KeyingSet> = None;
        let area = egui::Area::new(egui::Id::new("blender-popup")).fixed_pos(pos).order(egui::Order::Foreground);
        let response = area.show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(180.0);
                match &popup {
                    Popup::Add => {
                        ui.label(RichText::new("Add").strong());
                        ui.separator();
                        ui.label(RichText::new("Mesh").color(crate::theme::TEXT_WEAK));
                        for p in Primitive::ALL {
                            if ui.button(p.name()).clicked() {
                                run = Some(Op::Add(p));
                            }
                        }
                        ui.separator();
                        ui.label(RichText::new("Light").color(crate::theme::TEXT_WEAK));
                        for (op, label) in [(Op::AddPointLight, "Point"), (Op::AddSun, "Sun"), (Op::AddSpot, "Spot")] {
                            if ui.button(label).clicked() {
                                run = Some(op);
                            }
                        }
                        ui.separator();
                        for (op, label) in [(Op::AddCamera, "Camera"), (Op::AddEmpty, "Empty")] {
                            if ui.button(label).clicked() {
                                run = Some(op);
                            }
                        }
                    }
                    Popup::Mode => {
                        ui.label(RichText::new("Mode").strong());
                        ui.separator();
                        let (editing, sculpting) = (self.scene.edit.is_some(), self.sculpt.is_some());
                        if ui.selectable_label(!editing && !sculpting, "Object Mode").clicked() {
                            self.scene.edit = None;
                            self.sculpt = None;
                            self.status = "Object Mode".into();
                            close = true;
                        }
                        if ui.selectable_label(editing, "Edit Mode").clicked() {
                            if !editing {
                                self.toggle_edit_mode();
                            }
                            close = true;
                        }
                        if ui.selectable_label(sculpting, "Sculpt Mode").clicked() {
                            if !sculpting {
                                self.toggle_sculpt_mode();
                            }
                            close = true;
                        }
                    }
                    Popup::Shading => {
                        ui.label(RichText::new("Viewport Shading").strong());
                        ui.separator();
                        for (mode, label) in ShadingMode::ALL.map(|m| (m, m.name())) {
                            if ui.selectable_label(self.viewport.options.shading == mode, label).clicked() {
                                run = Some(Op::Shading(mode));
                            }
                        }
                    }
                    Popup::DeleteConfirm => {
                        ui.label(RichText::new("OK?").strong());
                        ui.separator();
                        if ui.button(format!("Delete {} object(s)", self.scene.selection.objects.len())).clicked() {
                            run = Some(Op::Delete);
                        }
                    }
                    Popup::Merge => {
                        ui.label(RichText::new("Merge").strong());
                        ui.separator();
                        for (at, label) in [
                            (shape_ops::MergeAt::Center, "At Center"),
                            (shape_ops::MergeAt::First, "At First"),
                            (shape_ops::MergeAt::Last, "At Last"),
                        ] {
                            if ui.button(label).clicked() {
                                merge = Some(at);
                            }
                        }
                    }
                    Popup::EditDelete => {
                        ui.label(RichText::new("Delete").strong());
                        ui.separator();
                        for what in crate::EditDelete::ALL {
                            if ui.button(what.name()).clicked() {
                                edit_delete = Some(what);
                            }
                        }
                    }
                    Popup::VertexMenu => {
                        ui.label(RichText::new("Vertex").strong());
                        ui.separator();
                        if ui.button("Merge Vertices    M").clicked() {
                            self.popup = Some(Popup::Merge);
                            return;
                        }
                        if ui.button("New Edge/Face from Vertices    F").clicked() {
                            edit_action = Some(EditAction::Fill);
                        }
                        if ui.button("Shrink/Fatten    Alt+S").clicked() {
                            edit_action = Some(EditAction::ShrinkFatten);
                        }
                        if ui.button("Dissolve Vertices").clicked() {
                            edit_delete = Some(crate::EditDelete::Vertices);
                        }
                    }
                    Popup::EdgeMenu => {
                        ui.label(RichText::new("Edge").strong());
                        ui.separator();
                        if ui.button("Bevel Edges    Ctrl+B").clicked() {
                            edit_action = Some(EditAction::Bevel);
                        }
                        if ui.button("Loop Cut and Slide    Ctrl+R").clicked() {
                            edit_action = Some(EditAction::LoopCut);
                        }
                        if ui.button("Bridge Edge Loops").clicked() {
                            edit_action = Some(EditAction::Bridge);
                        }
                        if ui.button("Select Loop (Alt+click)").clicked() {
                            close = true;
                        }
                        if ui.button("Dissolve Edges").clicked() {
                            edit_delete = Some(crate::EditDelete::DissolveEdges);
                        }
                        ui.separator();
                        if ui.button("Mark Seam").clicked() {
                            uv = Some(crate::UvOp::MarkSeam);
                        }
                        if ui.button("Clear Seam").clicked() {
                            uv = Some(crate::UvOp::ClearSeam);
                        }
                    }
                    Popup::InsertKeyframe => {
                        ui.label(RichText::new("Insert Keyframe Menu").strong());
                        ui.separator();
                        for set in shape_scene::KeyingSet::ALL {
                            if ui.button(set.name()).clicked() {
                                keying = Some(set);
                            }
                        }
                    }
                    Popup::UvMenu => {
                        ui.label(RichText::new("UV Mapping").strong());
                        ui.separator();
                        for op in crate::UvOp::MENU {
                            if ui.button(op.name()).clicked() {
                                uv = Some(op);
                            }
                        }
                    }
                    Popup::FaceMenu => {
                        ui.label(RichText::new("Face").strong());
                        ui.separator();
                        if ui.button("Extrude Faces    E").clicked() {
                            edit_action = Some(EditAction::Extrude);
                        }
                        if ui.button("Inset Faces    I").clicked() {
                            edit_action = Some(EditAction::Inset);
                        }
                        if ui.button("Fill    F").clicked() {
                            edit_action = Some(EditAction::Fill);
                        }
                        if ui.button("Spin (Y, 90°, 9 steps)").clicked() {
                            edit_action = Some(EditAction::Spin);
                        }
                        if ui.button("Triangulate Faces").clicked() {
                            run = Some(Op::Triangulate);
                        }
                        if ui.button("Dissolve Faces").clicked() {
                            edit_delete = Some(crate::EditDelete::DissolveFaces);
                        }
                    }
                    Popup::MoveToCollection => {
                        ui.label(RichText::new("Move to Collection").strong());
                        ui.separator();
                        if ui.button("Scene Collection").clicked() {
                            collection_target = Some(None);
                        }
                        let names: Vec<(usize, String)> = self.scene.collections.iter().enumerate().map(|(i, c)| (i, c.name.clone())).collect();
                        for (i, name) in names {
                            if ui.button(name).clicked() {
                                collection_target = Some(Some(shape_scene::CollectionId(i as u32)));
                            }
                        }
                        ui.separator();
                        if ui.button("+ New Collection").clicked() {
                            new_collection = true;
                        }
                    }
                    Popup::Search { query } => {
                        let mut q = query.clone();
                        let edit = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search operators…").desired_width(260.0));
                        edit.request_focus();
                        let hits = Self::search_ops(&q);
                        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if enter && let Some(first) = hits.first() {
                            run = Some(*first);
                        }
                        ui.separator();
                        for op in hits.iter().take(12) {
                            ui.horizontal(|ui| {
                                if ui.button(op.name()).clicked() {
                                    run = Some(*op);
                                }
                                ui.label(RichText::new(op.key()).color(crate::theme::TEXT_WEAK).size(11.0));
                            });
                        }
                        if hits.is_empty() {
                            ui.label(RichText::new("No results").weak());
                        }
                        if q != *query {
                            self.popup = Some(Popup::Search { query: q });
                        }
                    }
                }
            });
        });
        // Click outside or Esc closes; Esc is consumed by object_keys (and by the text edit).
        if ui.input(|i| i.pointer.any_pressed()) && !response.response.rect.contains(ui.input(|i| i.pointer.interact_pos().unwrap_or(pos))) {
            close = true;
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        if let Some(op) = run {
            self.run_op(op);
            close = true;
        }
        if let Some(at) = merge {
            self.merge_selected(at);
            close = true;
        }
        if let Some(what) = edit_delete {
            self.edit_delete(what);
            close = true;
        }
        if let Some(op) = uv {
            self.uv_op(op);
            close = true;
        }
        if let Some(set) = keying {
            self.insert_keyframes(set);
            close = true;
        }
        if let Some(target) = collection_target {
            self.move_to_collection(target);
            close = true;
        }
        if new_collection {
            self.new_collection_with_selection();
            close = true;
        }
        if let Some(action) = edit_action {
            match action {
                EditAction::Fill => self.fill_selected(),
                EditAction::Bridge => self.bridge_selected(),
                EditAction::Spin => self.spin_selected(90.0, 9),
                EditAction::LoopCut => self.loop_cut_at_pointer(),
                EditAction::Extrude => self.start_face_modal(crate::tools::ModalKind::Extrude),
                EditAction::Inset => self.start_face_modal(crate::tools::ModalKind::Inset),
                EditAction::Bevel => self.start_face_modal(crate::tools::ModalKind::Bevel),
                EditAction::ShrinkFatten => self.start_face_modal(crate::tools::ModalKind::ShrinkFatten),
            }
            close = true;
        }
        if close {
            self.popup = None;
            self.popup_pos = None;
            // A clicked button keeps keyboard focus, which would swallow every hotkey afterwards.
            ui.ctx().memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
    }

    /// Box select: start on a press with the Select tool (or after B), follow the pointer, select on
    /// release. Returns true when it consumed the input this frame.
    pub(crate) fn box_select_input(&mut self, input: &crate::viewport::ViewportInput) -> bool {
        let size = input.size.max(Vec2::ONE);
        if let Some(b) = &mut self.box_select {
            if let Some(p) = input.pointer {
                b.current = p;
            }
            if input.primary_released {
                let b = self.box_select.take().expect("box present");
                let min = b.start.min(b.current);
                let max = b.start.max(b.current);
                let dragged = (max - min).max_element() > 3.0;
                self.box_pending = false;
                if !dragged {
                    // A click, not a drag: fall through to picking next frame is too late, so pick now.
                    return false;
                }
                let inside = |p: Vec2| p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y;
                let cam = self.viewport.camera.clone();
                if self.scene.edit.is_some() {
                    self.edit_box_select(inside, size, input.shift);
                } else {
                    let hits: Vec<ObjectId> = self
                        .scene
                        .objects()
                        .filter(|(_, o)| o.visible)
                        .map(|(id, _)| id)
                        .filter(|&id| cam.project(self.scene.world_transform(id).translation, size).is_some_and(inside))
                        .collect();
                    if !input.shift {
                        self.scene.selection.clear();
                    }
                    for id in hits {
                        self.scene.selection.objects.insert(id);
                        self.scene.selection.active = Some(id);
                    }
                }
                self.status = "Box select".into();
                return true;
            }
            return true;
        }
        if input.primary_pressed
            && let Some(p) = input.pointer
            && (self.box_pending || self.tool == Tool::Select)
        {
            // The Select tool: a press on an object is a click-select, a press on nothing starts a box.
            let over = shape_gpu::pick(&self.scene, &self.viewport.camera, p, size).is_some();
            if self.box_pending || !over {
                self.box_select = Some(BoxSelect { start: p, current: p });
                return true;
            }
        }
        false
    }

    /// Draw the box select rectangle over the viewport.
    pub(crate) fn box_select_overlay(&self, ui: &egui::Ui) {
        let Some(b) = &self.box_select else { return };
        let rect = self.viewport.last_rect;
        let ppp = self.viewport.last_ppp.max(0.01);
        let to_screen = |p: Vec2| egui::pos2(rect.left() + p.x / ppp, rect.top() + p.y / ppp);
        let r = egui::Rect::from_two_pos(to_screen(b.start), to_screen(b.current));
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("box-select")));
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0xff, 0xff, 0xff, 0x18));
        painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::from_rgb(0xe6, 0xe6, 0xe6)), egui::StrokeKind::Outside);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_finds_operators_by_words_in_any_order() {
        let hits = ShapeApp::search_ops("cube add");
        assert_eq!(hits.first(), Some(&Op::Add(Primitive::Cube)));
        assert!(ShapeApp::search_ops("frame").contains(&Op::FrameAll));
        assert!(ShapeApp::search_ops("zzz").is_empty());
        for op in Op::ALL {
            assert!(!op.name().is_empty());
        }
    }

    #[test]
    fn join_merges_in_the_active_space_and_separate_splits_back() {
        let mut app = ShapeApp::default();
        let a = app.add_primitive(Primitive::Cube);
        let b = app.add_primitive(Primitive::Cube);
        app.scene.object_mut(b).unwrap().transform.translation = Vec3::new(5.0, 0.0, 0.0);
        app.scene.selection.set(a);
        app.scene.selection.toggle(b);
        app.scene.selection.active = Some(a);
        app.join_selected();
        assert_eq!(app.scene.object_count(), 1);
        let (_, mesh) = app.scene.object_mesh(a).unwrap();
        assert_eq!(mesh.face_count(), 12);
        assert_eq!(mesh.vertex_count(), 16);
        assert!(mesh.positions().any(|p| p.x > 4.0), "the other cube kept its world position");
        let far: std::collections::BTreeSet<_> = mesh.face_ids().filter(|&f| mesh.face_centroid(f).x > 2.5).collect();
        // Separate the far half back out.
        app.toggle_edit_mode();
        assert_eq!(far.len(), 6);
        app.scene.edit.as_mut().unwrap().faces = far;
        app.separate_selection();
        assert_eq!(app.scene.object_count(), 2);
        let (_, kept) = app.scene.object_mesh(a).unwrap();
        assert_eq!(kept.face_count(), 6);
        assert_eq!(kept.vertex_count(), 8);
        app.history.undo(&mut app.scene);
        app.history.undo(&mut app.scene);
        assert_eq!(app.scene.object_count(), 2, "Join and Separate are one undo step each");
    }

    #[test]
    fn frame_selected_and_axis_views() {
        let mut app = ShapeApp::default();
        let a = app.add_primitive(Primitive::Cube);
        app.scene.object_mut(a).unwrap().transform.translation = Vec3::new(10.0, 0.0, 0.0);
        app.scene.selection.set(a);
        app.frame_selected();
        assert!((app.viewport.camera.target - Vec3::new(10.0, 0.0, 0.0)).length() < 1e-3);
        app.run_op(Op::View(AxisView::Top));
        assert!(app.viewport.camera.ortho);
        app.run_op(Op::TogglePerspective);
        assert!(!app.viewport.camera.ortho);
    }

    #[test]
    fn box_select_picks_objects_whose_origin_is_inside() {
        let mut app = ShapeApp::default();
        let a = app.add_primitive(Primitive::Cube);
        let b = app.add_primitive(Primitive::Sphere);
        app.scene.object_mut(b).unwrap().transform.translation = Vec3::new(30.0, 0.0, 0.0);
        app.scene.selection.clear();
        app.viewport.camera.target = Vec3::ZERO;
        let size = Vec2::new(800.0, 600.0);
        let centre = app.viewport.camera.project(Vec3::ZERO, size).unwrap();
        let mut input =
            crate::viewport::ViewportInput { size, pointer: Some(centre - Vec2::splat(40.0)), primary_pressed: true, ..Default::default() };
        app.box_pending = true;
        assert!(app.box_select_input(&input));
        input.primary_pressed = false;
        input.pointer = Some(centre + Vec2::splat(40.0));
        input.primary_released = true;
        assert!(app.box_select_input(&input));
        assert!(app.scene.selection.contains(a));
        assert!(!app.scene.selection.contains(b));
        assert!(app.box_select.is_none());
    }
}
