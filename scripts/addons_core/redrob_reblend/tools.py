# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Strictly allow-listed, undo-safe Blender action implementation."""

import copy
import math
import struct

import bpy

MAX_ACTIONS = 128
_MAX_COORDINATE = 1_000_000.0
_MAX_SCALE = 10_000.0
_MIN_SCALE_INPUT = 1e-6
# Canonical float32 representation of the documented 1e-6 lower bound.
_MIN_SCALE = 9.999999974752427e-07
_MAX_NAME_BYTES = 63
_ALLOWED_PRIMITIVES = frozenset({"cube", "sphere", "cylinder", "cone", "plane", "torus"})
_ALLOWED_MODIFIERS = frozenset({"bevel", "subdivision", "solidify", "mirror"})
_MATERIAL_OBJECT_TYPES = frozenset({'MESH', 'CURVE', 'SURFACE', 'META', 'FONT', 'VOLUME'})
_PRIMITIVE_TOPOLOGY = {
    "cube": (8, 6),
    "sphere": (482, 512),
    "cylinder": (64, 34),
    "cone": (33, 33),
    "plane": (4, 1),
    "torus": (576, 576),
}
_PRIMITIVE_OPERATOR_ARGUMENTS = {
    "cube": {
        "size": 2.0,
        "align": 'WORLD',
        "rotation": [0.0, 0.0, 0.0],
        "enter_editmode": False,
        "calc_uvs": True,
    },
    "sphere": {
        "segments": 32,
        "ring_count": 16,
        "radius": 1.0,
        "align": 'WORLD',
        "rotation": [0.0, 0.0, 0.0],
        "enter_editmode": False,
        "calc_uvs": True,
    },
    "cylinder": {
        "vertices": 32,
        "radius": 1.0,
        "depth": 2.0,
        "end_fill_type": 'NGON',
        "align": 'WORLD',
        "rotation": [0.0, 0.0, 0.0],
        "enter_editmode": False,
        "calc_uvs": True,
    },
    "cone": {
        "vertices": 32,
        "radius1": 1.0,
        "radius2": 0.0,
        "depth": 2.0,
        "end_fill_type": 'NGON',
        "align": 'WORLD',
        "rotation": [0.0, 0.0, 0.0],
        "enter_editmode": False,
        "calc_uvs": True,
    },
    "plane": {
        "size": 2.0,
        "align": 'WORLD',
        "rotation": [0.0, 0.0, 0.0],
        "enter_editmode": False,
        "calc_uvs": True,
    },
    "torus": {
        "major_segments": 48,
        "minor_segments": 12,
        "mode": 'MAJOR_MINOR',
        "major_radius": 1.0,
        "minor_radius": 0.25,
        "align": 'WORLD',
        "rotation": [0.0, 0.0, 0.0],
        "generate_uvs": True,
    },
}


class PlanValidationError(ValueError):
    """Raised before mutation when a plan is unsafe, stale, or out of scope."""


def _strict_fields(value, required, optional=()):
    if not isinstance(value, dict):
        raise PlanValidationError("arguments must be an object")
    fields = set(value)
    required = set(required)
    allowed = required | set(optional)
    if not required <= fields or not fields <= allowed:
        raise PlanValidationError("arguments do not match the tool schema")


def _name(value):
    if not isinstance(value, str) or not value.strip() or len(value.encode("utf-8")) > _MAX_NAME_BYTES:
        raise PlanValidationError("name is empty or too large")
    if any(character in value for character in "\r\n\0"):
        raise PlanValidationError("name contains control characters")
    return value


def _number(value, minimum, maximum):
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise PlanValidationError("numeric value has the wrong type")
    try:
        value = float(value)
    except (OverflowError, TypeError, ValueError) as exc:
        raise PlanValidationError("numeric value is outside its allowed range") from exc
    if not math.isfinite(value) or not minimum <= value <= maximum:
        raise PlanValidationError("numeric value is outside its allowed range")
    return value


