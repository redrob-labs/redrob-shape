//! The Redrob Code app bridge.
//!
//! `redrob-app-bridge` serves this window's tools to the shared Redrob Code daemon over MCP on
//! loopback, so the agent in any Redrob surface (a phone chat included) can drive the scene the
//! user is looking at. This is separate from `acp.rs`, which is the other direction: the window
//! starting an agent and talking to it over ACP.
//!
//! The tool handlers run on the bridge's own threads while the scene lives on the UI thread, so
//! every call is a request/reply round trip: the handler pushes a [`BridgeRequest`] on a channel,
//! wakes egui and blocks on the reply; [`Bridged::ui`] drains the channel once per frame, before
//! drawing, and runs each request against the real [`ShapeApp`] with the same functions the menus
//! and keys use. A render is the one long operation: it runs on the render panel's own worker
//! thread and the handler is answered when the panel reports the job done.
//!
//! The daemon is optional: when it cannot be reached the failure is logged and the app runs
//! without the bridge.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use redrob_app_bridge::{AppBridge, AppBridgeConfig, Tool};
use serde_json::{Value, json};
use shape_math::{Quat, Vec3};
use shape_scene::{ObjectData, ObjectId};
use shape_ui_egui::ShapeApp;

/// The name the daemon lists the tools under (`app:shape`).
const APP_NAME: &str = "shape";
/// How long a tool call waits for the UI thread before giving up.
const REPLY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long `render` waits for the render worker.
const RENDER_TIMEOUT: Duration = Duration::from_secs(600);

type Reply = Result<String, String>;

/// One tool call waiting for the UI thread.
struct BridgeRequest {
    tool: String,
    args: Value,
    reply: Sender<Reply>,
}

/// A `render` call whose job is running: answered when the render panel reports it done.
struct PendingRender {
    path: PathBuf,
    reply: Sender<Reply>,
}

/// The editor window plus the bridge that lets the daemon's agent drive it.
pub struct Bridged {
    pub app: ShapeApp,
    requests: Receiver<BridgeRequest>,
    bridge: Arc<Mutex<Option<AppBridge>>>,
    pending_render: Option<PendingRender>,
}

impl Bridged {
    /// Wrap the app and start the bridge on its own thread, so a slow or absent daemon never
    /// delays the first frame.
    pub fn start(app: ShapeApp, ctx: egui::Context) -> Self {
        let (tx, requests) = channel::<BridgeRequest>();
        let bridge: Arc<Mutex<Option<AppBridge>>> = Arc::new(Mutex::new(None));
        let slot = bridge.clone();
        let spawned = std::thread::Builder::new().name("redrob-shape-app-bridge".into()).spawn(move || {
            let config = AppBridgeConfig { version: Some(env!("CARGO_PKG_VERSION").into()), ..AppBridgeConfig::new(APP_NAME, tools(tx, ctx)) };
            match AppBridge::start(config) {
                Ok(b) => {
                    log::info!("app bridge: bridged as {} at {}", b.id(), b.mcp_url());
                    if let Ok(mut slot) = slot.lock() {
                        *slot = Some(b);
                    }
                }
                Err(e) => log::warn!("app bridge: not bridged: {e}"),
            }
        });
        if let Err(e) = spawned {
            log::warn!("app bridge: could not start the bridge thread: {e}");
        }
        Self { app, requests, bridge, pending_render: None }
    }

    /// Unregister from the daemon and stop the MCP server.
    fn stop_bridge(&mut self) {
        let bridge = self.bridge.lock().ok().and_then(|mut slot| slot.take());
        if let Some(bridge) = bridge {
            bridge.stop();
        }
    }

    /// Run every queued tool call on the scene. Called once per frame before drawing.
    fn drain(&mut self, ctx: &egui::Context) {
        let pending: Vec<BridgeRequest> = self.requests.try_iter().collect();
        for req in pending {
            if req.tool == "render" {
                match self.start_render(&req.args, ctx) {
                    Ok(path) => self.pending_render = Some(PendingRender { path, reply: req.reply }),
                    Err(e) => {
                        let _ = req.reply.send(Err(e));
                    }
                }
                continue;
            }
            let reply = self.run(&req.tool, &req.args, ctx);
            let _ = req.reply.send(reply);
        }
    }

    /// Answer a `render` call once its job has finished; the panel pulls the frame while drawing.
    fn finish_render(&mut self) {
        if self.pending_render.is_none() || self.app.render.is_running() {
            return;
        }
        let Some(PendingRender { path, reply }) = self.pending_render.take() else { return };
        let result = self.app.render.save_png(&path).map(|()| {
            let (w, h) = (self.app.render.last.as_ref().map_or(0, |f| f.width), self.app.render.last.as_ref().map_or(0, |f| f.height));
            format!("rendered {w}x{h} to {}", path.display())
        });
        let _ = reply.send(result);
    }

