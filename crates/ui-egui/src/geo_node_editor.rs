//! The Geometry Node Editor: the bottom canvas of the Geometry Nodes workspace. It edits the
//! active object's first `Geometry Nodes` modifier the way Blender's editor does: drag nodes by
//! their header, drag links from an output onto an input (or off an input to detach), Shift+A
//! adds at the pointer, X deletes, Home frames, MMB / scroll pans. Node parameters are edited
//! in the node body. Every change re-evaluates the modifier stack through the viewport.

use egui::{Color32, CornerRadius, Pos2, Rect, RichText, Sense, Stroke, Vec2, pos2, vec2};
use shape_scene::{GeoNodeKind, GeoNodeTree, Modifier, ObjectId};

use crate::node_editor::EDITOR_H;
use crate::{ShapeApp, theme};

const NODE_W: f32 = 160.0;
const HEADER_H: f32 = 20.0;
const ROW_H: f32 = 22.0;
const SOCKET_R: f32 = 5.0;
/// Blender's geometry socket colour.
const GEOMETRY: Color32 = Color32::from_rgb(0x00, 0xd6, 0xa3);

/// Blender's header colours by node class.
fn header_color(kind: &GeoNodeKind) -> Color32 {
    match kind {
        GeoNodeKind::GroupInput | GeoNodeKind::GroupOutput => Color32::from_rgb(0x4d, 0x00, 0x17),
        GeoNodeKind::Transform { .. } | GeoNodeKind::JoinGeometry | GeoNodeKind::SetPosition { .. } => Color32::from_rgb(0x00, 0x7a, 0x5e),
        GeoNodeKind::MeshCube { .. } | GeoNodeKind::MeshUvSphere { .. } | GeoNodeKind::MeshCylinder { .. } | GeoNodeKind::MeshGrid { .. } => {
            Color32::from_rgb(0x00, 0x5c, 0x7a)
        }
        GeoNodeKind::SubdivideMesh { .. } | GeoNodeKind::SubdivisionSurface { .. } | GeoNodeKind::MeshBoolean { .. } => {
            Color32::from_rgb(0x00, 0x5c, 0x7a)
        }
        GeoNodeKind::InstanceOnPoints { .. } => Color32::from_rgb(0x4d, 0x67, 0x00),
    }
}

/// Parameter rows a node shows in its body.
fn param_rows(kind: &GeoNodeKind) -> usize {
    match kind {
        GeoNodeKind::Transform { .. } => 3,
        GeoNodeKind::SubdivideMesh { .. } | GeoNodeKind::SubdivisionSurface { .. } | GeoNodeKind::InstanceOnPoints { .. } | GeoNodeKind::MeshBoolean { .. } => 1,
        GeoNodeKind::SetPosition { .. } | GeoNodeKind::MeshCube { .. } => 1,
        GeoNodeKind::MeshUvSphere { .. } | GeoNodeKind::MeshCylinder { .. } => 3,
        GeoNodeKind::MeshGrid { .. } => 4,
        GeoNodeKind::GroupInput | GeoNodeKind::GroupOutput | GeoNodeKind::JoinGeometry => 0,
    }
}

fn node_size(kind: &GeoNodeKind) -> Vec2 {
    let rows = kind.inputs().len() + kind.outputs().len() + param_rows(kind);
    vec2(NODE_W, HEADER_H + ROW_H * rows as f32 + 6.0)
}

#[derive(Clone, Debug, Default)]
pub struct GeoNodeEditor {
    pub pan: Vec2,
    pub selected: Option<usize>,
    /// (node, socket, from an output?)
    pub drag: Option<(usize, usize, bool)>,
    pub add_menu: Option<Pos2>,
    framed: bool,
    pub hovered: bool,
    origin: Vec2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    Input(usize, usize),
    Output(usize, usize),
}

