# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""The Redrob Shape sidebar: ask, review step by step, apply, then keep or undo.

The flow the panel walks through, top to bottom:

  1. Say what should change, and whether to shape the selection or the whole scene.
  2. Review the plan. Every step shows its exact values on request, and can be switched off.
  3. Apply. The result is in the viewport as one Undo step: keep it, undo it, or try again.
  4. Refine ("rounder"). The conversation travels with the request, so the plan builds on the last.

Colours come from the theme (theme.py maps the design-system tokens onto Blender's own theme);
this file only chooses Blender's semantic states -- alert, active, enabled -- never a colour.
"""

import json
import time

import bpy
from bpy.types import Panel

from . import catalog, connection_ui, conversation, layout_text, theme, tools
from .i18n import iface_
from .state import session

_STATUS_PRESENTATION = {
    'IDLE': ('CHECKMARK', "Ready"),
    'WORKING': ('TIME', "Thinking"),
    'READY': ('PREVIEW_RANGE', "Review Plan"),
    'APPLYING': ('FILE_REFRESH', "Applying"),
    'APPLIED': ('CHECKMARK', "Applied"),
    'ERROR': ('ERROR', "Needs Attention"),
}

#: What a step is called in the panel. Modifier steps are named by their kind.
_STEP_NAMES = {
    "create_primitive": "Add",
    "transform_object": "Move",
    "boolean": "Boolean",
    "extrude_faces": "Extrude",
    "inset_faces": "Inset",
    "subdivide_mesh": "Subdivide",
    "shade_smooth": "Shade",
    "apply_modifier": "Apply Modifier",
    "duplicate_object": "Duplicate",
    "delete_object": "Delete",
    "rename_object": "Rename",
    "set_material": "Material",
}
_MODIFIER_NAMES = {
    "bevel": "Bevel",
    "subdivision": "Subdivision",
    "solidify": "Solidify",
    "mirror": "Mirror",
    "array": "Array",
    "twist": "Twist",
    "bend": "Bend",
    "taper": "Taper",
    "stretch": "Stretch",
    "displace": "Noise",
    "smooth": "Smooth",
}
_STEP_ICONS = {
    "create_primitive": 'MESH_CUBE',
    "transform_object": 'ORIENTATION_GLOBAL',
    "add_modifier": 'MODIFIER',
    "boolean": 'MOD_BOOLEAN',
    "extrude_faces": 'MOD_SOLIDIFY',
    "inset_faces": 'FACESEL',
    "subdivide_mesh": 'MOD_SUBSURF',
    "shade_smooth": 'SHADING_SOLID',
    "apply_modifier": 'CHECKMARK',
    "duplicate_object": 'DUPLICATE',
    "delete_object": 'TRASH',
    "rename_object": 'SORTALPHA',
    "set_material": 'MATERIAL',
}
_OUTCOME_ICONS = {
    "applied": 'CHECKMARK',
    "pending": 'PREVIEW_RANGE',
    "discarded": 'X',
    "undone": 'LOOP_BACK',
    "failed": 'ERROR',
}
_OUTCOME_NAMES = {
    "applied": "Applied",
    "pending": "In review",
    "discarded": "Discarded",
    "undone": "Undone",
    "failed": "Stopped",
}


def _wrapped_labels(layout, text, limit=6):
    layout_text.wrapped_labels(layout, text, limit=limit)


def _plan_actions(raw):
    try:
        payload = json.loads(raw)
    except (TypeError, ValueError):
        return []
    actions = payload.get("actions", []) if isinstance(payload, dict) else []
    return actions if isinstance(actions, list) else []


def step_title(action):
    """``Bevel · Cube``: what the step does, to what. Translated, except for the object's name."""
    action_type = str(action.get("type", ""))
    arguments = action.get("arguments") if isinstance(action.get("arguments"), dict) else {}
    if action_type == "add_modifier":
        name = _MODIFIER_NAMES.get(arguments.get("modifier"), "Modifier")
    else:
        name = _STEP_NAMES.get(action_type, action_type.replace("_", " ").title())
    target = arguments.get("object") or arguments.get("name") or arguments.get("primitive")
    return f"{iface_(name)}  ·  {target}" if target else iface_(name)


def _format_value(value):
    if isinstance(value, float):
        return f"{value:.6g}"
    if isinstance(value, (list, tuple)):
        return "[" + ", ".join(_format_value(item) for item in value) + "]"
    if isinstance(value, bool):
        return iface_("Yes") if value else iface_("No")
    return str(value)


def _draw_step(layout, index, action, step, locked):
    box = layout.box()
    row = box.row(align=True)
    toggle = row.row(align=True)
    toggle.enabled = not locked
    toggle.prop(step, "enabled", text="")
    icon = {"done": 'CHECKMARK', "failed": 'ERROR'}.get(step.outcome, _STEP_ICONS.get(action.get("type"), 'DOT'))
    title = row.row(align=True)
    title.active = step.enabled
    title.alert = step.outcome == "failed"
    title.label(text=f"{index}. {step_title(action)}", icon=icon, translate=False)
    row.prop(step, "expanded", text="", icon='DOWNARROW_HLT' if step.expanded else 'RIGHTARROW', emboss=False)
    warning = catalog.note(action.get("type"))
    if warning and step.enabled:
        box.label(text=warning, icon='INFO')
    if not step.expanded:
        return
    arguments = tools.preview_arguments(action)
    if not isinstance(arguments, dict):
        box.label(text="Invalid action arguments", icon='ERROR')
        return
    column = box.column(align=True)
    column.active = step.enabled
    for name in sorted(arguments):
        row = column.row(align=True)
        row.label(text=iface_(name.replace("_", " ").title()))
        row.label(text=_format_value(arguments[name]), translate=False)


def _draw_progress(layout, current):
    seconds = max(0, int(time.monotonic() - current.started_at)) if current.started_at else 0
    row = layout.row(align=True)
    row.label(text=iface_("Planning… {} s").format(seconds), icon='SORTTIME', translate=False)
    row.operator("redrob_shape.cancel", text="Cancel", icon='CANCEL')


def _draw_prompt(layout, current):
    pending = current.status in {'READY', 'APPLIED'} and bool(current.plan_json)
    box = layout.box()
    has_history = bool(conversation.load(current.history_json))
    heading = "Refine it, or ask for something new" if has_history else "What should change?"
    box.label(text=heading, icon='GREASEPENCIL')
    box.prop(current, "prompt", text="")
    row = box.row(align=True)
    row.label(text="Work on")
    row.prop(current, "scope", expand=True)
    row = box.row(align=True)
    row.scale_y = 1.35
    busy = current.status in {'WORKING', 'APPLYING'}
    submit = row.row(align=True)
    submit.enabled = not busy
    submit.operator("redrob_shape.submit", text="Refine Plan" if pending else "Create Plan", icon='PLAY')
    again = row.row(align=True)
    again.enabled = not busy and bool(current.last_instruction)
    again.operator("redrob_shape.retry", text="", icon='FILE_REFRESH')


def _draw_plan(layout, current):
    actions = _plan_actions(current.plan_json)
    if not actions:
        return
    steps = list(current.steps)
    if len(steps) != len(actions):
        return  # a plan from an older session; the next request rebuilds the rows
    applied = current.status == 'APPLIED'
    box = layout.box()
    _wrapped_labels(box, current.plan_summary or iface_("Proposed changes"), limit=3)
    enabled = sum(1 for step in steps if step.enabled)
    row = box.row(align=True)
    row.label(text=iface_("{} of {} steps").format(enabled, len(actions)), icon='PREVIEW_RANGE', translate=False)
    if not applied:
        layout_text.wrapped_labels(box, iface_("Switch off any step you do not want, then apply"), icon='INFO')
    for index, (action, step) in enumerate(zip(actions, steps), 1):
        if isinstance(action, dict):
            _draw_step(box, index, action, step, locked=applied or current.status == 'APPLYING')
    if current.plan_problem:
        problem = box.row()
        problem.alert = True
        problem.label(text=current.plan_problem, icon='ERROR', translate=False)

    row = box.row(align=True)
    row.scale_y = 1.3
    if applied:
        box.label(text="Look at the result in the viewport", icon='HIDE_OFF')
        row = box.row(align=True)
        row.scale_y = 1.3
        row.operator("redrob_shape.keep", text="Keep", icon='CHECKMARK')
        row.operator("ed.undo", text="Undo", icon='LOOP_BACK')
    elif current.status == 'ERROR' and current.revision_before_apply:
        row.operator("ed.undo", text="Undo", icon='LOOP_BACK')
        row.operator("redrob_shape.clear", text="Discard", icon='TRASH')
    else:
        row.operator("redrob_shape.apply_plan", text="Apply", icon='CHECKMARK')
        row.operator("redrob_shape.clear", text="Discard", icon='TRASH')


class REDROB_PT_workspace(Panel):
    bl_idname = "REDROB_PT_workspace"
    bl_label = "Redrob Shape"
    bl_category = "Redrob Shape"
    bl_space_type = 'VIEW_3D'
    bl_region_type = 'UI'
    # The workspace is the tab's main panel: it goes first, above Connection.
    bl_order = 0

    def draw_header(self, _context):
        # 'BLENDER' is the icon slot the brand step fills with the design system's own Redrob
        # symbol (release/datafiles/icons_svg/blender.svg, pinned by sha256). No stock glyph.
        self.layout.label(text="", icon='BLENDER')

    def draw(self, context):
        layout = self.layout
        current = session(context)
        icon, title = _STATUS_PRESENTATION.get(current.status, ('INFO', "Status"))

        status = layout.box()
        row = status.row(align=True)
        row.alert = current.status == 'ERROR'
        row.label(text=title, icon=icon)
        row.label(text="Connected" if current.connection == 'CONNECTED' else "Not connected")
        if current.status == 'WORKING':
            _draw_progress(status, current)
        else:
            _wrapped_labels(status, current.status_message, limit=3)

        # A first-time user's first question is "why does Create Plan not work". Answer it here,
        # not in the collapsed Connection panel.
        if current.connection != 'CONNECTED':
            connection_ui.draw_console_connection(layout, current)

        _draw_prompt(layout, current)
        _draw_plan(layout, current)

        footer = layout.row(align=True)
        footer.operator("redrob_shape.open_console", text="Console", icon='URL')
        footer.operator("redrob_shape.restart_agent", text="Reconnect", icon='FILE_REFRESH')


class REDROB_PT_history(Panel):
    bl_idname = "REDROB_PT_history"
    bl_parent_id = "REDROB_PT_workspace"
    bl_label = "History"
    bl_space_type = 'VIEW_3D'
    bl_region_type = 'UI'
    bl_options = {'DEFAULT_CLOSED'}  # noqa: RUF012

    def draw(self, context):
        layout = self.layout
        current = session(context)
        turns = conversation.load(current.history_json)
        if not turns:
            layout.label(text="Nothing asked yet", icon='INFO')
            return
        for index in range(len(turns) - 1, -1, -1):
            turn = turns[index]
            box = layout.box()
            row = box.row(align=True)
            outcome = turn["outcome"]
            row.label(text=iface_(_OUTCOME_NAMES[outcome]), icon=_OUTCOME_ICONS[outcome], translate=False)
            row.label(text=iface_("{} steps").format(turn.get("steps", 0)), translate=False)
            reuse = row.operator("redrob_shape.reuse_instruction", text="", icon='PASTEDOWN')
            reuse.index = index
            _wrapped_labels(box, turn["instruction"], limit=2)
            summary = box.column()
            summary.active = False
            _wrapped_labels(summary, turn["summary"], limit=2)
        layout.operator("redrob_shape.new_conversation", text="New Conversation", icon='FILE_NEW')


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
        row.label(text=active.name if active else iface_("None"), icon='OBJECT_DATA' if active else 'X', translate=False)
        row = layout.row()
        row.label(text="Selected")
        row.label(text=str(len(context.selected_objects)), translate=False)
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
        layout.label(text="Native Blender UI with Redrob Shape styling")
        row = layout.row(align=True)
        if current.theme_active or theme.has_snapshot():
            row.operator("redrob_shape.restore_theme", text="Restore Previous", icon='LOOP_BACK')
        else:
            row.operator("redrob_shape.apply_theme", text="Apply Modern Theme", icon='PREFERENCES')


_CLASSES = (
    REDROB_PT_workspace,
    REDROB_PT_history,
    REDROB_PT_scene_context,
    REDROB_PT_appearance,
)


def register():
    for cls in _CLASSES:
        bpy.utils.register_class(cls)


def unregister():
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
