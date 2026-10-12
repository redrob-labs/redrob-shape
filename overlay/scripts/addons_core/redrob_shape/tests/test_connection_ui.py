# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later
"""Structural tests for the connection UI.

`bpy` is not importable outside Blender, so this stubs the pieces connection_ui touches at import
time and asserts the things that can be wrong without a running Blender -- which are the ones that
matter most here:

  * The transport menu offers exactly the transports that exist. A stale entry would let a user
    select something the resolver cannot honour.
  * A pasted key does NOT live in an add-on preference. Preferences are stored in userpref.blend,
    an ordinary unprotected file that travels with a preferences backup, so the scratch field must
    carry SKIP_SAVE and the operator must clear it.

The drawing code itself needs Blender and is covered by tests/blender_integration.py.
"""

from __future__ import annotations

import importlib.util
import sys
import types
import unittest
from pathlib import Path


def _stub_bpy():
    """A bpy stub with just enough surface for connection_ui to import."""
    bpy = types.ModuleType("bpy")

    def _prop_factory(name):
        def prop(**kwargs):
            # Property declarations are recorded as plain dicts so the test can inspect the
            # options a real Blender would act on.
            return {"__prop__": name, **kwargs}

        return prop

    props = types.ModuleType("bpy.props")
    for name in ("BoolProperty", "EnumProperty", "StringProperty", "IntProperty", "FloatProperty"):
        setattr(props, name, _prop_factory(name))

    types_mod = types.ModuleType("bpy.types")
    for name in ("AddonPreferences", "Operator", "Panel", "PropertyGroup"):
        setattr(types_mod, name, type(name, (), {}))

    utils = types.ModuleType("bpy.utils")
    utils.register_class = lambda cls: None
    utils.unregister_class = lambda cls: None

    bpy.props = props
    bpy.types = types_mod
    bpy.utils = utils
    bpy.context = types.SimpleNamespace(preferences=types.SimpleNamespace(addons={}))

    sys.modules["bpy"] = bpy
    sys.modules["bpy.props"] = props
    sys.modules["bpy.types"] = types_mod
    sys.modules["bpy.utils"] = utils
    return bpy


def _load(name, package_stub):
    module_name = f"redrob_shape_{name}_ui_test"
    path = Path(__file__).resolve().parents[1] / f"{name}.py"
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    # connection_ui does `from . import credentials, transport` and uses __package__, so it needs a
    # package context. Building one here keeps the module itself free of test-only branches.
    module.__package__ = package_stub
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


_stub_bpy()

# Build a throwaway package so the relative imports in connection_ui resolve.
_PKG = "redrob_shape_ui_pkg"
_pkg = types.ModuleType(_PKG)
_pkg.__path__ = [str(Path(__file__).resolve().parents[1])]
sys.modules[_PKG] = _pkg

_spec = importlib.util.spec_from_file_location(
    f"{_PKG}.connection_ui", Path(__file__).resolve().parents[1] / "connection_ui.py"
)
connection_ui = importlib.util.module_from_spec(_spec)
sys.modules[f"{_PKG}.connection_ui"] = connection_ui
_spec.loader.exec_module(connection_ui)

transport = sys.modules[f"{_PKG}.transport"] if f"{_PKG}.transport" in sys.modules else connection_ui.transport


class MenuTests(unittest.TestCase):
    def test_every_menu_entry_except_auto_maps_to_a_real_transport(self):
        items = {item[0] for item in connection_ui._TRANSPORT_ITEMS}
        self.assertIn("AUTO", items)
        for key in items - {"AUTO"}:
            self.assertIn(key, connection_ui._KIND_BY_ITEM, f"{key} has no transport kind")

    def test_every_transport_kind_is_offered(self):
        # A kind with no menu entry could never be chosen explicitly.
        offered = set(connection_ui._KIND_BY_ITEM.values())
        for kind in transport.Kind:
            self.assertIn(kind, offered, f"{kind} is not offered in the menu")

    def test_each_entry_has_a_description(self):
        for key, label, description in connection_ui._TRANSPORT_ITEMS:
            self.assertTrue(label.strip(), f"{key} has no label")
            self.assertTrue(description.strip(), f"{key} has no description")


class SecretHandlingTests(unittest.TestCase):
    def test_the_key_field_is_never_saved_to_preferences(self):
        # userpref.blend is an ordinary unprotected file. SKIP_SAVE is what keeps a pasted key out
        # of it, and PASSWORD is what keeps it off the screen.
        field = connection_ui.REDROB_Preferences.__annotations__["key_entry"]
        self.assertIn("SKIP_SAVE", field["options"])
        self.assertEqual(field["subtype"], "PASSWORD")

    def test_no_preference_field_holds_a_credential_by_name(self):
        annotations = connection_ui.REDROB_Preferences.__annotations__
        for name in annotations:
            self.assertNotIn("api_key", name)
            self.assertNotIn("token", name)
            self.assertNotIn("secret", name)

    def test_subscription_is_off_by_default_in_preferences(self):
        field = connection_ui.REDROB_Preferences.__annotations__["subscription_enabled"]
        self.assertFalse(field["default"])

    def test_subscription_description_warns_about_policy(self):
        field = connection_ui.REDROB_Preferences.__annotations__["subscription_enabled"]
        self.assertIn("may change", field["description"])

    def test_transport_defaults_to_automatic(self):
        field = connection_ui.REDROB_Preferences.__annotations__["transport_choice"]
        self.assertEqual(field["default"], "AUTO")

    def test_key_targets_match_the_known_accounts(self):
        field = connection_ui.REDROB_Preferences.__annotations__["key_target"]
        targets = {item[0] for item in field["items"]}
        self.assertEqual(targets, set(connection_ui.credentials.KNOWN_ACCOUNTS))


class HealthCheckTests(unittest.TestCase):
    def test_health_check_returns_false_rather_than_raising(self):
        # It runs while a panel is drawing. An exception there would break the UI, not just the
        # probe, so a closed port has to be a False.
        self.assertFalse(connection_ui._health_check("http://127.0.0.1:1"))


class ConnectOperatorTests(unittest.TestCase):
    def test_every_connect_step_has_a_registered_operator(self):
        registered = {getattr(cls, "bl_idname", None) for cls in connection_ui._CLASSES}
        for name in (
            "redrob_shape.connect",
            "redrob_shape.connect_cancel",
            "redrob_shape.open_connect_page",
            "redrob_shape.copy_connect_code",
            "redrob_shape.disconnect",
        ):
            self.assertIn(name, registered)

    def test_disconnect_asks_before_removing_the_key(self):
        self.assertIn("invoke", vars(connection_ui.REDROB_OT_disconnect))

    def test_the_key_is_described_by_its_last_four_characters_only(self):
        current = types.SimpleNamespace(
            connection='CONNECTED', connection_hint="cdef", connection_source="keychain"
        )
        self.assertEqual(connection_ui._redrob_key_state(current), "connected (keychain, ending cdef)")
        current.connection = 'DISCONNECTED'
        self.assertEqual(connection_ui._redrob_key_state(current), "not connected")


if __name__ == "__main__":
    unittest.main()
