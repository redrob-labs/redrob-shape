# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Strictly allow-listed, undo-safe Blender action implementation.

What a plan may contain is decided by ``tool_catalog.json`` and checked by ``catalog.py``, the
same rules the agent applies. This file adds what only Blender can know -- the object exists, is
editable, is a mesh, a new name is free -- and then runs and verifies each step.
"""

import copy
import math
import struct

import bpy

from . import catalog

MAX_ACTIONS = catalog.LIMITS["max_actions"]
_MAX_NAME_BYTES = catalog.LIMITS["max_name_bytes"]
# Canonical float32 representation of the documented 1e-6 lower scale bound.
_MIN_SCALE = 9.999999974752427e-07
_MATERIAL_OBJECT_TYPES = frozenset({'MESH', 'CURVE', 'SURFACE', 'META', 'FONT', 'VOLUME'})
_NUMBER_KINDS = frozenset({"number", "vector3", "vector4"})
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
_MODIFIER_TYPES = {
    "bevel": 'BEVEL',
    "subdivision": 'SUBSURF',
    "solidify": 'SOLIDIFY',
    "mirror": 'MIRROR',
    "array": 'ARRAY',
    "twist": 'SIMPLE_DEFORM',
    "bend": 'SIMPLE_DEFORM',
    "taper": 'SIMPLE_DEFORM',
    "stretch": 'SIMPLE_DEFORM',
    "displace": 'DISPLACE',
    "smooth": 'SMOOTH',
    "boolean": 'BOOLEAN',
}
_AXES = {"x": (1.0, 0.0, 0.0), "y": (0.0, 1.0, 0.0), "z": (0.0, 0.0, 1.0)}
_FACE_AXES = {
    "+x": (1.0, 0.0, 0.0),
    "-x": (-1.0, 0.0, 0.0),
    "+y": (0.0, 1.0, 0.0),
    "-y": (0.0, -1.0, 0.0),
    "+z": (0.0, 0.0, 1.0),
    "-z": (0.0, 0.0, -1.0),
}


class PlanValidationError(ValueError):
    """Raised before mutation when a plan is unsafe, stale, or out of scope."""


def modifier_name(kind):
    """The name a plan's modifier gets, before Blender makes it unique on the object."""
    return f"Redrob Shape {kind.title()}"


def _float32(value):
    return struct.unpack("!f", struct.pack("!f", float(value)))[0]


def _truncate_utf8(value, maximum_bytes):
    encoded = value.encode("utf-8")
    if len(encoded) <= maximum_bytes:
        return value
    return encoded[:maximum_bytes].decode("utf-8", errors="ignore")


def _unique_name(base, existing, reserved=(), maximum_bytes=_MAX_NAME_BYTES):
    base = _truncate_utf8(base, maximum_bytes)
    if base not in existing and base not in reserved:
        return base
    index = 1
    while True:
        suffix = f".{index:03d}"
        prefix = _truncate_utf8(base, maximum_bytes - len(suffix))
        candidate = f"{prefix}{suffix}"
        if candidate not in existing and candidate not in reserved:
            return candidate
        index += 1


def _check_shape(payload, round_number=None):
    try:
        return catalog.check_plan(payload, round_number)
    except catalog.PlanShapeError as exc:
        raise PlanValidationError(str(exc)) from exc


def _canonicalize(action):
    """Round every float to what Blender stores, so preview, check and result agree exactly."""
    arguments = action["arguments"]
    _, fields = catalog.fields_for(action["type"], arguments)
    for name, field in fields.items():
        value = arguments.get(name)
        if field["type"] not in _NUMBER_KINDS or value is None:
            continue
        if isinstance(value, list):
            arguments[name] = [_float32(item) for item in value]
        else:
            arguments[name] = _float32(value)


