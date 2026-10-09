# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Operators for the Redrob Shape native workspace."""

import json
import time

import bpy
from bpy.app.handlers import persistent
from bpy.props import IntProperty
from bpy.types import Operator

from . import context as scene_context
from . import chat, conversation, runtime, theme, tools
from .i18n import iface_
from .state import session


def _turns(current):
    return conversation.load(current.history_json)


def _save_turns(current, turns):
    current.history_json = conversation.dump(turns)


def _plan(current):
    try:
        payload = json.loads(current.plan_json)
    except (TypeError, ValueError):
        return None
    return payload if isinstance(payload, dict) and isinstance(payload.get("actions"), list) else None


def _enabled_flags(current):
    return [step.enabled for step in current.steps]


def _allowed_objects(current):
    values = json.loads(current.allowed_objects_json)
    if not isinstance(values, list) or not all(isinstance(value, str) for value in values):
        raise tools.PlanValidationError("approved object scope is invalid")
    return set(values)


def _scene_revision(context):
    return scene_context.build_scene_context(context)[1]


def _reset_plan(current):
    current.plan_json = ""
    current.allowed_objects_json = "[]"
    current.plan_summary = ""
    current.plan_problem = ""
    current.scene_revision = ""
    current.revision_before_apply = ""
    current.revision_after_apply = ""
    current.applied_steps = 0
    current.steps.clear()


def steps_changed(context):
    """A step was switched on or off: say now whether what is left can still be applied."""
    current = session(context)
    plan = _plan(current)
    if plan is None or current.status not in {'READY', 'ERROR'}:
        return
    chosen = conversation.enabled_plan(plan, _enabled_flags(current))
    if not chosen["actions"]:
        current.plan_problem = iface_("Switch on at least one step")
        return
    try:
        tools.validate_plan(chosen, context, _allowed_objects(current))
    except (tools.PlanValidationError, ValueError) as exc:
        current.plan_problem = str(exc)
    else:
        current.plan_problem = ""


def _refuse(operator, current, text, level='WARNING'):
    """A send that never left Blender. Redrob says why in the chat; a plan under review stays as it
    was, with its Apply button, because nothing about it changed."""
    if current.status in {'READY', 'APPLIED'} and current.plan_json:
        chat.say(current, "agent", "error", text)
    else:
        chat.fail(current, text)
    operator.report({level}, text)
    return {'CANCELLED'}


def _submit(operator, context, instruction, turns, retry=False):
    """Send ``instruction``. Nothing that stops it here reaches the chat as YOUR message: the
    prompt keeps the text, and Redrob's answer says what to fix first."""
    current = session(context)
    if not bpy.app.online_access:
        return _refuse(operator, current, iface_("Online access is disabled in Blender preferences"))
    if current.scope == 'SELECTION' and not context.selected_objects:
        chat.say(current, "agent", "info", iface_("Select what to shape, or switch to Whole Scene"))
        operator.report({'WARNING'}, iface_("Select what to shape, or switch to Whole Scene"))
        return {'CANCELLED'}

    agent = runtime.runtime()
    if not agent.running and not agent.start():
        return _refuse(operator, current, agent.start_error, 'ERROR')

    summary, revision = scene_context.build_scene_context(context, current.scope)
    try:
        request_id = agent.submit_plan(
            instruction, summary, revision, scene_context.target_objects(summary), turns
        )
    except (RuntimeError, ValueError) as exc:
        chat.fail(current, str(exc))
        operator.report({'ERROR'}, str(exc))
        return {'CANCELLED'}

    if retry:
        chat.say(current, "event", "retry")
    else:
        chat.say(current, "user", text=instruction)
    if current.status == 'APPLIED':
        _keep(current)
    _reset_plan(current)
    current.response_text = ""
    current.last_instruction = instruction
    current.plan_scope = current.scope
    current.scene_revision = revision
    current.request_id = request_id
    current.started_at = time.monotonic()
    current.status = 'WORKING'
    current.status_message = iface_("Planning • no scene changes yet")
    return {'FINISHED'}


class REDROB_OT_submit(Operator):
    bl_idname = "redrob_shape.submit"
    bl_label = "Create Plan"
    bl_description = "Ask Redrob to prepare a plan you review before anything changes"

    def execute(self, context):
        current = session(context)
        instruction = current.prompt.strip()
        if not instruction:
            self.report({'WARNING'}, iface_("Enter an instruction first"))
            return {'CANCELLED'}
        # A plan still under review travels with the request, so this one can refine it.
        result = _submit(self, context, instruction, _turns(current))
        if result == {'FINISHED'}:
            current.prompt = ""
        return result


