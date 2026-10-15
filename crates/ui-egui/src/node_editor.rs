//! The Shader Editor: a node canvas at the bottom of the Shading workspace, behaving like
//! Blender's. Nodes drag by their header, links drag from an output socket onto an input (or off
//! an input to detach), MMB / scroll pans, Shift+A adds at the pointer, X deletes the selected
//! node, Home frames everything. Every change re-evaluates the tree into the flat material.

use egui::{Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, Vec2, pos2, vec2};
use shape_scene::nodes::{NodeKind, NodeTree, SocketType, SocketValue};
use shape_scene::{MaterialId, ObjectData};

use crate::{ShapeApp, theme};

pub const NODE_W: f32 = 150.0;
const HEADER_H: f32 = 20.0;
const ROW_H: f32 = 22.0;
const SOCKET_R: f32 = 5.0;
pub const EDITOR_H: f32 = 260.0;

/// Blender's node header colours by class.
fn header_color(kind: &NodeKind) -> Color32 {
    match kind {
        NodeKind::MaterialOutput => Color32::from_rgb(0x4d, 0x00, 0x17),
        NodeKind::PrincipledBsdf => Color32::from_rgb(0x2b, 0x65, 0x2b),
        NodeKind::ImageTexture { .. } => Color32::from_rgb(0x79, 0x46, 0x1d),
        NodeKind::MixColor => Color32::from_rgb(0x6e, 0x6e, 0x1d),
        NodeKind::Rgb | NodeKind::Value => Color32::from_rgb(0x83, 0x31, 0x4a),
    }
}

fn socket_color(ty: SocketType) -> Color32 {
    match ty {
        SocketType::Color => Color32::from_rgb(0xc7, 0xc7, 0x29),
        SocketType::Float => Color32::from_rgb(0xa1, 0xa1, 0xa1),
        SocketType::Shader => Color32::from_rgb(0x63, 0xc7, 0x63),
    }
}

/// Transient editor state (not saved).
#[derive(Clone, Debug, Default)]
pub struct NodeEditor {
    pub pan: Vec2,
    pub selected: Option<u32>,
    /// A link being dragged: (node, socket, from an output?).
    pub drag: Option<(u32, usize, bool)>,
    /// Pending Shift+A menu position (screen).
    pub add_menu: Option<Pos2>,
    framed: bool,
    /// The pointer was over the editor last frame: viewport hotkeys stand down.
    pub hovered: bool,
}

#[derive(Clone, Copy, Debug)]
enum Hit {
    Input(u32, usize),
    Output(u32, usize),
}

fn node_size(kind: &NodeKind) -> Vec2 {
    let rows =
        kind.outputs().len() + kind.inputs().len() + usize::from(matches!(kind, NodeKind::Rgb | NodeKind::Value | NodeKind::ImageTexture { .. }));
    vec2(NODE_W, HEADER_H + rows as f32 * ROW_H + 6.0)
}

impl ShapeApp {
    /// The material edited by the shader editor: the active object's.
    pub(crate) fn shader_material(&self) -> Option<MaterialId> {
        let id = self.scene.selection.active?;
        match &self.scene.object(id)?.data {
            ObjectData::Mesh { material, .. } => *material,
            _ => None,
        }
    }