def normalize_plan(payload, context):
    """Check the plan's shape, then fill every default so preview and execution are exact."""
    del context  # names are made unique against bpy.data, which is global
    normalized = _check_shape(payload)
    object_names = {obj.name for obj in bpy.data.objects}
    material_names = {material.name for material in bpy.data.materials}
    reserved_objects = set()
    reserved_materials = set()
    for action in normalized["actions"]:
        action_type = action["type"]
        arguments = action["arguments"]
        if action_type == "create_primitive" and "name" not in arguments:
            primitive = arguments["primitive"].replace("_", " ").title()
            arguments["name"] = _unique_name(f"Redrob Shape {primitive}", object_names, reserved_objects)
        elif action_type == "duplicate_object" and "name" not in arguments:
            arguments["name"] = _unique_name(arguments["object"], object_names, reserved_objects)
        elif action_type == "add_modifier" and arguments.get("modifier") == "subdivision":
            arguments.setdefault("render_levels", arguments["levels"])
        elif action_type == "set_material":
            arguments["name"] = _unique_name(arguments["name"], material_names, reserved_materials)
            reserved_materials.add(arguments["name"])
        if action_type in ("create_primitive", "duplicate_object", "rename_object"):
            reserved_objects.add(arguments["name"])
        for name, value in catalog.default_arguments(action_type, arguments).items():
            arguments[name] = value
        _canonicalize(action)
    return normalized


def preview_arguments(action):
    """Return every value that determines an action's approved effect."""
    raw_arguments = action.get("arguments", {})
    if not isinstance(raw_arguments, dict):
        return None
    arguments = dict(raw_arguments)
    if action.get("type") == "create_primitive":
        for name, value in _PRIMITIVE_OPERATOR_ARGUMENTS.get(arguments.get("primitive"), {}).items():
            arguments.setdefault(name, value)
    return arguments


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


class _PlannedScene:
    """What the scene will look like at each step, so a step can only name what exists by then."""

    def __init__(self, context, allowed_objects):
        scene_objects = {obj.name: obj for obj in context.scene.objects}
        captured = set(scene_objects) if allowed_objects is None else set(allowed_objects) & set(scene_objects)
        self.context = context
        self.types = {}
        self.modifiers = {}
        self.material_capable = set()
        for name in captured:
            obj = scene_objects[name]
            self.types[name] = obj.type
            self.modifiers[name] = [m.name for m in getattr(obj, "modifiers", ())]
            if obj.type in _MATERIAL_OBJECT_TYPES:
                self.material_capable.add(name)
        self.existing = set(captured)
        self.taken = {obj.name for obj in bpy.data.objects}
        self.materials = {material.name for material in bpy.data.materials}

    def require(self, value):
        name = catalog.check_name(value)
        if name not in self.types:
            raise PlanValidationError(f"object is not in the approved scope, or was removed earlier: {name}")
        if name in self.existing:
            _scene_object(self.context, name)
        return name

    def require_mesh(self, value, action_type):
        name = self.require(value)
        if self.types[name] != 'MESH':
            raise PlanValidationError(f"{action_type} needs a mesh object; {name} is {self.types[name].lower()}")
        return name

    def claim(self, name):
        if name in self.taken:
            raise PlanValidationError(f"object name is already in use: {name}")
        self.taken.add(name)

    def add(self, name, kind, modifiers=(), materials=True):
        self.claim(name)
        self.types[name] = kind
        self.modifiers[name] = list(modifiers)
        if materials:
            self.material_capable.add(name)

    def forget(self, name):
        self.types.pop(name, None)
        self.modifiers.pop(name, None)
        self.material_capable.discard(name)
        self.existing.discard(name)
        self.taken.discard(name)

    def add_modifier(self, name, kind):
        existing = self.modifiers[name]
        real = _unique_name(modifier_name(kind), existing, maximum_bytes=63)
        existing.append(real)
        return real


