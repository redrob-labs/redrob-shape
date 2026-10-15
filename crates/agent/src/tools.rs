//! The tools the agent can call, and how each one acts on a [`Scene`].
//!
//! Every tool is a plain function over the scene so it can be tested without a window or a
//! network. The app runs a tool through its undo history, so each call is one undo step.

use serde_json::{Value, json};
use shape_geom::primitives;
use shape_math::{Quat, Transform, Vec3};
use shape_scene::{Light, Material, Modifier, ObjectData, ObjectId, Scene};

/// MCP `tools/list` payload.
pub fn tool_definitions() -> Value {
    let object = json!({"type": "string", "description": "Object name (as in the outliner) or numeric id"});
    let vec3 = |d: &str| json!({"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 3, "description": d});
    json!({"tools": [
        {"name": "scene_summary", "description": "Everything in the scene: objects with ids, names, types, world location/rotation/scale, mesh counts, materials, modifiers, lights and cameras. Call this first.",
         "inputSchema": {"type": "object", "properties": {}}},
        {"name": "add_primitive", "description": "Add a mesh primitive: cube, plane, sphere, torus or cylinder. Y is up; one unit is one metre. Returns the new object's id and name.",
         "inputSchema": {"type": "object", "properties": {"kind": {"type": "string", "enum": ["cube", "plane", "sphere", "torus", "cylinder"]}, "name": {"type": "string"}, "location": vec3("x y z"), "size": {"type": "number", "description": "Overall size in metres (default 2 for a cube)"}}, "required": ["kind"]}},
        {"name": "transform_object", "description": "Set an object's location, rotation (degrees, XYZ Euler) and/or scale. Omitted parts are kept.",
         "inputSchema": {"type": "object", "properties": {"object": object, "location": vec3("x y z"), "rotation": vec3("degrees about X Y Z"), "scale": vec3("sx sy sz")}, "required": ["object"]}},
        {"name": "rename_object", "description": "Rename an object.", "inputSchema": {"type": "object", "properties": {"object": object, "name": {"type": "string"}}, "required": ["object", "name"]}},
        {"name": "delete_object", "description": "Delete an object.", "inputSchema": {"type": "object", "properties": {"object": object}, "required": ["object"]}},
        {"name": "set_material", "description": "Give a mesh object a material (created if it has none) and set its colour (linear RGBA 0..1), metallic, roughness and emissive.",
         "inputSchema": {"type": "object", "properties": {"object": object, "color": {"type": "array", "items": {"type": "number"}, "minItems": 3, "maxItems": 4}, "metallic": {"type": "number"}, "roughness": {"type": "number"}, "emissive": vec3("linear RGB"), "name": {"type": "string"}}, "required": ["object"]}},
        {"name": "mesh_op", "description": "Edit a mesh object's geometry destructively. op: subdivide (Catmull-Clark), subdivide_linear, smooth, extrude (all faces by amount), inset (all faces by amount), bevel (all edges by amount), shrink_fatten (amount), triangulate, flip_normals.",
         "inputSchema": {"type": "object", "properties": {"object": object, "op": {"type": "string", "enum": ["subdivide", "subdivide_linear", "smooth", "extrude", "inset", "bevel", "shrink_fatten", "triangulate", "flip_normals"]}, "amount": {"type": "number"}}, "required": ["object", "op"]}},
        {"name": "add_modifier", "description": "Add a non-destructive modifier to a mesh object. kind: mirror {axis: x|y|z}, array {count, offset:[x,y,z]}, subdivision {levels, smooth}, smooth {iterations, factor}, bevel {offset, segments}, solidify {thickness, offset}, decimate {ratio}, wireframe {thickness}, triangulate, screw {axis, angle(deg), steps, screw}, simple_deform {mode: twist|bend|taper|stretch, axis, factor(radians for twist/bend)}, displace {strength, scale}, weld {distance}, boolean {operation: union|difference|intersect, target: object}, shrinkwrap {target: object, offset}.",
         "inputSchema": {"type": "object", "properties": {"object": object, "kind": {"type": "string", "enum": ["mirror", "array", "subdivision", "smooth"]}, "axis": {"type": "string"}, "count": {"type": "integer"}, "offset": vec3("x y z"), "levels": {"type": "integer"}, "smooth": {"type": "boolean"}, "iterations": {"type": "integer"}, "factor": {"type": "number"}}, "required": ["object", "kind"]}},
        {"name": "clear_modifiers", "description": "Remove every modifier from a mesh object (apply=true bakes them into the mesh first).",
         "inputSchema": {"type": "object", "properties": {"object": object, "apply": {"type": "boolean"}}, "required": ["object"]}},
        {"name": "add_light", "description": "Add a light. kind: point (power in watts), sun (strength), spot (power). Lights placed away from the origin aim at it.",
         "inputSchema": {"type": "object", "properties": {"kind": {"type": "string", "enum": ["point", "sun", "spot"]}, "location": vec3("x y z"), "power": {"type": "number"}, "color": vec3("linear RGB"), "name": {"type": "string"}}, "required": ["kind"]}},
        {"name": "select", "description": "Select objects by name/id (replaces the selection; empty list clears it).",
         "inputSchema": {"type": "object", "properties": {"objects": {"type": "array", "items": object}}, "required": ["objects"]}},
        {"name": "import_file", "description": "Import a file into the scene: .rshape .obj .stl .ply .gltf .glb .blend. Objects are added to the current scene.",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}},
        {"name": "export_file", "description": "Write the scene to a file; the extension picks the format (.rshape .obj .stl .ply .gltf).",
         "inputSchema": {"type": "object", "properties": {"path": {"type": "string"}}, "required": ["path"]}},
    ]})
}

/// Tools that change the scene (the app records an undo step for these).
pub fn mutates(name: &str) -> bool {
    !matches!(name, "scene_summary" | "export_file")
}

/// Run a tool. Errors are plain messages meant for the agent to read.
pub fn execute(scene: &mut Scene, name: &str, args: &Value) -> Result<Value, String> {
    let a = if args.is_object() { args } else { &Value::Null };
    match name {
        "scene_summary" => Ok(summary(scene)),
        "add_primitive" => {
            let kind = str_arg(a, "kind")?;
            let size = a.get("size").and_then(Value::as_f64).map(|s| s as f32);
            let mesh = match kind {
                "cube" => primitives::cube(size.unwrap_or(2.0)),
                "plane" => primitives::plane(size.unwrap_or(4.0), 4, 4),
                "sphere" => primitives::uv_sphere(size.unwrap_or(2.0) * 0.5, 32, 16),
                "torus" => primitives::torus(size.unwrap_or(2.0) * 0.5, size.unwrap_or(2.0) * 0.125, 48, 12),
                "cylinder" => primitives::cylinder(size.unwrap_or(2.0) * 0.5, size.unwrap_or(2.0), 32),
                other => return Err(format!("unknown primitive `{other}`")),
            };
            let name = a.get("name").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| unique_name(scene, &capitalize(kind)));
            let id = scene.add_mesh_object(name.clone(), mesh, None);
            if let Some(loc) = vec3_arg(a, "location") {
                scene.object_mut(id).unwrap().transform = Transform::from_translation(loc);
            }
            scene.selection.set(id);
            Ok(json!({"id": id.0, "name": name}))
        }
        "transform_object" => {
            let id = find(scene, a.get("object"))?;
            let o = scene.object_mut(id).ok_or("object vanished")?;
            if let Some(loc) = vec3_arg(a, "location") {
                o.transform.translation = loc;
            }
            if let Some(rot) = vec3_arg(a, "rotation") {
                o.transform.rotation = Quat::from_euler(shape_math::glam_euler(), rot.x.to_radians(), rot.y.to_radians(), rot.z.to_radians());
            }
            if let Some(s) = vec3_arg(a, "scale") {
                o.transform.scale = s;
            }
            Ok(object_json(scene, id))
        }
        "rename_object" => {
            let id = find(scene, a.get("object"))?;
            let name = str_arg(a, "name")?.to_owned();
            scene.object_mut(id).ok_or("object vanished")?.name = name;
            Ok(object_json(scene, id))
        }
        "delete_object" => {
            let id = find(scene, a.get("object"))?;
            let removed = scene.remove_object(id).ok_or("object vanished")?;
            scene.prune_meshes();
            Ok(json!({"deleted": removed.name}))
        }
        "set_material" => {
            let id = find(scene, a.get("object"))?;
            let existing = match &scene.object(id).ok_or("object vanished")?.data {
                ObjectData::Mesh { material, .. } => *material,
                _ => return Err("not a mesh object".into()),
            };
            let mid = match existing {
                Some(m) if scene.material(m).is_some() => m,
                _ => {
                    let n = scene.materials().count();
                    let m = scene.add_material(Material { name: format!("Material.{n:03}"), ..Default::default() });
                    if let Some(ObjectData::Mesh { material, .. }) = scene.object_mut(id).map(|o| &mut o.data) {
                        *material = Some(m);
                    }
                    m
                }
            };
            let m = scene.material_mut(mid).ok_or("material vanished")?;
            if let Some(name) = a.get("name").and_then(Value::as_str) {
                m.name = name.to_owned();
            }
            if let Some(c) = a.get("color").and_then(Value::as_array) {
                let f = |i: usize, d: f32| c.get(i).and_then(Value::as_f64).map(|v| v as f32).unwrap_or(d);
                m.base_color = [f(0, 0.8), f(1, 0.8), f(2, 0.8), f(3, 1.0)];
            }
            if let Some(v) = a.get("metallic").and_then(Value::as_f64) {
                m.metallic = v.clamp(0.0, 1.0) as f32;
            }
            if let Some(v) = a.get("roughness").and_then(Value::as_f64) {
                m.roughness = v.clamp(0.0, 1.0) as f32;
            }
            if let Some(e) = vec3_arg(a, "emissive") {
                m.emissive = e.to_array();
            }
            Ok(json!({"object": id.0, "material": m.name, "base_color": m.base_color, "metallic": m.metallic, "roughness": m.roughness}))
        }
        "mesh_op" => {
            let id = find(scene, a.get("object"))?;
            let op = str_arg(a, "op")?;
            let amount = a.get("amount").and_then(Value::as_f64).map(|v| v as f32);
            let (mesh_id, mesh) = scene.object_mesh(id).ok_or("not a mesh object")?;
            let result = match op {
                "subdivide" => shape_ops::catmull_clark(mesh),
                "subdivide_linear" => shape_ops::subdivide_linear(mesh),
                "smooth" => shape_ops::smooth(mesh, amount.map(|a| a.max(1.0) as u32).unwrap_or(2), 0.5),
                "extrude" => shape_ops::extrude_faces(mesh, &shape_ops::all_faces(mesh), amount.unwrap_or(0.5)),
                "inset" => shape_ops::inset_faces(mesh, &shape_ops::all_faces(mesh), amount.unwrap_or(0.2)),
                "bevel" => shape_ops::bevel_all(mesh, amount.unwrap_or(0.1)),
                "shrink_fatten" => shape_ops::shrink_fatten(mesh, amount.unwrap_or(0.1)),
                "triangulate" => shape_ops::triangulate(mesh),
                "flip_normals" => shape_ops::flip_normals(mesh),
                other => return Err(format!("unknown op `{other}`")),
            }
            .map_err(|e| e.to_string())?;
            let counts = json!({"vertices": result.vertex_count(), "edges": result.edge_count(), "faces": result.face_count()});
            scene.mesh_mut(mesh_id).ok_or("mesh vanished")?.set_mesh(result);
            Ok(json!({"object": id.0, "op": op, "mesh": counts}))
        }
        "add_modifier" => {
            let id = find(scene, a.get("object"))?;
            let kind = str_arg(a, "kind")?;
            let modifier = match kind {
                "mirror" => Modifier::Mirror {
                    axis: match a.get("axis").and_then(Value::as_str).unwrap_or("x") {
                        "y" | "Y" => 1,
                        "z" | "Z" => 2,
                        _ => 0,
                    },
                    merge_distance: 1e-4,
                },
                "array" => Modifier::Array {
                    count: a.get("count").and_then(Value::as_u64).unwrap_or(2).clamp(1, 64) as u32,
                    offset: vec3_arg(a, "offset").unwrap_or(Vec3::new(2.5, 0.0, 0.0)),
                    merge_distance: 0.0,
                },
                "subdivision" => Modifier::Subdivision {
                    levels: a.get("levels").and_then(Value::as_u64).unwrap_or(1).min(4) as u32,
                    smooth: a.get("smooth").and_then(Value::as_bool).unwrap_or(true),
                },
                "smooth" => Modifier::Smooth {
                    iterations: a.get("iterations").and_then(Value::as_u64).unwrap_or(1).min(50) as u32,
                    factor: a.get("factor").and_then(Value::as_f64).unwrap_or(0.5).clamp(0.0, 1.0) as f32,
                },
                "bevel" => Modifier::Bevel {
                    offset: a.get("offset").and_then(Value::as_f64).unwrap_or(0.1).clamp(0.0, 10.0) as f32,
                    segments: a.get("segments").and_then(Value::as_u64).unwrap_or(1).clamp(1, 32) as u32,
                },
                "solidify" => Modifier::Solidify {
                    thickness: a.get("thickness").and_then(Value::as_f64).unwrap_or(0.1) as f32,
                    offset: a.get("offset").and_then(Value::as_f64).unwrap_or(-1.0).clamp(-1.0, 1.0) as f32,
                },
                "decimate" => Modifier::Decimate { ratio: a.get("ratio").and_then(Value::as_f64).unwrap_or(0.5).clamp(0.0, 1.0) as f32 },
                "wireframe" => Modifier::Wireframe { thickness: a.get("thickness").and_then(Value::as_f64).unwrap_or(0.05).max(0.0) as f32 },
                "triangulate" => Modifier::Triangulate,
                "screw" => Modifier::Screw {
                    axis: axis_arg(a),
                    angle: (a.get("angle").and_then(Value::as_f64).unwrap_or(360.0) as f32).to_radians(),
                    steps: a.get("steps").and_then(Value::as_u64).unwrap_or(16).clamp(1, 256) as u32,
                    screw: a.get("screw").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                },
                "simple_deform" => Modifier::SimpleDeform {
                    mode: match a.get("mode").and_then(Value::as_str).unwrap_or("twist") {
                        "bend" => shape_scene::DeformMode::Bend,
                        "taper" => shape_scene::DeformMode::Taper,
                        "stretch" => shape_scene::DeformMode::Stretch,
                        _ => shape_scene::DeformMode::Twist,
                    },
                    axis: axis_arg(a),
                    factor: a.get("factor").and_then(Value::as_f64).unwrap_or(0.785) as f32,
                },
                "displace" => Modifier::Displace {
                    strength: a.get("strength").and_then(Value::as_f64).unwrap_or(0.1) as f32,
                    scale: a.get("scale").and_then(Value::as_f64).unwrap_or(1.0).max(0.01) as f32,
                },
                "weld" => Modifier::Weld { distance: a.get("distance").and_then(Value::as_f64).unwrap_or(0.001).max(0.0) as f32 },
                "boolean" => Modifier::Boolean {
                    op: match a.get("operation").and_then(Value::as_str).unwrap_or("difference") {
                        "union" => shape_scene::BooleanOp::Union,
                        "intersect" => shape_scene::BooleanOp::Intersect,
                        _ => shape_scene::BooleanOp::Difference,
                    },
                    target: Some(find(scene, a.get("target"))?),
                },
                "shrinkwrap" => Modifier::Shrinkwrap {
                    target: Some(find(scene, a.get("target"))?),
                    offset: a.get("offset").and_then(Value::as_f64).unwrap_or(0.0) as f32,
                },
                other => return Err(format!("unknown modifier `{other}`")),
            };
            let stack = scene.modifiers_mut(id).ok_or("not a mesh object")?;
            stack.push(modifier);
            Ok(json!({"object": id.0, "modifiers": stack.iter().map(|m| m.name()).collect::<Vec<_>>()}))
        }
        "clear_modifiers" => {
            let id = find(scene, a.get("object"))?;
            let apply = a.get("apply").and_then(Value::as_bool).unwrap_or(false);
            if apply {
                let evaluated = shape_ops::evaluated_mesh(scene, id).ok_or("not a mesh object")?.into_owned();
                let (mesh_id, _) = scene.object_mesh(id).ok_or("not a mesh object")?;
                scene.mesh_mut(mesh_id).ok_or("mesh vanished")?.set_mesh(evaluated);
            }
            scene.modifiers_mut(id).ok_or("not a mesh object")?.clear();
            Ok(json!({"object": id.0, "applied": apply}))
        }
        "add_light" => {
            let kind = str_arg(a, "kind")?;
            let power = a.get("power").and_then(Value::as_f64).map(|v| v as f32);
            let mut light = match kind {
                "sun" => Light::sun(power.unwrap_or(3.0)),
                "spot" => Light::spot(power.unwrap_or(1500.0)),
                _ => Light::point(power.unwrap_or(1000.0)),
            };
            if let Some(c) = vec3_arg(a, "color") {
                light.color = c.to_array();
            }
            let at = vec3_arg(a, "location").unwrap_or(Vec3::new(3.0, 5.0, 3.0));
            let name = a.get("name").and_then(Value::as_str).map(str::to_owned).unwrap_or_else(|| unique_name(scene, &capitalize(kind)));
            let mut object = shape_scene::Object::new(name.clone(), ObjectData::Light(light));
            object.transform = look_at_origin(at);
            let id = scene.add_object(object);
            Ok(json!({"id": id.0, "name": name}))
        }
        "select" => {
            let list = a.get("objects").and_then(Value::as_array).cloned().unwrap_or_default();
            scene.selection.clear();
            for item in &list {
                let id = find(scene, Some(item))?;
                scene.selection.objects.insert(id);
                scene.selection.active = Some(id);
            }
            Ok(json!({"selected": scene.selection.objects.iter().map(|i| i.0).collect::<Vec<_>>()}))
        }
        "import_file" => {
            let path = std::path::PathBuf::from(str_arg(a, "path")?);
            let imported = shape_format::import(&path).map_err(|e| e.to_string())?;
            let before = scene.object_count();
            merge(scene, imported);
            Ok(json!({"added_objects": scene.object_count() - before}))
        }
        "export_file" => {
            let path = std::path::PathBuf::from(str_arg(a, "path")?);
            shape_format::export(scene, &path).map_err(|e| e.to_string())?;
            Ok(json!({"written": path.display().to_string()}))
        }
        other => Err(format!("unknown tool `{other}`")),
    }
}

