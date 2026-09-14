# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Native Redrob agent workspace for Blender."""

bl_info = {
    "name": "Redrob Reblend",
    "author": "Redrob Labs",
    "version": (0, 1, 0),
    "blender": (4, 5, 0),
    "description": "Native, approval-first agent workspace for Blender",
    "doc_url": "https://github.com/redrob-labs/redrob-reblend",
    "support": 'OFFICIAL',
    "category": "3D View",
}

from . import connection_ui, operators, panels, runtime, state

# connection_ui registers the AddonPreferences, so it goes before the panels that read them.
_MODULES = (state, connection_ui, operators, panels, runtime)


def register():
    for module in _MODULES:
        module.register()


def unregister():
    for module in reversed(_MODULES):
        module.unregister()