    fn start_render(&mut self, args: &Value, ctx: &egui::Context) -> Result<PathBuf, String> {
        if self.pending_render.is_some() {
            return Err("a render started from the bridge is still running".to_string());
        }
        if self.app.render.animation_progress().is_some() {
            return Err("an animation render is running; wait for it to finish".to_string());
        }
        let path = match args.get("path").and_then(Value::as_str) {
            Some(p) if !p.trim().is_empty() => PathBuf::from(p),
            _ => {
                let stem = self.app.file().and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "render".into());
                let dir = self.app.file().and_then(|f| f.parent()).map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
                dir.join(format!("{stem}.png"))
            }
        };
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| format!("could not create {}: {e}", parent.display()))?;
        }
        if let Some(w) = args.get("width").and_then(Value::as_u64) {
            self.app.render.width = (w as u32).clamp(1, 16384);
        }
        if let Some(h) = args.get("height").and_then(Value::as_u64) {
            self.app.render.height = (h as u32).clamp(1, 16384);
        }
        self.app.start_render(ctx);
        Ok(path)
    }

    /// One tool call against the scene, on the UI thread.
    fn run(&mut self, tool: &str, args: &Value, _ctx: &egui::Context) -> Reply {
        match tool {
            "scene_info" => Ok(self.scene_info()),
            "open_scene" => {
                let path = PathBuf::from(args.get("path").and_then(Value::as_str).ok_or("path is required")?);
                if !path.is_file() {
                    return Err(format!("{} is not a file", path.display()));
                }
                self.app.open(path.clone());
                if self.app.file() == Some(path.as_path()) {
                    Ok(format!("opened {} ({} objects)", path.display(), self.app.scene.object_count()))
                } else {
                    Err(self.app.status().to_string())
                }
            }
            "list_objects" => {
                let max = args.get("max").and_then(Value::as_u64).map_or(200, |m| m.max(1) as usize);
                let total = self.app.scene.object_count();
                let mut lines: Vec<String> = self
                    .app
                    .scene
                    .objects()
                    .take(max)
                    .map(|(id, o)| {
                        let parent = o.parent.and_then(|p| self.app.scene.object(p)).map_or_else(|| "-".to_string(), |p| p.name.clone());
                        format!("{}\t{}\t{}\tparent={}\t{}", id.0, o.name, kind(&o.data), parent, if o.visible { "visible" } else { "hidden" })
                    })
                    .collect();
                if total > max {
                    lines.push(format!("... {} more (raise max)", total - max));
                }
                if lines.is_empty() {
                    lines.push("no objects".into());
                }
                Ok(lines.join("\n"))
            }
            "select" => {
                let ids = args.get("ids").and_then(Value::as_array).cloned().ok_or("ids is required (an array of ids or names)")?;
                let resolved: Result<Vec<ObjectId>, String> = ids.iter().map(|v| self.find(v)).collect();
                let resolved = resolved?;
                self.app.edit("Bridge: Select", |scene| {
                    scene.selection.clear();
                    for id in &resolved {
                        scene.selection.objects.insert(*id);
                        scene.selection.active = Some(*id);
                    }
                });
                Ok(format!("selected {}", self.app.scene.selection.objects.len()))
            }
            "transform" => {
                let id = self.find(args.get("id").ok_or("id is required")?)?;
                let translate = vec3_arg(args, "translate")?;
                let rotate = vec3_arg(args, "rotate_deg")?;
                let scale = vec3_arg(args, "scale")?;
                if translate.is_none() && rotate.is_none() && scale.is_none() {
                    return Err("give at least one of translate, rotate_deg, scale".to_string());
                }
                self.app.edit("Bridge: Transform", move |scene| {
                    if let Some(o) = scene.object_mut(id) {
                        if let Some(t) = translate {
                            o.transform.translation += t;
                        }
                        if let Some(r) = rotate {
                            let delta = Quat::from_euler(shape_math::glam_euler(), r.x.to_radians(), r.y.to_radians(), r.z.to_radians());
                            o.transform.rotation = (delta * o.transform.rotation).normalize();
                        }
                        if let Some(s) = scale {
                            o.transform.scale *= s;
                        }
                    }
                });
                let o = self.app.scene.object(id).ok_or("object vanished")?;
                let (rx, ry, rz) = o.transform.rotation.to_euler(shape_math::glam_euler());
                Ok(format!(
                    "{} location={} rotation_deg={} scale={}",
                    o.name,
                    fmt3(o.transform.translation),
                    fmt3(Vec3::new(rx.to_degrees(), ry.to_degrees(), rz.to_degrees())),
                    fmt3(o.transform.scale)
                ))
            }
            "set_visible" => {
                let id = self.find(args.get("id").ok_or("id is required")?)?;
                let visible = args.get("visible").and_then(Value::as_bool).ok_or("visible is required (true or false)")?;
                self.app.edit(if visible { "Bridge: Show" } else { "Bridge: Hide" }, move |scene| {
                    if let Some(o) = scene.object_mut(id) {
                        o.visible = visible;
                    }
                    if !visible {
                        scene.selection.objects.remove(&id);
                        if scene.selection.active == Some(id) {
                            scene.selection.active = scene.selection.objects.iter().next().copied();
                        }
                    }
                });
                let name = self.app.scene.object(id).map_or_else(|| id.0.to_string(), |o| o.name.clone());
                Ok(format!("{name} is now {}", if visible { "visible" } else { "hidden" }))
            }
            "save" => {
                let path = self.app.file().map(PathBuf::from).ok_or("the scene is untitled; save it once from File > Save As to give it a path")?;
                self.app.save_as(path.clone());
                let status = self.app.status().to_string();
                if status.starts_with("Saved") { Ok(status) } else { Err(status) }
            }
            other => Err(format!("unknown tool {other}")),
        }
    }

    fn scene_info(&self) -> String {
        let scene = &self.app.scene;
        let path = self.app.file().map_or_else(|| "untitled".to_string(), |p| p.display().to_string());
        let selected: Vec<String> = scene
            .selection
            .objects
            .iter()
            .map(|id| scene.object(*id).map_or_else(|| id.0.to_string(), |o| format!("{} ({})", o.name, id.0)))
            .collect();
        let active = scene.selection.active.and_then(|id| scene.object(id)).map_or_else(|| "-".to_string(), |o| o.name.clone());
        let cam = &self.app.viewport.camera;
        let scene_camera = scene
            .objects()
            .find(|(_, o)| matches!(o.data, ObjectData::Camera(_)))
            .map_or_else(|| "none".to_string(), |(id, o)| format!("{} ({}) at {}", o.name, id.0, fmt3(o.transform.translation)));
        format!(
            "path: {path}\nscene: {}\nobjects: {}\nselection: {}\nactive: {active}\nviewport camera: position={} target={} fov_y_deg={:.1}\nscene camera: {scene_camera}\nrender: {}x{} {} samples",
            scene.name,
            scene.object_count(),
            if selected.is_empty() { "none".to_string() } else { selected.join(", ") },
            fmt3(cam.position()),
            fmt3(cam.target),
            cam.fov_y.to_degrees(),
            self.app.render.width,
            self.app.render.height,
            self.app.render.target_samples
        )
    }

    /// An object by numeric id or by name (the same lookup the agent's tools use).
    fn find(&self, key: &Value) -> Result<ObjectId, String> {
        shape_agent::tools::find(&self.app.scene, Some(key))
    }
}

