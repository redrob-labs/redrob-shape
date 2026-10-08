# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Operators for the Redrob Shape native workspace."""

import json

import bpy
from bpy.types import Operator

from . import context as scene_context
from . import runtime, theme, tools
from .state import session


class REDROB_OT_submit(Operator):
    bl_idname = "redrob_shape.submit"
    bl_label = "Create Plan"
    bl_description = "Ask Redrob to prepare a safe, reviewable scene plan"

    def execute(self, context):
        current = session(context)
        instruction = current.prompt.strip()
        if not instruction:
            self.report({'WARNING'}, "Enter an instruction first")
            return {'CANCELLED'}
        if not bpy.app.online_access:
            current.status = 'ERROR'
            current.status_message = "Online access is disabled in Blender preferences"
            self.report({'WARNING'}, current.status_message)
            return {'CANCELLED'}

        agent = runtime.runtime()
        if not agent.running and not agent.start():
            current.status = 'ERROR'
            current.status_message = agent.start_error
            self.report({'ERROR'}, agent.start_error)
            return {'CANCELLED'}

        summary, revision = scene_context.build_scene_context(context)
        try:
            request_id = agent.submit_plan(instruction, summary, revision)
        except (RuntimeError, ValueError) as exc:
            current.status = 'ERROR'
            current.status_message = str(exc)
            self.report({'ERROR'}, str(exc))
            return {'CANCELLED'}

        current.plan_json = ""
        current.allowed_objects_json = "[]"
        current.plan_summary = ""
        current.response_text = ""
        current.scene_revision = revision
        current.request_id = request_id
        current.status = 'WORKING'
        current.status_message = "Planning with model auto • no scene changes yet"
        return {'FINISHED'}


class REDROB_OT_cancel(Operator):
    bl_idname = "redrob_shape.cancel"
    bl_label = "Cancel"
    bl_description = "Cancel the current Redrob request"

    def execute(self, context):
        current = session(context)
        try:
            cancel_id = runtime.runtime().cancel(current.request_id)
        except RuntimeError as exc:
            current.status = 'ERROR'
            current.status_message = str(exc)
            self.report({'ERROR'}, str(exc))
            return {'CANCELLED'}
        if cancel_id is None:
            current.status = 'IDLE'
            current.status_message = "Request is no longer active"
            current.request_id = ""
        else:
            current.status_message = "Cancelling request"
        return {'FINISHED'}


class REDROB_OT_apply_plan(Operator):
    bl_idname = "redrob_shape.apply_plan"
    bl_label = "Apply Plan"
    bl_description = "Apply the reviewed plan as one undoable Blender operation"
    bl_options = {'REGISTER', 'UNDO'}  # noqa: RUF012

    @classmethod
    def poll(cls, context):
        current = session(context)
        return bool(current.plan_json) and current.status == 'READY'

    def execute(self, context):
        current = session(context)
        try:
            payload = json.loads(current.plan_json)
            allowed_values = json.loads(current.allowed_objects_json)
            if not isinstance(allowed_values, list) or not all(
                isinstance(value, str) for value in allowed_values
            ):
                raise tools.PlanValidationError("approved object scope is invalid")
            allowed_objects = set(allowed_values)
            _, revision = scene_context.build_scene_context(context)
            if revision != current.scene_revision:
                raise tools.PlanValidationError("scene changed after planning; create a new plan")
            tools.validate_plan(payload, context, allowed_objects)
        except (json.JSONDecodeError, TypeError, tools.PlanValidationError) as exc:
            current.status = 'ERROR'
            current.status_message = str(exc)
            self.report({'ERROR'}, str(exc))
            return {'CANCELLED'}

        current.status = 'APPLYING'
        current.status_message = "Applying approved actions as one undo step"
        try:
            results = tools.execute_plan(payload, context, allowed_objects)
        except (AttributeError, KeyError, RuntimeError, TypeError, ValueError) as exc:
            # Blender RNA/operator failures are surfaced without hiding the undo step.
            current.status = 'ERROR'
            current.status_message = f"Apply stopped: {exc}. Use Undo to roll back any partial changes."
            self.report({'ERROR'}, current.status_message)
            return {'FINISHED'}

        current.plan_json = ""
        current.allowed_objects_json = "[]"
        current.plan_summary = ""
        current.scene_revision = ""
        current.status = 'IDLE'
        current.status_message = f"Applied {len(results)} actions • Undo is available"
        current.response_text = current.status_message
        self.report({'INFO'}, current.status_message)
        return {'FINISHED'}