def validate_plan(payload, context, allowed_objects=None):
    """Validate the complete plan and all references without mutating Blender."""
    if not isinstance(payload, dict):
        raise PlanValidationError("plan must be an object")
    _check_shape(payload, _float32)
    if context.mode != 'OBJECT':
        raise PlanValidationError("switch to Object Mode before applying a plan")
    scene = _PlannedScene(context, allowed_objects)

    for number, action in enumerate(payload["actions"], 1):
        action_type = action["type"]
        arguments = action["arguments"]
        try:
            _validate_step(scene, action_type, arguments)
        except PlanValidationError as exc:
            raise PlanValidationError(f"step {number}: {exc}") from exc
    return payload


def _validate_step(scene, action_type, arguments):
    if catalog.mesh_only(action_type):
        target = scene.require_mesh(arguments["object"], action_type)
    elif "object" in arguments:
        target = scene.require(arguments["object"])
    else:
        target = None

    if action_type == "create_primitive":
        if "name" in arguments:
            scene.add(catalog.check_name(arguments["name"]), 'MESH')
    elif action_type == "add_modifier":
        scene.add_modifier(target, arguments["modifier"])
    elif action_type == "boolean":
        other = scene.require_mesh(arguments["with"], action_type)
        if other == target:
            raise PlanValidationError("boolean needs two different objects")
        scene.add_modifier(target, "boolean")
    elif action_type == "apply_modifier":
        if arguments["modifier"] not in scene.modifiers[target]:
            raise PlanValidationError(f"{target} has no modifier named {arguments['modifier']}")
        scene.modifiers[target].remove(arguments["modifier"])
    elif action_type == "duplicate_object":
        if "name" in arguments:
            scene.add(
                catalog.check_name(arguments["name"]),
                scene.types[target],
                scene.modifiers[target],
                target in scene.material_capable,
            )
    elif action_type == "delete_object":
        scene.forget(target)
    elif action_type == "rename_object":
        new_name = catalog.check_name(arguments["name"])
        kind, modifiers = scene.types[target], scene.modifiers[target]
        capable = target in scene.material_capable
        scene.forget(target)
        scene.add(new_name, kind, modifiers, capable)
    elif action_type == "set_material":
        if target not in scene.material_capable:
            raise PlanValidationError("object type does not support materials")
        material_name = catalog.check_name(arguments["name"])
        if material_name in scene.materials:
            raise PlanValidationError(f"material name is already in use: {material_name}")
        scene.materials.add(material_name)


class StepFailed(RuntimeError):
    """A step could not run or did not have its approved effect. ``number`` is 1-based."""

    def __init__(self, number, message):
        super().__init__(f"step {number}: {message}")
        self.number = number


def execute_plan(payload, context, allowed_objects=None, on_step=None):
    """Execute and verify a plan. Call only inside an UNDO Blender operator.

    ``on_step(number, result)`` is called after each verified step.
    """
    validate_plan(payload, context, allowed_objects)
    results = []
    for number, action in enumerate(payload["actions"], 1):
        runner = _RUNNERS[action["type"]]
        try:
            before = _snapshot(action, context)
            result = runner(action["arguments"], context)
            _verify_action(action, result, context, before)
        except StepFailed:
            raise
        except (AttributeError, KeyError, RuntimeError, TypeError, ValueError) as exc:
            raise StepFailed(number, str(exc)) from exc
        results.append(result)
        if on_step is not None:
            on_step(number, result)
    return results


def primitive_operator_arguments(arguments):
    """Return complete, context-independent bpy operator arguments for a primitive."""
    primitive = arguments["primitive"]
    kwargs = copy.deepcopy(_PRIMITIVE_OPERATOR_ARGUMENTS[primitive])
    kwargs["location"] = list(arguments.get("location", (0.0, 0.0, 0.0)))
    if primitive != "torus":
        kwargs["scale"] = list(arguments.get("scale", (1.0, 1.0, 1.0)))
    return kwargs


