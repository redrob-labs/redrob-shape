//! Redrob Shape desktop entry point.
//!
//! Stage 0: opens the editor shell with an empty viewport. Everything the window shows comes from
//! `shape-ui-egui`, so the binary stays a thin launcher.

use shape_ui_egui::ShapeApp;

fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(ShapeApp::WINDOW_TITLE)
            .with_app_id("ai.redrob.shape")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        ..Default::default()
    };
    eframe::run_native(ShapeApp::WINDOW_TITLE, options, Box::new(|cc| Ok(Box::new(ShapeApp::new(cc)))))
}
