# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

import importlib.util
import sys
import types
import unittest
from pathlib import Path
from types import SimpleNamespace


class _Ids(list):
    def get(self, name):
        return next((item for item in self if item.name == name), None)

    def __contains__(self, value):
        if isinstance(value, str):
            return self.get(value) is not None
        return super().__contains__(value)


def _object(name):
    return SimpleNamespace(
        name=name,
        type='MESH',
        library=None,
        data=SimpleNamespace(library=None),
    )


def _load_tools(objects, materials=()):
    bpy = types.ModuleType("bpy")
    bpy.data = SimpleNamespace(objects=_Ids(objects), materials=_Ids(materials))
    previous = sys.modules.get("bpy")
    sys.modules["bpy"] = bpy
    try:
        path = Path(__file__).resolve().parents[1] / "tools.py"
        spec = importlib.util.spec_from_file_location("redrob_reblend_tools_test", path)
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        return module
    finally:
        if previous is None:
            del sys.modules["bpy"]
        else:
            sys.modules["bpy"] = previous


class ToolValidationTests(unittest.TestCase):
    def test_rejects_global_object_outside_current_scene_and_approved_scope(self):
        local = _object("Local")
        other_scene = _object("Other Scene")
        tools = _load_tools([local, other_scene])
        context = SimpleNamespace(mode='OBJECT', scene=SimpleNamespace(objects=_Ids([local])))
        payload = {
            "summary": "Move an object",
            "actions": [
                {
                    "type": "transform_object",
                    "arguments": {"object": "Other Scene", "location": [1.0, 2.0, 3.0]},
                }
            ],
        }
        with self.assertRaises(tools.PlanValidationError):
            tools.validate_plan(payload, context, {"Local", "Other Scene"})

        payload["actions"][0]["arguments"]["object"] = "Local"
        with self.assertRaises(tools.PlanValidationError):
            tools.validate_plan(payload, context, set())

    def test_all_primitive_operator_calls_have_explicit_transform_and_geometry(self):
        tools = _load_tools([])
        expected_geometry = {
            "cube": {"size", "calc_uvs"},
            "sphere": {"segments", "ring_count", "radius", "calc_uvs"},
            "cylinder": {"vertices", "radius", "depth", "end_fill_type", "calc_uvs"},
            "cone": {"vertices", "radius1", "radius2", "depth", "end_fill_type", "calc_uvs"},
            "plane": {"size", "calc_uvs"},
            "torus": {
                "major_segments",
                "minor_segments",
                "major_radius",
                "minor_radius",
                "generate_uvs",
            },
        }
        for primitive, geometry_fields in expected_geometry.items():
            arguments = {
                "primitive": primitive,
                "location": [1.0, 2.0, 3.0],
                "scale": [0.5, 1.0, 2.0],
            }
            kwargs = tools.primitive_operator_arguments(arguments)
            self.assertEqual(kwargs["location"], arguments["location"])
            if primitive == "torus":
                self.assertNotIn("scale", kwargs)
            else:
                self.assertEqual(kwargs["scale"], arguments["scale"])
            self.assertEqual(kwargs["align"], 'WORLD')
            self.assertEqual(kwargs["rotation"], [0.0, 0.0, 0.0])
            self.assertLessEqual(geometry_fields, set(kwargs))

    def test_documented_minimum_scale_survives_float32_canonicalization(self):
        local = _object("Local")
        tools = _load_tools([local])
        context = SimpleNamespace(mode='OBJECT', scene=SimpleNamespace(objects=_Ids([local])))
        payload = {
            "summary": "Boundary scales",
            "actions": [
                {
                    "type": "create_primitive",
                    "arguments": {"primitive": "cube", "scale": [1e-6, 1.0, 1.0]},
                },
                {
                    "type": "transform_object",
                    "arguments": {"object": "Local", "scale": [1e-6, 1.0, 1.0]},
                },
            ],
        }
        normalized = tools.normalize_plan(payload, context)
        self.assertEqual(
            normalized["actions"][0]["arguments"]["scale"][0], tools._MIN_SCALE
        )
        tools.validate_plan(normalized, context, {"Local"})
        for invalid_scale in (9.9999995e-7, 10_000.0001, 10**400):
            invalid = {
                "summary": "Invalid boundary",
                "actions": [
                    {
                        "type": "create_primitive",
                        "arguments": {
                            "primitive": "cube",
                            "scale": [invalid_scale, 1.0, 1.0],
                        },
                    }
                ],
            }
            with self.assertRaises(tools.PlanValidationError):
                tools.normalize_plan(invalid, context)

    def test_normalization_materializes_preview_defaults_and_unique_names(self):
        tools = _load_tools([_object("Reblend Cube")], [SimpleNamespace(name="Material")])
        payload = {
            "summary": "Create and shade",
            "actions": [
                {"type": "create_primitive", "arguments": {"primitive": "cube"}},
                {
                    "type": "set_material",
                    "arguments": {
                        "object": "Reblend Cube.001",
                        "name": "Material",
                        "base_color": [0.1, 0.2, 0.3, 1.0],
                    },
                },
            ],
        }
        normalized = tools.normalize_plan(payload, None)
        create = normalized["actions"][0]["arguments"]
        material = normalized["actions"][1]["arguments"]
        self.assertEqual(create["name"], "Reblend Cube.001")
        self.assertEqual(create["location"], [0.0, 0.0, 0.0])
        self.assertEqual(create["scale"], [1.0, 1.0, 1.0])
        self.assertEqual(material["name"], "Material.001")
        self.assertEqual(material["metallic"], 0.0)
        self.assertEqual(material["roughness"], 0.5)
        self.assertNotIn("name", payload["actions"][0]["arguments"])


if __name__ == "__main__":
    unittest.main()
