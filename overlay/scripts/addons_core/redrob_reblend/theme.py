# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""A modern, durable and reversible native Blender theme overlay."""

import json
import os
from pathlib import Path

import bpy

_THEME_SNAPSHOT = {}
_THEME_SNAPSHOT_VERSION = 1

# --- The Reblend palette ---------------------------------------------------------
#
# Derived from the ORGANISATION'S canonical tokens, not chosen here:
# redrob-design/packages/brand/src/tokens.css. One place decides a colour, and it is not this file.
#
#   --rr-black   #0a0b0c      --rr-gray-1  #f8f9fb
#   --rr-white   #ffffff      --rr-gray-8  #292e37
#   --rr-blue-5  #507fff      --rr-gray-9  #141719
#   --rr-blue-6  #2b52ff
#
# The accent is BLUE. An earlier version of this file used a red accent invented for the purpose,
# which was simply wrong about the brand -- and the test that asserted it was red encoded the same
# mistake. Both are corrected here.
#
# Blender's shipped Blender_Dark.xml is EMPTY -- the default theme lives in C
# (source/blender/editors/interface/resources.cc), which our patch set is forbidden to touch because
# UPSTREAM.md rules out patching upstream editors. So the whole theme is applied here at runtime
# instead, and it is reversible: every value is snapshotted before it is changed, which a C patch
# could never offer. That constraint produced the better design.

RR_BLACK = (0.039216, 0.043137, 0.047059)
RR_WHITE = (1.0, 1.0, 1.0)
RR_BLUE_5 = (0.313725, 0.498039, 1.0)
RR_BLUE_6 = (0.168627, 0.321569, 1.0)
RR_GRAY_1 = (0.972549, 0.976471, 0.984314)
RR_GRAY_8 = (0.160784, 0.180392, 0.215686)
RR_GRAY_9 = (0.078431, 0.090196, 0.098039)


def _rgba(rgb, alpha=1.0):
    return (*rgb, alpha)


_PALETTE = {
    "bg_deep": _rgba(RR_BLACK),        # editor backgrounds
    "bg": _rgba(RR_GRAY_9),            # panels, widget interiors
    "bg_raised": (0.098, 0.110, 0.125, 1.0),   # headers, toolbars: between gray-9 and gray-8
    "bg_hover": (0.128, 0.144, 0.170, 1.0),
    "line": _rgba(RR_GRAY_8),          # outlines, separators
    "accent": _rgba(RR_BLUE_6),        # selection and emphasis
    "accent_dim": _rgba(RR_BLUE_5),
    "text": _rgba(RR_GRAY_1),
    "text_bright": _rgba(RR_WHITE),
    "text_dim": (0.52, 0.55, 0.60, 1.0),
}

#: Every editor space Blender exposes on a theme, and the roles each one gets.
#:
#: Listing a role a given space does not have is deliberately safe: `_resolve` skips a path the
#: running Blender does not support rather than raising, which is also what makes this survive an
#: upstream version bump that adds or removes a space.
_SPACES = (
    "view_3d", "graph_editor", "dopesheet_editor", "nla_editor", "image_editor",
    "sequence_editor", "text_editor", "node_editor", "properties", "outliner",
    "preferences", "info", "file_browser", "console", "clip_editor", "topbar",
    "statusbar", "spreadsheet",
)

_SPACE_ROLES = (
    ("space.back", "bg_deep"),
    ("space.header", "bg_raised"),
    ("space.header_text", "text"),
    ("space.header_text_hi", "text_bright"),
    ("space.button", "bg"),
    ("space.button_text", "text"),
    ("space.button_text_hi", "text_bright"),
    ("space.button_title", "text_bright"),
    ("space.text", "text"),
    ("space.text_hi", "text_bright"),
    ("space.title", "text_bright"),
    ("space.tab_active", "bg"),
    ("space.tab_inactive", "bg_deep"),
    ("space.tab_back", "bg_raised"),
    ("space.tab_outline", "line"),
    ("space.navigation_bar", "bg_raised"),
    ("space.execution_buts", "bg"),
    # List/tree regions, present on outliner, file browser, spreadsheet and others.
    ("space_list.list", "bg_deep"),
    ("space_list.list_title", "text_bright"),
    ("space_list.list_text", "text"),
    ("space_list.list_text_hi", "text_bright"),
)


