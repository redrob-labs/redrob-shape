# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""The Redrob Shape sidebar: a chat with Redrob about the scene.

Messages stack top to bottom, newest last, and the composer sits under them:

  1. You say what should change (right-hand bubble), on the selection or the whole scene.
  2. Redrob answers with a plan (left-hand bubble). Every step shows its values on request and can
     be switched off; Apply is inside the answer.
  3. Applying, undoing, keeping or discarding leaves one line in the chat ("Applied: Bevel · Cube").
  4. Refine ("rounder"). The conversation travels with the request, so the plan builds on the last.

What the chat shows is chat.py; what the agent is told is conversation.py.

Colours come from the theme (theme.py maps the design-system tokens onto Blender's own theme);
this file only chooses Blender's semantic states -- alert, active, enabled -- never a colour.
"""

import json
import time

import bpy
from bpy.types import Panel

from . import catalog, chat, connection_ui, layout_text, theme, tools
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


#: The newest messages the sidebar draws; "Show Earlier" brings back the rest.
_VISIBLE_MESSAGES = 14
#: Share of the sidebar's width a bubble takes. Yours sit on the right, Redrob's on the left.
_USER_SHARE = 0.82
_AGENT_SHARE = 0.94

#: One line in the chat for what happened to the plan, between the bubbles.
_EVENT_PRESENTATION = {
    "applied": ('CHECKMARK', "Applied: {}"),
    "undone": ('LOOP_BACK', "Undone"),
    "redone": ('LOOP_FORWARDS', "Applied again"),
    "kept": ('CHECKMARK', "Kept"),
    "discarded": ('X', "Plan discarded"),
    "cancelled": ('CANCEL', "Request cancelled"),
    "retry": ('FILE_REFRESH', "Asked again for a different plan"),
}


def _bubble(layout, role):
    """A chat bubble: a box on the right for you, on the left for Redrob. Returns the column to
    draw into and the share of the sidebar it has, which the text wraps to."""
    if role == "user":
        split = layout.split(factor=1.0 - _USER_SHARE)
        split.label(text="")
        return split.box().column(align=True), _USER_SHARE
    split = layout.split(factor=_AGENT_SHARE)
    column = split.box().column(align=True)
    split.label(text="")
    return column, _AGENT_SHARE


def _agent_header(column):
    # 'BLENDER' is the brand slot: the design system's Redrob symbol, as in the panel header.
    column.label(text="Redrob Shape", icon='BLENDER')


def _steps_line(steps):
    return ", ".join(step_title(step) for step in steps)


def _draw_user(layout, index, item):
    column, share = _bubble(layout, "user")
    header = column.row(align=True)
    header.alignment = 'RIGHT'
    reuse = header.operator("redrob_shape.reuse_instruction", text="", icon='PASTEDOWN', emboss=False)
    reuse.index = index
    header.label(text="You", icon='USER')
    layout_text.wrapped_labels(column, item["text"], limit=12, fraction=share, alignment='RIGHT')


def _draw_agent(layout, current, item, live):
    column, share = _bubble(layout, "agent")
    _agent_header(column)
    kind = item.get("kind")
    if kind == "error":
        body = column.column(align=True)
        body.alert = True
        layout_text.wrapped_labels(body, item["text"], limit=8, icon='ERROR', fraction=share)
        return
    text = item["text"] or (iface_("Proposed changes") if kind == "plan" else "")
    layout_text.wrapped_labels(column, text, limit=8, fraction=share)
    if kind != "plan":
        return
    if live:
        _draw_plan_card(column, current, share)
        return
    steps = item.get("steps", [])
    earlier = column.column(align=True)
    earlier.active = False
    earlier.label(text=iface_("{} steps").format(len(steps)), icon='PREVIEW_RANGE', translate=False)
    if steps:
        layout_text.wrapped_labels(earlier, _steps_line(steps), limit=3, fraction=share)


def _draw_event(layout, item):
    icon, text = _EVENT_PRESENTATION[item["kind"]]
    text = iface_(text)
    if item["kind"] == "applied":
        text = text.format(_steps_line(item.get("steps", ())))
    column = layout.column(align=True)
    column.active = False
    layout_text.wrapped_labels(column, text, limit=4, icon=icon, alignment='CENTER')


def _draw_thinking(layout, current):
    column, _share = _bubble(layout, "agent")
    _agent_header(column)
    seconds = max(0, int(time.monotonic() - current.started_at)) if current.started_at else 0
    row = column.row(align=True)
    row.label(text=iface_("Thinking… {} s").format(seconds), icon='SORTTIME', translate=False)
    row.operator("redrob_shape.cancel", text="", icon='CANCEL')


def _draw_greeting(layout):
    column, share = _bubble(layout, "agent")
    _agent_header(column)
    layout_text.wrapped_labels(
        column,
        iface_("Tell me what to change. You see every step before anything in the scene changes."),
        fraction=share,
    )
    hint = column.column(align=True)
    hint.active = False
    layout_text.wrapped_labels(hint, iface_("Try: “bevel the cube and smooth it”"), fraction=share)


def _plan_is_live(current):
    """The plan under review, applied or stopped part-way still has buttons; nothing else does."""
    return bool(current.plan_json) and current.status in {'READY', 'APPLYING', 'APPLIED', 'ERROR'}


def _draw_transcript(layout, current):
    messages = chat.load(current.chat_json)
    if not messages and current.status != 'WORKING':
        _draw_greeting(layout)
        return
    limit = 0 if current.show_earlier else _VISIBLE_MESSAGES
    shown, hidden = chat.visible(messages, limit)
    if hidden:
        layout.prop(
            current,
            "show_earlier",
            text=iface_("Show {} earlier messages").format(hidden),
            icon='TRIA_UP',
            toggle=True,
            translate=False,
        )
    elif current.show_earlier and len(messages) > _VISIBLE_MESSAGES:
        layout.prop(current, "show_earlier", text="Show Fewer", icon='TRIA_DOWN', toggle=True)
    live_index = chat.latest_plan_index(messages) if _plan_is_live(current) else -1
    for index, item in shown:
        if item["role"] == "user":
            _draw_user(layout, index, item)
        elif item["role"] == "agent":
            _draw_agent(layout, current, item, live=index == live_index)
        else:
            _draw_event(layout, item)
    if current.status == 'WORKING':
        _draw_thinking(layout, current)


def _draw_composer(layout, current):
    pending = current.status in {'READY', 'APPLIED'} and bool(current.plan_json)
    busy = current.status in {'WORKING', 'APPLYING'}
    box = layout.box()
    hint = iface_("Refine it, or ask for something new") if pending else iface_("What should change?")
    field = box.column(align=True)
    field.enabled = not busy
    if hasattr(field, "textbox"):  # Blender 5.3: a multi-line field that grows with the text
        field.textbox(current, "prompt", initial_visible_lines=2, placeholder=hint, translate=False)
    else:
        field.prop(current, "prompt", text="", placeholder=hint, translate=False)
    row = box.row(align=True)
    row.label(text="Work on")
    row.prop(current, "scope", expand=True)
    row = box.row(align=True)
    row.scale_y = 1.35
    submit = row.row(align=True)
    submit.enabled = not busy
    submit.operator("redrob_shape.submit", text="Send", icon='PLAY')
    again = row.row(align=True)
    again.enabled = not busy and bool(current.last_instruction)
    again.operator("redrob_shape.retry", text="", icon='FILE_REFRESH')


def _draw_plan_card(layout, current, share):
    """The live plan inside Redrob's answer: every step, then Apply, or Keep and Undo."""
    actions = _plan_actions(current.plan_json)
    steps = list(current.steps)
    if not actions or len(steps) != len(actions):
        return  # a plan from an older session; the next request rebuilds the rows
    applied = current.status == 'APPLIED'
    layout.separator()
    enabled = sum(1 for step in steps if step.enabled)
    layout.label(text=iface_("{} of {} steps").format(enabled, len(actions)), icon='PREVIEW_RANGE', translate=False)
    if not applied:
        layout_text.wrapped_labels(
            layout, iface_("Switch off any step you do not want, then apply"), icon='INFO', fraction=share
        )
    for index, (action, step) in enumerate(zip(actions, steps), 1):
        if isinstance(action, dict):
            _draw_step(layout, index, action, step, locked=applied or current.status == 'APPLYING')
    if current.plan_problem:
        problem = layout.column(align=True)
        problem.alert = True
        layout_text.wrapped_labels(problem, current.plan_problem, limit=4, icon='ERROR', fraction=share)

    if applied:
        layout_text.wrapped_labels(
            layout, iface_("Look at the result in the viewport, then keep or undo it"), icon='HIDE_OFF', fraction=share
        )
    row = layout.row(align=True)
    row.scale_y = 1.3
    if applied:
        row.operator("redrob_shape.keep", text="Keep", icon='CHECKMARK')
        row.operator("ed.undo", text="Undo", icon='LOOP_BACK')
    elif current.status == 'ERROR' and current.revision_before_apply:
        row.operator("ed.undo", text="Undo", icon='LOOP_BACK')
        row.operator("redrob_shape.clear", text="Discard", icon='TRASH')
    else:
        row.operator("redrob_shape.apply_plan", text="Apply", icon='CHECKMARK')
        row.operator("redrob_shape.clear", text="Discard", icon='TRASH')


class REDROB_PT_workspace(Panel):
    """The chat: messages top to bottom, the plan inside Redrob's answer, the composer last."""

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

        row = layout.row(align=True)
        status = row.row(align=True)
        status.alert = current.status == 'ERROR'
        status.label(text=title, icon=icon)
        # Not connected is said by the Connect block right under this row.
        if current.connection == 'CONNECTED':
            status.label(text="Connected")
        tools_row = row.row(align=True)
        tools_row.alignment = 'RIGHT'
        tools_row.operator("redrob_shape.open_workspace", text="", icon='FULLSCREEN_ENTER')
        tools_row.operator("redrob_shape.new_conversation", text="", icon='FILE_NEW')

        # A first-time user's first question is "why does Send not work". Answer it here, not in
        # the collapsed Connection panel.
        if current.connection != 'CONNECTED':
            connection_ui.draw_console_connection(layout, current)

        _draw_transcript(layout.column(), current)
        _draw_composer(layout, current)

        footer = layout.row(align=True)
        footer.operator("redrob_shape.open_console", text="Console", icon='URL')
        footer.operator("redrob_shape.restart_agent", text="Reconnect", icon='FILE_REFRESH')


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
    REDROB_PT_scene_context,
    REDROB_PT_appearance,
)


def register():
    for cls in _CLASSES:
        bpy.utils.register_class(cls)


def unregister():
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