def _integer(value, minimum, maximum):
    if isinstance(value, bool) or not isinstance(value, int) or not minimum <= value <= maximum:
        raise PlanValidationError("integer value is outside its allowed range")
    return value


def _vector(value, size, minimum, maximum):
    if not isinstance(value, list) or len(value) != size:
        raise PlanValidationError(f"expected a {size}-component array")
    return tuple(_number(item, minimum, maximum) for item in value)


def _is_editable(obj):
    data = getattr(obj, "data", None)
    return obj.library is None and getattr(data, "library", None) is None


def _scene_object(context, name):
    obj = context.scene.objects.get(name)
    if obj is None:
        raise PlanValidationError(f"object is not in the approved scene: {name}")
    if not _is_editable(obj):
        raise PlanValidationError(f"object is linked or read-only: {name}")
    return obj


def _truncate_utf8(value, maximum_bytes):
    encoded = value.encode("utf-8")
    if len(encoded) <= maximum_bytes:
        return value
    return encoded[:maximum_bytes].decode("utf-8", errors="ignore")


def _unique_name(base, existing, reserved):
    base = _truncate_utf8(base, _MAX_NAME_BYTES)
    if base not in existing and base not in reserved:
        return base
    index = 1
    while True:
        suffix = f".{index:03d}"
        prefix = _truncate_utf8(base, _MAX_NAME_BYTES - len(suffix))
        candidate = f"{prefix}{suffix}"
        if candidate not in existing and candidate not in reserved:
            return candidate
        index += 1


def _validate_original_numbers(action_type, arguments):
    vector_ranges = {
        "create_primitive": {
            "location": (-_MAX_COORDINATE, _MAX_COORDINATE),
            "scale": (_MIN_SCALE_INPUT, _MAX_SCALE),
        },
        "transform_object": {
            "location": (-_MAX_COORDINATE, _MAX_COORDINATE),
            "rotation": (-_MAX_COORDINATE, _MAX_COORDINATE),
            "scale": (_MIN_SCALE_INPUT, _MAX_SCALE),
        },
        "set_material": {"base_color": (0.0, 1.0)},
    }.get(action_type, {})
    for field, (minimum, maximum) in vector_ranges.items():
        value = arguments.get(field)
        if value is not None:
            expected_size = 4 if field == "base_color" else 3
            _vector(value, expected_size, minimum, maximum)
    scalar_ranges = {
        "width": (0.0, _MAX_COORDINATE),
        "thickness": (-_MAX_COORDINATE, _MAX_COORDINATE),
        "metallic": (0.0, 1.0),
        "roughness": (0.0, 1.0),
    }
    for field, (minimum, maximum) in scalar_ranges.items():
        value = arguments.get(field)
        if value is not None:
            _number(value, minimum, maximum)


def _float32(value):
    return struct.unpack("!f", struct.pack("!f", float(value)))[0]


def _canonicalize_action_numbers(action_type, arguments):
    vector_fields = {
        "create_primitive": ("location", "scale"),
        "transform_object": ("location", "rotation", "scale"),
        "set_material": ("base_color",),
    }.get(action_type, ())
    for field in vector_fields:
        value = arguments.get(field)
        if isinstance(value, list):
            arguments[field] = [_float32(item) for item in value]
    for field in ("width", "thickness", "metallic", "roughness"):
        value = arguments.get(field)
        if isinstance(value, (int, float)) and not isinstance(value, bool):
            arguments[field] = _float32(value)