def _space_theme_values():
    """Expand the palette across every space, so no editor is left on Blender's own colours."""
    values = []
    for space in _SPACES:
        for suffix, role in _SPACE_ROLES:
            values.append((f"{space}.{suffix}", _PALETTE[role]))
    return tuple(values)


# Paths are resolved defensively so the preset remains compatible across Blender
# versions. Unsupported properties are skipped rather than breaking startup.
_THEME_VALUES = (
    ("user_interface.wcol_regular.outline", _rgba(RR_GRAY_8)),
    ("user_interface.wcol_regular.inner", (0.055, 0.063, 0.078, 1.0)),
    ("user_interface.wcol_regular.inner_sel", _rgba(RR_BLUE_6)),
    ("user_interface.wcol_regular.text", _rgba(RR_GRAY_1)),
    ("user_interface.wcol_regular.text_sel", _rgba(RR_WHITE)),
    ("user_interface.wcol_regular.roundness", 0.38),
    ("user_interface.wcol_tool.outline", _rgba(RR_GRAY_8)),
    ("user_interface.wcol_tool.inner", (0.07, 0.08, 0.10, 1.0)),
    ("user_interface.wcol_tool.inner_sel", _rgba(RR_BLUE_6)),
    ("user_interface.wcol_tool.text", _rgba(RR_GRAY_1)),
    ("user_interface.wcol_tool.text_sel", _rgba(RR_WHITE)),
    ("user_interface.wcol_tool.roundness", 0.48),
    ("user_interface.wcol_toolbar_item.outline", _rgba(RR_GRAY_8)),
    ("user_interface.wcol_toolbar_item.inner", (0.055, 0.063, 0.078, 1.0)),
    ("user_interface.wcol_toolbar_item.inner_sel", _rgba(RR_BLUE_6)),
    ("user_interface.wcol_toolbar_item.roundness", 0.48),
    ("user_interface.wcol_menu.outline", (0.14, 0.16, 0.20, 1.0)),
    ("user_interface.wcol_menu.inner", (0.045, 0.052, 0.065, 1.0)),
    ("user_interface.wcol_menu.inner_sel", _rgba(RR_BLUE_6)),
    ("user_interface.wcol_menu.text", _rgba(RR_GRAY_1)),
    ("user_interface.wcol_menu.text_sel", _rgba(RR_WHITE)),
    ("user_interface.wcol_menu.roundness", 0.42),
    ("user_interface.wcol_menu_back.outline", (0.13, 0.15, 0.19, 1.0)),
    ("user_interface.wcol_menu_back.inner", (0.035, 0.041, 0.052, 1.0)),
    ("user_interface.wcol_menu_back.roundness", 0.42),
    ("user_interface.wcol_box.outline", _rgba(RR_GRAY_8)),
    ("user_interface.wcol_box.inner", (0.045, 0.052, 0.065, 0.94)),
    ("user_interface.wcol_box.roundness", 0.32),
    ("user_interface.wcol_tab.outline", (0.13, 0.15, 0.19, 1.0)),
    ("user_interface.wcol_tab.inner", (0.045, 0.052, 0.065, 1.0)),
    ("user_interface.wcol_tab.inner_sel", (0.12, 0.135, 0.16, 1.0)),
    ("user_interface.wcol_tab.roundness", 0.42),
    ("user_interface.menu_shadow_fac", 0.38),
    ("user_interface.menu_shadow_width", 8),
    ("user_interface.icon_alpha", 1.0),
    ("user_interface.icon_saturation", 0.9),
    ("view_3d.space.back", (0.018, 0.022, 0.029)),
    ("view_3d.space.header", (0.035, 0.041, 0.052, 1.0)),
    ("view_3d.space.text", (0.76, 0.80, 0.86)),
    ("view_3d.space.text_hi", (1.0, 1.0, 1.0)),
    ("view_3d.space.button", (0.055, 0.063, 0.078, 1.0)),
    ("view_3d.space.navigation_bar", (0.028, 0.033, 0.043, 1.0)),
    ("view_3d.space.tab_active", _rgba(RR_BLUE_6)),
    ("view_3d.space.tab_inactive", (0.055, 0.063, 0.078, 1.0)),
    ("topbar.space.back", (0.022, 0.027, 0.035, 1.0)),
    ("topbar.space.header", (0.028, 0.033, 0.043, 1.0)),
    ("statusbar.space.back", (0.022, 0.027, 0.035, 1.0)),
    ("outliner.space.back", (0.024, 0.029, 0.038, 1.0)),
    ("properties.space.back", (0.024, 0.029, 0.038, 1.0)),
    ("node_editor.space.back", (0.018, 0.022, 0.029, 1.0)),
    ("file_browser.space.back", (0.024, 0.029, 0.038, 1.0)),
    ("text_editor.space.back", (0.018, 0.022, 0.029, 1.0)),
    ("console.space.back", (0.018, 0.022, 0.029, 1.0)),
)