class REDROB_OT_retry(Operator):
    bl_idname = "redrob_shape.retry"
    bl_label = "Try Again"
    bl_description = "Ask for a different plan for the same instruction"

    @classmethod
    def poll(cls, context):
        current = session(context)
        return bool(current.last_instruction) and current.status not in {'WORKING', 'APPLYING'}

    def execute(self, context):
        current = session(context)
        turns = _turns(current)
        # The proposal being retried is not sent back as one to refine: a retry starts it over.
        if turns and turns[-1]["outcome"] in {"pending", "undone"}:
            turns = conversation.set_last_outcome(turns, "discarded")
            turns[-1].pop("plan", None)
            _save_turns(current, turns)
        return _submit(self, context, current.last_instruction, turns, retry=True)


class REDROB_OT_cancel(Operator):
    bl_idname = "redrob_shape.cancel"
    bl_label = "Cancel"
    bl_description = "Cancel the current Redrob request"

    def execute(self, context):
        current = session(context)
        try:
            cancel_id = runtime.runtime().cancel(current.request_id)
        except RuntimeError as exc:
            chat.fail(current, str(exc))
            self.report({'ERROR'}, str(exc))
            return {'CANCELLED'}
        if cancel_id is None:
            current.status = 'IDLE'
            current.status_message = iface_("Request is no longer active")
            current.request_id = ""
            chat.say(current, "event", "cancelled")
        else:
            current.status_message = iface_("Cancelling request")
        return {'FINISHED'}


class REDROB_OT_apply_plan(Operator):
    bl_idname = "redrob_shape.apply_plan"
    bl_label = "Apply Plan"
    bl_description = "Apply the switched-on steps as one undoable Blender operation"
    bl_options = {'REGISTER', 'UNDO'}  # noqa: RUF012

    @classmethod
    def poll(cls, context):
        current = session(context)
        return bool(current.plan_json) and current.status == 'READY' and not current.plan_problem

    def execute(self, context):
        current = session(context)
        plan = _plan(current)
        if plan is not None and len(current.steps) != len(plan["actions"]):
            runtime.show_plan(current, plan)  # one row per step, all switched on
        flags = _enabled_flags(current)
        try:
            if plan is None:
                raise tools.PlanValidationError("there is no plan to apply")
            payload = conversation.enabled_plan(plan, flags)
            allowed_objects = _allowed_objects(current)
            _, revision = scene_context.build_scene_context(context, current.plan_scope)
            if revision != current.scene_revision:
                raise tools.PlanValidationError(iface_("The scene changed after planning; create a new plan"))
            tools.validate_plan(payload, context, allowed_objects)
        except (json.JSONDecodeError, TypeError, ValueError) as exc:
            chat.fail(current, str(exc))
            self.report({'ERROR'}, str(exc))
            return {'CANCELLED'}

        # Which row of the panel each applied step is, so results land on the right row.
        rows = [index for index, keep in enumerate(flags) if keep]
        for step in current.steps:
            step.outcome = ""

        def mark_done(number, _result):
            current.steps[rows[number - 1]].outcome = "done"

        current.revision_before_apply = _scene_revision(context)
        current.status = 'APPLYING'
        try:
            results = tools.execute_plan(payload, context, allowed_objects, on_step=mark_done)
        except tools.StepFailed as exc:
            current.steps[rows[exc.number - 1]].outcome = "failed"
            chat.fail(current, iface_("Apply stopped at {}. Use Undo to roll back.").format(exc))
            _save_turns(current, conversation.set_last_outcome(_turns(current), "failed"))
            self.report({'ERROR'}, current.status_message)
            return {'FINISHED'}
        except (AttributeError, KeyError, RuntimeError, TypeError, ValueError) as exc:
            chat.fail(current, iface_("Apply stopped at {}. Use Undo to roll back.").format(exc))
            self.report({'ERROR'}, current.status_message)
            return {'FINISHED'}

        current.applied_steps = len(results)
        current.revision_after_apply = _scene_revision(context)
        current.status = 'APPLIED'
        current.status_message = iface_("Applied {} steps • look at the result, then keep or undo it").format(
            len(results)
        )
        _save_turns(current, conversation.set_last_outcome(_turns(current), "applied", len(results)))
        chat.say(current, "event", "applied", steps=payload["actions"])
        self.report({'INFO'}, current.status_message)
        return {'FINISHED'}


def _keep(current):
    _reset_plan(current)
    current.status = 'IDLE'
    current.status_message = iface_("Kept • describe the next change")


