# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

import importlib.util
import json
import unittest
from pathlib import Path
from types import SimpleNamespace


def _load_context_module():
    path = Path(__file__).resolve().parents[1] / "context.py"
    spec = importlib.util.spec_from_file_location("redrob_reblend_context_test", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


scene_context = _load_context_module()


class _Objects(list):
    def get(self, name):
        return next((item for item in self if item.name == name), None)


class ContextTests(unittest.TestCase):
    def _context(self, count=100):
        objects = _Objects()
        for index in range(count):
            data = SimpleNamespace(library=None, materials=[])
            objects.append(
                SimpleNamespace(
                    name=f"Object {index:03d}",
                    type='MESH',
                    library=None,
                    data=data,
                    location=(index, 0.0, 0.0),
                    rotation_euler=(0.0, 0.0, 0.0),
                    scale=(1.0, 1.0, 1.0),
                    dimensions=(1.0, 1.0, 1.0),
                    active_material_index=0,
                    modifiers=[],
                )
            )
        scene = SimpleNamespace(
            name="Scene",
            objects=objects,
            unit_settings=SimpleNamespace(system='METRIC'),
        )
        return SimpleNamespace(
            scene=scene,
            view_layer=SimpleNamespace(name="ViewLayer"),
            collection=SimpleNamespace(name="Collection"),
            mode='OBJECT',
            active_object=objects[-1],
            selected_objects=[objects[0], objects[-1]],
        )

    def test_context_is_bounded_and_scope_matches_serialized_objects(self):
        summary, revision = scene_context.build_scene_context(self._context())
        encoded = json.dumps(
            summary, sort_keys=True, separators=(",", ":"), ensure_ascii=False
        ).encode("utf-8")
        captured = {item["name"] for item in summary["objects"]}
        self.assertLessEqual(len(encoded), 40 * 1024)
        self.assertLessEqual(len(summary["objects"]), 48)
        self.assertEqual(set(summary["selected_objects"]), {"Object 000", "Object 099"} & captured)
        self.assertEqual(len(revision), 64)
        self.assertTrue(summary["objects_truncated"])

    def test_material_slot_positions_and_active_slot_are_fingerprinted(self):
        context = self._context(1)
        obj = context.active_object
        material_a = SimpleNamespace(name="Material A")
        material_b = SimpleNamespace(name="Material B")
        obj.data.materials = [None, material_a]
        obj.active_material_index = 0
        _, first_revision = scene_context.build_scene_context(context)
        obj.data.materials = [material_a, None]
        _, swapped_revision = scene_context.build_scene_context(context)
        self.assertNotEqual(first_revision, swapped_revision)

        obj.data.materials = [None] * 9 + [material_a]
        obj.active_material_index = 9
        _, active_a_revision = scene_context.build_scene_context(context)
        obj.data.materials[9] = material_b
        _, active_b_revision = scene_context.build_scene_context(context)
        self.assertNotEqual(active_a_revision, active_b_revision)

    def test_revision_is_deterministic_and_changes_with_scene_state(self):
        context = self._context(3)
        first_summary, first_revision = scene_context.build_scene_context(context)
        context.scene.objects.reverse()
        second_summary, second_revision = scene_context.build_scene_context(context)
        self.assertEqual(first_summary, second_summary)
        self.assertEqual(first_revision, second_revision)
        context.active_object.location = (2.000000001, 0.0, 0.0)
        _, changed_revision = scene_context.build_scene_context(context)
        self.assertNotEqual(first_revision, changed_revision)
        context.active_object.location = (2.0, 0.0, 0.0)
        context.collection.name = "Another Collection"
        _, collection_revision = scene_context.build_scene_context(context)
        self.assertNotEqual(first_revision, collection_revision)


if __name__ == "__main__":
    unittest.main()