/// Move every object, mesh and material of `other` into `scene` (ids are remapped).
pub fn merge(scene: &mut Scene, other: Scene) {
    use std::collections::HashMap;
    let mut mats: HashMap<shape_scene::MaterialId, shape_scene::MaterialId> = HashMap::new();
    for (id, m) in other.materials() {
        mats.insert(id, scene.add_material(m.clone()));
    }
    let mut meshes: HashMap<shape_scene::MeshId, shape_scene::MeshId> = HashMap::new();
    for (id, d) in other.meshes() {
        meshes.insert(id, scene.add_mesh(d.clone()));
    }
    let mut objects: HashMap<ObjectId, ObjectId> = HashMap::new();
    let list: Vec<(ObjectId, shape_scene::Object)> = other.objects().map(|(id, o)| (id, o.clone())).collect();
    for (old, mut o) in list {
        if let ObjectData::Mesh { mesh, material, .. } = &mut o.data {
            if let Some(m) = meshes.get(mesh) {
                *mesh = *m;
            }
            *material = material.and_then(|m| mats.get(&m).copied());
        }
        o.parent = None; // parents are re-linked below
        let new = scene.add_object(o);
        objects.insert(old, new);
    }
    for (old, o) in other.objects() {
        if let (Some(p), Some(&new)) = (o.parent, objects.get(&old))
            && let Some(&np) = objects.get(&p)
        {
            scene.set_parent(new, Some(np));
        }
    }
}

