//! Redrob Shape desktop entry point.
//!
//! `redrob-shape [--demo] [--no-grid] [--shading wire|solid|material] [FILE]` opens the editor; `FILE` is any format `shape-format`
//! reads, `--demo` fills the scene with the primitives and `--no-grid` hides the floor grid.
//! `redrob-shape mcp --connect ADDR` runs the MCP server the agent uses to reach a window.

// A GUI app: no console window behind the editor when a person double-clicks it on Windows.
// `mcp --connect ADDR` talks over a socket, so it keeps working without a console.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod acp;
mod app_bridge;
mod work_area;

use shape_ui_egui::{Primitive, ShapeApp};

fn main() -> eframe::Result {
    env_logger::init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("mcp") {
        let addr = args.iter().position(|a| a == "--connect").and_then(|i| args.get(i + 1)).cloned();
        let Some(addr) = addr else {
            eprintln!("usage: redrob-shape mcp --connect 127.0.0.1:PORT");
            std::process::exit(2);
        };
        if let Err(e) = shape_agent::serve_stdio(&addr) {
            eprintln!("redrob-shape mcp: {e}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let demo = args.iter().any(|a| a == "--demo");
    let no_grid = args.iter().any(|a| a == "--no-grid");
    // `--shading wire|solid|material`: the viewport shading to start in.
    let shading = args.iter().position(|a| a == "--shading").and_then(|i| args.get(i + 1)).map(|s| match s.as_str() {
        "wire" | "wireframe" => shape_ui_egui::ShadingMode::Wireframe,
        "material" | "preview" => shape_ui_egui::ShadingMode::MaterialPreview,
        _ => shape_ui_egui::ShadingMode::Solid,
    });
    let mut skip_next = false;
    let file = args
        .iter()
        .find(|a| {
            if skip_next {
                skip_next = false;
                return false;
            }
            if *a == "--shading" {
                skip_next = true;
                return false;
            }
            !a.starts_with("--")
        })
        .map(std::path::PathBuf::from);

    let mut options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(ShapeApp::WINDOW_TITLE)
            .with_app_id("ai.redrob.shape")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 400.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    // Centre only a window with no saved position; one that doesn't fit its monitor is maximized
    // into the work area on the first frame (`work_area::fit_window`).
    options.centered = !has_saved_position(options.persistence_path.as_deref());
    eframe::run_native(
        ShapeApp::WINDOW_TITLE,
        options,
        Box::new(move |cc| {
            let (factory, control) = agent_services(cc.egui_ctx.clone());
            let mut app = ShapeApp::new(cc).with_agent(factory, control);
            if let Some(path) = file {
                app.open(path);
            }
            if demo {
                demo_scene(&mut app);
            }
            app.viewport.options.show_grid = !no_grid;
            if let Some(mode) = shading {
                app.viewport.options.shading = mode;
            }
            // The Redrob Code daemon's agent reaches this window's tools through the app bridge.
            Ok(Box::new(app_bridge::Bridged::start(app, cc.egui_ctx.clone())))
        }),
    )
}

/// The control channel for this window and the factory that starts `redrob acp --attach` against
/// it. Without a control port the agent panel reports why.
fn agent_services(ctx: egui::Context) -> (Option<shape_ui_egui::AgentFactory>, Option<shape_ui_egui::ControlServer>) {
    let wake_ctx = ctx.clone();
    let control = match shape_ui_egui::ControlServer::start(move || wake_ctx.request_repaint()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("redrob-shape: control server failed to bind: {e}");
            return (None, None);
        }
    };
    let link = acp::McpLink { addr: control.addr(), token: Some(control.token.clone()) };
    let factory: shape_ui_egui::AgentFactory = Box::new(move || {
        let bin = acp::find_binary().ok_or_else(|| acp::INSTALL_HINT.to_string())?;
        // The agent starts in the user's home, never the app's folder: a child holding the install
        // folder as its working directory blocks the Windows setup from replacing the app.
        let cwd = home_dir().or_else(|| std::env::current_dir().ok()).map(|d| d.to_string_lossy().into_owned()).unwrap_or_else(|| ".".into());
        let host = acp::AcpHost::start(&bin, &cwd, link.clone(), &[], ctx.clone())?;
        Ok(Box::new(host) as Box<dyn shape_ui_egui::agent::AgentHost>)
    });
    (Some(factory), Some(control))
}

/// The user's home folder (`%USERPROFILE%` on Windows, `$HOME` elsewhere).
fn home_dir() -> Option<std::path::PathBuf> {
    let var = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(var).filter(|v| !v.is_empty()).map(std::path::PathBuf::from).filter(|p| p.is_dir())
}

/// Whether eframe's saved state (`app.ron` in its persistence folder) holds a window position
/// from an earlier session. The window entry is a RON string inside the file, so a text match is
/// enough; eframe restores the inner position on macOS and the outer one elsewhere. Shape builds
/// eframe without persistence today, so this is false and the window is always centred.
fn has_saved_position(dir: Option<&std::path::Path>) -> bool {
    let Some(text) = dir.and_then(|d| std::fs::read_to_string(d.join("app.ron")).ok()) else {
        return false;
    };
    let key = if cfg!(target_os = "macos") { "inner_position_pixels:Some(" } else { "outer_position_pixels:Some(" };
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    compact.contains(key)
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
    // A checkerboard image texture on the plane shows UV-mapped textures (Material Preview).
    let checker: Vec<[f32; 4]> =
        (0..64 * 64).map(|i| if ((i % 64) / 8 + (i / 64) / 8) % 2 == 0 { [0.9, 0.85, 0.75, 1.0] } else { [0.75, 0.2, 0.15, 1.0] }).collect();
    let tex = app.scene.add_texture(shape_scene::Texture::from_rgba("checker", 64, 64, checker));
    let checker_mat =
        app.scene.add_material(shape_scene::Material { name: "Checker".into(), base_color_texture: Some(tex), roughness: 0.8, ..Default::default() });
    let plane = app.scene.objects().find(|(_, o)| o.name.starts_with("Plane")).map(|(id, _)| id);
    if let Some(id) = plane {
        if let Some((mesh_id, mesh)) = app.scene.object_mesh(id) {
            let uvs = shape_ui_egui::shape_ops::uv_planar(mesh, 1);
            if let Some(d) = app.scene.mesh_mut(mesh_id) {
                d.uvs = Some(uvs);
            }
        }
        if let Some(shape_scene::ObjectData::Mesh { material, .. }) = app.scene.object_mut(id).map(|o| &mut o.data) {
            *material = Some(checker_mat);
        }
    }
    app.add_object("Sun", shape_scene::ObjectData::Light(shape_scene::Light::sun(3.0)), Vec3::new(4.0, 8.0, 3.0));
    app.add_object("Lamp", shape_scene::ObjectData::Light(shape_scene::Light::point(600.0)), Vec3::new(-3.0, 3.5, 2.5));
    app.add_object("Camera", shape_scene::ObjectData::Camera(Default::default()), Vec3::new(7.0, 5.0, 8.0));
    app.scene.selection.clear();
    app.viewport.frame_scene(&app.scene);
}
