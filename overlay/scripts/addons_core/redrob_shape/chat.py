# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""The chat the sidebar shows: what you said, what Redrob answered, and what happened after.

Pure Python, no bpy, so it is tested without Blender. It is separate from ``conversation.py`` on
purpose: that one is what the AGENT is told (bounded, plans only while pending), this one is what
the USER reads, and it also keeps the messages the agent never sees -- a refused request, a
cancel, an Undo.

A message is a dict:

    {"role": "user",  "text": "make it rounder"}
    {"role": "agent", "kind": "plan",  "text": <summary>, "turn": <index into conversation>}
    {"role": "agent", "kind": "error", "text": <what went wrong>}
    {"role": "event", "kind": "applied", "steps": [<action>, ...]}
    {"role": "event", "kind": "undone" | "redone" | "kept" | "discarded" | "cancelled" | "retry"}

Event and step text is drawn by the panel, in the user's language at draw time; only what a
person or the agent wrote is stored as text.
"""

import json
import time

MAX_MESSAGES = 80
MAX_TEXT_BYTES = 2048
MAX_EVENT_STEPS = 32

ROLES = ("user", "agent", "event")
AGENT_KINDS = ("plan", "error", "info")
EVENT_KINDS = ("applied", "undone", "redone", "kept", "discarded", "cancelled", "retry")

#: The action fields an "applied" line needs to name its steps; nothing else is kept.
_STEP_FIELDS = ("object", "name", "primitive", "modifier")


def _bounded(text):
    text = str(text).strip()
    encoded = text.encode("utf-8")
    if len(encoded) <= MAX_TEXT_BYTES:
        return text
    return encoded[: MAX_TEXT_BYTES - 3].decode("utf-8", errors="ignore") + "..."


def _valid(message):
    if not isinstance(message, dict) or message.get("role") not in ROLES:
        return False
    role = message["role"]
    if role == "user":
        return isinstance(message.get("text"), str)
    if role == "agent":
        return message.get("kind") in AGENT_KINDS and isinstance(message.get("text"), str)
    return message.get("kind") in EVENT_KINDS


def load(raw):
    """Messages from the session's JSON. Anything malformed is dropped, never drawn."""
    try:
        messages = json.loads(raw or "[]")
    except ValueError:
        return []
    if not isinstance(messages, list):
        return []
    return [message for message in messages if _valid(message)][-MAX_MESSAGES:]


def dump(messages):
    return json.dumps(messages[-MAX_MESSAGES:], ensure_ascii=False, separators=(",", ":"))


def _step(action):
    if not isinstance(action, dict):
        return {"type": "unknown", "arguments": {}}
    arguments = action.get("arguments") if isinstance(action.get("arguments"), dict) else {}
    kept = {key: str(arguments[key]) for key in _STEP_FIELDS if key in arguments}
    return {"type": str(action.get("type", "unknown")), "arguments": kept}


def message(role, kind=None, text=None, turn=None, steps=None, now=None):
    """One well-formed message. Raises ValueError for a role or kind the panel cannot draw."""
    if role not in ROLES:
        raise ValueError(f"unknown chat role {role!r}")
    item = {"role": role, "at": time.time() if now is None else now}
    if role == "agent" and kind not in AGENT_KINDS:
        raise ValueError(f"unknown agent message kind {kind!r}")
    if role == "event" and kind not in EVENT_KINDS:
        raise ValueError(f"unknown chat event {kind!r}")
    if kind is not None and role != "user":
        item["kind"] = kind
    if role != "event":
        item["text"] = _bounded(text or "")
    if turn is not None:
        item["turn"] = int(turn)
    if steps is not None:
        item["steps"] = [_step(action) for action in list(steps)[:MAX_EVENT_STEPS]]
    return item


def append(messages, item):
    """The chat with ``item`` added. Two identical events in a row collapse into one."""
    messages = list(messages)
    last = messages[-1] if messages else None
    if (
        item["role"] == "event"
        and last is not None
        and last.get("role") == "event"
        and last.get("kind") == item.get("kind")
        and last.get("steps") == item.get("steps")
    ):
        return messages
    messages.append(item)
    return messages[-MAX_MESSAGES:]


def say(current, role, kind=None, text=None, turn=None, steps=None):
    """Append to the chat kept on ``current`` (the session property group, or any object with a
    ``chat_json`` string; one without it starts an empty chat)."""
    current.chat_json = dump(
        append(load(getattr(current, "chat_json", "[]")), message(role, kind, text, turn, steps))
    )


def fail(current, text):
    """The session needs attention: say so in the status AND as Redrob's answer in the chat."""
    current.status = 'ERROR'
    current.status_message = str(text)
    say(current, "agent", "error", text)


def latest_plan_index(messages):
    """Index of the newest plan answer, or -1. Only that one carries the live plan card."""
    for index in range(len(messages) - 1, -1, -1):
        item = messages[index]
        if item["role"] == "agent" and item.get("kind") == "plan":
            return index
    return -1


def visible(messages, limit):
    """The tail the panel draws, and how many earlier messages it leaves out."""
    if limit <= 0 or len(messages) <= limit:
        return list(enumerate(messages)), 0
    start = len(messages) - limit
    return list(enumerate(messages))[start:], start