def _object(context, name):
    obj = context.scene.objects.get(name)
    if obj is None:
        raise RuntimeError(f"{name} is not in the scene")
    return obj


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
    operation = operators[primitive](**primitive_operator_arguments(arguments))
    if primitive == "torus" and context.mode != 'OBJECT':
        bpy.ops.object.mode_set(mode='OBJECT')
    obj = context.active_object
    if 'FINISHED' not in operation or obj is None or context.scene.objects.get(obj.name) is None:
        raise RuntimeError(f"Blender could not create {primitive}")
    if "name" in arguments:
        obj.name = arguments["name"]
        if obj.name != arguments["name"]:
            raise RuntimeError("Blender changed the approved object name")
    if "scale" in arguments:
        obj.scale = arguments["scale"]
    return {"type": "create_primitive", "object": obj.name}


def _transform_object(arguments, context):
    obj = _object(context, arguments["object"])
    if "location" in arguments:
        obj.location = arguments["location"]
    if "rotation" in arguments:
        obj.rotation_euler = arguments["rotation"]
    if "scale" in arguments:
        obj.scale = arguments["scale"]
    return {"type": "transform_object", "object": obj.name}


def _noise_texture(scale):
    texture = bpy.data.textures.new(_unique_name("Redrob Shape Noise", bpy.data.textures.keys()), type='CLOUDS')
    texture.noise_scale = scale
    return texture


def _add_modifier(arguments, context):
    obj = _object(context, arguments["object"])
    kind = arguments["modifier"]
    modifier = obj.modifiers.new(name=modifier_name(kind), type=_MODIFIER_TYPES[kind])
    if modifier is None:
        raise RuntimeError(f"{obj.name} cannot take a {kind} modifier")
    if kind == "bevel":
        modifier.width = arguments["width"]
        modifier.segments = arguments["segments"]
    elif kind == "subdivision":
        modifier.levels = arguments["levels"]
        modifier.render_levels = arguments.get("render_levels", arguments["levels"])
    elif kind == "solidify":
        modifier.thickness = arguments["thickness"]
    elif kind == "mirror":
        axis = arguments["axis"]
        modifier.use_axis = (axis == "x", axis == "y", axis == "z")
    elif kind == "array":
        modifier.count = arguments["count"]
        modifier.use_relative_offset = True
        modifier.relative_offset_displace = arguments["offset"]
    elif kind in ("twist", "bend", "taper", "stretch"):
        modifier.deform_method = kind.upper()
        modifier.deform_axis = arguments["axis"].upper()
        if kind in ("twist", "bend"):
            modifier.angle = math.radians(arguments["angle"])
        else:
            modifier.factor = arguments["factor"]
    elif kind == "displace":
        modifier.strength = arguments["strength"]
        modifier.texture = _noise_texture(arguments["noise_scale"])
    elif kind == "smooth":
        modifier.factor = arguments["factor"]
        modifier.iterations = arguments["iterations"]
    return {"type": "add_modifier", "object": obj.name, "modifier": modifier.name}


def _boolean(arguments, context):
    obj = _object(context, arguments["object"])
    other = _object(context, arguments["with"])
    modifier = obj.modifiers.new(name=modifier_name("boolean"), type='BOOLEAN')
    modifier.object = other
    modifier.operation = arguments["operation"].upper()
    if arguments["hide_cutter"]:
        other.display_type = 'WIRE'
        other.hide_render = True
    return {"type": "boolean", "object": obj.name, "modifier": modifier.name}


def _mesh_counts(obj):
    mesh = obj.data
    return (len(mesh.vertices), len(mesh.edges), len(mesh.polygons))


def _edit_mesh(obj, edit):
    import bmesh

    if obj.data.users > 1:
        raise RuntimeError(f"{obj.name} shares its mesh with other objects; duplicate it first")
    bm = bmesh.new()
    try:
        bm.from_mesh(obj.data)
        bm.faces.ensure_lookup_table()
        edit(bm)
        bm.normal_update()
        bm.to_mesh(obj.data)
    finally:
        bm.free()
    obj.data.update()


