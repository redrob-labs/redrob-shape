# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""The shaping conversation: what was asked, what was proposed, and what became of it.

Pure Python, no bpy. The panel's history and every plan request read from here, so "rounder"
can mean rounder than the last plan. The agent checks the same limits (``conversation.rs``).
"""

import json
import time

MAX_TURNS = 8  # sent to the agent; the agent refuses more
MAX_KEPT_TURNS = 32  # shown in the panel's history
MAX_TURN_TEXT_BYTES = 2048
OUTCOMES = ("pending", "applied", "discarded", "undone", "failed")


def _bounded(text, maximum=MAX_TURN_TEXT_BYTES):
    text = " ".join(str(text).split())
    encoded = text.encode("utf-8")
    if len(encoded) <= maximum:
        return text
    return encoded[: maximum - 3].decode("utf-8", errors="ignore") + "..."


def load(raw):
    """Turns from the session's JSON, dropping anything malformed instead of failing the panel."""
    try:
        turns = json.loads(raw or "[]")
    except ValueError:
        return []
    if not isinstance(turns, list):
        return []
    kept = []
    for turn in turns:
        if (
            isinstance(turn, dict)
            and isinstance(turn.get("instruction"), str)
            and isinstance(turn.get("summary"), str)
            and turn.get("outcome") in OUTCOMES
        ):
            kept.append(turn)
    return kept[-MAX_KEPT_TURNS:]


def dump(turns):
    return json.dumps(turns[-MAX_KEPT_TURNS:], ensure_ascii=False, separators=(",", ":"))


def add_turn(turns, instruction, plan, outcome="pending", now=None):
    """Record a new proposal. An older pending proposal it replaces becomes discarded."""
    turns = [dict(turn) for turn in turns]
    for turn in turns:
        if turn["outcome"] == "pending":
            turn["outcome"] = "discarded"
            turn.pop("plan", None)
    turns.append(
        {
            "instruction": _bounded(instruction) or "(empty)",
            "summary": _bounded(plan.get("summary", "")) or "(no summary)",
            "outcome": outcome,
            "steps": len(plan.get("actions", ())),
            "plan": plan,
            "at": time.time() if now is None else now,
        }
    )
    return turns


def set_last_outcome(turns, outcome, steps=None):
    """Change what became of the newest turn. Only a pending turn keeps its plan."""
    if not turns:
        return turns
    turns = [dict(turn) for turn in turns]
    last = turns[-1]
    last["outcome"] = outcome
    if steps is not None:
        last["steps"] = steps
    return turns


def request_history(turns, budget_bytes):
    """The bounded tail the agent gets: newest turns first to survive, plan only while pending.

    Returns the longest suffix of the conversation whose JSON fits ``budget_bytes``. A pending
    plan that does not fit is sent as its summary alone rather than dropping the turn.
    """
    shaped = []
    for turn in turns[-MAX_TURNS:]:
        item = {
            "instruction": _bounded(turn["instruction"]),
            "summary": _bounded(turn["summary"]),
            "outcome": turn["outcome"],
        }
        if turn["outcome"] == "pending" and isinstance(turn.get("plan"), dict):
            item["plan"] = {"summary": turn["plan"].get("summary", ""), "actions": turn["plan"].get("actions", [])}
        shaped.append(item)

    def size(items):
        return len(json.dumps(items, ensure_ascii=False, separators=(",", ":")).encode("utf-8"))

    while shaped and size(shaped) > budget_bytes:
        if "plan" in shaped[-1]:
            del shaped[-1]["plan"]
        else:
            shaped.pop(0)
    return shaped


def enabled_plan(plan, enabled):
    """The plan with only the steps the user left switched on."""
    actions = plan.get("actions", [])
    return {
        "summary": plan.get("summary", ""),
        "actions": [action for action, keep in zip(actions, enabled) if keep],
    }
