# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""The chat the sidebar draws: chat.py, without Blender."""

import importlib.util
import json
import unittest
from pathlib import Path
from types import SimpleNamespace

ROOT = Path(__file__).resolve().parents[1]
_spec = importlib.util.spec_from_file_location("redrob_shape_chat_test", ROOT / "chat.py")
chat = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(chat)


def _session():
    return SimpleNamespace(chat_json="[]", status="IDLE", status_message="")


class ChatTests(unittest.TestCase):
    def test_a_round_reads_in_order(self):
        current = _session()
        plan = [{"type": "add_modifier", "arguments": {"object": "Cube", "modifier": "bevel", "width": 0.1}}]
        chat.say(current, "user", text="round the edges")
        chat.say(current, "agent", "plan", "Bevel the cube", turn=0, steps=plan)
        chat.say(current, "event", "applied", steps=plan)
        chat.say(current, "event", "undone")
        messages = chat.load(current.chat_json)
        self.assertEqual(
            [(m["role"], m.get("kind")) for m in messages],
            [("user", None), ("agent", "plan"), ("event", "applied"), ("event", "undone")],
        )
        # Only what names a step is kept: the line reads "Bevel · Cube", values stay out.
        self.assertEqual(messages[2]["steps"], [{"type": "add_modifier", "arguments": {"object": "Cube", "modifier": "bevel"}}])
        self.assertEqual(chat.latest_plan_index(messages), 1)

    def test_fail_sets_the_status_and_answers_in_the_chat(self):
        current = _session()
        chat.fail(current, "This workspace is out of credit")
        self.assertEqual(current.status, "ERROR")
        self.assertEqual(current.status_message, "This workspace is out of credit")
        self.assertEqual(chat.load(current.chat_json)[-1], {**chat.load(current.chat_json)[-1], "role": "agent", "kind": "error"})

    def test_repeated_events_collapse(self):
        current = _session()
        chat.say(current, "event", "undone")
        chat.say(current, "event", "undone")
        chat.say(current, "event", "redone")
        self.assertEqual([m["kind"] for m in chat.load(current.chat_json)], ["undone", "redone"])

    def test_unknown_kinds_are_refused_and_malformed_rows_dropped(self):
        with self.assertRaises(ValueError):
            chat.message("agent", "shout", "hi")
        with self.assertRaises(ValueError):
            chat.message("event", "exploded")
        raw = json.dumps([{"role": "user", "text": "ok"}, {"role": "agent"}, "junk", {"role": "event", "kind": "nope"}])
        self.assertEqual(chat.load(raw), [{"role": "user", "text": "ok"}])
        self.assertEqual(chat.load("not json"), [])

    def test_the_chat_is_bounded(self):
        current = _session()
        for number in range(chat.MAX_MESSAGES + 10):
            chat.say(current, "user", text=f"message {number}")
        messages = chat.load(current.chat_json)
        self.assertEqual(len(messages), chat.MAX_MESSAGES)
        self.assertEqual(messages[-1]["text"], f"message {chat.MAX_MESSAGES + 9}")
        chat.say(current, "user", text="x" * 10_000)
        self.assertLessEqual(len(chat.load(current.chat_json)[-1]["text"].encode()), chat.MAX_TEXT_BYTES)

    def test_visible_keeps_the_newest_and_counts_the_rest(self):
        messages = [{"role": "user", "text": str(n)} for n in range(20)]
        shown, hidden = chat.visible(messages, 14)
        self.assertEqual(hidden, 6)
        self.assertEqual([index for index, _ in shown], list(range(6, 20)))
        shown, hidden = chat.visible(messages, 0)
        self.assertEqual((len(shown), hidden), (20, 0))


if __name__ == "__main__":
    unittest.main()
