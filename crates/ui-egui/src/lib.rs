//! Editor panels, hotkeys and the egui shell.
//!
//! Stage 0 ships the frame only: a menu bar, an outliner on the left, properties on the right, an
//! empty viewport in the middle and a status bar. Later stages fill each region from its own crate.

use egui::{Color32, RichText};

/// Crate name, used by the layering test in `apps/redrob-shape`.
pub const CRATE: &str = "shape-ui-egui";

/// Version shown in the status bar; comes from the workspace `Cargo.toml`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The whole editor window.
#[derive(Default)]
pub struct ShapeApp {
    /// Text in the status bar.
    status: String,
}

impl ShapeApp {
    /// Title of the main window.
    pub const WINDOW_TITLE: &str = "Redrob Shape";

    /// Build the app. `cc` is the eframe creation context; unused until styles arrive.
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self::default()
    }

    /// Draw one frame. Split out of `eframe::App::update` so `egui_kittest` can drive it.
    pub fn ui(&mut self, ui: &mut egui::Ui) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Edit", |_ui| {});
                ui.menu_button("Add", |_ui| {});
                ui.menu_button("Render", |_ui| {});
                ui.menu_button("Help", |_ui| {});
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(if self.status.is_empty() { "Ready" } else { &self.status });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("Redrob Shape {VERSION}")).weak());
                });
            });
        });
        egui::Panel::left("outliner").default_size(220.0).show(ui, |ui| {
            ui.heading("Outliner");
            ui.separator();
            ui.label(RichText::new("Scene is empty").weak());
        });
        egui::Panel::right("properties").default_size(260.0).show(ui, |ui| {
            ui.heading("Properties");
            ui.separator();
            ui.label(RichText::new("Nothing selected").weak());
        });
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(Color32::from_rgb(0x29, 0x2e, 0x37))).show(ui, |ui| {
            let rect = ui.max_rect();
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Viewport",
                egui::FontId::proportional(18.0),
                Color32::from_rgb(0x7c, 0x83, 0x90),
            );
        });
    }
}

impl eframe::App for ShapeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.ui(ui);
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
        for label in ["Outliner", "Properties", "Ready", "File"] {
            harness.get_by_label(label);
        }
    }
}