class REDROB_OT_clear(Operator):
    bl_idname = "redrob_shape.clear"
    bl_label = "Clear"
    bl_description = "Clear the current response and plan"

    def execute(self, context):
        current = session(context)
        if current.request_id:
            runtime.runtime().cancel(current.request_id)
        current.status = 'IDLE'
        current.status_message = "Ready for a new instruction"
        current.response_text = ""
        current.plan_json = ""
        current.allowed_objects_json = "[]"
        current.plan_summary = ""
        current.request_id = ""
        current.scene_revision = ""
        return {'FINISHED'}


class REDROB_OT_restart_agent(Operator):
    bl_idname = "redrob_shape.restart_agent"
    bl_label = "Reconnect Agent"
    bl_description = "Restart the local agent and re-check Redrob configuration"

    def execute(self, context):
        agent = runtime.runtime()
        agent.stop()
        current = session(context)
        if agent.start():
            current.status = 'IDLE'
            current.status_message = "Checking local agent configuration"
            return {'FINISHED'}
        current.status = 'ERROR'
        current.status_message = agent.start_error
        self.report({'ERROR'}, agent.start_error)
        return {'CANCELLED'}


class REDROB_OT_apply_theme(Operator):
    bl_idname = "redrob_shape.apply_theme"
    bl_label = "Apply Redrob Shape Theme"
    bl_description = "Apply the modern Redrob Shape visual preset to Blender's native UI"

    def execute(self, context):
        try:
            count = theme.apply(context.preferences.themes[0])
        except RuntimeError as exc:
            self.report({'ERROR'}, str(exc))
            return {'CANCELLED'}
        current = session(context)
        current.theme_active = count > 0
        self.report({'INFO'}, f"Applied {count} supported Redrob Shape theme values")
        return {'FINISHED'}


class REDROB_OT_restore_theme(Operator):
    bl_idname = "redrob_shape.restore_theme"
    bl_label = "Restore Previous Theme"
    bl_description = "Restore the theme values captured before applying Redrob Shape"

    def execute(self, context):
        try:
            count = theme.restore(context.preferences.themes[0])
        except RuntimeError as exc:
            self.report({'ERROR'}, str(exc))
            return {'CANCELLED'}
        current = session(context)
        current.theme_active = False
        if count:
            self.report({'INFO'}, f"Restored {count} theme values")
        else:
            self.report({'INFO'}, "No Redrob Shape theme changes to restore")
        return {'FINISHED'}


class REDROB_OT_open_console(Operator):
    bl_idname = "redrob_shape.open_console"
    bl_label = "Open Redrob Console"
    bl_description = "Open Redrob Console in the default browser"

    def execute(self, _context):
        bpy.ops.wm.url_open(url="https://console.redrob.ai/")
        return {'FINISHED'}


_CLASSES = (
    REDROB_OT_submit,
    REDROB_OT_cancel,
    REDROB_OT_apply_plan,
    REDROB_OT_clear,
    REDROB_OT_restart_agent,
    REDROB_OT_apply_theme,
    REDROB_OT_restore_theme,
    REDROB_OT_open_console,
)


def register():
    for cls in _CLASSES:
        bpy.utils.register_class(cls)


def unregister():
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
