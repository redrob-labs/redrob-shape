# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Bounded, privacy-conscious Blender scene context collection."""

import hashlib
import json
from itertools import islice

_MAX_OBJECTS = 48
_MAX_MODIFIERS = 8
_MAX_MATERIALS = 8
# Leave space in the 64 KiB request for a 16 KiB instruction and envelope.
_MAX_CONTEXT_BYTES = 40 * 1024


def _vector(value, size=3):
    return [float(value[index]) for index in range(size)]


def _object_summary(obj, selected):
    modifiers = [
        {"name": modifier.name[:128], "type": modifier.type}
        for modifier in islice(obj.modifiers, _MAX_MODIFIERS)
    ]
    materials = []
    data = getattr(obj, "data", None)
    object_slots = getattr(obj, "material_slots", None)
    if object_slots is None:
        material_slots = list(getattr(data, "materials", ()))
        effective_materials = material_slots
        material_links = ['DATA'] * len(material_slots)
    else:
        object_slots = list(object_slots)
        effective_materials = [slot.material for slot in object_slots]
        material_links = [slot.link for slot in object_slots]
    for slot in islice(effective_materials, _MAX_MATERIALS):
        materials.append(slot.name[:128] if slot is not None else None)
    active_material_index = int(getattr(obj, "active_material_index", 0))
    active_material = None
    if 0 <= active_material_index < len(effective_materials):
        slot = effective_materials[active_material_index]
        active_material = slot.name[:128] if slot is not None else None
    summary = {
        "name": obj.name[:128],
        "type": obj.type,
        "editable": obj.library is None and getattr(data, "library", None) is None,
        "selected": obj.name in selected,
        "location": _vector(obj.location),
        "rotation": _vector(obj.rotation_euler),
        "scale": _vector(obj.scale),
        "dimensions": _vector(obj.dimensions),
        "active_material_index": active_material_index,
        "active_material": active_material,
        "active_material_link": (
            material_links[active_material_index]
            if 0 <= active_material_index < len(material_links)
            else None
        ),
        "material_slot_count": len(effective_materials),
        "material_links": material_links[:_MAX_MATERIALS],
        "modifiers": modifiers,
        "materials": materials,
    }
    # Mesh size, so the model can judge "more detail" and the revision sees an edited mesh.
    vertices = getattr(data, "vertices", None) if obj.type == 'MESH' else None
    if vertices is not None:
        summary["mesh"] = {"vertices": len(vertices), "faces": len(getattr(data, "polygons", ()))}
    return summary


def _canonical(summary):
    return json.dumps(summary, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def build_scene_context(context, scope='SCENE'):
    """Return a byte-bounded scene summary and its stable revision.

    With ``scope='SELECTION'`` the summary also names the objects the user chose to shape, as
    ``target``; ``target_objects`` gives the same set for checking the plan.
    """
    selected = {obj.name for obj in context.selected_objects}
    active = context.active_object.name if context.active_object else None
    objects = list(context.scene.objects)
    objects.sort(key=lambda obj: (obj.name != active, obj.name not in selected, obj.name.casefold()))
    summary = {
        "schema": 1,
        "scene": context.scene.name[:128],
        "view_layer": context.view_layer.name[:128],
        "active_collection": context.collection.name[:128],
        "mode": context.mode,
        "active_object": active,
        "selected_objects": [],
        "unit_system": context.scene.unit_settings.system,
        "objects": [],
        "object_count": len(objects),
        "objects_truncated": False,
    }

    for obj in objects[:_MAX_OBJECTS]:
        summary["objects"].append(_object_summary(obj, selected))
        captured = {item["name"] for item in summary["objects"]}
        summary["selected_objects"] = sorted(selected & captured)
        if len(_canonical(summary).encode("utf-8")) > _MAX_CONTEXT_BYTES:
            summary["objects"].pop()
            captured = {item["name"] for item in summary["objects"]}
            summary["selected_objects"] = sorted(selected & captured)
            break

    summary["objects_truncated"] = len(summary["objects"]) < len(objects)
    if scope == 'SELECTION':
        summary["target"] = list(summary["selected_objects"])
    canonical = _canonical(summary)
    revision = hashlib.sha256(canonical.encode("utf-8")).hexdigest()
    return summary, revision


def target_objects(summary):
    """The objects a plan built from this summary may touch (besides those it creates)."""
    if "target" in summary:
        return set(summary["target"])
    return {item["name"] for item in summary.get("objects", ())}
