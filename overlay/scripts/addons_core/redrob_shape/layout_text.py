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


def wrap_width(context=None, fraction=1.0):
    """Characters that fit on one line of the current sidebar region, or of the ``fraction`` of
    its width a chat bubble takes."""
    context = context or bpy.context
    region = getattr(context, "region", None)
    if region is None or region.width <= 0:
        return max(12, int(_FALLBACK_WIDTH * fraction))
    scale = context.preferences.system.ui_scale or 1.0
    return max(12, int((region.width * fraction - _PADDING_PX * scale) / (_GLYPH_PX * scale)))


def wrap_lines(text, width):
    lines = []
    for paragraph in text.splitlines() or ("",):
        lines.extend(textwrap.wrap(paragraph, width=width) or [""])
    return lines


def wrapped_labels(layout, text, limit=6, icon='NONE', context=None, fraction=1.0, alignment=None):
    """Draw already-translated text as several labels. The icon goes on the first line.

    ``alignment`` ('RIGHT', 'CENTER') puts each line in its own row aligned that way: your own
    chat messages sit on the right.
    """
    lines = wrap_lines(text, wrap_width(context, fraction))
    shown = lines[:limit] + (["…"] if len(lines) > limit else [])
    for index, line in enumerate(shown):
        target = layout
        if alignment:
            target = layout.row()
            target.alignment = alignment
        target.label(text=line, translate=False, icon=icon if index == 0 else 'NONE')