def normalize_plan(payload, context):
    """Materialize context-sensitive defaults so preview and execution are exact."""
    normalized = copy.deepcopy(payload)
    if not isinstance(normalized, dict) or not isinstance(normalized.get("actions"), list):
        return normalized

    object_names = {obj.name for obj in bpy.data.objects}
    material_names = {material.name for material in bpy.data.materials}
    reserved_objects = set()
    reserved_materials = set()
    for action in normalized["actions"]:
        if not isinstance(action, dict) or not isinstance(action.get("arguments"), dict):
            continue
        arguments = action["arguments"]
        _validate_original_numbers(action.get("type"), arguments)
        if action.get("type") == "create_primitive":
            if arguments.get("name") is None:
                primitive = str(arguments.get("primitive", "Object")).replace("_", " ").title()
                arguments["name"] = _unique_name(
                    f"Reblend {primitive}", object_names, reserved_objects
                )
            if isinstance(arguments.get("name"), str):
                reserved_objects.add(arguments["name"])
            if arguments.get("location") is None:
                arguments["location"] = [0.0, 0.0, 0.0]
            if arguments.get("scale") is None:
                arguments["scale"] = [1.0, 1.0, 1.0]
        elif action.get("type") == "add_modifier" and arguments.get("modifier") == "subdivision":
            if arguments.get("render_levels") is None and arguments.get("levels") is not None:
                arguments["render_levels"] = arguments["levels"]
        elif action.get("type") == "set_material":
            if isinstance(arguments.get("name"), str):
                arguments["name"] = _unique_name(
                    arguments["name"], material_names, reserved_materials
                )
                reserved_materials.add(arguments["name"])
            if arguments.get("metallic") is None:
                arguments["metallic"] = 0.0
            if arguments.get("roughness") is None:
                arguments["roughness"] = 0.5
        _canonicalize_action_numbers(action.get("type"), arguments)
    return normalized


def preview_arguments(action):
    """Return every value that determines an action's approved effect."""
    raw_arguments = action.get("arguments", {})
    if not isinstance(raw_arguments, dict):
        return None
    arguments = dict(raw_arguments)
    if action.get("type") == "create_primitive":
        primitive = arguments.get("primitive")
        operator_arguments = _PRIMITIVE_OPERATOR_ARGUMENTS.get(primitive, {})
        for name, value in operator_arguments.items():
            arguments.setdefault(name, value)
    return arguments


def validate_plan(payload, context, allowed_objects=None):
    """Validate the complete plan and all references without mutating Blender."""
    _strict_fields(payload, {"summary", "actions"})
    summary = payload["summary"]
    if not isinstance(summary, str) or not summary.strip() or len(summary.encode("utf-8")) > 8192:
        raise PlanValidationError("summary is empty or too large")
    actions = payload["actions"]
    if not isinstance(actions, list) or len(actions) > MAX_ACTIONS:
        raise PlanValidationError("plan has an invalid number of actions")
    if context.mode != 'OBJECT':
        raise PlanValidationError("switch to Object Mode before applying a plan")

    scene_objects = {obj.name: obj for obj in context.scene.objects}
    captured = set(scene_objects) if allowed_objects is None else set(allowed_objects) & set(scene_objects)
    available = set(captured)
    planned_created = set()
    planned_materials = set()
    material_capable = {
        name for name in captured if scene_objects[name].type in _MATERIAL_OBJECT_TYPES
    }

    def require_object(value):
        name = _name(value)
        if name in planned_created:
            return name
        if name not in available:
            raise PlanValidationError(f"object was not included in the approved context: {name}")
        _scene_object(context, name)
        return name

    for action in actions:
        _strict_fields(action, {"type", "arguments"})
        action_type = action["type"]
        arguments = action["arguments"]
        if action_type == "create_primitive":
            _strict_fields(arguments, {"primitive"}, {"name", "location", "scale"})
            if arguments["primitive"] not in _ALLOWED_PRIMITIVES:
                raise PlanValidationError("unsupported primitive")
            if arguments.get("name") is not None:
                object_name = _name(arguments["name"])
                if object_name in bpy.data.objects or object_name in planned_created:
                    raise PlanValidationError(f"object name is already in use: {object_name}")
                planned_created.add(object_name)
                available.add(object_name)
                material_capable.add(object_name)
            if arguments.get("location") is not None:
                _vector(arguments["location"], 3, -_MAX_COORDINATE, _MAX_COORDINATE)
            if arguments.get("scale") is not None:
                _vector(arguments["scale"], 3, _MIN_SCALE, _MAX_SCALE)
        elif action_type == "transform_object":
            _strict_fields(arguments, {"object"}, {"location", "rotation", "scale"})
            require_object(arguments["object"])
            if not any(arguments.get(field) is not None for field in ("location", "rotation", "scale")):
                raise PlanValidationError("transform has no values")
            if arguments.get("location") is not None:
                _vector(arguments["location"], 3, -_MAX_COORDINATE, _MAX_COORDINATE)
            if arguments.get("rotation") is not None:
                _vector(arguments["rotation"], 3, -_MAX_COORDINATE, _MAX_COORDINATE)
            if arguments.get("scale") is not None:
                _vector(arguments["scale"], 3, _MIN_SCALE, _MAX_SCALE)
        elif action_type == "add_modifier":
            _strict_fields(
                arguments,
                {"object", "modifier"},
                {"width", "segments", "levels", "render_levels", "thickness", "axis"},
            )
            require_object(arguments["object"])
            modifier = arguments["modifier"]
            if modifier not in _ALLOWED_MODIFIERS:
                raise PlanValidationError("unsupported modifier")
            _validate_modifier(modifier, arguments)
        elif action_type == "set_material":
            _strict_fields(arguments, {"object", "name", "base_color"}, {"metallic", "roughness"})
            object_name = require_object(arguments["object"])
            if object_name not in material_capable:
                raise PlanValidationError("object type does not support materials")
            material_name = _name(arguments["name"])
            if material_name in bpy.data.materials or material_name in planned_materials:
                raise PlanValidationError(f"material name is already in use: {material_name}")
            planned_materials.add(material_name)
            _vector(arguments["base_color"], 4, 0.0, 1.0)
            if arguments.get("metallic") is not None:
                _number(arguments["metallic"], 0.0, 1.0)
            if arguments.get("roughness") is not None:
                _number(arguments["roughness"], 0.0, 1.0)
        else:
            raise PlanValidationError(f"unsupported action: {action_type}")
    return payload


