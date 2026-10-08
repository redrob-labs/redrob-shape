# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Headless Blender integration checks.

Run with:
  blender --background --factory-startup --python blender_integration.py
"""

import json
import os
import sys
import tempfile
import time
from pathlib import Path

import bpy

_ADDONS_CORE = Path(__file__).resolve().parents[2]
if str(_ADDONS_CORE) not in sys.path:
    sys.path.insert(0, str(_ADDONS_CORE))

import redrob_shape
from redrob_shape import context as scene_context
from redrob_shape import runtime, theme, tools


def _clear_objects():
    if bpy.context.mode != 'OBJECT':
        bpy.ops.object.mode_set(mode='OBJECT')
    bpy.ops.object.select_all(action='SELECT')
    bpy.ops.object.delete(use_global=False)


def _set_plan(payload):
    current = bpy.context.window_manager.redrob_shape
    summary, revision = scene_context.build_scene_context(bpy.context)
    normalized = tools.normalize_plan(payload, bpy.context)
    tools.validate_plan(normalized, bpy.context, {item["name"] for item in summary["objects"]})
    current.plan_json = json.dumps(normalized, separators=(",", ":"))
    current.allowed_objects_json = json.dumps([item["name"] for item in summary["objects"]])
    current.scene_revision = revision
    current.status = 'READY'
    return normalized


def _undo_signature():
    _, revision = scene_context.build_scene_context(bpy.context)
    return {
        "revision": revision,
        "objects": tuple(sorted(bpy.data.objects.keys())),
        "meshes": tuple(sorted(bpy.data.meshes.keys())),
        "materials": tuple(sorted(bpy.data.materials.keys())),
    }


def _push_background_undo_baseline(message):
    baseline = _undo_signature()
    assert bpy.ops.ed.undo_push(message=message) == {'FINISHED'}
    return baseline


def _undo_background_step(message, baseline):
    # Background mode has no UI operator context to materialize automatic Undo checkpoints.
    # Explicitly bracket the operation with memfile snapshots and verify the exact prior state.
    assert bpy.ops.ed.undo_push(message=message) == {'FINISHED'}
    assert bpy.ops.ed.undo() == {'FINISHED'}
    assert _undo_signature() == baseline


def _test_registration_preview_apply_and_undo():
    assert hasattr(bpy.context.window_manager, "redrob_shape")
    assert {'REGISTER', 'UNDO'} <= redrob_shape.operators.REDROB_OT_apply_plan.bl_options
    _clear_objects()
    plan = {
        "summary": "Create an exact cube and material",
        "actions": [
            {
                "type": "create_primitive",
                "arguments": {
                    "primitive": "cube",
                    "name": "Redrob Shape Integration Cube",
                    "location": [1.25, -2.5, 3.75],
                    "scale": [1.0, 2.0, 0.5],
                },
            },
            {
                "type": "set_material",
                "arguments": {
                    "object": "Redrob Shape Integration Cube",
                    "name": "Redrob Shape Integration Material",
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
    baseline = _push_background_undo_baseline("Before Redrob Shape integration apply")
    assert bpy.ops.redrob_shape.apply_plan() == {'FINISHED'}
    obj = bpy.data.objects["Redrob Shape Integration Cube"]
    assert tuple(obj.location) == (1.25, -2.5, 3.75)
    assert tuple(obj.scale) == (1.0, 2.0, 0.5)
    material = bpy.data.materials["Redrob Shape Integration Material"]
    assert obj.active_material == material
    _undo_background_step("Redrob Shape integration apply", baseline)
    assert bpy.data.objects.get("Redrob Shape Integration Cube") is None

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
                        "name": "Redrob Shape Integration Torus",
                        "location": [0.0, 0.0, 0.0],
                        "scale": [0.5, 1.0, 2.0],
                    },
                }
            ],
        }
        _set_plan(torus_plan)
        baseline = _push_background_undo_baseline("Before Redrob Shape integration torus")
        assert bpy.ops.redrob_shape.apply_plan() == {'FINISHED'}
        assert bpy.context.mode == 'OBJECT'
        assert tuple(bpy.data.objects["Redrob Shape Integration Torus"].scale) == (0.5, 1.0, 2.0)
        _undo_background_step("Redrob Shape integration torus", baseline)
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
                    "name": "Redrob Shape Partial Plane",
                    "location": [0.0, 0.0, 0.0],
                    "scale": [1.0, 1.0, 1.0],
                },
            }
        ],
    }
    _set_plan(plan)
    baseline = _push_background_undo_baseline("Before Redrob Shape integration partial failure")
    original_verify = tools._verify_action

    def fail_verification(*_args):
        raise RuntimeError("intentional integration-test verification failure")

    tools._verify_action = fail_verification
    try:
        try:
            bpy.ops.redrob_shape.apply_plan()
        except RuntimeError as exc:
            assert "intentional integration-test verification failure" in str(exc)
        else:
            raise AssertionError("partial apply failure was not surfaced")
    finally:
        tools._verify_action = original_verify
    assert bpy.data.objects.get("Redrob Shape Partial Plane") is not None
    _undo_background_step("Redrob Shape integration partial failure", baseline)
    assert bpy.data.objects.get("Redrob Shape Partial Plane") is None


def _test_scene_scope_and_theme_round_trip():
    _clear_objects()
    other_scene = bpy.data.scenes.new("Redrob Shape Other Scene")
    mesh = bpy.data.meshes.new("Redrob Shape Other Mesh")
    other_object = bpy.data.objects.new("Redrob Shape Other Object", mesh)
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
    data_material = bpy.data.materials.new("Redrob Shape Data Slot")
    object_material_a = bpy.data.materials.new("Redrob Shape Object Slot A")
    object_material_b = bpy.data.materials.new("Redrob Shape Object Slot B")
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
    with tempfile.TemporaryDirectory(prefix="redrob-shape-theme-") as temporary:
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


def _test_sidecar_lifecycle():
    agent = runtime.runtime()
    current = bpy.context.window_manager.redrob_shape
    executable_name = "redrob-shape-agent.exe" if sys.platform == "win32" else "redrob-shape-agent"
    packaged_executable = Path(bpy.app.binary_path).resolve().parent / executable_name
    absent = object()
    saved_environment = {
        name: os.environ.pop(name, absent) for name in ("REDROB_AGENT_BIN", "REDROB_API_KEY")
    }

    try:
        assert not agent.running
        assert packaged_executable.is_file()
        assert agent.locate_executable() == packaged_executable
        current.status = 'IDLE'
        current.status_message = "Ready for a new instruction"
        current.sidecar_available = False

        assert agent.start(), agent.start_error
        assert agent.running
        deadline = time.monotonic() + 5.0
        while agent.requests and time.monotonic() < deadline:
            agent.drain(bpy.context)
            time.sleep(0.05)
        agent.drain(bpy.context)

        assert not agent.requests
        assert agent.running
        assert current.status == 'IDLE'
        # A developer machine may hold a key in its keychain; a clean one does not. Either way the
        # message must match the state the agent reported.
        assert current.status_message == runtime._idle_message(current.sidecar_available)
        assert current.connection in {'CONNECTED', 'DISCONNECTED'}

        process = agent.process
        agent.stop()
        assert not agent.running
        assert process is not None and process.returncode == 0
    finally:
        agent.stop(force=True)
        for name, value in saved_environment.items():
            if value is absent:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value


def main():
    registered_here = not hasattr(bpy.types.WindowManager, "redrob_shape")
    if registered_here:
        redrob_shape.register()
    try:
        bpy.ops.ed.undo_push(message="Initialize Redrob Shape integration test")
        _test_registration_preview_apply_and_undo()
        _test_partial_failure_remains_undoable()
        _test_scene_scope_and_theme_round_trip()
        _test_sidecar_lifecycle()
    finally:
        if registered_here:
            redrob_shape.unregister()
    print("Redrob Shape Blender integration checks passed")


if __name__ == "__main__":
    main()
