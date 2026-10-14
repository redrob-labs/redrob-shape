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
        if p == Primitive::Cube {
            // Show the modifier stack working: a rounded cube.
            app.add_modifier(shape_scene::Modifier::Subdivision { levels: 2, smooth: true });
        }
    }
    // A red metal sphere, a sun and a lamp show materials and scene lighting.
    let red = app.scene.add_material(shape_scene::Material {
        name: "Red".into(),
        base_color: [0.8, 0.08, 0.06, 1.0],
        metallic: 0.3,
        roughness: 0.35,
        ..Default::default()
    });
    let sphere = app.scene.objects().find(|(_, o)| o.name.starts_with("UV Sphere")).map(|(id, _)| id);
    if let Some(id) = sphere
        && let Some(shape_scene::ObjectData::Mesh { material, .. }) = app.scene.object_mut(id).map(|o| &mut o.data)
    {
        *material = Some(red);
    }
    app.add_object("Sun", shape_scene::ObjectData::Light(shape_scene::Light::sun(3.0)), Vec3::new(4.0, 8.0, 3.0));
    app.add_object("Lamp", shape_scene::ObjectData::Light(shape_scene::Light::point(600.0)), Vec3::new(-3.0, 3.5, 2.5));
    app.add_object("Camera", shape_scene::ObjectData::Camera(Default::default()), Vec3::new(7.0, 5.0, 8.0));
    app.scene.selection.clear();
    app.viewport.frame_scene(&app.scene);
}
