# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""The "Shape" workspace tab: the same chat in a wide sidebar, beside a large viewport.

Blender's sidebar opens about 250 px wide, which is narrow for a conversation. The first time you
ask for the wide view, this copies the current workspace into a tab named "Shape", opens its
viewport's sidebar on the Redrob Shape tab and widens it. Later the tab is simply there: it is
saved with the file like any workspace, and the button only switches to it.

The width needs ``Region.sidebar_width`` (patches/0006). A Blender without it still gets the tab,
at the default width, which you can drag wider.
"""

import bpy
from bpy.types import Operator

from .i18n import iface_

WORKSPACE_NAME = "Shape"
CATEGORY = "Redrob Shape"
#: Sidebar width before interface scaling: room for the plan card's buttons side by side and a
#: long line of chat.
SIDEBAR_WIDTH = 480
_MAX_TRIES = 25  # timer ticks spent waiting for Blender to draw the new tab once


def largest_area(screen, area_type='VIEW_3D'):
    areas = [area for area in screen.areas if area.type == area_type]
    return max(areas, key=lambda area: area.width * area.height, default=None)


def prepare(screen, width=SIDEBAR_WIDTH):
    """Open the main viewport's sidebar on our tab, widened. True once the tab is selected."""
    area = largest_area(screen)
    if area is None:
        return True  # nothing to prepare: a workspace without a 3D viewport
    area.spaces.active.show_region_ui = True
    region = next((region for region in area.regions if region.type == 'UI'), None)
    if region is None:
        return False
    if hasattr(region, "sidebar_width") and region.sidebar_width < width:
        region.sidebar_width = width
    try:
        region.active_panel_category = CATEGORY
    except (AttributeError, TypeError):
        # Blender lists a sidebar's tabs while drawing it, so a sidebar that was hidden until now
        # has none yet. Try again on the next tick.
        area.tag_redraw()
        return False
    area.tag_redraw()
    return True


def _prepare_soon(window_pointer, tries=_MAX_TRIES):
    def tick():
        nonlocal tries
        tries -= 1
        for window in bpy.context.window_manager.windows:
            if window.as_pointer() != window_pointer:
                continue
            if window.workspace.name == WORKSPACE_NAME and prepare(window.screen):
                return None
            break
        return 0.1 if tries > 0 else None

    bpy.app.timers.register(tick, first_interval=0.05)


class REDROB_OT_open_workspace(Operator):
    bl_idname = "redrob_shape.open_workspace"
    bl_label = "Wide View"
    bl_description = "Open the chat in the Shape workspace tab, with a wide sidebar"

    def execute(self, context):
        window = context.window
        if window is None:
            return {'CANCELLED'}
        workspace = bpy.data.workspaces.get(WORKSPACE_NAME)
        if workspace is None:
            before = set(bpy.data.workspaces.keys())
            # Copies the workspace you are in, so the tab starts with the editors you know.
            if bpy.ops.workspace.duplicate() != {'FINISHED'}:
                self.report({'ERROR'}, iface_("Could not add the Shape workspace"))
                return {'CANCELLED'}
            added = [space for space in bpy.data.workspaces if space.name not in before]
            if not added:
                self.report({'ERROR'}, iface_("Could not add the Shape workspace"))
                return {'CANCELLED'}
            workspace = added[0]
            workspace.name = WORKSPACE_NAME
        window.workspace = workspace
        _prepare_soon(window.as_pointer())
        return {'FINISHED'}


_CLASSES = (REDROB_OT_open_workspace,)


def register():
    for cls in _CLASSES:
        bpy.utils.register_class(cls)


def unregister():
    for cls in reversed(_CLASSES):
        bpy.utils.unregister_class(cls)