class REDROB_OT_keep(Operator):
    bl_idname = "redrob_shape.keep"
    bl_label = "Keep"
    bl_description = "Keep the applied result and continue the conversation"

    @classmethod
    def poll(cls, context):
        return session(context).status == 'APPLIED'

    def execute(self, context):
        current = session(context)
        _keep(current)
        chat.say(current, "event", "kept")
        return {'FINISHED'}


@persistent
def _after_undo(*_args):
    """Undo took the scene back to just before Apply: the plan is up for review again."""
    try:
        context = bpy.context
        current = session(context)
        if current.status not in {'APPLIED', 'ERROR'} or not current.revision_before_apply:
            return
        if _scene_revision(context) != current.revision_before_apply:
            return
        # The scene is back where it was planned, so the plan can be applied again as it is.
        _, current.scene_revision = scene_context.build_scene_context(context, current.plan_scope)
        for step in current.steps:
            step.outcome = ""
        current.status = 'READY'
        current.status_message = iface_("Undone • change the steps and apply again, or describe a change")
        _save_turns(current, conversation.set_last_outcome(_turns(current), "undone"))
        chat.say(current, "event", "undone")
        runtime.redraw()
    except (AttributeError, ReferenceError, RuntimeError):
        pass


@persistent
def _after_redo(*_args):
    try:
        context = bpy.context
        current = session(context)
        if current.status != 'READY' or not current.revision_after_apply:
            return
        if _scene_revision(context) != current.revision_after_apply:
            return
        current.status = 'APPLIED'
        current.status_message = iface_("Applied {} steps • look at the result, then keep or undo it").format(
            current.applied_steps
        )
        _save_turns(current, conversation.set_last_outcome(_turns(current), "applied"))
        chat.say(current, "event", "redone")
        runtime.redraw()
    except (AttributeError, ReferenceError, RuntimeError):
        pass


class REDROB_OT_clear(Operator):
    bl_idname = "redrob_shape.clear"
    bl_label = "Discard"
    bl_description = "Throw the plan away without applying it"

    def execute(self, context):
        current = session(context)
        if current.request_id:
            runtime.runtime().cancel(current.request_id)
        turns = _turns(current)
        if turns and turns[-1]["outcome"] in {"pending", "undone"}:
            _save_turns(current, conversation.set_last_outcome(turns, "discarded"))
        if current.plan_json or current.request_id:
            chat.say(current, "event", "discarded")
        if current.status == 'APPLIED':
            _keep(current)
        else:
            _reset_plan(current)
            current.status = 'IDLE'
            current.status_message = iface_("Ready for a new instruction")
        current.response_text = ""
        current.request_id = ""
        return {'FINISHED'}


class REDROB_OT_new_conversation(Operator):
    bl_idname = "redrob_shape.new_conversation"
    bl_label = "New Conversation"
    bl_description = "Forget earlier requests and start over"

    def execute(self, context):
        bpy.ops.redrob_shape.clear()
        current = session(context)
        current.history_json = "[]"
        current.chat_json = "[]"
        current.show_earlier = False
        current.last_instruction = ""
        current.prompt = ""
        return {'FINISHED'}


class REDROB_OT_reuse_instruction(Operator):
    bl_idname = "redrob_shape.reuse_instruction"
    bl_label = "Use Again"
    bl_description = "Put this earlier instruction back in the prompt"

    index: IntProperty(default=-1, options={'SKIP_SAVE'})

    def execute(self, context):
        current = session(context)
        messages = chat.load(current.chat_json)
        if not 0 <= self.index < len(messages) or messages[self.index]["role"] != "user":
            return {'CANCELLED'}
        current.prompt = messages[self.index]["text"]
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
        chat.fail(current, agent.start_error)
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
    REDROB_OT_retry,
    REDROB_OT_cancel,
    REDROB_OT_apply_plan,
    REDROB_OT_keep,
    REDROB_OT_clear,
    REDROB_OT_new_conversation,
    REDROB_OT_reuse_instruction,
    REDROB_OT_restart_agent,
    REDROB_OT_apply_theme,
    REDROB_OT_restore_theme,
    REDROB_OT_open_console,
)


_HANDLERS = (
    (bpy.app.handlers.undo_post, "_after_undo"),
    (bpy.app.handlers.redo_post, "_after_redo"),
)


def register():
    for cls in _CLASSES:
        bpy.utils.register_class(cls)
    for handlers, name in _HANDLERS:
        if globals()[name] not in handlers:
            handlers.append(globals()[name])


def unregister():
    for handlers, name in _HANDLERS:
        if globals()[name] in handlers:
            handlers.remove(globals()[name])
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
