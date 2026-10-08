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
from redrob_shape import conversation, i18n, runtime, theme, tools


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
    current.steps.clear()
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


def _review(payload, scope='SCENE'):
    """Put a plan up for review the way a planning answer does: normalized, checked, one row per step."""
    current = bpy.context.window_manager.redrob_shape
    summary, revision = scene_context.build_scene_context(bpy.context, scope)
    allowed = scene_context.target_objects(summary)
    normalized = tools.normalize_plan(payload, bpy.context)
    tools.validate_plan(normalized, bpy.context, allowed)
    runtime.show_plan(current, normalized)
    current.allowed_objects_json = json.dumps(sorted(allowed))
    current.scene_revision = revision
    current.plan_scope = scope
    current.history_json = conversation.dump(conversation.add_turn([], "Shape it", payload))
    current.status = 'READY'
    return normalized


def _cube(name, location=(0.0, 0.0, 0.0)):
    bpy.ops.mesh.primitive_cube_add(enter_editmode=False, location=location)
    obj = bpy.context.active_object
    obj.name = name
    return obj


def _counts(name):
    mesh = bpy.data.objects[name].data
    return len(mesh.vertices), len(mesh.polygons)


def _test_every_shaping_tool_runs_and_one_undo_restores():
    _clear_objects()
    _cube("Block")
    _cube("Cutter", location=(1.0, 0.0, 0.0))
    _cube("Spare", location=(0.0, 4.0, 0.0))
    _cube("Doomed", location=(0.0, -4.0, 0.0))
    steps = [
        ("extrude_faces", {"object": "Block", "faces": "+z", "distance": 0.5}),
        ("inset_faces", {"object": "Block", "faces": "-z", "thickness": 0.2}),
        ("subdivide_mesh", {"object": "Spare", "cuts": 2}),
        ("shade_smooth", {"object": "Spare", "smooth": True, "angle": 40}),
        ("boolean", {"object": "Block", "with": "Cutter", "operation": "difference"}),
        ("add_modifier", {"object": "Block", "modifier": "bevel", "width": 0.05, "segments": 2}),
        ("apply_modifier", {"object": "Block", "modifier": "Redrob Shape Bevel"}),
        ("add_modifier", {"object": "Spare", "modifier": "array", "count": 3}),
        ("add_modifier", {"object": "Spare", "modifier": "twist", "angle": 45}),
        ("add_modifier", {"object": "Spare", "modifier": "bend", "angle": 30, "axis": "x"}),
        ("add_modifier", {"object": "Spare", "modifier": "taper", "factor": 0.5}),
        ("add_modifier", {"object": "Spare", "modifier": "stretch", "factor": 0.2}),
        ("add_modifier", {"object": "Spare", "modifier": "displace", "strength": 0.1}),
        ("add_modifier", {"object": "Spare", "modifier": "smooth", "factor": 0.5, "iterations": 3}),
        ("duplicate_object", {"object": "Spare", "name": "Spare Twin", "offset": [3.0, 0.0, 0.0]}),
        ("rename_object", {"object": "Spare Twin", "name": "Twin"}),
        ("delete_object", {"object": "Doomed"}),
    ]
    payload = {"summary": "Every tool", "actions": [{"type": t, "arguments": a} for t, a in steps]}
    _review(payload)
    current = bpy.context.window_manager.redrob_shape
    baseline = _push_background_undo_baseline("Before every shaping tool")
    assert bpy.ops.redrob_shape.apply_plan() == {'FINISHED'}, current.status_message
    assert current.status == 'APPLIED', current.status_message
    assert [step.outcome for step in current.steps] == ["done"] * len(steps)

    # Extrude +z on a cube: 4 new vertices and 4 side walls (12 / 10), then inset -z adds 4 and 4.
    # The boolean and bevel were applied into the mesh? Only the bevel: the boolean stays a modifier.
    block = bpy.data.objects["Block"]
    assert [m.type for m in block.modifiers] == ['BOOLEAN'], [m.name for m in block.modifiers]
    assert block.modifiers[0].object.name == "Cutter"
    assert bpy.data.objects["Cutter"].display_type == 'WIRE' and bpy.data.objects["Cutter"].hide_render
    assert _counts("Block")[0] > 16  # bevel baked in
    spare = bpy.data.objects["Spare"]
    assert _counts("Spare") == (56, 54), _counts("Spare")  # 2 cuts per edge, grid-filled: 3x3 per face
    assert all(polygon.use_smooth for polygon in spare.data.polygons)
    kinds = [(m.type, getattr(m, "deform_method", None)) for m in spare.modifiers]
    assert kinds == [
        ('ARRAY', None),
        ('SIMPLE_DEFORM', 'TWIST'),
        ('SIMPLE_DEFORM', 'BEND'),
        ('SIMPLE_DEFORM', 'TAPER'),
        ('SIMPLE_DEFORM', 'STRETCH'),
        ('DISPLACE', None),
        ('SMOOTH', None),
    ], kinds
    twin = bpy.data.objects["Twin"]
    assert twin.data != spare.data and tuple(twin.location) == (3.0, 4.0, 0.0)
    assert len(twin.modifiers) == len(spare.modifiers)
    assert bpy.data.objects.get("Doomed") is None and bpy.data.objects.get("Spare Twin") is None
    turns = conversation.load(current.history_json)
    assert turns[-1]["outcome"] == "applied" and turns[-1]["steps"] == len(steps)

    _undo_background_step("Every shaping tool", baseline)
    assert bpy.data.objects.get("Doomed") is not None and bpy.data.objects.get("Twin") is None
    assert _counts("Block") == (8, 6) and not bpy.data.objects["Block"].modifiers