impl eframe::App for Bridged {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.drain(ui.ctx());
        eframe::App::ui(&mut self.app, ui, frame);
        self.finish_render();
    }

    fn on_exit(&mut self) {
        self.stop_bridge();
    }
}

fn kind(data: &ObjectData) -> &'static str {
    match data {
        ObjectData::Mesh { .. } => "mesh",
        ObjectData::Light(_) => "light",
        ObjectData::Camera(_) => "camera",
        ObjectData::Empty => "empty",
        ObjectData::Armature(_) => "armature",
        ObjectData::Curve(_) => "curve",
        ObjectData::Text { .. } => "text",
        ObjectData::Metaball { .. } => "metaball",
    }
}

fn fmt3(v: Vec3) -> String {
    format!("[{:.3}, {:.3}, {:.3}]", v.x, v.y, v.z)
}

/// An optional `[x, y, z]` argument; an error when present but malformed.
fn vec3_arg(args: &Value, key: &str) -> Result<Option<Vec3>, String> {
    let Some(v) = args.get(key) else { return Ok(None) };
    if v.is_null() {
        return Ok(None);
    }
    let arr = v.as_array().ok_or_else(|| format!("{key} must be an array of 3 numbers"))?;
    if arr.len() != 3 {
        return Err(format!("{key} must be an array of 3 numbers"));
    }
    let n = |i: usize| arr[i].as_f64().map(|f| f as f32).ok_or_else(|| format!("{key}[{i}] must be a number"));
    Ok(Some(Vec3::new(n(0)?, n(1)?, n(2)?)))
}

