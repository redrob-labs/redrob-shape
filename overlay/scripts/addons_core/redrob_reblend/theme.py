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
# One place where a colour is decided. Every per-space value below is derived from this, so a
# palette change does not mean revisiting forty editors and missing three.
#
# Blender's shipped Blender_Dark.xml is EMPTY -- the default theme lives in C
# (source/blender/editors/interface/resources.cc), which our patch set is forbidden to touch
# because UPSTREAM.md rules out patching upstream editors. So the whole theme is applied here at
# runtime instead, and it is reversible: every value is snapshotted before it is changed, which a
# C patch could never offer. That constraint produced the better design.
_PALETTE = {
    "bg_deep": (0.043, 0.049, 0.062, 1.0),      # editor backgrounds
    "bg": (0.055, 0.063, 0.078, 1.0),           # panels, widget interiors
    "bg_raised": (0.075, 0.085, 0.104, 1.0),    # headers, toolbars
    "bg_hover": (0.098, 0.110, 0.133, 1.0),
    "line": (0.16, 0.18, 0.22, 1.0),            # outlines, separators
    "accent": (0.72, 0.025, 0.055, 1.0),        # Redrob red -- selection and emphasis
    "accent_dim": (0.44, 0.03, 0.05, 1.0),
    "text": (0.78, 0.81, 0.86, 1.0),
    "text_bright": (1.0, 1.0, 1.0, 1.0),
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
    ("user_interface.wcol_regular.outline", (0.16, 0.18, 0.22, 1.0)),
    ("user_interface.wcol_regular.inner", (0.055, 0.063, 0.078, 1.0)),
    ("user_interface.wcol_regular.inner_sel", (0.72, 0.025, 0.055, 1.0)),
    ("user_interface.wcol_regular.text", (0.78, 0.81, 0.86, 1.0)),
    ("user_interface.wcol_regular.text_sel", (1.0, 1.0, 1.0, 1.0)),
    ("user_interface.wcol_regular.roundness", 0.38),
    ("user_interface.wcol_tool.outline", (0.17, 0.19, 0.23, 1.0)),
    ("user_interface.wcol_tool.inner", (0.07, 0.08, 0.10, 1.0)),
    ("user_interface.wcol_tool.inner_sel", (0.78, 0.025, 0.06, 1.0)),
    ("user_interface.wcol_tool.text", (0.82, 0.85, 0.90, 1.0)),
    ("user_interface.wcol_tool.text_sel", (1.0, 1.0, 1.0, 1.0)),
    ("user_interface.wcol_tool.roundness", 0.48),
    ("user_interface.wcol_toolbar_item.outline", (0.16, 0.18, 0.22, 1.0)),
    ("user_interface.wcol_toolbar_item.inner", (0.055, 0.063, 0.078, 1.0)),
    ("user_interface.wcol_toolbar_item.inner_sel", (0.72, 0.025, 0.055, 1.0)),
    ("user_interface.wcol_toolbar_item.roundness", 0.48),
    ("user_interface.wcol_menu.outline", (0.14, 0.16, 0.20, 1.0)),
    ("user_interface.wcol_menu.inner", (0.045, 0.052, 0.065, 1.0)),
    ("user_interface.wcol_menu.inner_sel", (0.72, 0.025, 0.055, 1.0)),
    ("user_interface.wcol_menu.text", (0.82, 0.85, 0.90, 1.0)),
    ("user_interface.wcol_menu.text_sel", (1.0, 1.0, 1.0, 1.0)),
    ("user_interface.wcol_menu.roundness", 0.42),
    ("user_interface.wcol_menu_back.outline", (0.13, 0.15, 0.19, 1.0)),
    ("user_interface.wcol_menu_back.inner", (0.035, 0.041, 0.052, 1.0)),
    ("user_interface.wcol_menu_back.roundness", 0.42),
    ("user_interface.wcol_box.outline", (0.16, 0.18, 0.22, 1.0)),
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
    ("view_3d.space.tab_active", (0.72, 0.025, 0.055, 1.0)),
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