fn summary(scene: &Scene) -> Value {
    let objects: Vec<Value> = scene.objects().map(|(id, _)| object_json(scene, id)).collect();
    let materials: Vec<Value> = scene
        .materials()
        .map(|(id, m)| json!({"id": id.0, "name": m.name, "base_color": m.base_color, "metallic": m.metallic, "roughness": m.roughness}))
        .collect();
    let b = scene.bounds();
    json!({
        "name": scene.name,
        "objects": objects,
        "materials": materials,
        "selection": scene.selection.objects.iter().map(|i| i.0).collect::<Vec<_>>(),
        "bounds": if b.is_empty() { Value::Null } else { json!({"min": b.min.to_array(), "max": b.max.to_array()}) },
        "up_axis": "Y",
    })
}

fn object_json(scene: &Scene, id: ObjectId) -> Value {
    let Some(o) = scene.object(id) else { return Value::Null };
    let (rx, ry, rz) = o.transform.rotation.to_euler(shape_math::glam_euler());
    let mut v = json!({
        "id": id.0,
        "name": o.name,
        "type": match o.data { ObjectData::Mesh { .. } => "mesh", ObjectData::Light(_) => "light", ObjectData::Camera(_) => "camera", ObjectData::Empty => "empty" },
        "location": o.transform.translation.to_array(),
        "rotation_deg": [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()],
        "scale": o.transform.scale.to_array(),
        "visible": o.visible,
        "parent": o.parent.map(|p| p.0),
    });
    let obj = v.as_object_mut().expect("object");
    match &o.data {
        ObjectData::Mesh { mesh, material, modifiers } => {
            if let Some(d) = scene.mesh(*mesh) {
                obj.insert("mesh".into(), json!({"vertices": d.mesh.vertex_count(), "edges": d.mesh.edge_count(), "faces": d.mesh.face_count(), "closed": d.mesh.is_closed()}));
            }
            obj.insert("material".into(), json!(material.and_then(|m| scene.material(m)).map(|m| m.name.clone())));
            obj.insert("modifiers".into(), json!(modifiers.iter().map(|m| m.name()).collect::<Vec<_>>()));
        }
        ObjectData::Light(l) => {
            obj.insert("light".into(), json!({"kind": l.kind_name(), "power": l.power, "color": l.color}));
        }
        ObjectData::Camera(c) => {
            obj.insert("camera".into(), json!({"fov_y_deg": c.fov_y.to_degrees(), "near": c.near, "far": c.far}));
        }
        ObjectData::Empty => {}
    }
    v
}

