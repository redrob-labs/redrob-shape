# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later
"""Coverage tests for the Reblend theme.

Blender ships an EMPTY Blender_Dark.xml -- its default theme lives in C, in
source/blender/editors/interface/resources.cc, which our patch set is forbidden to touch because
UPSTREAM.md rules out patching upstream editors. So the theme is applied at runtime by theme.py,
and the thing that can silently go wrong is coverage: a handful of widget colours changed while
every editor keeps Blender's own background, which looks like a broken half-rebrand rather than a
product.

These tests assert the sweep is complete and internally consistent. They need no Blender: `bpy` is
stubbed, because theme.py only touches it inside the apply functions.
"""

from __future__ import annotations

import importlib.util
import sys
import types
import unittest
from pathlib import Path


def _load_theme():
    # theme.py imports bpy at module scope. A stub is enough: nothing at import time calls into it,
    # and stubbing is what lets this run in CI without a Blender build.
    sys.modules.setdefault("bpy", types.ModuleType("bpy"))
    module_name = "redrob_reblend_theme_coverage_test"
    path = Path(__file__).resolve().parents[1] / "theme.py"
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


theme = _load_theme()


class PaletteTests(unittest.TestCase):
    def test_every_palette_entry_is_rgba(self):
        for name, value in theme._PALETTE.items():
            self.assertEqual(len(value), 4, f"{name} is not RGBA")
            for channel in value:
                self.assertGreaterEqual(channel, 0.0, f"{name} has a negative channel")
                self.assertLessEqual(channel, 1.0, f"{name} exceeds 1.0")

    def test_every_space_role_names_a_real_palette_entry(self):
        # A typo here would silently skip that role for every editor.
        for path, role in theme._SPACE_ROLES:
            self.assertIn(role, theme._PALETTE, f"{path} refers to unknown palette role {role!r}")

    def test_accent_is_the_redrob_red_not_blender_blue(self):
        r, g, b, _a = theme._PALETTE["accent"]
        self.assertGreater(r, 0.5, "accent should be red-dominant")
        self.assertLess(b, 0.2, "accent still looks like Blender's blue")
        self.assertLess(g, 0.2)


class CoverageTests(unittest.TestCase):
    def setUp(self):
        self.paths = [path for path, _ in theme._THEME_VALUES]

    def test_no_duplicate_paths(self):
        # A duplicate would be applied twice and, worse, the SECOND value would win over a
        # deliberately hand-tuned first one.
        duplicates = {p for p in self.paths if self.paths.count(p) > 1}
        self.assertEqual(duplicates, set(), f"duplicated theme paths: {sorted(duplicates)}")

    def test_the_sweep_did_not_override_a_hand_tuned_value(self):
        # The generated sweep is filtered against the explicit list. If that filter regressed, an
        # editor tuned on purpose would quietly get the generic colour.
        for path in theme._EXPLICIT_PATHS:
            self.assertEqual(
                self.paths.count(path), 1, f"{path} was both hand-tuned and swept"
            )

    def test_every_declared_space_is_actually_covered(self):
        covered = {p.split(".", 1)[0] for p in self.paths}
        for space in theme._SPACES:
            self.assertIn(space, covered, f"{space} is declared but never themed")

    def test_the_major_editors_are_themed(self):
        # Named explicitly so dropping one from _SPACES is a failure rather than a silent
        # reduction in coverage.
        covered = {p.split(".", 1)[0] for p in self.paths}
        for space in ("view_3d", "outliner", "properties", "node_editor", "sequence_editor",
                      "image_editor", "text_editor", "graph_editor", "topbar", "statusbar"):
            self.assertIn(space, covered)

    def test_coverage_is_a_full_theme_not_a_few_widgets(self):
        # The regression this guards: 53 values, of which 36 were widget colours and most editors
        # had only a background. That reads as a broken half-rebrand.
        self.assertGreater(len(self.paths), 250, "the theme is too sparse to be a rebrand")
        non_widget = [p for p in self.paths if not p.startswith("user_interface.")]
        self.assertGreater(len(non_widget), 200, "editors are still mostly Blender's own colours")

    def test_every_backgroundish_value_is_dark(self):
        # A light value slipping into a dark theme is the kind of thing nobody notices until a
        # screenshot. Only the roles that ARE backgrounds are checked.
        dark_roles = ("bg_deep", "bg", "bg_raised", "bg_hover")
        for role in dark_roles:
            r, g, b, _a = theme._PALETTE[role]
            self.assertLess(max(r, g, b), 0.2, f"{role} is not dark")

    def test_values_are_either_colours_or_scalars(self):
        for path, value in theme._THEME_VALUES:
            if isinstance(value, tuple):
                self.assertIn(len(value), (3, 4), f"{path} is not a colour")
            else:
                self.assertIsInstance(value, (int, float), f"{path} is neither colour nor scalar")


if __name__ == "__main__":
    unittest.main()