def _faces_along(bm, obj, direction):
    from mathutils import Vector

    normal_matrix = obj.matrix_world.to_3x3().inverted_safe().transposed()
    axis = Vector(_FACE_AXES[direction])
    limit = math.cos(math.radians(catalog.FACE_DEGREES)) - 1e-6
    faces = []
    for face in bm.faces:
        world = normal_matrix @ face.normal
        if world.length > 0 and world.normalized().dot(axis) >= limit:
            faces.append(face)
    if not faces:
        raise RuntimeError(f"no faces of {obj.name} point along {direction}")
    return faces


def _extrude_faces(arguments, context):
    import bmesh
    from mathutils import Vector

    obj = _object(context, arguments["object"])

    def edit(bm):
        faces = _faces_along(bm, obj, arguments["faces"])
        extruded = bmesh.ops.extrude_face_region(bm, geom=faces)
        bmesh.ops.delete(bm, geom=faces, context='FACES')
        verts = [element for element in extruded["geom"] if isinstance(element, bmesh.types.BMVert)]
        world = Vector(_FACE_AXES[arguments["faces"]]) * arguments["distance"]
        local = obj.matrix_world.to_3x3().inverted_safe() @ world
        bmesh.ops.translate(bm, vec=local, verts=verts)

    _edit_mesh(obj, edit)
    return {"type": "extrude_faces", "object": obj.name}


def _inset_faces(arguments, context):
    import bmesh

    obj = _object(context, arguments["object"])

    def edit(bm):
        if arguments["faces"] == "all":
            bmesh.ops.inset_individual(
                bm, faces=list(bm.faces), thickness=arguments["thickness"], depth=arguments["depth"]
            )
        else:
            faces = _faces_along(bm, obj, arguments["faces"])
            bmesh.ops.inset_region(bm, faces=faces, thickness=arguments["thickness"], depth=arguments["depth"])

    _edit_mesh(obj, edit)
    return {"type": "inset_faces", "object": obj.name}


def _subdivide_mesh(arguments, context):
    import bmesh

    obj = _object(context, arguments["object"])

    def edit(bm):
        bmesh.ops.subdivide_edges(
            bm,
            edges=list(bm.edges),
            cuts=arguments["cuts"],
            use_grid_fill=True,
            smooth=arguments["smoothness"],
        )

    _edit_mesh(obj, edit)
    return {"type": "subdivide_mesh", "object": obj.name}


def _shade_smooth(arguments, context):
    obj = _object(context, arguments["object"])
    mesh = obj.data
    if arguments["smooth"]:
        mesh.shade_smooth()
        if "angle" in arguments:
            mesh.set_sharp_from_angle(angle=math.radians(arguments["angle"]))
    else:
        mesh.shade_flat()
    mesh.update()
    return {"type": "shade_smooth", "object": obj.name}


def _apply_modifier(arguments, context):
    obj = _object(context, arguments["object"])
    if obj.data.users > 1:
        raise RuntimeError(f"{obj.name} shares its mesh with other objects; duplicate it first")
    with context.temp_override(object=obj, active_object=obj, selected_objects=[obj]):
        operation = bpy.ops.object.modifier_apply(modifier=arguments["modifier"])
    if 'FINISHED' not in operation:
        raise RuntimeError(f"Blender could not apply {arguments['modifier']} to {obj.name}")
    return {"type": "apply_modifier", "object": obj.name}


def _duplicate_object(arguments, context):
    source = _object(context, arguments["object"])
    copy_object = source.copy()
    if source.data is not None:
        copy_object.data = source.data.copy()
    collections = list(source.users_collection) or [context.scene.collection]
    for collection in collections:
        collection.objects.link(copy_object)
    copy_object.name = arguments["name"]
    if copy_object.name != arguments["name"]:
        raise RuntimeError("Blender changed the approved object name")
    copy_object.location = [a + b for a, b in zip(source.location, arguments["offset"])]
    return {"type": "duplicate_object", "object": copy_object.name, "source": source.name}


def _delete_object(arguments, context):
    obj = _object(context, arguments["object"])
    name = obj.name
    bpy.data.objects.remove(obj, do_unlink=True)
    return {"type": "delete_object", "object": name}


