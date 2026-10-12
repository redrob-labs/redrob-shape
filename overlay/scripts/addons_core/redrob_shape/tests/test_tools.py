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
    directory = Path(__file__).resolve().parents[1]
    package = types.ModuleType("redrob_shape_tools_pkg")
    package.__path__ = [str(directory)]
    names = ("bpy", "redrob_shape_tools_pkg", "redrob_shape_tools_pkg.catalog", "redrob_shape_tools_pkg.tools")
    previous = {name: sys.modules.get(name) for name in names}
    sys.modules["bpy"] = bpy
    sys.modules["redrob_shape_tools_pkg"] = package
    try:
        spec = importlib.util.spec_from_file_location("redrob_shape_tools_pkg.tools", directory / "tools.py")
        module = importlib.util.module_from_spec(spec)
        sys.modules["redrob_shape_tools_pkg.tools"] = module
        spec.loader.exec_module(module)
        return module
    finally:
        for name, old in previous.items():
            if old is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = old


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
        tools = _load_tools([_object("Redrob Shape Cube")], [SimpleNamespace(name="Material")])
        payload = {
            "summary": "Create and shade",
            "actions": [
                {"type": "create_primitive", "arguments": {"primitive": "cube"}},
                {
                    "type": "set_material",
                    "arguments": {
                        "object": "Redrob Shape Cube.001",
                        "name": "Material",
                        "base_color": [0.1, 0.2, 0.3, 1.0],
                    },
                },
            ],
        }
        normalized = tools.normalize_plan(payload, None)
        create = normalized["actions"][0]["arguments"]
        material = normalized["actions"][1]["arguments"]
        self.assertEqual(create["name"], "Redrob Shape Cube.001")
        self.assertEqual(create["location"], [0.0, 0.0, 0.0])
        self.assertEqual(create["scale"], [1.0, 1.0, 1.0])
        self.assertEqual(material["name"], "Material.001")
        self.assertEqual(material["metallic"], 0.0)
        self.assertEqual(material["roughness"], 0.5)
        self.assertNotIn("name", payload["actions"][0]["arguments"])


class SceneRuleTests(unittest.TestCase):
    """What only Blender can know: the plan's names must exist, in order, with the right type."""

    def setUp(self):
        cube = _object("Cube")
        cube.modifiers = [SimpleNamespace(name="Bevel")]
        lamp = _object("Lamp")
        lamp.type = 'LIGHT'
        self.objects = [cube, _object("Cutter"), lamp]
        self.tools = _load_tools(self.objects)
        self.context = SimpleNamespace(mode='OBJECT', scene=SimpleNamespace(objects=_Ids(self.objects)))

    def _check(self, *actions):
        plan = {"summary": "s", "actions": [{"type": t, "arguments": a} for t, a in actions]}
        return self.tools.validate_plan(plan, self.context, {"Cube", "Cutter", "Lamp"})

    def _refused(self, *actions):
        with self.assertRaises(self.tools.PlanValidationError) as raised:
            self._check(*actions)
        return str(raised.exception)

    def test_mesh_tools_refuse_other_object_types(self):
        message = self._refused(("extrude_faces", {"object": "Lamp", "faces": "+z", "distance": 1.0}))
        self.assertIn("step 1", message)
        self.assertIn("needs a mesh", message)
        self._check(("transform_object", {"object": "Lamp", "location": [0.0, 0.0, 1.0]}))

    def test_boolean_needs_two_different_meshes(self):
        self._check(("boolean", {"object": "Cube", "with": "Cutter", "operation": "difference"}))
        self._refused(("boolean", {"object": "Cube", "with": "Cube", "operation": "union"}))
        self._refused(("boolean", {"object": "Cube", "with": "Lamp", "operation": "union"}))

    def test_apply_modifier_names_one_that_exists_by_then(self):
        self._check(("apply_modifier", {"object": "Cube", "modifier": "Bevel"}))
        self._refused(("apply_modifier", {"object": "Cube", "modifier": "Redrob Shape Bevel"}))
        self._check(
            ("add_modifier", {"object": "Cube", "modifier": "bevel", "width": 0.1, "segments": 2}),
            ("apply_modifier", {"object": "Cube", "modifier": "Redrob Shape Bevel"}),
        )
        message = self._refused(
            ("apply_modifier", {"object": "Cube", "modifier": "Bevel"}),
            ("apply_modifier", {"object": "Cube", "modifier": "Bevel"}),
        )
        self.assertIn("step 2", message)

    def test_a_deleted_or_renamed_object_is_gone_for_later_steps(self):
        self._refused(
            ("delete_object", {"object": "Cutter"}),
            ("transform_object", {"object": "Cutter", "location": [1.0, 0.0, 0.0]}),
        )
        self._refused(
            ("rename_object", {"object": "Cube", "name": "Box"}),
            ("transform_object", {"object": "Cube", "location": [1.0, 0.0, 0.0]}),
        )
        self._check(
            ("rename_object", {"object": "Cube", "name": "Box"}),
            ("subdivide_mesh", {"object": "Box", "cuts": 1}),
        )

    def test_new_names_must_be_free(self):
        self._refused(("rename_object", {"object": "Cube", "name": "Cutter"}))
        self._refused(("duplicate_object", {"object": "Cube", "name": "Lamp"}))
        self._check(
            ("duplicate_object", {"object": "Cube", "name": "Cube Copy"}),
            ("extrude_faces", {"object": "Cube Copy", "faces": "-x", "distance": 0.5}),
        )

    def test_a_duplicate_keeps_the_sources_modifiers_and_type(self):
        self._check(
            ("duplicate_object", {"object": "Cube", "name": "Twin"}),
            ("apply_modifier", {"object": "Twin", "modifier": "Bevel"}),
        )
        self._refused(
            ("duplicate_object", {"object": "Lamp", "name": "Lamp 2"}),
            ("shade_smooth", {"object": "Lamp 2", "smooth": True}),
        )


if __name__ == "__main__":
    unittest.main()
