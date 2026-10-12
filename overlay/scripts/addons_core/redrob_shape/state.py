# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Session state displayed by the native Redrob Shape workspace."""

import bpy
from bpy.props import (
    BoolProperty,
    CollectionProperty,
    EnumProperty,
    FloatProperty,
    IntProperty,
    PointerProperty,
    StringProperty,
)
from bpy.types import PropertyGroup

_STATUS_ITEMS = (
    ('IDLE', "Ready", "Ready for a new instruction", 'CHECKMARK', 0),
    ('WORKING', "Thinking", "Redrob is preparing a plan", 'TIME', 1),
    ('READY', "Review", "A validated plan is ready to review", 'PREVIEW_RANGE', 2),
    ('APPLYING', "Applying", "The approved plan is being applied", 'FILE_REFRESH', 3),
    ('ERROR', "Needs Attention", "The last operation could not complete", 'ERROR', 4),
    ('APPLIED', "Applied", "The plan is in the scene: keep it or undo it", 'CHECKMARK', 5),
)

_SCOPE_ITEMS = (
    ('SELECTION', "Selected", "Shape only the selected objects", 'RESTRICT_SELECT_OFF', 0),
    ('SCENE', "Whole Scene", "Let the plan touch any object in the scene", 'SCENE_DATA', 1),
)


def _step_toggled(self, context):
    # Late import: operators imports this module.
    from . import operators

    operators.steps_changed(context)


class REDROB_PG_step(PropertyGroup):
    """One step of the plan under review. Switching it off leaves it out of Apply."""

    enabled: BoolProperty(name="Include", description="Include this step when applying", default=True, update=_step_toggled)
    expanded: BoolProperty(name="Details", description="Show every value of this step", default=False)
    outcome: StringProperty(default="")  # "", "done" or "failed", after an apply


_CONNECTION_ITEMS = (
    ('UNKNOWN', "Checking", "Asking the local agent about the connection", 'TIME', 0),
    ('DISCONNECTED', "Not Connected", "Connect Redrob Console to create plans", 'UNLINKED', 1),
    ('STARTING', "Starting", "Asking Redrob Console for a code", 'TIME', 2),
    ('WAITING', "Waiting for Approval", "Approve the code in your browser", 'URL', 3),
    ('CONNECTED', "Connected", "Plans use your Redrob Console key", 'LINKED', 4),
    ('ERROR', "Not Connected", "The last connection attempt did not finish", 'ERROR', 5),
)


class REDROB_PG_session(PropertyGroup):
    prompt: StringProperty(
        name="Instruction",
        description="Describe the result you want in the current scene",
        default="",
    )
    status: EnumProperty(name="Status", items=_STATUS_ITEMS, default='IDLE')
    status_message: StringProperty(default="Ready for a new instruction")
    response_text: StringProperty(default="")
    plan_json: StringProperty(default="")
    plan_summary: StringProperty(default="")
    request_id: StringProperty(default="", options={'SKIP_SAVE'})
    scene_revision: StringProperty(default="", options={'SKIP_SAVE'})
    allowed_objects_json: StringProperty(default="[]", options={'SKIP_SAVE'})
    sidecar_available: BoolProperty(default=False, options={'SKIP_SAVE'})

    # The shaping conversation (conversation.py) and the plan under review.
    scope: EnumProperty(name="Shape", items=_SCOPE_ITEMS, default='SELECTION', options={'SKIP_SAVE'})
    plan_scope: StringProperty(default="SCENE", options={'SKIP_SAVE'})
    history_json: StringProperty(default="[]", options={'SKIP_SAVE'})
    # What the chat shows (chat.py): every message, including the ones the agent is never sent.
    chat_json: StringProperty(default="[]", options={'SKIP_SAVE'})
    show_earlier: BoolProperty(
        name="Show Earlier Messages",
        description="Show the whole conversation, not only the newest messages",
        default=False,
        options={'SKIP_SAVE'},
    )
    last_instruction: StringProperty(default="", options={'SKIP_SAVE'})
    steps: CollectionProperty(type=REDROB_PG_step, options={'SKIP_SAVE'})
    plan_problem: StringProperty(default="", options={'SKIP_SAVE'})
    started_at: FloatProperty(default=0.0, options={'SKIP_SAVE'})
    applied_steps: IntProperty(default=0, options={'SKIP_SAVE'})
    revision_before_apply: StringProperty(default="", options={'SKIP_SAVE'})
    revision_after_apply: StringProperty(default="", options={'SKIP_SAVE'})
    show_history: BoolProperty(name="History", default=False, options={'SKIP_SAVE'})
    theme_active: BoolProperty(default=False, options={'SKIP_SAVE'})

    # The connection to Redrob Console. None of these is a secret: the key stays in the agent, and
    # what reaches Blender is its last four characters and where it is kept.
    connection: EnumProperty(name="Connection", items=_CONNECTION_ITEMS, default='UNKNOWN', options={'SKIP_SAVE'})
    connection_message: StringProperty(default="", options={'SKIP_SAVE'})
    connection_source: StringProperty(default="", options={'SKIP_SAVE'})
    connection_hint: StringProperty(default="", options={'SKIP_SAVE'})
    connection_keychain: BoolProperty(default=True, options={'SKIP_SAVE'})
    user_code: StringProperty(default="", options={'SKIP_SAVE'})
    verification_uri: StringProperty(default="", options={'SKIP_SAVE'})
    verification_uri_complete: StringProperty(default="", options={'SKIP_SAVE'})
    connect_request_id: StringProperty(default="", options={'SKIP_SAVE'})
    connect_expires_at: FloatProperty(default=0.0, options={'SKIP_SAVE'})
    key_name: StringProperty(default="", options={'SKIP_SAVE'})
    account_name: StringProperty(default="", options={'SKIP_SAVE'})


def session(context):
    return context.window_manager.redrob_shape


_CLASSES = (REDROB_PG_step, REDROB_PG_session)


def register():
    for cls in _CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.WindowManager.redrob_shape = PointerProperty(type=REDROB_PG_session)


def unregister():
    del bpy.types.WindowManager.redrob_shape
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
