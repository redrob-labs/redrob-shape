//! The default screen, laid out the way Blender lays out its "Layout" workspace: a top bar with
//! menus and workspace tabs, the 3D viewport with its own header, a tool column (T) and a sidebar
//! (N), a right column holding the Outliner over the Properties editor with its tab strip, and a
//! status bar. Measurements and colours are in [`crate::theme`].

use crate::keymap::Popup;
use crate::theme::{self, HEADER_H, OUTLINER_H, PROPS_TABS_W, RIGHT_COLUMN_W, SIDEBAR_W, STATUS_H, TOOLBAR_W, TOP_BAR_H};
use crate::tools::Tool;
use crate::{ShapeApp, VERSION};
use egui::{Color32, RichText};
use shape_gpu::ShadingMode;
use shape_math::Vec3;
use shape_scene::ObjectData;

/// Tabs of the Properties editor, in Blender's order, limited to the ones that have content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropsTab {
    Render,
    Output,
    World,
    Object,
    Modifiers,
    Data,
    Material,
}

impl PropsTab {
    pub const ALL: [PropsTab; 7] =
        [PropsTab::Render, PropsTab::Output, PropsTab::World, PropsTab::Object, PropsTab::Modifiers, PropsTab::Data, PropsTab::Material];

    pub fn name(self) -> &'static str {
        match self {
            PropsTab::Render => "Render",
            PropsTab::Output => "Output",
            PropsTab::World => "World",
            PropsTab::Object => "Object",
            PropsTab::Modifiers => "Modifiers",
            PropsTab::Data => "Data",
            PropsTab::Material => "Material",
        }
    }

    /// One-glyph stand-in for Blender's tab icons.
    pub fn glyph(self) -> &'static str {
        match self {
            PropsTab::Render => "📷",
            PropsTab::Output => "🖨",
            PropsTab::World => "🌐",
            PropsTab::Object => "▣",
            PropsTab::Modifiers => "🔧",
            PropsTab::Data => "△",
            PropsTab::Material => "●",
        }
    }

    /// Tabs that only make sense for the active object's type.
    pub fn applies(self, data: Option<&ObjectData>) -> bool {
        match self {
            PropsTab::Render | PropsTab::Output | PropsTab::World => true,
            PropsTab::Object => data.is_some(),
            PropsTab::Modifiers | PropsTab::Material => matches!(data, Some(ObjectData::Mesh { .. })),
            PropsTab::Data => matches!(data, Some(ObjectData::Mesh { .. } | ObjectData::Light(_) | ObjectData::Camera(_))),
        }
    }
}

/// Workspace tabs in the top bar. Only tabs that change something are offered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Workspace {
    Layout,
    Modeling,
    Rendering,
}

impl Workspace {
    pub const ALL: [Workspace; 3] = [Workspace::Layout, Workspace::Modeling, Workspace::Rendering];
    pub fn name(self) -> &'static str {
        match self {
            Workspace::Layout => "Layout",
            Workspace::Modeling => "Modeling",
            Workspace::Rendering => "Rendering",
        }
    }
}

/// Region toggles the user controls with keys (T, N, Ctrl+Space) and the Window menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Regions {
    pub toolbar: bool,
    pub sidebar: bool,
    pub right_column: bool,
    pub agent: bool,
    /// Ctrl+Space: the viewport alone.
    pub maximized: bool,
    pub props_tab: PropsTab,
    pub workspace: Workspace,
}

impl Default for Regions {
    fn default() -> Self {
        Self {
            toolbar: true,
            sidebar: false,
            right_column: true,
            agent: true,
            maximized: false,
            props_tab: PropsTab::Object,
            workspace: Workspace::Layout,
        }
    }
}