impl ShapeApp {
    /// The active mesh object and the index of its first Geometry Nodes modifier.
    fn geo_target(&self) -> Option<(ObjectId, usize)> {
        let id = self.scene.selection.active?;
        let i = self.scene.modifiers(id).iter().position(|m| matches!(m, Modifier::GeometryNodes { .. }))?;
        Some((id, i))
    }

    fn geo_tree(&self, id: ObjectId, index: usize) -> Option<&GeoNodeTree> {
        match self.scene.modifiers(id).get(index) {
            Some(Modifier::GeometryNodes { tree }) => Some(tree),
            _ => None,
        }
    }

    pub(crate) fn geo_node_editor_panel(&mut self, ui: &mut egui::Ui) {
        let resp = egui::Panel::bottom("geometry-node-editor").default_size(EDITOR_H).resizable(true).frame(theme::region_frame(theme::TIMELINE)).show(
            ui,
            |ui| {
                self.geo_header(ui);
                ui.set_min_height(EDITOR_H - theme::HEADER_H);
                let Some(active) = self.scene.selection.active else {
                    ui.centered_and_justified(|ui| ui.label(RichText::new("Select a mesh object").color(theme::TEXT_WEAK)));
                    return;
                };
                if self.scene.object_mesh(active).is_none() {
                    ui.centered_and_justified(|ui| ui.label(RichText::new("Select a mesh object").color(theme::TEXT_WEAK)));
                    return;
                }
                let Some((id, index)) = self.geo_target() else {
                    ui.vertical_centered(|ui| {
                        ui.add_space(40.0);
                        if ui.button("New").on_hover_text("Add a Geometry Nodes modifier with a default tree").clicked() {
                            self.add_modifier(Modifier::GeometryNodes { tree: GeoNodeTree::default_tree() });
                        }
                    });
                    return;
                };
                self.geo_canvas(ui, id, index);
            },
        );
        self.geo_editor.hovered = ui.rect_contains_pointer(resp.response.rect);
    }