def _rename_object(arguments, context):
    obj = _object(context, arguments["object"])
    obj.name = arguments["name"]
    if obj.name != arguments["name"]:
        raise RuntimeError("Blender changed the approved object name")
    return {"type": "rename_object", "object": obj.name}


def _set_material(arguments, context):
    obj = _object(context, arguments["object"])
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
        if "Metallic" in principled.inputs:
            principled.inputs["Metallic"].default_value = arguments.get("metallic", 0.0)
        if "Roughness" in principled.inputs:
            principled.inputs["Roughness"].default_value = arguments.get("roughness", 0.5)
    materials = obj.data.materials
    if len(obj.material_slots) == 0:
        materials.append(material)
    else:
        obj.material_slots[obj.active_material_index].material = material
    return {"type": "set_material", "object": obj.name, "material": material.name}


_RUNNERS = {
    "create_primitive": _create_primitive,
    "transform_object": _transform_object,
    "add_modifier": _add_modifier,
    "boolean": _boolean,
    "extrude_faces": _extrude_faces,
    "inset_faces": _inset_faces,
    "subdivide_mesh": _subdivide_mesh,
    "shade_smooth": _shade_smooth,
    "apply_modifier": _apply_modifier,
    "duplicate_object": _duplicate_object,
    "delete_object": _delete_object,
    "rename_object": _rename_object,
    "set_material": _set_material,
}


def _snapshot(action, context):
    """What a step's check compares against: the target's state just before it runs."""
    obj = context.scene.objects.get(action["arguments"].get("object", ""))
    if obj is None or obj.type != 'MESH':
        return None
    return {"counts": _mesh_counts(obj), "modifiers": [m.name for m in obj.modifiers]}


def _vectors_match(actual, expected, tolerance=1e-5):
    return all(
        math.isclose(float(left), float(right), rel_tol=0.0, abs_tol=tolerance)
        for left, right in zip(actual, expected)
    )


def _close(actual, expected, tolerance=1e-5):
    return math.isclose(float(actual), float(expected), rel_tol=0.0, abs_tol=tolerance)


def _verify_modifier(obj, arguments, result):
    kind = arguments["modifier"]
    modifier = obj.modifiers.get(result["modifier"])
    if modifier is None or modifier.type != _MODIFIER_TYPES[kind]:
        raise RuntimeError(f"modifier verification failed for {obj.name}")
    checks = {
        "bevel": lambda: _close(modifier.width, arguments["width"]) and modifier.segments == arguments["segments"],
        "subdivision": lambda: modifier.levels == arguments["levels"]
        and modifier.render_levels == arguments.get("render_levels", arguments["levels"]),
        "solidify": lambda: _close(modifier.thickness, arguments["thickness"]),
        "mirror": lambda: tuple(modifier.use_axis) == tuple(a == arguments["axis"] for a in "xyz"),
        "array": lambda: modifier.count == arguments["count"]
        and _vectors_match(modifier.relative_offset_displace, arguments["offset"]),
        "twist": lambda: _close(modifier.angle, math.radians(arguments["angle"]))
        and modifier.deform_axis == arguments["axis"].upper(),
        "bend": lambda: _close(modifier.angle, math.radians(arguments["angle"]))
        and modifier.deform_axis == arguments["axis"].upper(),
        "taper": lambda: _close(modifier.factor, arguments["factor"]) and modifier.deform_axis == arguments["axis"].upper(),
        "stretch": lambda: _close(modifier.factor, arguments["factor"])
        and modifier.deform_axis == arguments["axis"].upper(),
        "displace": lambda: _close(modifier.strength, arguments["strength"]) and modifier.texture is not None,
        "smooth": lambda: _close(modifier.factor, arguments["factor"]) and modifier.iterations == arguments["iterations"],
    }
    if not checks[kind]():
        raise RuntimeError(f"{kind} settings verification failed for {obj.name}")
    if kind in ("twist", "bend", "taper", "stretch") and modifier.deform_method != kind.upper():
        raise RuntimeError(f"{kind} method verification failed for {obj.name}")