def _validate_modifier(modifier, arguments):
    if modifier == "bevel":
        _strict_fields(arguments, {"object", "modifier", "width", "segments"})
        _number(arguments["width"], 0.0, _MAX_COORDINATE)
        _integer(arguments["segments"], 1, 64)
    elif modifier == "subdivision":
        _strict_fields(arguments, {"object", "modifier", "levels"}, {"render_levels"})
        _integer(arguments["levels"], 0, 6)
        if arguments.get("render_levels") is not None:
            _integer(arguments["render_levels"], 0, 6)
    elif modifier == "solidify":
        _strict_fields(arguments, {"object", "modifier", "thickness"})
        _number(arguments["thickness"], -_MAX_COORDINATE, _MAX_COORDINATE)
    elif modifier == "mirror":
        _strict_fields(arguments, {"object", "modifier", "axis"})
        if arguments["axis"] not in {"x", "y", "z"}:
            raise PlanValidationError("mirror axis must be x, y, or z")


def execute_plan(payload, context, allowed_objects=None):
    """Execute and verify a plan. Call only inside an UNDO Blender operator."""
    validate_plan(payload, context, allowed_objects)
    results = []
    for action in payload["actions"]:
        action_type = action["type"]
        arguments = action["arguments"]
        if action_type == "create_primitive":
            result = _create_primitive(arguments, context)
        elif action_type == "transform_object":
            result = _transform_object(arguments, context)
        elif action_type == "add_modifier":
            result = _add_modifier(arguments, context)
        elif action_type == "set_material":
            result = _set_material(arguments, context)
        _verify_action(action, result, context)
        results.append(result)
    return results


def primitive_operator_arguments(arguments):
    """Return complete, context-independent bpy operator arguments for a primitive."""
    primitive = arguments["primitive"]
    kwargs = copy.deepcopy(_PRIMITIVE_OPERATOR_ARGUMENTS[primitive])
    kwargs["location"] = list(arguments.get("location", (0.0, 0.0, 0.0)))
    if primitive != "torus":
        kwargs["scale"] = list(arguments.get("scale", (1.0, 1.0, 1.0)))
    return kwargs


