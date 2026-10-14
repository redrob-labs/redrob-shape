//! Redrob Shape desktop entry point.
//!
//! `redrob-shape [--demo] [--no-grid] [FILE]` opens the editor; `FILE` is any format `shape-format`
//! reads, `--demo` fills the scene with the primitives and `--no-grid` hides the floor grid.

use shape_ui_egui::{Primitive, ShapeApp};

fn main() -> eframe::Result {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let demo = args.iter().any(|a| a == "--demo");
    let no_grid = args.iter().any(|a| a == "--no-grid");
    let file = args.iter().find(|a| !a.starts_with("--")).map(std::path::PathBuf::from);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(ShapeApp::WINDOW_TITLE)
            .with_app_id("ai.redrob.shape")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        ShapeApp::WINDOW_TITLE,
        options,
        Box::new(move |cc| {
            let mut app = ShapeApp::new(cc);
            if let Some(path) = file {
                app.open(path);
            }
            if demo {
                demo_scene(&mut app);
            }
            app.viewport.options.show_grid = !no_grid;
            Ok(Box::new(app))
        }),
    )
}

fn demo_scene(app: &mut ShapeApp) {
    use shape_math::{Transform, Vec3};
    let placements = [
        (Primitive::Cube, Vec3::new(-3.0, 1.0, 0.0)),
        (Primitive::Sphere, Vec3::new(0.0, 1.0, 0.0)),
        (Primitive::Torus, Vec3::new(3.0, 1.0, 0.0)),
        (Primitive::Cylinder, Vec3::new(0.0, 1.0, -3.5)),
        (Primitive::Plane, Vec3::ZERO),
    ];
    for (p, at) in placements {
        let id = app.add_primitive(p);
        app.scene.object_mut(id).unwrap().transform = Transform::from_translation(at);
    }
    app.scene.selection.clear();
    app.viewport.frame_scene(&app.scene);
}
