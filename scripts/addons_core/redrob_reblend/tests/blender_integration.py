# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Headless Blender integration checks.

Run with:
  blender --background --factory-startup --python blender_integration.py
"""

import json
import sys
import tempfile
from pathlib import Path

import bpy

_ADDONS_CORE = Path(__file__).resolve().parents[2]
if str(_ADDONS_CORE) not in sys.path:
    sys.path.insert(0, str(_ADDONS_CORE))

import redrob_reblend
from redrob_reblend import context as scene_context
from redrob_reblend import theme, tools


def _clear_objects():
    if bpy.context.mode != 'OBJECT':
        bpy.ops.object.mode_set(mode='OBJECT')
    bpy.ops.object.select_all(action='SELECT')
    bpy.ops.object.delete(use_global=False)


def _set_plan(payload):
    current = bpy.context.window_manager.redrob_reblend
    summary, revision = scene_context.build_scene_context(bpy.context)
    normalized = tools.normalize_plan(payload, bpy.context)
    tools.validate_plan(normalized, bpy.context, {item["name"] for item in summary["objects"]})
    current.plan_json = json.dumps(normalized, separators=(",", ":"))
    current.allowed_objects_json = json.dumps([item["name"] for item in summary["objects"]])
    current.scene_revision = revision
    current.status = 'READY'
    return normalized


def _test_registration_preview_apply_and_undo():
    assert hasattr(bpy.context.window_manager, "redrob_reblend")
    _clear_objects()
    plan = {
        "summary": "Create an exact cube and material",
        "actions": [
            {
                "type": "create_primitive",
                "arguments": {
                    "primitive": "cube",
                    "name": "Reblend Integration Cube",
                    "location": [1.25, -2.5, 3.75],
                    "scale": [1.0, 2.0, 0.5],
                },
            },
            {
                "type": "set_material",
                "arguments": {
                    "object": "Reblend Integration Cube",
                    "name": "Reblend Integration Material",
                    "base_color": [0.125, 0.25, 0.5, 1.0],
                    "metallic": 0.75,
                    "roughness": 0.2,
                },
            },
        ],
    }
    normalized = _set_plan(plan)
    preview = tools.preview_arguments(normalized["actions"][0])
    assert preview["size"] == 2.0 and preview["calc_uvs"] is True
    assert bpy.ops.redrob_reblend.apply_plan() == {'FINISHED'}
    obj = bpy.data.objects["Reblend Integration Cube"]
    assert tuple(obj.location) == (1.25, -2.5, 3.75)
    assert tuple(obj.scale) == (1.0, 2.0, 0.5)
    material = bpy.data.materials["Reblend Integration Material"]
    assert obj.active_material == material
    assert bpy.ops.ed.undo() == {'FINISHED'}
    assert bpy.data.objects.get("Reblend Integration Cube") is None

    enter_edit_mode = bpy.context.preferences.edit.use_enter_edit_mode
    bpy.context.preferences.edit.use_enter_edit_mode = True
    try:
        torus_plan = {
            "summary": "Create a deterministic torus",
            "actions": [
                {
                    "type": "create_primitive",
                    "arguments": {
                        "primitive": "torus",
                        "name": "Reblend Integration Torus",
                        "location": [0.0, 0.0, 0.0],
                        "scale": [0.5, 1.0, 2.0],
                    },
                }
            ],
        }
        _set_plan(torus_plan)
        assert bpy.ops.redrob_reblend.apply_plan() == {'FINISHED'}
        assert bpy.context.mode == 'OBJECT'
        assert tuple(bpy.data.objects["Reblend Integration Torus"].scale) == (0.5, 1.0, 2.0)
        assert bpy.ops.ed.undo() == {'FINISHED'}
    finally:
        bpy.context.preferences.edit.use_enter_edit_mode = enter_edit_mode


def _test_partial_failure_remains_undoable():
    _clear_objects()
    plan = {
        "summary": "Exercise failed postcondition rollback",
        "actions": [
            {
                "type": "create_primitive",
                "arguments": {
                    "primitive": "plane",
                    "name": "Reblend Partial Plane",
                    "location": [0.0, 0.0, 0.0],
                    "scale": [1.0, 1.0, 1.0],
                },
            }
        ],
    }
    _set_plan(plan)
    original_verify = tools._verify_action

    def fail_verification(*_args):
        raise RuntimeError("intentional integration-test verification failure")

    tools._verify_action = fail_verification
    try:
        assert bpy.ops.redrob_reblend.apply_plan() == {'FINISHED'}
    finally:
        tools._verify_action = original_verify
    assert bpy.data.objects.get("Reblend Partial Plane") is not None
    assert bpy.ops.ed.undo() == {'FINISHED'}
    assert bpy.data.objects.get("Reblend Partial Plane") is None


def _test_scene_scope_and_theme_round_trip():
    _clear_objects()
    other_scene = bpy.data.scenes.new("Reblend Other Scene")
    mesh = bpy.data.meshes.new("Reblend Other Mesh")
    other_object = bpy.data.objects.new("Reblend Other Object", mesh)
    other_scene.collection.objects.link(other_object)
    payload = {
        "summary": "Must reject another scene",
        "actions": [
            {
                "type": "transform_object",
                "arguments": {"object": other_object.name, "location": [1.0, 0.0, 0.0]},
            }
        ],
    }
    try:
        tools.validate_plan(payload, bpy.context, {other_object.name})
    except tools.PlanValidationError:
        pass
    else:
        raise AssertionError("cross-scene object was accepted")
    bpy.data.scenes.remove(other_scene)

    bpy.ops.mesh.primitive_cube_add(enter_editmode=False)
    slot_object = bpy.context.active_object
    data_material = bpy.data.materials.new("Reblend Data Slot")
    object_material_a = bpy.data.materials.new("Reblend Object Slot A")
    object_material_b = bpy.data.materials.new("Reblend Object Slot B")
    slot_object.data.materials.append(data_material)
    slot_object.material_slots[0].link = 'OBJECT'
    slot_object.material_slots[0].material = object_material_a
    _, object_slot_a_revision = scene_context.build_scene_context(bpy.context)
    slot_object.material_slots[0].material = object_material_b
    _, object_slot_b_revision = scene_context.build_scene_context(bpy.context)
    assert object_slot_a_revision != object_slot_b_revision

    blender_theme = bpy.context.preferences.themes[0]
    original = tuple(blender_theme.user_interface.wcol_regular.inner)
    original_snapshot_path = theme._snapshot_path
    with tempfile.TemporaryDirectory(prefix="redrob-reblend-theme-") as temporary:
        snapshot_directory = Path(temporary)

        def isolated_snapshot_path(create=False):
            if create:
                snapshot_directory.mkdir(parents=True, exist_ok=True)
            return snapshot_directory / "theme_snapshot.json"

        theme._THEME_SNAPSHOT.clear()
        theme._snapshot_path = isolated_snapshot_path
        try:
            assert theme.apply(blender_theme) > 0
            assert tuple(blender_theme.user_interface.wcol_regular.inner) != original
            assert theme.restore(blender_theme) > 0
            assert tuple(blender_theme.user_interface.wcol_regular.inner) == original
        finally:
            theme._snapshot_path = original_snapshot_path
            theme._THEME_SNAPSHOT.clear()


def main():
    registered_here = not hasattr(bpy.types.WindowManager, "redrob_reblend")
    if registered_here:
        redrob_reblend.register()
    try:
        _test_registration_preview_apply_and_undo()
        _test_partial_failure_remains_undoable()
        _test_scene_scope_and_theme_round_trip()
    finally:
        if registered_here:
            redrob_reblend.unregister()
    print("Redrob Reblend Blender integration checks passed")


if __name__ == "__main__":
    main()