# The hand-tuned values above come first and the generated per-space sweep follows, so a specific
# choice made deliberately for one editor is not overwritten by the general rule. Application is
# in order, so later wins -- hence the sweep is filtered to paths the explicit list does not claim.
_EXPLICIT_PATHS = frozenset(path for path, _ in _THEME_VALUES)
_THEME_VALUES = _THEME_VALUES + tuple(
    (path, value) for path, value in _space_theme_values() if path not in _EXPLICIT_PATHS
)


def _resolve(root, path):
    target = root
    parts = path.split(".")
    for name in parts[:-1]:
        target = getattr(target, name)
    return target, parts[-1]


def _snapshot_path(create=False):
    directory = bpy.utils.user_resource('CONFIG', path="redrob_reblend", create=create)
    return Path(directory) / "theme_snapshot.json" if directory else None


def _load_snapshot():
    if _THEME_SNAPSHOT:
        return
    path = _snapshot_path()
    if path is None or not path.is_file():
        return
    try:
        payload = json.loads(path.read_text(encoding="utf-8"))
        if payload.get("version") == _THEME_SNAPSHOT_VERSION and isinstance(payload.get("values"), dict):
            _THEME_SNAPSHOT.update(payload["values"])
    except (OSError, TypeError, ValueError):
        return


def _persist_snapshot():
    path = _snapshot_path(create=True)
    if path is None:
        return False
    payload = {"version": _THEME_SNAPSHOT_VERSION, "values": _THEME_SNAPSHOT}
    temporary = path.with_suffix(".tmp")
    try:
        with temporary.open("w", encoding="utf-8") as handle:
            json.dump(payload, handle, sort_keys=True, separators=(",", ":"))
            handle.flush()
            os.fsync(handle.fileno())
        temporary.replace(path)
    except OSError:
        try:
            temporary.unlink(missing_ok=True)
        except OSError:
            pass
        return False
    return True


def has_snapshot():
    _load_snapshot()
    return bool(_THEME_SNAPSHOT)


def apply(theme):
    """Persist rollback data before applying any supported theme value."""
    _load_snapshot()
    previous_snapshot = dict(_THEME_SNAPSHOT)
    resolved = []
    for path, value in _THEME_VALUES:
        try:
            target, name = _resolve(theme, path)
            current = getattr(target, name)
        except (AttributeError, TypeError, ValueError):
            continue
        if path not in _THEME_SNAPSHOT:
            _THEME_SNAPSHOT[path] = list(current) if hasattr(current, "__len__") else current
        resolved.append((target, name, value))

    if resolved and not _persist_snapshot():
        _THEME_SNAPSHOT.clear()
        _THEME_SNAPSHOT.update(previous_snapshot)
        raise RuntimeError("Could not persist the Reblend theme rollback snapshot")

    changed = 0
    for target, name, value in resolved:
        try:
            setattr(target, name, value)
            changed += 1
        except (AttributeError, TypeError, ValueError):
            continue
    return changed


def restore(theme):
    """Restore values captured before Reblend, including across restarts."""
    _load_snapshot()
    restored = 0
    failed = {}
    for path, value in tuple(_THEME_SNAPSHOT.items()):
        try:
            target, name = _resolve(theme, path)
            setattr(target, name, value)
            restored += 1
        except (AttributeError, TypeError, ValueError):
            failed[path] = value
    _THEME_SNAPSHOT.clear()
    _THEME_SNAPSHOT.update(failed)
    path = _snapshot_path()
    if failed:
        if not _persist_snapshot():
            raise RuntimeError("Could not update the Reblend theme rollback snapshot")
    elif path is not None:
        try:
            path.unlink(missing_ok=True)
        except OSError as exc:
            raise RuntimeError("Could not remove the Reblend theme rollback snapshot") from exc
    return restored
