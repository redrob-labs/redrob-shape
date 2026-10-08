# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

import importlib.util
import json
import unittest
from pathlib import Path

_spec = importlib.util.spec_from_file_location(
    "redrob_shape_conversation_test", Path(__file__).resolve().parents[1] / "conversation.py"
)
conversation = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(conversation)


def _plan(summary="Bevel", steps=1):
    return {"summary": summary, "actions": [{"type": "delete_object", "arguments": {"object": "Cube"}}] * steps}


class ConversationTests(unittest.TestCase):
    def test_a_new_proposal_replaces_the_pending_one(self):
        turns = conversation.add_turn([], "Add a cube", _plan("A cube"), now=1)
        turns = conversation.add_turn(turns, "Rounder", _plan("Bevel"), now=2)
        self.assertEqual([t["outcome"] for t in turns], ["discarded", "pending"])
        self.assertNotIn("plan", turns[0])
        self.assertEqual(turns[1]["steps"], 1)

    def test_only_a_pending_plan_travels_and_the_newest_turns_survive_the_budget(self):
        turns = []
        for index in range(12):
            turns = conversation.add_turn(turns, f"Step {index}", _plan(f"Plan {index}"), now=index)
            turns = conversation.set_last_outcome(turns, "applied")
        turns = conversation.add_turn(turns, "Rounder", _plan("Bevel"), now=99)
        sent = conversation.request_history(turns, 10_000)
        self.assertEqual(len(sent), conversation.MAX_TURNS)
        self.assertEqual(sent[-1]["instruction"], "Rounder")
        self.assertIn("plan", sent[-1])
        self.assertTrue(all("plan" not in turn for turn in sent[:-1]))
        self.assertEqual(set(sent[-1]["plan"]), {"summary", "actions"})

        tight = conversation.request_history(turns, 300)
        self.assertLessEqual(len(json.dumps(tight, ensure_ascii=False, separators=(",", ":")).encode()), 300)
        self.assertEqual(tight[-1]["instruction"], "Rounder")
        self.assertNotIn("plan", tight[-1])  # dropped before the turn itself
        self.assertEqual(conversation.request_history(turns, 10), [])

    def test_text_is_bounded_for_the_agent(self):
        turns = conversation.add_turn([], "가" * 2000, _plan("s"))
        sent = conversation.request_history(turns, 60_000)
        self.assertLessEqual(len(sent[0]["instruction"].encode()), conversation.MAX_TURN_TEXT_BYTES)

    def test_malformed_history_is_dropped_not_fatal(self):
        self.assertEqual(conversation.load("not json"), [])
        self.assertEqual(conversation.load('{"a": 1}'), [])
        good = {"instruction": "a", "summary": "b", "outcome": "applied"}
        self.assertEqual(conversation.load(json.dumps([good, {"instruction": 1}, {**good, "outcome": "x"}])), [good])

    def test_switched_off_steps_are_left_out(self):
        plan = {"summary": "s", "actions": [{"n": 1}, {"n": 2}, {"n": 3}]}
        self.assertEqual(conversation.enabled_plan(plan, [True, False, True])["actions"], [{"n": 1}, {"n": 3}])


if __name__ == "__main__":
    unittest.main()