def _test_extrude_counts():
    _clear_objects()
    _cube("Box")
    _review({"summary": "Extrude", "actions": [
        {"type": "extrude_faces", "arguments": {"object": "Box", "faces": "+z", "distance": 1.0}},
    ]})
    assert bpy.ops.redrob_shape.apply_plan() == {'FINISHED'}
    assert _counts("Box") == (12, 10), _counts("Box")
    top = max(v.co.z for v in bpy.data.objects["Box"].data.vertices)
    assert abs(top - 2.0) < 1e-5, top


def _test_switched_off_steps_and_their_dependencies():
    _test_extrude_counts()
    _clear_objects()
    _cube("Box")
    _review({"summary": "Two steps", "actions": [
        {"type": "create_primitive", "arguments": {"primitive": "sphere", "name": "Ball"}},
        {"type": "add_modifier", "arguments": {"object": "Ball", "modifier": "subdivision", "levels": 1}},
        {"type": "transform_object", "arguments": {"object": "Box", "location": [0.0, 0.0, 3.0]}},
    ]})
    current = bpy.context.window_manager.redrob_shape
    current.steps[0].enabled = False  # the update callback re-checks what is left
    assert "Ball" in current.plan_problem, current.plan_problem
    assert not redrob_shape.operators.REDROB_OT_apply_plan.poll(bpy.context)
    current.steps[1].enabled = False
    assert current.plan_problem == ""
    assert bpy.ops.redrob_shape.apply_plan() == {'FINISHED'}
    assert bpy.data.objects.get("Ball") is None
    assert tuple(bpy.data.objects["Box"].location) == (0.0, 0.0, 3.0)
    assert [step.outcome for step in current.steps] == ["", "", "done"]
    assert bpy.ops.redrob_shape.keep() == {'FINISHED'}
    assert current.status == 'IDLE' and len(current.steps) == 0


def _test_undo_and_redo_move_the_review_state():
    _clear_objects()
    _cube("Box")
    _review({"summary": "Round it", "actions": [
        {"type": "add_modifier", "arguments": {"object": "Box", "modifier": "bevel", "width": 0.1, "segments": 3}},
    ]})
    current = bpy.context.window_manager.redrob_shape
    bpy.ops.ed.undo_push(message="Before rounding")
    assert bpy.ops.redrob_shape.apply_plan() == {'FINISHED'}
    assert current.status == 'APPLIED'
    bpy.ops.ed.undo_push(message="Rounded")
    assert bpy.ops.ed.undo() == {'FINISHED'}
    current = bpy.context.window_manager.redrob_shape
    assert current.status == 'READY', current.status
    assert not bpy.data.objects["Box"].modifiers
    assert conversation.load(current.history_json)[-1]["outcome"] == "undone"
    assert len(current.steps) == 1 and current.steps[0].outcome == ""
    assert bpy.ops.ed.redo() == {'FINISHED'}
    current = bpy.context.window_manager.redrob_shape
    assert current.status == 'APPLIED', current.status
    assert conversation.load(current.history_json)[-1]["outcome"] == "applied"
    # Undo again, then apply the same plan a second time: the review state allows it.
    assert bpy.ops.ed.undo() == {'FINISHED'}
    assert bpy.ops.redrob_shape.apply_plan() == {'FINISHED'}
    assert bpy.data.objects["Box"].modifiers[0].type == 'BEVEL'
    assert bpy.ops.redrob_shape.clear() == {'FINISHED'}
    assert bpy.context.window_manager.redrob_shape.status == 'IDLE'


def _test_selection_scope_limits_the_plan():
    _clear_objects()
    chosen = _cube("Chosen")
    _cube("Other", location=(3.0, 0.0, 0.0))
    bpy.ops.object.select_all(action='DESELECT')
    chosen.select_set(True)
    bpy.context.view_layer.objects.active = chosen
    summary, _ = scene_context.build_scene_context(bpy.context, 'SELECTION')
    assert summary["target"] == ["Chosen"]
    assert scene_context.target_objects(summary) == {"Chosen"}
    try:
        _review({"summary": "Outside", "actions": [
            {"type": "transform_object", "arguments": {"object": "Other", "location": [0.0, 0.0, 1.0]}},
        ]}, scope='SELECTION')
    except tools.PlanValidationError:
        pass
    else:
        raise AssertionError("a plan touching an unselected object was accepted in selection scope")
    whole, _ = scene_context.build_scene_context(bpy.context, 'SCENE')
    assert "target" not in whole and scene_context.target_objects(whole) == {"Chosen", "Other"}


def _test_korean_interface():
    view = bpy.context.preferences.view
    saved = (view.language, view.use_translate_interface)
    try:
        view.language = 'ko_KR'
        view.use_translate_interface = True
        assert bpy.app.translations.pgettext_iface("Create Plan") == "계획 만들기"
        assert bpy.app.translations.pgettext_iface("Applied {} steps • look at the result, then keep or undo it").format(3) \
            .startswith("3단계")
        assert i18n.iface_("Undo") == "실행 취소"
    finally:
        view.language, view.use_translate_interface = saved
    assert bpy.app.translations.pgettext_iface("Create Plan") == "Create Plan"


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
        _test_every_shaping_tool_runs_and_one_undo_restores()
        _test_switched_off_steps_and_their_dependencies()
        _test_undo_and_redo_move_the_review_state()
        _test_selection_scope_limits_the_plan()
        _test_korean_interface()
        _test_sidecar_lifecycle()
    finally:
        if registered_here:
            redrob_shape.unregister()
    print("Redrob Shape Blender integration checks passed")


if __name__ == "__main__":
    main()
