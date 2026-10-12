# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Cases both plan validators must judge alike, generated from tool_catalog.json.

The agent (Rust) and the add-on (Python) each check a plan against the catalog, in their own
language. Agreeing on the catalog is not the same as agreeing on what it means, so this file turns
the catalog into concrete plans with a known verdict -- every boundary, every wrong type, every
missing field -- and both test suites run all of them: validation.rs and test_conformance.py.

    python3 tests/conformance.py --write    # after editing the catalog

The committed tool_conformance.json must equal what this produces; a test checks that too.
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
CATALOG_PATH = HERE.parent / "tool_catalog.json"
CASES_PATH = HERE / "tool_conformance.json"

_NAME_KINDS = {"object", "name", "material_name", "modifier_name"}


def _valid_value(field):
    kind = field["type"]
    if kind in _NAME_KINDS:
        return "Paint" if kind == "material_name" else "Cube"
    if kind == "enum":
        return field["values"][0]
    if kind == "boolean":
        return True
    if kind == "integer":
        return int(field.get("min", 1))
    middle = (field.get("min", 0) + field.get("max", 2)) / 2
    if kind == "number":
        return middle
    return [middle] * (3 if kind == "vector3" else 4)


def _wrong_type(field):
    kind = field["type"]
    if kind in _NAME_KINDS or kind == "enum":
        return 5
    if kind == "boolean":
        return "true"
    if kind in ("number", "integer"):
        return "1"
    return [0.0, 0.0]  # wrong length


def _plan(action_type, arguments, summary="Shape it"):
    return {"summary": summary, "actions": [{"type": action_type, "arguments": arguments}]}


def _field_sets(catalog):
    """(label, action type, base arguments, fields) for every tool and every variant."""
    for name, tool in catalog["tools"].items():
        common = tool.get("fields", {})
        variant_field = tool.get("variant_field")
        if variant_field is None:
            yield name, name, {}, common
            continue
        for variant, details in tool["variants"].items():
            fields = dict(common)
            fields.update(details.get("fields", {}))
            yield f"{name}.{variant}", name, {variant_field: variant}, fields


def cases(catalog):
    out = []

    def case(label, plan, valid):
        out.append({"name": label, "plan": plan, "valid": valid})

    for label, action_type, base, fields in _field_sets(catalog):
        required = {k: _valid_value(f) for k, f in fields.items() if f.get("required")}
        complete = {k: _valid_value(f) for k, f in fields.items()}
        minimal = {**base, **required}
        at_least_one = catalog["tools"][action_type].get("at_least_one_of", [])
        if at_least_one:
            minimal[at_least_one[0]] = _valid_value(fields[at_least_one[0]])
            case(f"{label}: none of {at_least_one}", _plan(action_type, {**base, **required}), False)
        case(f"{label}: minimal", _plan(action_type, minimal), True)
        case(f"{label}: every field", _plan(action_type, {**base, **complete}), True)
        case(f"{label}: unknown argument", _plan(action_type, {**minimal, "script": "x"}), False)

        for field_name, field in fields.items():
            kind = field["type"]
            if field.get("required"):
                missing = {k: v for k, v in minimal.items() if k != field_name}
                case(f"{label}.{field_name}: missing", _plan(action_type, missing), False)
                case(f"{label}.{field_name}: null", _plan(action_type, {**minimal, field_name: None}), False)
            elif field_name not in at_least_one:
                case(f"{label}.{field_name}: null means absent", _plan(action_type, {**minimal, field_name: None}), True)
            def variant(value, valid, why):
                case(f"{label}.{field_name}: {why}", _plan(action_type, {**minimal, field_name: value}), valid)
            variant(_wrong_type(field), False, "wrong type")
            if kind in _NAME_KINDS:
                variant("", False, "empty")
                variant("   ", False, "blank")
                variant("n" * 63, True, "63 bytes")
                variant("n" * 64, False, "64 bytes")
                variant("한" * 21, True, "63 bytes of Hangul")
                variant("한" * 22, False, "66 bytes of Hangul")
                variant("Cube\u0007", False, "control character")
                variant("Cube\u0085", False, "C1 control character")
            elif kind == "enum":
                for allowed in field["values"]:
                    variant(allowed, True, f"value {allowed}")
                variant("nope", False, "unknown value")
            elif kind == "boolean":
                variant(False, True, "false")
                variant(1, False, "integer")
            elif kind in ("number", "integer", "vector3", "vector4"):
                low, high = field.get("min"), field.get("max")
                size = {"vector3": 3, "vector4": 4}.get(kind)

                def shaped(number):
                    return [number] * size if size else number

                if low is not None:
                    variant(shaped(low), True, "at min")
                    variant(shaped(low - (0.5 if kind != "integer" else 1)), False, "below min")
                if high is not None:
                    variant(shaped(high), True, "at max")
                    variant(shaped(high + (0.5 if kind != "integer" else 1)), False, "above max")
                variant(shaped(True), False, "boolean in place of a number")
                if kind == "integer":
                    variant(float(_valid_value(field)) + 0.5, False, "fractional")
                if size:
                    variant([_valid_value({**field, "type": "number"})] * (size + 1), False, "too long")

    first = next(iter(catalog["tools"]))
    tool = catalog["tools"][first]
    sample = {k: _valid_value(f) for k, f in tool.get("fields", {}).items() if f.get("required")}
    limits = catalog["limits"]
    case("plan: unknown tool", _plan("run_python", {}), False)
    case("plan: empty summary", _plan(first, sample, summary=" "), False)
    case("plan: summary at the limit", _plan(first, sample, summary="s" * limits["max_summary_bytes"]), True)
    case("plan: summary over the limit", _plan(first, sample, summary="s" * (limits["max_summary_bytes"] + 1)), False)
    many = {"summary": "Many", "actions": [{"type": first, "arguments": sample}] * limits["max_actions"]}
    case("plan: actions at the limit", many, True)
    case("plan: too many actions", {**many, "actions": many["actions"] + many["actions"][:1]}, False)
    case("plan: extra field on an action", {"summary": "x", "actions": [{"type": first, "arguments": sample, "run": True}]}, False)
    case("plan: extra field on the plan", {"summary": "x", "actions": [], "run": True}, False)
    case("plan: arguments not an object", {"summary": "x", "actions": [{"type": first, "arguments": []}]}, False)
    case("plan: no actions", {"summary": "x", "actions": []}, True)
    for name, tool in catalog["tools"].items():
        if tool.get("variant_field"):
            object_only = {"object": "Cube"}
            case(f"{name}: unknown {tool['variant_field']}", _plan(name, {**object_only, tool["variant_field"]: "nope"}), False)
            case(f"{name}: no {tool['variant_field']}", _plan(name, object_only), False)
    return out


def load_catalog():
    return json.loads(CATALOG_PATH.read_text(encoding="utf-8"))


def render(generated):
    return json.dumps(generated, ensure_ascii=False, indent=1) + "\n"


if __name__ == "__main__":
    text = render(cases(load_catalog()))
    if "--write" in sys.argv[1:]:
        CASES_PATH.write_text(text, encoding="utf-8")
        print(f"wrote {CASES_PATH} ({text.count(chr(10))} lines)")
    else:
        sys.stdout.write(text)
