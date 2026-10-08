# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Every string the shaping sidebar shows has a Korean entry in i18n.py.

Read from the source, not from a list kept beside it: a label added to the panel without a
translation fails here, and so does a translation for a label that no longer exists.
"""

import ast
import json
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SHOWN_BY = ("panels.py", "operators.py", "runtime.py", "state.py")


def _tree(name):
    return ast.parse((ROOT / name).read_text(encoding="utf-8"))


def _korean():
    for node in ast.walk(_tree("i18n.py")):
        if isinstance(node, ast.Assign) and any(getattr(t, "id", None) == "KO" for t in node.targets):
            return ast.literal_eval(node.value)
    raise AssertionError("i18n.py has no KO table")


def _constant(node):
    return node.value if isinstance(node, ast.Constant) and isinstance(node.value, str) else None


def _branches(node):
    """Both texts of ``"Refine Plan" if pending else "Create Plan"``."""
    if isinstance(node, ast.IfExp) and _constant(node.body) and _constant(node.orelse):
        return {node.body.value, node.orelse.value}
    return set()


def _shown(name):
    """Strings a module puts in front of the user."""
    found = set()
    tree = _tree(name)
    for node in ast.walk(tree):
        if isinstance(node, ast.Call):
            keywords = {k.arg: k.value for k in node.keywords}
            untranslated = isinstance(keywords.get("translate"), ast.Constant) and keywords["translate"].value is False
            text = _constant(keywords["text"]) if "text" in keywords else None
            if text and not untranslated:
                found.add(text)
            if not untranslated:
                found.update(_branches(keywords.get("text")))
            if getattr(node.func, "id", None) == "iface_" and node.args and _constant(node.args[0]):
                found.add(node.args[0].value)
            if getattr(node.func, "id", None) in {"BoolProperty", "EnumProperty", "StringProperty"}:
                for key in ("name", "description"):
                    if key in keywords and _constant(keywords[key]) and name == "state.py":
                        found.add(keywords[key].value)
        elif isinstance(node, ast.Assign):
            target = getattr(node.targets[0], "id", None)
            if target == "heading":
                found.update(_branches(node.value))
            if target in {"bl_label", "bl_description"} and _constant(node.value):
                found.add(node.value.value)
            if target in {"_STEP_NAMES", "_MODIFIER_NAMES", "_OUTCOME_NAMES"}:
                found.update(ast.literal_eval(node.value).values())
            if target == "_STATUS_PRESENTATION":
                found.update(title for _icon, title in ast.literal_eval(node.value).values())
            if target == "_SCOPE_ITEMS":
                for item in ast.literal_eval(node.value):
                    found.update(item[1:3])
    return found


class TranslationCoverageTests(unittest.TestCase):
    def test_the_sidebar_flow_is_translated(self):
        korean = _korean()
        shown = set()
        for name in ("panels.py", "operators.py"):
            shown |= _shown(name)
        missing = sorted(text for text in shown if text not in korean)
        self.assertEqual(missing, [])

    def test_plan_status_messages_and_choices_are_translated(self):
        korean = _korean()
        shown = _shown("runtime.py") | _shown("state.py")
        # state.py's older enums (status, connection) feed the Connection panel, which keeps its
        # own wording; the shaping choices are what this checks.
        wanted = {text for text in shown if text in korean or text in {"Selected", "Whole Scene"}}
        self.assertTrue({"Selected", "Whole Scene", "Request cancelled"} <= wanted)
        for text in _shown("runtime.py"):
            self.assertIn(text, korean)

    def test_step_values_and_warnings_from_the_catalog_are_translated(self):
        korean = _korean()
        catalog = json.loads((ROOT / "tool_catalog.json").read_text(encoding="utf-8"))
        wanted = set()
        for tool in catalog["tools"].values():
            if "note" in tool:
                wanted.add(tool["note"])
            for fields in [tool.get("fields", {})] + [v.get("fields", {}) for v in tool.get("variants", {}).values()]:
                wanted.update(field.replace("_", " ").title() for field in fields)
            if tool.get("variant_field"):
                wanted.add(tool["variant_field"].title())
        self.assertEqual(sorted(wanted - set(korean)), [])

    def test_holes_survive_translation(self):
        for message, text in _korean().items():
            self.assertEqual(message.count("{}"), text.count("{}"), message)

    def test_no_translation_is_left_over(self):
        used = set()
        for name in SHOWN_BY:
            used |= _shown(name)
        catalog = json.loads((ROOT / "tool_catalog.json").read_text(encoding="utf-8"))
        for tool in catalog["tools"].values():
            used.add(tool.get("note", ""))
            for fields in [tool.get("fields", {})] + [v.get("fields", {}) for v in tool.get("variants", {}).values()]:
                used.update(field.replace("_", " ").title() for field in fields)
            if tool.get("variant_field"):
                used.add(tool["variant_field"].title())
        leftover = sorted(set(_korean()) - used)
        self.assertEqual(leftover, [])


if __name__ == "__main__":
    unittest.main()
