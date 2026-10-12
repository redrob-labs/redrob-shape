# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""The add-on's structural check gives every shared case the verdict the agent gives it."""

import importlib.util
import json
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


def _load(name, path):
    spec = importlib.util.spec_from_file_location(name, path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


catalog = _load("redrob_shape_catalog_test", HERE.parent / "catalog.py")
conformance = _load("redrob_shape_conformance_test", HERE / "conformance.py")


class ConformanceTests(unittest.TestCase):
    def test_the_committed_cases_are_what_the_catalog_generates(self):
        committed = (HERE / "tool_conformance.json").read_text(encoding="utf-8")
        self.assertEqual(committed, conformance.render(conformance.cases(conformance.load_catalog())))

    def test_every_shared_case_gets_the_agents_verdict(self):
        cases = json.loads((HERE / "tool_conformance.json").read_text(encoding="utf-8"))
        self.assertGreater(len(cases), 100)
        wrong = []
        for case in cases:
            try:
                catalog.check_plan(case["plan"])
                verdict = True
            except catalog.PlanShapeError:
                verdict = False
            if verdict != case["valid"]:
                wrong.append(case["name"])
        self.assertEqual(wrong, [])

    def test_null_optional_arguments_are_dropped(self):
        plan = {
            "summary": "s",
            "actions": [{"type": "subdivide_mesh", "arguments": {"object": "Cube", "cuts": 2, "smoothness": None}}],
        }
        self.assertEqual(catalog.check_plan(plan)["actions"][0]["arguments"], {"object": "Cube", "cuts": 2})

    def test_defaults_come_from_the_catalog(self):
        self.assertEqual(
            catalog.default_arguments("add_modifier", {"object": "Cube", "modifier": "twist", "angle": 90}),
            {"axis": "z"},
        )
        self.assertEqual(catalog.default_arguments("run_python", {}), {})


if __name__ == "__main__":
    sys.exit(unittest.main())