def _verify_action(action, result, context, before=None):
    arguments = action["arguments"]
    action_type = action["type"]
    if action_type == "delete_object":
        if context.scene.objects.get(result["object"]) is not None:
            raise RuntimeError(f"{result['object']} is still in the scene")
        return
    obj = _scene_object(context, result["object"])
    if action_type == "create_primitive":
        if "name" in arguments and obj.name != arguments["name"]:
            raise RuntimeError("created object name verification failed")
        if not _vectors_match(obj.location, arguments.get("location", (0.0, 0.0, 0.0))):
            raise RuntimeError("created object location verification failed")
        if not _vectors_match(obj.rotation_euler, (0.0, 0.0, 0.0)):
            raise RuntimeError("created object rotation verification failed")
        if not _vectors_match(obj.scale, arguments.get("scale", (1.0, 1.0, 1.0)), tolerance=0.0):
            raise RuntimeError("created object scale verification failed")
        expected_vertices, expected_polygons = _PRIMITIVE_TOPOLOGY[arguments["primitive"]]
        if len(obj.data.vertices) != expected_vertices or len(obj.data.polygons) != expected_polygons:
            raise RuntimeError("created primitive topology verification failed")
    elif action_type == "transform_object":
        for field, actual in (("location", obj.location), ("rotation", obj.rotation_euler), ("scale", obj.scale)):
            tolerance = 0.0 if field == "scale" else 1e-5
            if field in arguments and not _vectors_match(actual, arguments[field], tolerance=tolerance):
                raise RuntimeError(f"{field} verification failed for {obj.name}")
    elif action_type == "add_modifier":
        _verify_modifier(obj, arguments, result)
    elif action_type == "boolean":
        modifier = obj.modifiers.get(result["modifier"])
        if (
            modifier is None
            or modifier.type != 'BOOLEAN'
            or modifier.object is None
            or modifier.object.name != arguments["with"]
            or modifier.operation != arguments["operation"].upper()
        ):
            raise RuntimeError(f"boolean verification failed for {obj.name}")
    elif action_type in ("extrude_faces", "inset_faces", "subdivide_mesh"):
        vertices, _, faces = _mesh_counts(obj)
        old_vertices, _, old_faces = before["counts"]
        grew = vertices > old_vertices and (faces > old_faces or action_type == "extrude_faces")
        if not grew:
            raise RuntimeError(f"{action_type.replace('_', ' ')} did not change {obj.name}")
    elif action_type == "shade_smooth":
        if any(polygon.use_smooth != arguments["smooth"] for polygon in obj.data.polygons):
            raise RuntimeError(f"shading verification failed for {obj.name}")
    elif action_type == "apply_modifier":
        if obj.modifiers.get(arguments["modifier"]) is not None:
            raise RuntimeError(f"{arguments['modifier']} is still a modifier on {obj.name}")
    elif action_type == "duplicate_object":
        source = _scene_object(context, result["source"])
        if source.data is not None and obj.data == source.data:
            raise RuntimeError(f"{obj.name} shares its mesh with {source.name}")
        expected = [a + b for a, b in zip(source.location, arguments["offset"])]
        if not _vectors_match(obj.location, expected):
            raise RuntimeError(f"duplicate location verification failed for {obj.name}")
    elif action_type == "rename_object":
        if obj.name != arguments["name"]:
            raise RuntimeError(f"rename verification failed for {obj.name}")
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
        if base_color is None or not _vectors_match(base_color.default_value, arguments["base_color"]):
            raise RuntimeError(f"material shader color verification failed for {obj.name}")
        for field, socket_name in (("metallic", "Metallic"), ("roughness", "Roughness")):
            if field not in arguments:
                continue
            socket = principled.inputs.get(socket_name)
            if socket is None or not _close(socket.default_value, arguments[field]):
                raise RuntimeError(f"material {field} verification failed for {obj.name}")