/// An object by name (exact, then case-insensitive) or by numeric id.
pub fn find(scene: &Scene, key: Option<&Value>) -> Result<ObjectId, String> {
    let key = key.ok_or("missing `object`")?;
    if let Some(n) = key.as_u64() {
        let id = ObjectId(n as u32);
        return scene.object(id).map(|_| id).ok_or_else(|| format!("no object with id {n}"));
    }
    let name = key.as_str().ok_or("`object` must be a name or an id")?;
    if let Ok(n) = name.parse::<u32>()
        && scene.object(ObjectId(n)).is_some()
    {
        return Ok(ObjectId(n));
    }
    scene
        .objects()
        .find(|(_, o)| o.name == name)
        .or_else(|| scene.objects().find(|(_, o)| o.name.eq_ignore_ascii_case(name)))
        .map(|(id, _)| id)
        .ok_or_else(|| {
            let names: Vec<&str> = scene.objects().map(|(_, o)| o.name.as_str()).collect();
            format!("no object named `{name}`; objects: {names:?}")
        })
}

fn str_arg<'a>(a: &'a Value, key: &str) -> Result<&'a str, String> {
    a.get(key).and_then(Value::as_str).ok_or_else(|| format!("missing `{key}`"))
}

/// `axis`: x | y | z (default y).
fn axis_arg(a: &Value) -> u8 {
    match a.get("axis").and_then(Value::as_str).unwrap_or("y") {
        "x" | "X" => 0,
        "z" | "Z" => 2,
        _ => 1,
    }
}