def _create_primitive(arguments, context):
    primitive = arguments["primitive"]
    operators = {
        "cube": bpy.ops.mesh.primitive_cube_add,
        "sphere": bpy.ops.mesh.primitive_uv_sphere_add,
        "cylinder": bpy.ops.mesh.primitive_cylinder_add,
        "cone": bpy.ops.mesh.primitive_cone_add,
        "plane": bpy.ops.mesh.primitive_plane_add,
        "torus": bpy.ops.mesh.primitive_torus_add,
    }
    kwargs = primitive_operator_arguments(arguments)
    operation = operators[primitive](**kwargs)
    if primitive == "torus" and context.mode != 'OBJECT':
        bpy.ops.object.mode_set(mode='OBJECT')
    obj = context.active_object
    if 'FINISHED' not in operation or obj is None or context.scene.objects.get(obj.name) is None:
        raise RuntimeError(f"Blender could not create {primitive}")
    if arguments.get("name") is not None:
        obj.name = arguments["name"]
        if obj.name != arguments["name"]:
            raise RuntimeError("Blender changed the approved object name")
    if arguments.get("scale") is not None:
        obj.scale = arguments["scale"]
    return {"type": "create_primitive", "object": obj.name}


def _transform_object(arguments, context):
    obj = _scene_object(context, arguments["object"])
    if arguments.get("location") is not None:
        obj.location = arguments["location"]
    if arguments.get("rotation") is not None:
        obj.rotation_euler = arguments["rotation"]
    if arguments.get("scale") is not None:
        obj.scale = arguments["scale"]
    return {"type": "transform_object", "object": obj.name}


def _add_modifier(arguments, context):
    obj = _scene_object(context, arguments["object"])
    modifier_type = {
        "bevel": 'BEVEL',
        "subdivision": 'SUBSURF',
        "solidify": 'SOLIDIFY',
        "mirror": 'MIRROR',
    }[arguments["modifier"]]
    modifier = obj.modifiers.new(name=f"Reblend {arguments['modifier'].title()}", type=modifier_type)
    if arguments["modifier"] == "bevel":
        modifier.width = arguments["width"]
        modifier.segments = arguments["segments"]
    elif arguments["modifier"] == "subdivision":
        modifier.levels = arguments["levels"]
        modifier.render_levels = arguments.get("render_levels", arguments["levels"])
    elif arguments["modifier"] == "solidify":
        modifier.thickness = arguments["thickness"]
    elif arguments["modifier"] == "mirror":
        axis = arguments["axis"]
        modifier.use_axis = (axis == "x", axis == "y", axis == "z")
    return {"type": "add_modifier", "object": obj.name, "modifier": modifier.name}


def _set_material(arguments, context):
    obj = _scene_object(context, arguments["object"])
    # Always create a single-user material; never mutate a shared global material.
    material = bpy.data.materials.new(arguments["name"])
    if material.name != arguments["name"]:
        bpy.data.materials.remove(material)
        raise RuntimeError("Blender changed the approved material name")
    color = tuple(arguments["base_color"])
    material.diffuse_color = color
    material.use_nodes = True
    principled = material.node_tree.nodes.get("Principled BSDF") if material.node_tree else None
    if principled:
        if "Base Color" in principled.inputs:
            principled.inputs["Base Color"].default_value = color
        if arguments.get("metallic") is not None and "Metallic" in principled.inputs:
            principled.inputs["Metallic"].default_value = arguments["metallic"]
        if arguments.get("roughness") is not None and "Roughness" in principled.inputs:
            principled.inputs["Roughness"].default_value = arguments["roughness"]
    materials = obj.data.materials
    if len(obj.material_slots) == 0:
        materials.append(material)
    else:
        obj.material_slots[obj.active_material_index].material = material
    return {"type": "set_material", "object": obj.name, "material": material.name}


def _vectors_match(actual, expected, tolerance=1e-5):
    return all(
        math.isclose(float(left), float(right), rel_tol=0.0, abs_tol=tolerance)
        for left, right in zip(actual, expected)
    )


