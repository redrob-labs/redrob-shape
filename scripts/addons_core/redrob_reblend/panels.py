# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Modern native panels for the Reblend workspace."""

import json
import textwrap

import bpy
from bpy.types import Panel

from . import theme, tools
from .state import session

_STATUS_PRESENTATION = {
    'IDLE': ('CHECKMARK', "Ready"),
    'WORKING': ('TIME', "Thinking"),
    'READY': ('PREVIEW_RANGE', "Review Plan"),
    'APPLYING': ('FILE_REFRESH', "Applying"),
    'ERROR': ('ERROR', "Needs Attention"),
}


def _wrapped_labels(layout, text, width=42, limit=6):
    lines = []
    for paragraph in text.splitlines() or ("",):
        lines.extend(textwrap.wrap(paragraph, width=width) or [""])
    for line in lines[:limit]:
        layout.label(text=line)
    if len(lines) > limit:
        layout.label(text="…")


def _plan_actions(raw):
    try:
        payload = json.loads(raw)
    except (TypeError, ValueError):
        return []
    actions = payload.get("actions", []) if isinstance(payload, dict) else []
    return actions if isinstance(actions, list) else []


def _action_label(action):
    action_type = str(action.get("type", "Unknown")).replace("_", " ").title()
    arguments = action.get("arguments")
    if not isinstance(arguments, dict):
        return action_type
    target = arguments.get("object") or arguments.get("name") or arguments.get("primitive")
    return f"{action_type}  ·  {target}" if target else action_type


def _format_value(value):
    if isinstance(value, float):
        return repr(value)
    if isinstance(value, (list, tuple)):
        return "[" + ", ".join(_format_value(item) for item in value) + "]"
    if value is None:
        return "Default"
    return str(value)


def _draw_exact_action(layout, index, action):
    action_box = layout.box()
    action_box.label(text=f"{index}. {_action_label(action)}", icon='DOT')
    arguments = tools.preview_arguments(action)
    if not isinstance(arguments, dict):
        action_box.label(text="Invalid action arguments", icon='ERROR')
        return
    for name in sorted(arguments):
        value = arguments[name]
        if value is None:
            continue
        row = action_box.row(align=True)
        row.label(text=name.replace("_", " ").title())
        row.label(text=_format_value(value))
    if action.get("type") == "set_material":
        action_box.label(text="Creates a new single-user material", icon='MATERIAL')


class REDROB_PT_workspace(Panel):
    bl_idname = "REDROB_PT_workspace"
    bl_label = "Reblend"
    bl_category = "Reblend"
    bl_space_type = 'VIEW_3D'
    bl_region_type = 'UI'

    def draw_header(self, _context):
        self.layout.label(text="", icon='LIGHT')

    def draw(self, context):
        layout = self.layout
        current = session(context)
        icon, title = _STATUS_PRESENTATION.get(current.status, ('INFO', "Status"))

        status = layout.box()
        row = status.row(align=True)
        row.label(text=title, icon=icon)
        connection = "Connected" if current.sidecar_available else "Local"
        row.label(text=connection)
        status.label(text=current.status_message)

        prompt = layout.box()
        prompt.label(text="What do you want to make?", icon='GREASEPENCIL')
        prompt.prop(current, "prompt", text="")
        row = prompt.row(align=True)
        row.scale_y = 1.35
        submit = row.row(align=True)
        submit.enabled = current.status not in {'WORKING', 'APPLYING'}
        submit.operator("redrob_reblend.submit", text="Create Plan", icon='PLAY')
        if current.status == 'WORKING':
            row.operator("redrob_reblend.cancel", text="", icon='CANCEL')

        if current.response_text:
            response = layout.box()
            response.label(text="Response", icon='INFO')
            _wrapped_labels(response, current.response_text)

        actions = _plan_actions(current.plan_json)
        if actions:
            plan = layout.box()
            row = plan.row(align=True)
            row.label(text=current.plan_summary or "Proposed changes", icon='PREVIEW_RANGE')
            row.label(text=f"{len(actions)} actions")
            plan.label(text="Review every effective value below before applying", icon='INFO')
            for index, action in enumerate(actions, 1):
                if isinstance(action, dict):
                    _draw_exact_action(plan, index, action)
            row = plan.row(align=True)
            row.scale_y = 1.3
            row.operator("redrob_reblend.apply_plan", text="Apply All", icon='CHECKMARK')
            row.operator("redrob_reblend.clear", text="Discard", icon='TRASH')

        footer = layout.row(align=True)
        footer.operator("redrob_reblend.open_console", text="Console", icon='URL')
        footer.operator("redrob_reblend.restart_agent", text="Reconnect", icon='FILE_REFRESH')
        footer.operator("redrob_reblend.clear", text="New", icon='FILE_NEW')


class REDROB_PT_scene_context(Panel):
    bl_idname = "REDROB_PT_scene_context"
    bl_parent_id = "REDROB_PT_workspace"
    bl_label = "Scene Context"
    bl_space_type = 'VIEW_3D'
    bl_region_type = 'UI'
    bl_options = {'DEFAULT_CLOSED'}  # noqa: RUF012

    def draw(self, context):
        layout = self.layout
        active = context.active_object
        row = layout.row()
        row.label(text="Active")
        row.label(text=active.name if active else "None", icon='OBJECT_DATA' if active else 'X')
        row = layout.row()
        row.label(text="Selected")
        row.label(text=str(len(context.selected_objects)))
        row = layout.row()
        row.label(text="Mode")
        row.label(text=context.mode.replace("_", " ").title())
        layout.label(text="Geometry, images, files and paths stay local", icon='LOCKED')


class REDROB_PT_appearance(Panel):
    bl_idname = "REDROB_PT_appearance"
    bl_parent_id = "REDROB_PT_workspace"
    bl_label = "Appearance"
    bl_space_type = 'VIEW_3D'
    bl_region_type = 'UI'
    bl_options = {'DEFAULT_CLOSED'}  # noqa: RUF012

    def draw(self, context):
        layout = self.layout
        current = session(context)
        layout.label(text="Native Blender UI with Reblend styling")
        row = layout.row(align=True)
        if current.theme_active or theme.has_snapshot():
            row.operator("redrob_reblend.restore_theme", text="Restore Previous", icon='LOOP_BACK')
        else:
            row.operator("redrob_reblend.apply_theme", text="Apply Modern Theme", icon='PREFERENCES')


_CLASSES = (
    REDROB_PT_workspace,
    REDROB_PT_scene_context,
    REDROB_PT_appearance,
)


def register():
    for cls in _CLASSES:
        bpy.utils.register_class(cls)


def unregister():
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