    pub(crate) fn node_editor_panel(&mut self, ui: &mut egui::Ui) {
        let resp =
            egui::Panel::bottom("shader-editor").default_size(EDITOR_H).resizable(true).frame(theme::region_frame(theme::TIMELINE)).show(ui, |ui| {
                self.node_editor_header(ui);
                ui.set_min_height(EDITOR_H - theme::HEADER_H);
                let Some(mid) = self.shader_material() else {
                    ui.centered_and_justified(|ui| ui.label(RichText::new("Select a mesh with a material").color(theme::TEXT_WEAK)));
                    return;
                };
                let has_nodes = self.scene.material(mid).is_some_and(|m| m.nodes.is_some());
                if !has_nodes {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        if ui.button("Use Nodes").clicked() {
                            self.edit("Use Nodes", |scene| {
                                if let Some(m) = scene.material_mut(mid) {
                                    m.nodes = Some(NodeTree::from_material(m));
                                }
                            });
                        }
                    });
                    return;
                }
                self.node_canvas(ui, mid);
            });
        self.node_editor.hovered = ui.rect_contains_pointer(resp.response.rect);
    }

    fn node_editor_header(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("shader-editor-header").exact_size(theme::HEADER_H).frame(theme::region_frame(theme::HEADER)).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("Shader Editor").strong());
                ui.separator();
                if let Some(mid) = self.shader_material() {
                    let name = self.scene.material(mid).map(|m| m.name.clone()).unwrap_or_default();
                    ui.label(RichText::new(name).color(theme::TEXT_WEAK));
                    let has_nodes = self.scene.material(mid).is_some_and(|m| m.nodes.is_some());
                    if has_nodes {
                        ui.separator();
                        ui.menu_button("Add", |ui| {
                            if let Some(kind) = Self::add_node_menu(ui) {
                                self.add_node_at(mid, kind, None);
                            }
                        });
                        ui.label(RichText::new("Shift+A add · X delete · Home frame · MMB/scroll pan").color(theme::TEXT_WEAK).small());
                    }
                }
            });
        });
    }

    fn add_node_menu(ui: &mut egui::Ui) -> Option<NodeKind> {
        let mut pick = None;
        for category in ["Input", "Shader", "Texture", "Color", "Output"] {
            ui.label(RichText::new(category).color(theme::TEXT_WEAK));
            for kind in NodeKind::ALL {
                if kind.category() == category && ui.button(kind.name()).clicked() {
                    pick = Some(kind);
                    ui.close();
                }
            }
        }
        pick
    }

    /// Add a node at a screen position (or the canvas centre when `None`).
    fn add_node_at(&mut self, mid: MaterialId, kind: NodeKind, at: Option<Pos2>) {
        let pan = self.node_editor.pan;
        let origin = self.node_editor_origin;
        let pos = at.map(|p| (p - origin - pan).to_vec2()).unwrap_or(vec2(80.0, 40.0));
        let name = format!("Add {}", kind.name());
        let mut new_id = None;
        self.edit(&name, |scene| {
            if let Some(m) = scene.material_mut(mid)
                && let Some(t) = m.nodes.as_mut()
            {
                new_id = Some(t.add(kind, [pos.x, pos.y]));
            }
        });
        self.node_editor.selected = new_id;
    }

    fn node_canvas(&mut self, ui: &mut egui::Ui, mid: MaterialId) {
        // egui panels size to their content, so the canvas asks for the editor's height itself.
        let size = vec2(ui.available_width(), ui.available_height().max(EDITOR_H - theme::HEADER_H - 8.0));
        let (bg, bg_painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let canvas = bg.rect;
        self.node_editor_origin = canvas.min.to_vec2();
        bg_painter.rect_filled(canvas, 0.0, Color32::from_rgb(0x1d, 0x1d, 0x1d));
        // Dot grid like Blender's.
        let step = 20.0;
        let off = vec2(self.node_editor.pan.x.rem_euclid(step), self.node_editor.pan.y.rem_euclid(step));
        let mut x = canvas.min.x + off.x;
        while x < canvas.max.x {
            let mut y = canvas.min.y + off.y;
            while y < canvas.max.y {
                bg_painter.circle_filled(pos2(x, y), 0.8, Color32::from_rgb(0x28, 0x28, 0x28));
                y += step;
            }
            x += step;
        }
        ui.set_clip_rect(canvas);
        let painter = ui.painter_at(canvas);

        // Pan: middle drag, scroll, or dragging the empty background.
        if self.node_editor.drag.is_none() && (bg.dragged_by(egui::PointerButton::Middle) || bg.dragged_by(egui::PointerButton::Primary)) {
            self.node_editor.pan += bg.drag_delta();
        }
        if bg.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta);
            self.node_editor.pan += scroll;
        }
        if bg.clicked() {
            self.node_editor.selected = None;
            ui.ctx().memory_mut(|m| {
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        if bg.hovered() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary)) {
            self.node_editor.add_menu = ui.ctx().pointer_latest_pos();
        }

        let Some(original) = self.scene.material(mid).cloned() else { return };
        let Some(mut tree) = original.nodes.clone() else { return };
        let origin = canvas.min + self.node_editor.pan;
        let to_screen = |p: [f32; 2]| origin + vec2(p[0], p[1]);
        let mut sockets: Vec<(Hit, Pos2, SocketType)> = Vec::new();
        let mut changed = false;
        let mut live = false; // a widget is mid-drag: don't record yet
        let mut delete: Option<u32> = None;
        let mut hovered_node = None;

        // Nodes, in order; the selected one last so it draws on top.
        let mut order: Vec<u32> = tree.nodes.iter().map(|n| n.id).collect();
        if let Some(sel) = self.node_editor.selected {
            order.retain(|&id| id != sel);
            order.push(sel);
        }
        for id in order {
            let Some(node) = tree.node(id).cloned() else { continue };
            let size = node_size(&node.kind);
            let rect = Rect::from_min_size(to_screen(node.pos), size);
            let selected = self.node_editor.selected == Some(id);
            painter.rect_filled(rect, CornerRadius::same(4), Color32::from_rgb(0x30, 0x30, 0x30));
            painter.rect_filled(
                Rect::from_min_size(rect.min, vec2(size.x, HEADER_H)),
                CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 },
                header_color(&node.kind),
            );
            painter.rect_stroke(
                rect,
                CornerRadius::same(4),
                Stroke::new(1.0, if selected { Color32::WHITE } else { theme::OUTLINE }),
                egui::StrokeKind::Outside,
            );
            painter.text(
                rect.min + vec2(8.0, HEADER_H / 2.0),
                egui::Align2::LEFT_CENTER,
                node.kind.name(),
                egui::FontId::proportional(12.0),
                theme::TEXT,
            );
            // Header: drag moves, click selects.
            let header =
                ui.interact(Rect::from_min_size(rect.min, vec2(size.x, HEADER_H)), ui.id().with(("node-header", id)), Sense::click_and_drag());
            if header.drag_started() || header.clicked() {
                self.node_editor.selected = Some(id);
            }
            if header.dragged()
                && let Some(n) = tree.node_mut(id)
            {
                n.pos[0] += header.drag_delta().x;
                n.pos[1] += header.drag_delta().y;
                changed = true;
                live = true;
            }
            if header.drag_stopped() {
                changed = true;
            }
            if header.hovered() || ui.rect_contains_pointer(rect) {
                hovered_node = Some(id);
            }
            let mut y = rect.min.y + HEADER_H + 3.0;
            // Outputs: right side, label right-aligned.
            for (i, out) in node.kind.outputs().iter().enumerate() {
                let row = Rect::from_min_size(pos2(rect.min.x, y), vec2(size.x, ROW_H));
                painter.text(
                    pos2(rect.max.x - 10.0, row.center().y),
                    egui::Align2::RIGHT_CENTER,
                    out.name,
                    egui::FontId::proportional(11.0),
                    theme::TEXT,
                );
                let c = pos2(rect.max.x, row.center().y);
                painter.circle(c, SOCKET_R, socket_color(out.ty), Stroke::new(1.0, Color32::BLACK));
                sockets.push((Hit::Output(id, i), c, out.ty));
                y += ROW_H;
            }
            // Own value row for RGB / Value / Image Texture.
            match &node.kind {
                NodeKind::Rgb => {
                    let row = Rect::from_min_size(pos2(rect.min.x + 8.0, y + 2.0), vec2(size.x - 16.0, ROW_H - 4.0));
                    if let Some(SocketValue::Color(c)) = tree.node_mut(id).and_then(|n| n.values.get_mut(0)) {
                        let r = ui.put(row, ColorButton(c));
                        if r.changed() {
                            changed = true;
                            live = true;
                        }
                    }
                    y += ROW_H;
                }
                NodeKind::Value => {
                    let row = Rect::from_min_size(pos2(rect.min.x + 8.0, y + 2.0), vec2(size.x - 16.0, ROW_H - 4.0));
                    if let Some(SocketValue::Float(f)) = tree.node_mut(id).and_then(|n| n.values.get_mut(0)) {
                        let r = ui.put(row, egui::DragValue::new(f).speed(0.01));
                        if r.changed() {
                            changed = true;
                            live = r.dragged() || r.has_focus();
                        }
                    }
                    y += ROW_H;
                }
                NodeKind::ImageTexture { texture } => {
                    let row = Rect::from_min_size(pos2(rect.min.x + 8.0, y + 2.0), vec2(size.x - 16.0, ROW_H - 4.0));
                    let label = texture.and_then(|t| self.scene.texture(t)).map(|t| t.name.clone()).unwrap_or("Open…".into());
                    if ui.put(row, egui::Button::new(RichText::new(label).small())).clicked()
                        && let Some(path) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg"]).pick_file()
                    {
                        match shape_scene::Texture::load(&path) {
                            Ok(tex) => {
                                let tid = self.scene.add_texture(tex);
                                if let Some(n) = tree.node_mut(id) {
                                    n.kind = NodeKind::ImageTexture { texture: Some(tid) };
                                }
                                self.ensure_uvs_for_material(mid);
                                changed = true;
                            }
                            Err(e) => self.status = format!("Image failed: {e}"),
                        }
                    }
                    y += ROW_H;
                }
                _ => {}
            }
            // Inputs: left side; unlinked ones show their constant.
            for (i, inp) in node.kind.inputs().iter().enumerate() {
                let row = Rect::from_min_size(pos2(rect.min.x, y), vec2(size.x, ROW_H));
                let c = pos2(rect.min.x, row.center().y);
                painter.circle(c, SOCKET_R, socket_color(inp.ty), Stroke::new(1.0, Color32::BLACK));
                sockets.push((Hit::Input(id, i), c, inp.ty));
                let linked = tree.input_link(id, i).is_some();
                painter.text(
                    pos2(rect.min.x + 10.0, row.center().y),
                    egui::Align2::LEFT_CENTER,
                    inp.name,
                    egui::FontId::proportional(11.0),
                    theme::TEXT,
                );
                if !linked && inp.ty != SocketType::Shader {
                    let w = Rect::from_min_size(pos2(rect.max.x - 54.0, y + 3.0), vec2(46.0, ROW_H - 6.0));
                    match tree.node_mut(id).and_then(|n| n.values.get_mut(i)) {
                        Some(SocketValue::Color(col)) => {
                            let r = ui.put(w, ColorButton(col));
                            if r.changed() {
                                changed = true;
                                live = true;
                            }
                        }
                        Some(SocketValue::Float(f)) => {
                            let r = ui.put(w, egui::DragValue::new(f).speed(0.01).range(0.0..=10.0));
                            if r.changed() {
                                changed = true;
                                live = r.dragged() || r.has_focus();
                            }
                        }
                        _ => {}
                    }
                }
                y += ROW_H;
            }
        }

        // Links as bezier curves.
        let find = |hit: Hit| {
            sockets.iter().find(|(h, _, _)| matches!((h, hit), (Hit::Input(a, b), Hit::Input(c, d)) | (Hit::Output(a, b), Hit::Output(c, d)) if *a == c && *b == d)).map(|(_, p, ty)| (*p, *ty))
        };
        for l in &tree.links {
            if let (Some((a, ty)), Some((b, _))) = (find(Hit::Output(l.from_node, l.from_socket)), find(Hit::Input(l.to_node, l.to_socket))) {
                draw_link(&painter, a, b, socket_color(ty));
            }
        }

        // Socket interaction: drag from an output (or off a linked input) to a target input.
        let pointer = ui.input(|i| i.pointer.clone());
        let pointer_pos = pointer.latest_pos();
        let hit_at = |p: Pos2| sockets.iter().find(|(_, c, _)| c.distance(p) <= SOCKET_R + 4.0).map(|(h, c, ty)| (*h, *c, *ty));
        if let Some(p) = pointer_pos
            && canvas.contains(p)
        {
            if pointer.primary_pressed()
                && let Some((hit, _, _)) = hit_at(p)
            {
                match hit {
                    Hit::Output(n, s) => self.node_editor.drag = Some((n, s, true)),
                    Hit::Input(n, s) => {
                        if let Some(l) = tree.input_link(n, s) {
                            // Pick the link up from its source.
                            tree.unlink_input(n, s);
                            changed = true;
                            self.node_editor.drag = Some((l.from_node, l.from_socket, true));
                        } else {
                            self.node_editor.drag = Some((n, s, false));
                        }
                    }
                }
            }
            if let Some((n, s, from_output)) = self.node_editor.drag {
                let start = find(if from_output { Hit::Output(n, s) } else { Hit::Input(n, s) });
                if let Some((a, ty)) = start {
                    if from_output {
                        draw_link(&painter, a, p, socket_color(ty));
                    } else {
                        draw_link(&painter, p, a, socket_color(ty));
                    }
                }
                if pointer.primary_released() {
                    if let Some((hit, _, _)) = hit_at(p) {
                        let ok = match (from_output, hit) {
                            (true, Hit::Input(tn, ts)) => tree.link(n, s, tn, ts),
                            (false, Hit::Output(fn_, fs)) => tree.link(fn_, fs, n, s),
                            _ => false,
                        };
                        if ok {
                            changed = true;
                        } else {
                            self.status = "Link refused: sockets must match and not form a cycle".into();
                        }
                    }
                    self.node_editor.drag = None;
                }
            }
        } else if pointer.primary_released() {
            self.node_editor.drag = None;
        }
        if self.node_editor.drag.is_some() {
            ui.ctx().request_repaint();
        }

        // Keys while the pointer is over the editor.
        if ui.rect_contains_pointer(canvas) && ui.ctx().memory(|m| m.focused().is_none()) {
            if ui.input_mut(|i| crate::key(i, egui::Modifiers::SHIFT, egui::Key::A)) {
                self.node_editor.add_menu = pointer_pos;
            }
            if ui.input_mut(|i| crate::key(i, egui::Modifiers::NONE, egui::Key::X) || crate::key(i, egui::Modifiers::NONE, egui::Key::Delete)) {
                delete = self.node_editor.selected.or(hovered_node);
            }
            if ui.input_mut(|i| crate::key(i, egui::Modifiers::NONE, egui::Key::Home)) {
                self.frame_nodes(&tree, canvas);
            }
        }
        if let Some(id) = delete
            && tree.node(id).is_some_and(|n| n.kind != NodeKind::MaterialOutput)
        {
            tree.remove(id);
            self.node_editor.selected = None;
            changed = true;
        }

        // Shift+A / RMB menu at the pointer.
        if let Some(at) = self.node_editor.add_menu {
            let mut close = false;
            let mut pick = None;
            let area = egui::Area::new(egui::Id::new("node-add-menu")).fixed_pos(at).order(egui::Order::Foreground);
            let resp = area.show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(150.0);
                    ui.label(RichText::new("Add").strong());
                    ui.separator();
                    pick = Self::add_node_menu(ui);
                });
            });
            let outside =
                ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !resp.response.rect.expand(4.0).contains(p)));
            if pick.is_some() || ui.input(|i| i.key_pressed(egui::Key::Escape)) || outside {
                close = true;
            }
            if let Some(kind) = pick {
                self.add_node_at(mid, kind, Some(at));
            }
            if close {
                self.node_editor.add_menu = None;
            }
        }

        if changed {
            let mut m = original.clone();
            m.nodes = Some(tree);
            m.apply_nodes();
            if live {
                if self.property_before.is_none() {
                    self.property_before = Some(self.scene.clone());
                }
                if let Some(slot) = self.scene.material_mut(mid) {
                    *slot = m;
                }
            } else if self.property_before.is_some() {
                // A drag just ended: the pending snapshot becomes the undo step.
                if let Some(slot) = self.scene.material_mut(mid) {
                    *slot = m;
                }
            } else {
                self.edit("Edit Node", move |scene| {
                    if let Some(slot) = scene.material_mut(mid) {
                        *slot = m;
                    }
                });
            }
        }
        if !self.node_editor.framed {
            self.node_editor.framed = true;
            if let Some(t) = self.scene.material(mid).and_then(|m| m.nodes.clone()) {
                self.frame_nodes(&t, canvas);
            }
        }
    }

    /// Home: pan so every node is visible (no zoom).
    fn frame_nodes(&mut self, tree: &NodeTree, canvas: Rect) {
        if tree.nodes.is_empty() {
            return;
        }
        let mut bb = Rect::NOTHING;
        for n in &tree.nodes {
            bb = bb.union(Rect::from_min_size(pos2(n.pos[0], n.pos[1]), node_size(&n.kind)));
        }
        self.node_editor.pan = canvas.center() - canvas.min - bb.center().to_vec2();
    }

    /// An image texture needs UVs: box-project meshes that use this material and have none.
    fn ensure_uvs_for_material(&mut self, mid: MaterialId) {
        let targets: Vec<_> = self
            .scene
            .objects()
            .filter_map(|(_, o)| match &o.data {
                ObjectData::Mesh { material: Some(m), mesh, .. } if *m == mid => Some(*mesh),
                _ => None,
            })
            .collect();
        for mesh_id in targets {
            if self.scene.mesh(mesh_id).is_some_and(|d| d.uvs.is_none()) {
                let uvs = self.scene.mesh(mesh_id).map(|d| shape_ops::uv_box(&d.mesh));
                if let (Some(uvs), Some(d)) = (uvs, self.scene.mesh_mut(mesh_id)) {
                    d.uvs = Some(uvs);
                }
            }
        }
    }
}

/// `ui.put`-able colour button (egui only offers the free function).
struct ColorButton<'a>(&'a mut [f32; 4]);

impl egui::Widget for ColorButton<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        ui.color_edit_button_rgba_unmultiplied(self.0)
    }
}

fn draw_link(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32) {
    let dx = ((b.x - a.x).abs() * 0.5).max(30.0);
    let shape = egui::epaint::CubicBezierShape::from_points_stroke(
        [a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b],
        false,
        Color32::TRANSPARENT,
        Stroke::new(2.0, color),
    );
    painter.add(shape);
}