def _verify_action(action, result, context):
    arguments = action["arguments"]
    obj = _scene_object(context, result["object"])
    action_type = action["type"]
    if action_type == "create_primitive":
        if arguments.get("name") is not None and obj.name != arguments["name"]:
            raise RuntimeError("created object name verification failed")
        if arguments.get("location") is not None and not _vectors_match(obj.location, arguments["location"]):
            raise RuntimeError("created object location verification failed")
        if not _vectors_match(obj.rotation_euler, (0.0, 0.0, 0.0)):
            raise RuntimeError("created object rotation verification failed")
        if arguments.get("scale") is not None and not _vectors_match(
            obj.scale, arguments["scale"], tolerance=0.0
        ):
            raise RuntimeError("created object scale verification failed")
        expected_vertices, expected_polygons = _PRIMITIVE_TOPOLOGY[arguments["primitive"]]
        if len(obj.data.vertices) != expected_vertices or len(obj.data.polygons) != expected_polygons:
            raise RuntimeError("created primitive topology verification failed")
    elif action_type == "transform_object":
        for field, actual in (("location", obj.location), ("rotation", obj.rotation_euler), ("scale", obj.scale)):
            tolerance = 0.0 if field == "scale" else 1e-5
            if arguments.get(field) is not None and not _vectors_match(
                actual, arguments[field], tolerance=tolerance
            ):
                raise RuntimeError(f"{field} verification failed for {obj.name}")
    elif action_type == "add_modifier":
        modifier = obj.modifiers.get(result["modifier"])
        expected_type = {
            "bevel": 'BEVEL',
            "subdivision": 'SUBSURF',
            "solidify": 'SOLIDIFY',
            "mirror": 'MIRROR',
        }[arguments["modifier"]]
        if modifier is None or modifier.type != expected_type:
            raise RuntimeError(f"modifier verification failed for {obj.name}")
        if arguments["modifier"] == "bevel":
            if not math.isclose(modifier.width, arguments["width"], abs_tol=1e-5):
                raise RuntimeError(f"bevel width verification failed for {obj.name}")
            if modifier.segments != arguments["segments"]:
                raise RuntimeError(f"bevel segments verification failed for {obj.name}")
        elif arguments["modifier"] == "subdivision":
            if modifier.levels != arguments["levels"]:
                raise RuntimeError(f"subdivision level verification failed for {obj.name}")
            if modifier.render_levels != arguments.get("render_levels", arguments["levels"]):
                raise RuntimeError(f"subdivision render level verification failed for {obj.name}")
        elif arguments["modifier"] == "solidify":
            if not math.isclose(modifier.thickness, arguments["thickness"], abs_tol=1e-5):
                raise RuntimeError(f"solidify thickness verification failed for {obj.name}")
        elif arguments["modifier"] == "mirror":
            axis = arguments["axis"]
            expected_axis = (axis == "x", axis == "y", axis == "z")
            if tuple(modifier.use_axis) != expected_axis:
                raise RuntimeError(f"mirror axis verification failed for {obj.name}")
    elif action_type == "set_material":
        material = bpy.data.materials.get(result["material"])
        if material is None or obj.active_material != material:
            raise RuntimeError(f"material assignment verification failed for {obj.name}")
        if not _vectors_match(material.diffuse_color, arguments["base_color"]):
            raise RuntimeError(f"material color verification failed for {obj.name}")
        principled = material.node_tree.nodes.get("Principled BSDF") if material.node_tree else None
        if principled is None:
            raise RuntimeError(f"material node verification failed for {obj.name}")
        base_color = principled.inputs.get("Base Color")
        if base_color is None or not _vectors_match(
            base_color.default_value, arguments["base_color"]
        ):
            raise RuntimeError(f"material shader color verification failed for {obj.name}")
        for field, socket_name in (("metallic", "Metallic"), ("roughness", "Roughness")):
            if arguments.get(field) is not None:
                socket = principled.inputs.get(socket_name)
                if socket is None or not math.isclose(
                    float(socket.default_value), float(arguments[field]), abs_tol=1e-5
                ):
                    raise RuntimeError(f"material {field} verification failed for {obj.name}")