/// Push one call on the UI thread's queue and block until it answers or times out.
fn call(tx: &Sender<BridgeRequest>, ctx: &egui::Context, tool: &str, args: Value, timeout: Duration) -> Reply {
    let (reply, rx) = channel();
    tx.send(BridgeRequest { tool: tool.to_string(), args, reply }).map_err(|_| "the Redrob Shape window is gone".to_string())?;
    ctx.request_repaint();
    match rx.recv_timeout(timeout) {
        Ok(reply) => reply,
        Err(_) => Err(format!("{tool}: the editor did not answer within {} s", timeout.as_secs())),
    }
}

fn tool(tx: &Sender<BridgeRequest>, ctx: &egui::Context, name: &'static str, description: &str, schema: Value, timeout: Duration) -> Tool {
    let (tx, ctx) = (tx.clone(), ctx.clone());
    Tool::new(name, description, schema, move |args| call(&tx, &ctx, name, args, timeout))
}

fn tools(tx: Sender<BridgeRequest>, ctx: egui::Context) -> Vec<Tool> {
    let id_schema =
        json!({ "description": "Object id from list_objects, or the object's name", "anyOf": [{ "type": "integer" }, { "type": "string" }] });
    let vec3 = |what: &str| json!({ "type": "array", "items": { "type": "number" }, "minItems": 3, "maxItems": 3, "description": what });
    vec![
        tool(
            &tx,
            &ctx,
            "scene_info",
            "Describe the open scene: file path (or untitled), object count, selection, viewport and scene camera, render size.",
            json!({ "type": "object", "properties": {} }),
            REPLY_TIMEOUT,
        ),
        tool(
            &tx,
            &ctx,
            "open_scene",
            "Open a scene file (.rshape, .obj, .gltf, .glb, .stl, .ply, ...) in the window, replacing the current scene.",
            json!({ "type": "object", "properties": { "path": { "type": "string", "description": "Absolute path of the file" } }, "required": ["path"] }),
            REPLY_TIMEOUT,
        ),
        tool(
            &tx,
            &ctx,
            "list_objects",
            "List the scene's objects, one per line: id, name, type, parent, visible or hidden.",
            json!({ "type": "object", "properties": { "max": { "type": "integer", "minimum": 1, "description": "Maximum number of lines (default 200)" } } }),
            REPLY_TIMEOUT,
        ),
        tool(
            &tx,
            &ctx,
            "select",
            "Select the objects with the given ids or names, replacing the selection; answers how many are selected.",
            json!({ "type": "object", "properties": { "ids": { "type": "array", "items": id_schema.clone() } }, "required": ["ids"] }),
            REPLY_TIMEOUT,
        ),
        tool(
            &tx,
            &ctx,
            "transform",
            "Move, rotate and/or scale one object relative to where it is: translate adds to its location, rotate_deg applies XYZ Euler degrees, scale multiplies; answers its new transform.",
            json!({
                "type": "object",
                "properties": {
                    "id": id_schema.clone(),
                    "translate": vec3("Offset to add to the location, in scene units"),
                    "rotate_deg": vec3("Rotation to apply, XYZ Euler angles in degrees"),
                    "scale": vec3("Factors to multiply the scale by")
                },
                "required": ["id"]
            }),
            REPLY_TIMEOUT,
        ),
        tool(
            &tx,
            &ctx,
            "set_visible",
            "Show or hide one object (hiding also deselects it).",
            json!({ "type": "object", "properties": { "id": id_schema, "visible": { "type": "boolean" } }, "required": ["id", "visible"] }),
            REPLY_TIMEOUT,
        ),
        tool(
            &tx,
            &ctx,
            "render",
            "Render the scene with the render panel's settings (scene camera, else the viewport view) and write a PNG; answers the written path. Takes seconds to minutes.",
            json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "description": "Absolute output .png path; defaults to <scene name>.png next to the scene file, or the temp dir" },
                    "width": { "type": "integer", "minimum": 1, "description": "Output width in pixels (default: the render panel's)" },
                    "height": { "type": "integer", "minimum": 1, "description": "Output height in pixels (default: the render panel's)" }
                }
            }),
            RENDER_TIMEOUT,
        ),
        tool(
            &tx,
            &ctx,
            "save",
            "Save the scene to the file it was opened from or last saved to; fails when the scene is untitled.",
            json!({ "type": "object", "properties": {} }),
            REPLY_TIMEOUT,
        ),
    ]
}
