# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later
"""The sidebar tab as a user first sees it.

Two defects shipped once and were only caught on screen:
- Connection sat ABOVE the Redrob Shape panel, because panels without bl_order stack in
  registration order and connection_ui registers first.
- The Redrob Shape header showed Blender's stock light-bulb icon ('LIGHT'), not our mark.
And fixed-width wrapping let Blender cut sentences to "Connect to plan wit...Console workspace".

These run without Blender: the panel classes are read from source, bpy is stubbed for wrapping.
"""

from __future__ import annotations

import ast
import importlib.util
import sys
import types
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def _class_attrs(path, class_name):
    tree = ast.parse(path.read_text(encoding="utf-8"))
    for node in tree.body:
        if isinstance(node, ast.ClassDef) and node.name == class_name:
            attrs = {}
            for item in node.body:
                if isinstance(item, ast.Assign) and isinstance(item.value, ast.Constant):
                    for target in item.targets:
                        if isinstance(target, ast.Name):
                            attrs[target.id] = item.value.value
            return node, attrs
    raise AssertionError(f"{class_name} not found in {path.name}")


def _header_icons(class_node):
    icons = []
    for item in class_node.body:
        if isinstance(item, ast.FunctionDef) and item.name == "draw_header":
            for call in ast.walk(item):
                if isinstance(call, ast.Call):
                    for keyword in call.keywords:
                        if keyword.arg == "icon" and isinstance(keyword.value, ast.Constant):
                            icons.append(keyword.value.value)
    return icons


def _load_layout_text():
    sys.modules.setdefault("bpy", types.ModuleType("bpy"))
    spec = importlib.util.spec_from_file_location("redrob_shape_layout_text_test", ROOT / "layout_text.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class SidebarOrderTest(unittest.TestCase):
    def test_workspace_panel_comes_before_connection(self):
        _, workspace = _class_attrs(ROOT / "panels.py", "REDROB_PT_workspace")
        _, connection = _class_attrs(ROOT / "connection_ui.py", "REDROB_PT_connection")
        self.assertEqual(workspace["bl_category"], connection["bl_category"])
        self.assertIn("bl_order", workspace)
        self.assertIn("bl_order", connection)
        self.assertLess(workspace["bl_order"], connection["bl_order"])

    def test_header_uses_the_brand_slot_not_a_stock_glyph(self):
        node, _ = _class_attrs(ROOT / "panels.py", "REDROB_PT_workspace")
        # 'BLENDER' is the slot scripts/apply-brand-assets.sh fills with the Redrob symbol.
        self.assertEqual(_header_icons(node), ["BLENDER"])


class _Region:
    def __init__(self, width):
        self.width = width


class _Context:
    def __init__(self, width, scale=1.0):
        self.region = _Region(width)
        self.preferences = types.SimpleNamespace(system=types.SimpleNamespace(ui_scale=scale))


class WrapTest(unittest.TestCase):
    layout_text = _load_layout_text()

    def test_width_follows_the_sidebar(self):
        narrow = self.layout_text.wrap_width(_Context(250))
        wide = self.layout_text.wrap_width(_Context(600))
        self.assertLess(narrow, 42)  # the old fixed width, which Blender cut at 250 px
        self.assertGreater(wide, narrow)
        self.assertLess(self.layout_text.wrap_width(_Context(500, scale=2.0)), wide)

    def test_no_line_is_longer_than_the_width(self):
        text = "Connect to plan with your Redrob Console workspace"
        width = self.layout_text.wrap_width(_Context(250))
        lines = self.layout_text.wrap_lines(text, width)
        self.assertGreater(len(lines), 1)
        self.assertTrue(all(len(line) <= width for line in lines))
        self.assertEqual(" ".join(lines), text)


if __name__ == "__main__":
    unittest.main()