    fn geo_header(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("geometry-node-editor-header").exact_size(theme::HEADER_H).frame(theme::region_frame(theme::HEADER)).show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new("Geometry Node Editor").strong());
                ui.separator();
                if let Some((id, index)) = self.geo_target() {
                    let name = self.scene.object(id).map(|o| o.name.clone()).unwrap_or_default();
                    ui.label(RichText::new(format!("{name} · Geometry Nodes")).color(theme::TEXT_WEAK));
                    ui.separator();
                    ui.menu_button("Add", |ui| {
                        if let Some(kind) = Self::geo_add_menu(ui) {
                            self.geo_add_node(id, index, kind, None);
                        }
                    });
                    ui.label(RichText::new("Shift+A add · X delete · Home frame · MMB/scroll pan").color(theme::TEXT_WEAK).small());
                }
            });
        });
    }

    fn geo_add_menu(ui: &mut egui::Ui) -> Option<GeoNodeKind> {
        let mut pick = None;
        for category in ["Geometry", "Mesh", "Mesh Primitives", "Instances", "Group"] {
            ui.label(RichText::new(category).color(theme::TEXT_WEAK));
            for kind in GeoNodeKind::ALL {
                if kind.category() == category && ui.button(kind.name()).clicked() {
                    pick = Some(kind);
                    ui.close();
                }
            }
        }
        pick
    }

    fn geo_add_node(&mut self, id: ObjectId, index: usize, kind: GeoNodeKind, at: Option<Pos2>) {
        let pos = at.map(|p| (p - self.geo_editor.origin - self.geo_editor.pan).to_vec2()).unwrap_or(vec2(80.0, 40.0));
        let name = format!("Add {}", kind.name());
        let mut new = None;
        self.edit(&name, |scene| {
            if let Some(Modifier::GeometryNodes { tree }) = scene.modifiers_mut(id).and_then(|s| s.get_mut(index)) {
                new = Some(tree.add_node(kind, [pos.x, pos.y]));
            }
        });
        self.geo_editor.selected = new;
    }

    fn geo_canvas(&mut self, ui: &mut egui::Ui, id: ObjectId, index: usize) {
        let size = vec2(ui.available_width(), ui.available_height().max(EDITOR_H - theme::HEADER_H - 8.0));
        let (bg, bg_painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let canvas = bg.rect;
        self.geo_editor.origin = canvas.min.to_vec2();
        bg_painter.rect_filled(canvas, 0.0, Color32::from_rgb(0x1d, 0x1d, 0x1d));
        let step = 20.0;
        let off = vec2(self.geo_editor.pan.x.rem_euclid(step), self.geo_editor.pan.y.rem_euclid(step));
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

        if self.geo_editor.drag.is_none() && (bg.dragged_by(egui::PointerButton::Middle) || bg.dragged_by(egui::PointerButton::Primary)) {
            self.geo_editor.pan += bg.drag_delta();
        }
        if bg.hovered() {
            self.geo_editor.pan += ui.input(|i| i.smooth_scroll_delta);
        }
        if bg.clicked() {
            self.geo_editor.selected = None;
            ui.ctx().memory_mut(|m| {
                if let Some(f) = m.focused() {
                    m.surrender_focus(f);
                }
            });
        }
        if bg.hovered() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary)) {
            self.geo_editor.add_menu = ui.ctx().pointer_latest_pos();
        }

        let Some(original) = self.geo_tree(id, index).cloned() else { return };
        let mut tree = original.clone();
        let origin = canvas.min + self.geo_editor.pan;
        let to_screen = |p: [f32; 2]| origin + vec2(p[0], p[1]);
        let mut sockets: Vec<(Hit, Pos2)> = Vec::new();
        let mut changed = false;
        let mut live = false;
        let mut delete: Option<usize> = None;
        let mut hovered_node = None;

        let mut order: Vec<usize> = (0..tree.nodes.len()).collect();
        if let Some(sel) = self.geo_editor.selected {
            order.retain(|&i| i != sel);
            if sel < tree.nodes.len() {
                order.push(sel);
            }
        }
        for ni in order {
            let node = tree.nodes[ni].clone();
            let size = node_size(&node.kind);
            let rect = Rect::from_min_size(to_screen(node.pos), size);
            let selected = self.geo_editor.selected == Some(ni);
            painter.rect_filled(rect, CornerRadius::same(4), Color32::from_rgb(0x30, 0x30, 0x30));
            painter.rect_filled(Rect::from_min_size(rect.min, vec2(size.x, HEADER_H)), CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, header_color(&node.kind));
            painter.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, if selected { Color32::WHITE } else { theme::OUTLINE }), egui::StrokeKind::Outside);
            painter.text(rect.min + vec2(8.0, HEADER_H / 2.0), egui::Align2::LEFT_CENTER, node.kind.name(), egui::FontId::proportional(12.0), theme::TEXT);
            let header = ui.interact(Rect::from_min_size(rect.min, vec2(size.x, HEADER_H)), ui.id().with(("geo-node-header", ni)), Sense::click_and_drag());
            if header.drag_started() || header.clicked() {
                self.geo_editor.selected = Some(ni);
            }
            if header.dragged() {
                let n = &mut tree.nodes[ni];
                n.pos[0] += header.drag_delta().x;
                n.pos[1] += header.drag_delta().y;
                changed = true;
                live = true;
            }
            if header.drag_stopped() {
                changed = true;
            }
            if header.hovered() || ui.rect_contains_pointer(rect) {
                hovered_node = Some(ni);
            }
            let mut y = rect.min.y + HEADER_H + 3.0;
            for (i, out) in node.kind.outputs().iter().enumerate() {
                let row = Rect::from_min_size(pos2(rect.min.x, y), vec2(size.x, ROW_H));
                painter.text(pos2(rect.max.x - 10.0, row.center().y), egui::Align2::RIGHT_CENTER, *out, egui::FontId::proportional(11.0), theme::TEXT);
                let c = pos2(rect.max.x, row.center().y);
                painter.circle(c, SOCKET_R, GEOMETRY, Stroke::new(1.0, Color32::BLACK));
                sockets.push((Hit::Output(ni, i), c));
                y += ROW_H;
            }
            // Parameters.
            {
                let kind = &mut tree.nodes[ni].kind;
                let mut row = |ui: &mut egui::Ui, label: &str, add: &mut dyn FnMut(&mut egui::Ui) -> egui::Response| {
                    let r = Rect::from_min_size(pos2(rect.min.x + 8.0, y + 2.0), vec2(size.x - 16.0, ROW_H - 4.0));
                    let resp = ui.scope_builder(egui::UiBuilder::new().max_rect(r), |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(label).small());
                            add(ui)
                        })
                        .inner
                    });
                    y += ROW_H;
                    resp.inner
                };
                let mut touched: Vec<egui::Response> = Vec::new();
                match kind {
                    GeoNodeKind::Transform { translation, rotation, scale } => {
                        touched.push(row(ui, "Move", &mut |ui| vec3_row(ui, translation, 0.01)));
                        touched.push(row(ui, "Rot", &mut |ui| vec3_row(ui, rotation, 0.01)));
                        touched.push(row(ui, "Scale", &mut |ui| vec3_row(ui, scale, 0.01)));
                    }
                    GeoNodeKind::SetPosition { offset } => touched.push(row(ui, "Offset", &mut |ui| vec3_row(ui, offset, 0.01))),
                    GeoNodeKind::MeshCube { size: s } => touched.push(row(ui, "Size", &mut |ui| vec3_row(ui, s, 0.01))),
                    GeoNodeKind::SubdivideMesh { level } | GeoNodeKind::SubdivisionSurface { level } => {
                        touched.push(row(ui, "Level", &mut |ui| ui.add(egui::DragValue::new(level).range(0..=6))));
                    }
                    GeoNodeKind::InstanceOnPoints { scale } => {
                        touched.push(row(ui, "Scale", &mut |ui| ui.add(egui::DragValue::new(scale).speed(0.01).range(0.0..=100.0))));
                    }
                    GeoNodeKind::MeshBoolean { op } => {
                        touched.push(row(ui, "", &mut |ui| {
                            let mut any = ui.label("");
                            for o in shape_scene::BooleanOp::ALL {
                                let r = ui.selectable_value(op, o, o.name());
                                if r.clicked() {
                                    any = r;
                                }
                            }
                            any
                        }));
                    }
                    GeoNodeKind::MeshUvSphere { radius, segments, rings } => {
                        touched.push(row(ui, "Radius", &mut |ui| ui.add(egui::DragValue::new(radius).speed(0.01).range(0.001..=1000.0))));
                        touched.push(row(ui, "Segments", &mut |ui| ui.add(egui::DragValue::new(segments).range(3..=256))));
                        touched.push(row(ui, "Rings", &mut |ui| ui.add(egui::DragValue::new(rings).range(2..=128))));
                    }
                    GeoNodeKind::MeshCylinder { radius, depth, vertices } => {
                        touched.push(row(ui, "Radius", &mut |ui| ui.add(egui::DragValue::new(radius).speed(0.01).range(0.001..=1000.0))));
                        touched.push(row(ui, "Depth", &mut |ui| ui.add(egui::DragValue::new(depth).speed(0.01).range(0.001..=1000.0))));
                        touched.push(row(ui, "Vertices", &mut |ui| ui.add(egui::DragValue::new(vertices).range(3..=256))));
                    }
                    GeoNodeKind::MeshGrid { size_x, size_y, vertices_x, vertices_y } => {
                        touched.push(row(ui, "Size X", &mut |ui| ui.add(egui::DragValue::new(size_x).speed(0.01).range(0.001..=1000.0))));
                        touched.push(row(ui, "Size Y", &mut |ui| ui.add(egui::DragValue::new(size_y).speed(0.01).range(0.001..=1000.0))));
                        touched.push(row(ui, "Verts X", &mut |ui| ui.add(egui::DragValue::new(vertices_x).range(2..=256))));
                        touched.push(row(ui, "Verts Y", &mut |ui| ui.add(egui::DragValue::new(vertices_y).range(2..=256))));
                    }
                    GeoNodeKind::GroupInput | GeoNodeKind::GroupOutput | GeoNodeKind::JoinGeometry => {}
                }
                for r in touched {
                    if r.changed() {
                        changed = true;
                        live = r.dragged() || r.has_focus();
                    }
                }
            }
            for (i, inp) in node.kind.inputs().iter().enumerate() {
                let row = Rect::from_min_size(pos2(rect.min.x, y), vec2(size.x, ROW_H));
                let c = pos2(rect.min.x, row.center().y);
                painter.circle(c, SOCKET_R, GEOMETRY, Stroke::new(1.0, Color32::BLACK));
                sockets.push((Hit::Input(ni, i), c));
                painter.text(pos2(rect.min.x + 10.0, row.center().y), egui::Align2::LEFT_CENTER, *inp, egui::FontId::proportional(11.0), theme::TEXT);
                y += ROW_H;
            }
        }

        let find = |hit: Hit| sockets.iter().find(|(h, _)| *h == hit).map(|(_, p)| *p);
        for l in &tree.links {
            if let (Some(a), Some(b)) = (find(Hit::Output(l.from_node, l.from_socket)), find(Hit::Input(l.to_node, l.to_socket))) {
                draw_link(&painter, a, b);
            }
        }

        let pointer = ui.input(|i| i.pointer.clone());
        let pointer_pos = pointer.latest_pos();
        let hit_at = |p: Pos2| sockets.iter().find(|(_, c)| c.distance(p) <= SOCKET_R + 4.0).map(|(h, _)| *h);
        if let Some(p) = pointer_pos
            && canvas.contains(p)
        {
            if pointer.primary_pressed()
                && let Some(hit) = hit_at(p)
            {
                match hit {
                    Hit::Output(n, s) => self.geo_editor.drag = Some((n, s, true)),
                    Hit::Input(n, s) => {
                        if let Some(l) = tree.input_link(n, s).copied() {
                            tree.links.retain(|x| !(x.to_node == n && x.to_socket == s));
                            changed = true;
                            self.geo_editor.drag = Some((l.from_node, l.from_socket, true));
                        } else {
                            self.geo_editor.drag = Some((n, s, false));
                        }
                    }
                }
            }
            if let Some((n, s, from_output)) = self.geo_editor.drag {
                if let Some(a) = find(if from_output { Hit::Output(n, s) } else { Hit::Input(n, s) }) {
                    if from_output {
                        draw_link(&painter, a, p);
                    } else {
                        draw_link(&painter, p, a);
                    }
                }
                if pointer.primary_released() {
                    if let Some(hit) = hit_at(p) {
                        let ok = match (from_output, hit) {
                            (true, Hit::Input(tn, ts)) => tree.link(n, s, tn, ts),
                            (false, Hit::Output(fn_, fs)) => tree.link(fn_, fs, n, s),
                            _ => false,
                        };
                        if ok {
                            changed = true;
                        } else {
                            self.status = "Link refused: would form a cycle".into();
                        }
                    }
                    self.geo_editor.drag = None;
                }
            }
        } else if pointer.primary_released() {
            self.geo_editor.drag = None;
        }
        if self.geo_editor.drag.is_some() {
            ui.ctx().request_repaint();
        }

        if ui.rect_contains_pointer(canvas) && ui.ctx().memory(|m| m.focused().is_none()) {
            if ui.input_mut(|i| crate::key(i, egui::Modifiers::SHIFT, egui::Key::A)) {
                self.geo_editor.add_menu = pointer_pos;
            }
            if ui.input_mut(|i| crate::key(i, egui::Modifiers::NONE, egui::Key::X) || crate::key(i, egui::Modifiers::NONE, egui::Key::Delete)) {
                delete = self.geo_editor.selected.or(hovered_node);
            }
            if ui.input_mut(|i| crate::key(i, egui::Modifiers::NONE, egui::Key::Home)) {
                self.geo_frame(&tree, canvas);
            }
        }
        if let Some(i) = delete
            && tree.nodes.get(i).is_some_and(|n| !matches!(n.kind, GeoNodeKind::GroupOutput | GeoNodeKind::GroupInput))
        {
            tree.remove_node(i);
            self.geo_editor.selected = None;
            changed = true;
        }

        if let Some(at) = self.geo_editor.add_menu {
            let mut pick = None;
            let area = egui::Area::new(egui::Id::new("geo-node-add-menu")).fixed_pos(at).order(egui::Order::Foreground);
            let resp = area.show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(160.0);
                    ui.label(RichText::new("Add").strong());
                    ui.separator();
                    pick = Self::geo_add_menu(ui);
                });
            });
            let outside = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !resp.response.rect.expand(4.0).contains(p)));
            if let Some(kind) = pick.clone() {
                self.geo_add_node(id, index, kind, Some(at));
            }
            if pick.is_some() || ui.input(|i| i.key_pressed(egui::Key::Escape)) || outside {
                self.geo_editor.add_menu = None;
            }
        }

        if changed && tree != original {
            let write = move |scene: &mut shape_scene::Scene| {
                if let Some(Modifier::GeometryNodes { tree: slot }) = scene.modifiers_mut(id).and_then(|s| s.get_mut(index)) {
                    *slot = tree;
                }
            };
            if live {
                if self.property_before.is_none() {
                    self.property_before = Some(self.scene.clone());
                }
                write(&mut self.scene);
            } else if self.property_before.is_some() {
                write(&mut self.scene);
            } else {
                self.edit("Edit Geometry Nodes", write);
            }
        } else if changed && self.property_before.is_some() && !live {
            // A drag ended with no net change; the pending snapshot is dropped by the frame end.
        }
        if !self.geo_editor.framed {
            self.geo_editor.framed = true;
            if let Some(t) = self.geo_tree(id, index).cloned() {
                self.geo_frame(&t, canvas);
            }
        }
    }

    fn geo_frame(&mut self, tree: &GeoNodeTree, canvas: Rect) {
        if tree.nodes.is_empty() {
            return;
        }
        let mut min = pos2(f32::MAX, f32::MAX);
        let mut max = pos2(f32::MIN, f32::MIN);
        for n in &tree.nodes {
            let s = node_size(&n.kind);
            min = min.min(pos2(n.pos[0], n.pos[1]));
            max = max.max(pos2(n.pos[0] + s.x, n.pos[1] + s.y));
        }
        let centre = (min.to_vec2() + max.to_vec2()) * 0.5;
        self.geo_editor.pan = canvas.size() * 0.5 - centre;
    }
}

fn vec3_row(ui: &mut egui::Ui, v: &mut shape_math::Vec3, speed: f64) -> egui::Response {
    let a = ui.add(egui::DragValue::new(&mut v.x).speed(speed).max_decimals(2));
    let b = ui.add(egui::DragValue::new(&mut v.y).speed(speed).max_decimals(2));
    let c = ui.add(egui::DragValue::new(&mut v.z).speed(speed).max_decimals(2));
    a | b | c
}

fn draw_link(painter: &egui::Painter, a: Pos2, b: Pos2) {
    let dx = ((b.x - a.x).abs() * 0.5).max(30.0);
    let shape = egui::epaint::CubicBezierShape::from_points_stroke([a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b], false, Color32::TRANSPARENT, Stroke::new(2.0, GEOMETRY));
    painter.add(shape);
}