impl ShapeApp {
    pub(crate) fn top_bar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("topbar").exact_size(TOP_BAR_H).frame(theme::region_frame(theme::TOP_BAR)).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                self.file_menu(ui);
                self.edit_menu(ui);
                self.render_menu(ui);
                self.window_menu(ui);
                self.help_menu(ui);
                ui.add_space(12.0);
                for w in Workspace::ALL {
                    let active = self.regions.workspace == w;
                    let text = if active { RichText::new(w.name()).color(Color32::WHITE) } else { RichText::new(w.name()).color(theme::TEXT_WEAK) };
                    if ui.selectable_label(active, text).clicked() && !active {
                        self.set_workspace(w);
                    }
                }
            });
        });
    }

    /// Switch workspace: Modeling enters edit mode on the active mesh, Layout leaves it, Rendering
    /// opens the Render window.
    pub fn set_workspace(&mut self, w: Workspace) {
        self.regions.workspace = w;
        match w {
            Workspace::Layout => {
                if self.scene.edit.is_some() {
                    self.toggle_edit_mode();
                }
            }
            Workspace::Modeling => {
                if self.scene.edit.is_none() {
                    self.toggle_edit_mode();
                }
            }
            Workspace::Rendering => self.render.open = true,
        }
    }

    pub(crate) fn window_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Window", |ui| {
            ui.checkbox(&mut self.regions.toolbar, "Toolbar (T)");
            ui.checkbox(&mut self.regions.sidebar, "Sidebar (N)");
            ui.checkbox(&mut self.regions.right_column, "Outliner and Properties");
            ui.checkbox(&mut self.regions.agent, "Agent panel");
            ui.separator();
            ui.checkbox(&mut self.regions.maximized, "Maximize Viewport (Ctrl+Space)");
        });
    }

    pub(crate) fn status_bar(&self, ui: &mut egui::Ui) {
        egui::Panel::bottom("status").exact_size(STATUS_H).frame(theme::region_frame(theme::STATUS_BAR)).show(ui, |ui| {
            ui.horizontal(|ui| {
                let hint = if self.modal.is_some() {
                    ""
                } else if self.scene.edit.is_some() {
                    "1/2/3 modes · B C box/circle · K knife · E I F M X · Ctrl+B bevel(wheel) · Ctrl+R · Ctrl+E/V/F menus"
                } else {
                    "G move · R rotate · S scale · X delete · Home frame"
                };
                ui.label(RichText::new(if self.status.is_empty() { "Ready" } else { &self.status }).size(12.0));
                ui.label(RichText::new(hint).size(12.0).color(theme::TEXT_WEAK));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("Redrob Shape {VERSION}")).size(12.0).color(theme::TEXT_WEAK));
                    ui.label(RichText::new(format!("{} objects", self.scene.object_count())).size(12.0).color(theme::TEXT_WEAK));
                });
            });
        });
    }

    /// Outliner over Properties, the right edge of Blender's default screen.
    pub(crate) fn right_column(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("right-column").exact_size(RIGHT_COLUMN_W).frame(egui::Frame::NONE.fill(theme::TOP_BAR)).show(ui, |ui| {
            egui::Panel::top("outliner").exact_size(OUTLINER_H).frame(theme::region_frame(theme::REGION)).show(ui, |ui| {
                self.outliner(ui);
            });
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(theme::PROPERTIES_BG).outer_margin(egui::Margin { top: 2, ..Default::default() }))
                .show(ui, |ui| {
                    egui::Panel::left("props-tabs").exact_size(PROPS_TABS_W).frame(egui::Frame::NONE.fill(theme::TABS_COLUMN).inner_margin(2)).show(
                        ui,
                        |ui| {
                            self.properties_tabs(ui);
                        },
                    );
                    egui::CentralPanel::default().frame(theme::region_frame(theme::PROPERTIES_BG)).show(ui, |ui| {
                        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.properties(ui));
                    });
                });
        });
    }

    fn properties_tabs(&mut self, ui: &mut egui::Ui) {
        let data = self.scene.selection.active.and_then(|id| self.scene.object(id)).map(|o| o.data.clone());
        ui.vertical(|ui| {
            ui.add_space(4.0);
            for tab in PropsTab::ALL {
                if !tab.applies(data.as_ref()) {
                    continue;
                }
                let active = self.regions.props_tab == tab;
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(PROPS_TABS_W - 4.0, 22.0), egui::Sense::click());
                if active {
                    ui.painter().rect_filled(rect, 3.0, theme::PROPERTIES_BG);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 3.0, theme::REGION);
                }
                let color = if active { Color32::WHITE } else { theme::TEXT_WEAK };
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, tab.glyph(), egui::FontId::proportional(13.0), color);
                let resp = resp.on_hover_text(tab.name());
                if resp.clicked() {
                    self.regions.props_tab = tab;
                }
            }
        });
        if !self.regions.props_tab.applies(data.as_ref()) {
            self.regions.props_tab = if data.is_some() { PropsTab::Object } else { PropsTab::Render };
        }
    }

    /// Header strip over the viewport: mode, menus, shading. Blender's 3D Viewport header.
    pub(crate) fn viewport_header(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("viewport-header").exact_size(HEADER_H).frame(theme::region_frame(theme::HEADER)).show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                let editing = self.scene.edit.is_some();
                let has_mesh = self.scene.selection.active.is_some_and(|id| self.scene.object_mesh(id).is_some());
                let mode = if editing { "Edit Mode" } else { "Object Mode" };
                egui::ComboBox::from_id_salt("mode").selected_text(mode).width(110.0).show_ui(ui, |ui| {
                    if ui.selectable_label(!editing, "Object Mode").clicked() && editing {
                        self.toggle_edit_mode();
                    }
                    if ui.add_enabled(has_mesh, egui::Button::selectable(editing, "Edit Mode")).clicked() && !editing {
                        self.toggle_edit_mode();
                    }
                });
                ui.separator();
                if editing {
                    // Blender's vertex / edge / face select-mode buttons (1 / 2 / 3).
                    let current = self.scene.edit.as_ref().map(|e| e.mode).unwrap_or_default();
                    for (mode, glyph, tip) in [
                        (shape_scene::SelectMode::Vertex, "·", "Vertex select (1)"),
                        (shape_scene::SelectMode::Edge, "╱", "Edge select (2)"),
                        (shape_scene::SelectMode::Face, "▰", "Face select (3)"),
                    ] {
                        if ui.selectable_label(current == mode, glyph).on_hover_text(tip).clicked() {
                            self.set_select_mode(mode);
                        }
                    }
                    ui.separator();
                }
                self.view_menu(ui);
                self.select_menu(ui);
                self.add_menu(ui);
                if editing {
                    self.mesh_menu(ui);
                } else {
                    self.object_menu(ui);
                }
                ui.separator();
                self.transform_header(ui);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    for (mode, glyph, tip) in [
                        (ShadingMode::SolidWire, "◫", "Solid + wireframe"),
                        (ShadingMode::Solid, "●", "Solid shading (Z: 2)"),
                        (ShadingMode::Wireframe, "◌", "Wireframe (Z: 1)"),
                    ] {
                        let on = self.viewport.options.shading == mode;
                        if ui.selectable_label(on, glyph).on_hover_text(tip).clicked() {
                            self.viewport.options.shading = mode;
                        }
                    }
                    ui.separator();
                    ui.toggle_value(&mut self.viewport.options.show_grid, "⊞").on_hover_text("Overlays: floor grid");
                    ui.separator();
                    ui.label(RichText::new(format!("{} objects", self.scene.object_count())).color(theme::TEXT_WEAK).size(12.0));
                });
            });
        });
    }

    pub(crate) fn select_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Select", |ui| {
            if ui.button("All (A)").clicked() {
                self.select_all();
            }
            if ui.button("None (Alt+A)").clicked() {
                self.select_none();
            }
            if ui.button("Invert (Ctrl+I)").clicked() {
                self.select_invert();
            }
        });
    }

    pub(crate) fn object_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Object", |ui| {
            let any = !self.scene.selection.is_empty();
            if ui.add_enabled(any, egui::Button::new("Duplicate Objects (Shift+D)")).clicked() {
                self.duplicate_selected();
            }
            if ui.add_enabled(any, egui::Button::new("Delete (X)")).clicked() {
                self.delete_selected();
            }
            ui.separator();
            if ui.add_enabled(any, egui::Button::new("Hide Selected (H)")).clicked() {
                self.hide_selected(true);
            }
            if ui.button("Reveal Hidden (Alt+H)").clicked() {
                self.reveal_all();
            }
            ui.separator();
            ui.menu_button("Set Origin", |ui| {
                for how in crate::SetOrigin::ALL {
                    if ui.add_enabled(any, egui::Button::new(how.name())).clicked() {
                        self.set_origin(how);
                    }
                }
            });
            ui.menu_button("Parent", |ui| {
                if ui.add_enabled(any, egui::Button::new("Object (Ctrl+P)")).clicked() {
                    self.parent_to_active();
                }
                if ui.add_enabled(any, egui::Button::new("Clear Parent (Alt+P)")).clicked() {
                    self.clear_parent();
                }
            });
            if ui.add_enabled(any, egui::Button::new("Move to Collection (M)")).clicked() {
                self.popup = Some(Popup::MoveToCollection);
            }
            ui.separator();
            if ui.add_enabled(any, egui::Button::new("Apply Modifiers")).clicked() {
                self.apply_modifiers();
            }
        });
    }

    pub(crate) fn mesh_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Mesh", |ui| {
            if ui.button("Extrude Faces (E)").clicked() {
                self.start_face_modal(crate::tools::ModalKind::Extrude);
            }
            if ui.button("Inset Faces (I)").clicked() {
                self.start_face_modal(crate::tools::ModalKind::Inset);
            }
            if ui.button("Bevel (Ctrl+B)").clicked() {
                self.start_face_modal(crate::tools::ModalKind::Bevel);
            }
            if ui.button("Loop Cut (Ctrl+R)").clicked() {
                self.loop_cut_at_pointer();
            }
            ui.separator();
            if ui.button("Subdivide (Catmull-Clark)").clicked() {
                self.apply_mesh_op("Subdivide", shape_ops::catmull_clark);
            }
            if ui.button("Subdivide (Linear)").clicked() {
                self.apply_mesh_op("Subdivide Linear", shape_ops::subdivide_linear);
            }
            if ui.button("Smooth Vertices").clicked() {
                self.apply_mesh_op("Smooth", |m| shape_ops::smooth(m, 2, 0.5));
            }
            if ui.button("Triangulate Faces").clicked() {
                self.apply_mesh_op("Triangulate", shape_ops::triangulate);
            }
            if ui.button("Flip Normals").clicked() {
                self.apply_mesh_op("Flip Normals", shape_ops::flip_normals);
            }
        });
    }

    /// Blender's T panel: one column of tool buttons at the left edge of the viewport.
    pub(crate) fn toolbar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("toolbar").exact_size(TOOLBAR_W).frame(egui::Frame::NONE.fill(theme::VIEWPORT_BG).inner_margin(4)).show(ui, |ui| {
            ui.vertical_centered(|ui| {
                egui::Frame::NONE.fill(theme::REGION).corner_radius(4).inner_margin(2).show(ui, |ui| {
                    for t in Tool::ALL {
                        let on = self.tool == t;
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::click());
                        if on {
                            ui.painter().rect_filled(rect, 3.0, theme::ACCENT);
                        } else if resp.hovered() {
                            ui.painter().rect_filled(rect, 3.0, theme::WIDGET_HOVER);
                        }
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, t.glyph(), egui::FontId::proportional(18.0), Color32::WHITE);
                        if resp.on_hover_text(format!("{} ({})", t.name(), t.key())).clicked() {
                            self.tool = t;
                        }
                    }
                });
            });
        });
    }

    /// Blender's N panel: the Item tab with the active object's transform, and View.
    pub(crate) fn sidebar(&mut self, ui: &mut egui::Ui) {
        egui::Panel::right("sidebar").exact_size(SIDEBAR_W).frame(theme::region_frame(theme::PROPERTIES_BG)).show(ui, |ui| {
            ui.label(RichText::new("Item").strong());
            match self.scene.selection.active.and_then(|id| self.scene.object(id).cloned()) {
                Some(object) => {
                    let id = self.scene.selection.active.unwrap();
                    let mut edited = object.clone();
                    ui.label(RichText::new("Transform").color(theme::TEXT_WEAK));
                    crate::panels::vec3_row(ui, "Location", &mut edited.transform.translation, 0.01);
                    let (rx, ry, rz) = edited.transform.rotation.to_euler(shape_math::glam_euler());
                    let mut deg = Vec3::new(rx.to_degrees(), ry.to_degrees(), rz.to_degrees());
                    if crate::panels::vec3_row(ui, "Rotation", &mut deg, 0.5) {
                        edited.transform.rotation =
                            shape_math::Quat::from_euler(shape_math::glam_euler(), deg.x.to_radians(), deg.y.to_radians(), deg.z.to_radians());
                    }
                    crate::panels::vec3_row(ui, "Scale", &mut edited.transform.scale, 0.01);
                    if edited != object {
                        self.edit_object_live(id, edited);
                    }
                }
                None => {
                    ui.label(RichText::new("No active object").color(theme::TEXT_WEAK));
                }
            }
            ui.separator();
            ui.label(RichText::new("View").strong());
            let mut fov = self.viewport.camera.fov_y.to_degrees();
            if ui.add(egui::DragValue::new(&mut fov).range(10.0..=120.0).prefix("Focal angle ° ")).changed() {
                self.viewport.camera.fov_y = fov.to_radians();
            }
            ui.add(egui::DragValue::new(&mut self.viewport.camera.distance).range(0.1..=1000.0).speed(0.05).prefix("Distance "));
        });
    }
}
