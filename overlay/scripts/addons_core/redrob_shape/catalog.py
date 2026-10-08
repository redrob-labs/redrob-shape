# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""The tool catalog, and the structural check every plan passes before Blender looks at it.

Pure Python, no bpy: the same rules as the agent's ``validation.rs``, read from the same
``tool_catalog.json``. ``tests/tool_conformance.json`` holds cases both must judge alike.
Scene rules (does the object exist, is it a mesh) live in ``tools.py``; this file only answers
"is this a plan the catalog allows at all".
"""

import copy
import json
import math
import unicodedata
from pathlib import Path

CATALOG_PATH = Path(__file__).resolve().parent / "tool_catalog.json"
_NAME_KINDS = frozenset({"object", "name", "material_name", "modifier_name"})


class PlanShapeError(ValueError):
    """The plan is not one the catalog allows."""


def _load():
    with open(CATALOG_PATH, encoding="utf-8") as handle:
        return json.load(handle)


CATALOG = _load()
LIMITS = CATALOG["limits"]
TOOLS = CATALOG["tools"]
FACE_DEGREES = float(CATALOG["face_selector_degrees"])


def fields_for(action_type, arguments):
    """Every field this action may carry: the tool's own plus the chosen variant's.

    Returns ``(tool, fields)``; raises when the tool or its variant is unknown.
    """
    tool = TOOLS.get(action_type) if isinstance(action_type, str) else None
    if tool is None:
        raise PlanShapeError(f"unsupported action: {_reported(action_type)}")
    fields = dict(tool.get("fields", {}))
    variant_field = tool.get("variant_field")
    if variant_field is not None:
        chosen = arguments.get(variant_field) if isinstance(arguments, dict) else None
        variant = tool["variants"].get(chosen) if isinstance(chosen, str) else None
        if variant is None:
            raise PlanShapeError("modifier arguments do not match the selected modifier")
        fields.update(variant.get("fields", {}))
    return tool, fields


def note(action_type):
    """The catalog's warning for a step that edits or removes something, or ``None``."""
    tool = TOOLS.get(action_type)
    return tool.get("note") if tool else None


def mesh_only(action_type):
    tool = TOOLS.get(action_type)
    return bool(tool and tool.get("mesh_only"))


def _reported(value):
    text = value if isinstance(value, str) else repr(value)
    return "".join(c for c in text if unicodedata.category(c) != "Cc")[:48]


def _is_control(character):
    # Rust's char::is_control is exactly the Unicode Cc category: C0, DEL and C1.
    return unicodedata.category(character) == "Cc"


def check_name(value):
    if not isinstance(value, str):
        raise PlanShapeError("name has the wrong type")
    if (
        not value.strip()
        or len(value.encode("utf-8")) > LIMITS["max_name_bytes"]
        or any(_is_control(c) for c in value)
    ):
        raise PlanShapeError("name is empty or too large")
    return value


def _is_number(value):
    return isinstance(value, (int, float)) and not isinstance(value, bool)


def _in_range(value, field, round_number=None):
    try:
        value = float(value)
    except OverflowError as exc:
        raise PlanShapeError("numeric argument is not finite or is outside its allowed range") from exc
    low, high = field.get("min"), field.get("max")
    if round_number is not None and math.isfinite(value):
        # A plan already rounded to what Blender stores is checked against bounds rounded the same
        # way. Rounding is monotonic, so every value that was in range stays in range.
        value = round_number(value)
        low = None if low is None else round_number(low)
        high = None if high is None else round_number(high)
    if not math.isfinite(value) or (low is not None and value < low) or (high is not None and value > high):
        raise PlanShapeError("numeric argument is not finite or is outside its allowed range")