fn vec3_arg(a: &Value, key: &str) -> Option<Vec3> {
    let arr = a.get(key)?.as_array()?;
    if arr.len() < 3 {
        return None;
    }
    Some(Vec3::new(arr[0].as_f64()? as f32, arr[1].as_f64()? as f32, arr[2].as_f64()? as f32))
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn unique_name(scene: &Scene, base: &str) -> String {
    if !scene.objects().any(|(_, o)| o.name == base) {
        return base.to_owned();
    }
    (1..).map(|i| format!("{base}.{i:03}")).find(|n| !scene.objects().any(|(_, o)| &o.name == n)).expect("unbounded")
}

fn look_at_origin(at: Vec3) -> Transform {
    if at.length_squared() < 1e-6 {
        return Transform::from_translation(at);
    }
    let dir = at.normalize();
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    Transform::from_matrix(shape_math::Mat4::look_at_rh(at, Vec3::ZERO, up).inverse())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_transform_material_and_summary() {
        let mut scene = Scene::new("t");
        let r = execute(&mut scene, "add_primitive", &json!({"kind": "cube", "location": [1, 2, 3]})).unwrap();
        assert_eq!(r["name"], "Cube");
        let r2 = execute(&mut scene, "add_primitive", &json!({"kind": "cube"})).unwrap();
        assert_eq!(r2["name"], "Cube.001");
        execute(&mut scene, "transform_object", &json!({"object": "Cube", "rotation": [0, 90, 0], "scale": [2, 2, 2]})).unwrap();
        let id = find(&scene, Some(&json!("Cube"))).unwrap();
        let t = scene.object(id).unwrap().transform;
        assert_eq!(t.translation, Vec3::new(1.0, 2.0, 3.0));
        assert_eq!(t.scale, Vec3::splat(2.0));
        execute(&mut scene, "set_material", &json!({"object": "Cube", "color": [1, 0, 0], "metallic": 0.5})).unwrap();
        let s = execute(&mut scene, "scene_summary", &json!({})).unwrap();
        assert_eq!(s["objects"].as_array().unwrap().len(), 2);
        assert_eq!(s["objects"][0]["material"], "Material.000");
        assert_eq!(s["materials"][0]["base_color"][0], 1.0);
        assert!(find(&scene, Some(&json!("Nothing"))).unwrap_err().contains("Cube"));
        assert_eq!(find(&scene, Some(&json!(1))).unwrap(), ObjectId(1));
    }

    #[test]
    fn mesh_ops_modifiers_lights_delete() {
        let mut scene = Scene::new("t");
        execute(&mut scene, "add_primitive", &json!({"kind": "cube", "name": "Box"})).unwrap();
        let r = execute(&mut scene, "mesh_op", &json!({"object": "Box", "op": "subdivide"})).unwrap();
        assert_eq!(r["mesh"]["faces"], 24);
        execute(&mut scene, "add_modifier", &json!({"object": "Box", "kind": "array", "count": 3})).unwrap();
        let r = execute(&mut scene, "add_modifier", &json!({"object": "Box", "kind": "mirror", "axis": "z"})).unwrap();
        assert_eq!(r["modifiers"], json!(["Array", "Mirror"]));
        let id = find(&scene, Some(&json!("Box"))).unwrap();
        // the mirror of a centred cube collapses onto itself and is skipped; the array still applies
        assert_eq!(shape_ops::evaluated_mesh(&scene, id).unwrap().face_count(), 24 * 3);
        execute(&mut scene, "clear_modifiers", &json!({"object": "Box", "apply": true})).unwrap();
        assert!(scene.modifiers(id).is_empty());
        assert!(scene.object_mesh(id).unwrap().1.face_count() >= 24 * 3);
        execute(&mut scene, "add_light", &json!({"kind": "sun", "location": [0, 5, 0]})).unwrap();
        assert_eq!(scene.object_count(), 2);
        assert!(execute(&mut scene, "mesh_op", &json!({"object": "Sun", "op": "subdivide"})).is_err());
        execute(&mut scene, "delete_object", &json!({"object": "Box"})).unwrap();
        assert_eq!(scene.object_count(), 1);
        assert!(!mutates("scene_summary") && mutates("delete_object"));
        assert!(tool_definitions()["tools"].as_array().unwrap().len() >= 13);
    }

    #[test]
    fn merge_remaps_ids() {
        let mut a = Scene::new("a");
        execute(&mut a, "add_primitive", &json!({"kind": "cube"})).unwrap();
        let mut b = Scene::new("b");
        execute(&mut b, "add_primitive", &json!({"kind": "sphere"})).unwrap();
        execute(&mut b, "set_material", &json!({"object": "Sphere", "color": [0, 1, 0]})).unwrap();
        merge(&mut a, b);
        assert_eq!(a.object_count(), 2);
        let sphere = find(&a, Some(&json!("Sphere"))).unwrap();
        let ObjectData::Mesh { material: Some(m), .. } = &a.object(sphere).unwrap().data else { panic!() };
        assert_eq!(a.material(*m).unwrap().base_color[1], 1.0);
        assert!(a.object_mesh(sphere).is_some());
    }
}
