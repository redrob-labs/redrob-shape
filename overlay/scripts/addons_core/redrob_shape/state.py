# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Session state displayed by the native Redrob Shape workspace."""

import bpy
from bpy.props import BoolProperty, EnumProperty, PointerProperty, StringProperty
from bpy.types import PropertyGroup

_STATUS_ITEMS = (
    ('IDLE', "Ready", "Ready for a new instruction", 'CHECKMARK', 0),
    ('WORKING', "Thinking", "Redrob is preparing a plan", 'TIME', 1),
    ('READY', "Review", "A validated plan is ready to review", 'PREVIEW_RANGE', 2),
    ('APPLYING', "Applying", "The approved plan is being applied", 'FILE_REFRESH', 3),
    ('ERROR', "Needs Attention", "The last operation could not complete", 'ERROR', 4),
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
    theme_active: BoolProperty(default=False, options={'SKIP_SAVE'})


def session(context):
    return context.window_manager.redrob_shape


_CLASSES = (REDROB_PG_session,)


def register():
    for cls in _CLASSES:
        bpy.utils.register_class(cls)
    bpy.types.WindowManager.redrob_shape = PointerProperty(type=REDROB_PG_session)


def unregister():
    del bpy.types.WindowManager.redrob_shape
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