def check_value(name, field, value, round_number=None):
    kind = field["type"]
    if kind in _NAME_KINDS:
        if not isinstance(value, str):
            raise PlanShapeError(f"argument has the wrong type: {name}")
        check_name(value)
    elif kind == "enum":
        if not isinstance(value, str):
            raise PlanShapeError(f"argument has the wrong type: {name}")
        if value not in field["values"]:
            raise PlanShapeError(f"argument is not one of the allowed values: {name}")
    elif kind == "boolean":
        if not isinstance(value, bool):
            raise PlanShapeError(f"argument has the wrong type: {name}")
    elif kind == "number":
        if not _is_number(value):
            raise PlanShapeError(f"argument has the wrong type: {name}")
        _in_range(value, field, round_number)
    elif kind == "integer":
        # 2.0 is not an integer, here or in the agent.
        if not isinstance(value, int) or isinstance(value, bool):
            raise PlanShapeError(f"argument has the wrong type: {name}")
        _in_range(value, field, round_number)
    elif kind in ("vector3", "vector4"):
        size = 3 if kind == "vector3" else 4
        if not isinstance(value, list) or len(value) != size:
            raise PlanShapeError(f"argument has the wrong type: {name}")
        for item in value:
            if not _is_number(item):
                raise PlanShapeError(f"argument has the wrong type: {name}")
            _in_range(item, field, round_number)
    else:  # pragma: no cover - the catalog test pins the kinds
        raise PlanShapeError(f"unknown field type in the catalog: {kind}")


def _check_action(action, round_number=None):
    if not isinstance(action, dict) or set(action) != {"type", "arguments"}:
        raise PlanShapeError("action must have exactly a type and arguments")
    arguments = action["arguments"]
    if not isinstance(arguments, dict):
        raise PlanShapeError("arguments must be an object")
    tool, fields = fields_for(action["type"], arguments)
    variant_field = tool.get("variant_field")
    for name in arguments:
        if name != variant_field and name not in fields:
            if variant_field is not None:
                raise PlanShapeError("modifier arguments do not match the selected modifier")
            raise PlanShapeError(f"unsupported argument: {_reported(name)}")
    for name, field in fields.items():
        if name in arguments:
            check_value(name, field, arguments[name], round_number)
        elif field.get("required"):
            if variant_field is not None:
                raise PlanShapeError("modifier arguments do not match the selected modifier")
            raise PlanShapeError(f"missing argument: {name}")
    at_least_one = tool.get("at_least_one_of", [])
    if at_least_one and not any(name in arguments for name in at_least_one):
        raise PlanShapeError(f"{action['type']} must specify at least one of {', '.join(at_least_one)}")


def check_plan(payload, round_number=None):
    """Return a copy of the plan with ``null`` arguments dropped, or raise ``PlanShapeError``.

    Models often send ``null`` for an optional argument they do not use; the agent reads that as
    absent, so this does too. A required argument sent as null is then simply missing.
    """
    if not isinstance(payload, dict) or set(payload) != {"summary", "actions"}:
        raise PlanShapeError("plan must have exactly a summary and actions")
    summary = payload["summary"]
    if (
        not isinstance(summary, str)
        or not summary.strip()
        or len(summary.encode("utf-8")) > LIMITS["max_summary_bytes"]
    ):
        raise PlanShapeError("summary is empty or too large")
    actions = payload["actions"]
    if not isinstance(actions, list):
        raise PlanShapeError("actions must be a list")
    if len(actions) > LIMITS["max_actions"]:
        raise PlanShapeError("plan contains too many actions")
    cleaned = copy.deepcopy(payload)
    for action in cleaned["actions"]:
        if isinstance(action, dict) and isinstance(action.get("arguments"), dict):
            action["arguments"] = {k: v for k, v in action["arguments"].items() if v is not None}
        _check_action(action, round_number)
    return cleaned


def default_arguments(action_type, arguments):
    """Catalog defaults for the optional fields this action left out."""
    try:
        _, fields = fields_for(action_type, arguments)
    except PlanShapeError:
        return {}
    return {
        name: copy.deepcopy(field["default"])
        for name, field in fields.items()
        if "default" in field and name not in arguments
    }
