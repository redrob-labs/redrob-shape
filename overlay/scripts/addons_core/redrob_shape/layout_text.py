# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Sidebar text that wraps to the sidebar, instead of being cut to "wit...ace" by Blender."""

import textwrap

import bpy

# Average glyph width of Blender's UI font at scale 1, in pixels, plus the box's own padding.
_GLYPH_PX = 7.0
_PADDING_PX = 40
_FALLBACK_WIDTH = 42


def wrap_width(context=None):
    """Characters that fit on one line of the current sidebar region."""
    context = context or bpy.context
    region = getattr(context, "region", None)
    if region is None or region.width <= 0:
        return _FALLBACK_WIDTH
    scale = context.preferences.system.ui_scale or 1.0
    return max(16, int((region.width - _PADDING_PX * scale) / (_GLYPH_PX * scale)))


def wrap_lines(text, width):
    lines = []
    for paragraph in text.splitlines() or ("",):
        lines.extend(textwrap.wrap(paragraph, width=width) or [""])
    return lines


def wrapped_labels(layout, text, limit=6, icon='NONE', context=None):
    """Draw already-translated text as several labels. The icon goes on the first line."""
    lines = wrap_lines(text, wrap_width(context))
    for index, line in enumerate(lines[:limit]):
        layout.label(text=line, translate=False, icon=icon if index == 0 else 'NONE')
    if len(lines) > limit:
        layout.label(text="…", translate=False)
