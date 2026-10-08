# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

import importlib.util
import sys
import types
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


def _load_theme():
    bpy = types.ModuleType("bpy")
    bpy.utils = SimpleNamespace(user_resource=lambda *_args, **_kwargs: None)
    previous = sys.modules.get("bpy")
    sys.modules["bpy"] = bpy
    try:
        path = Path(__file__).resolve().parents[1] / "theme.py"
        spec = importlib.util.spec_from_file_location("redrob_shape_theme_test", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    finally:
        if previous is None:
            del sys.modules["bpy"]
        else:
            sys.modules["bpy"] = previous


class ThemeDurabilityTests(unittest.TestCase):
    def test_snapshot_failure_aborts_before_first_mutation(self):
        theme_module = _load_theme()
        theme_module._THEME_VALUES = (("value", 2.0),)
        theme_object = SimpleNamespace(value=1.0)
        with (
            mock.patch.object(theme_module, "_persist_snapshot", return_value=False),
            self.assertRaises(RuntimeError),
        ):
            theme_module.apply(theme_object)
        self.assertEqual(theme_object.value, 1.0)
        self.assertEqual(theme_module._THEME_SNAPSHOT, {})

    def test_snapshot_is_persisted_before_mutation(self):
        theme_module = _load_theme()
        theme_module._THEME_VALUES = (("value", 2.0),)
        theme_object = SimpleNamespace(value=1.0)
        calls = []

        def persist():
            calls.append(("persist", theme_object.value, dict(theme_module._THEME_SNAPSHOT)))
            return True

        with mock.patch.object(theme_module, "_persist_snapshot", side_effect=persist):
            self.assertEqual(theme_module.apply(theme_object), 1)
        self.assertEqual(calls, [("persist", 1.0, {"value": 1.0})])
        self.assertEqual(theme_object.value, 2.0)


if __name__ == "__main__":
    unittest.main()
