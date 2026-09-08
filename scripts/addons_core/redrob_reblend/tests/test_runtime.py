# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

import importlib.util
import sys
import types
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock


def _load_runtime():
    package = types.ModuleType("redrob_reblend")
    package.__path__ = []
    protocol = types.ModuleType("redrob_reblend.protocol")
    protocol.MAX_RESPONSE_BYTES = 256 * 1024
    protocol.ProtocolError = ValueError
    protocol.encode_request = lambda *_args: b"{}\n"
    tools = types.ModuleType("redrob_reblend.tools")
    tools.PlanValidationError = ValueError
    tools.normalize_plan = lambda payload, _context: payload
    tools.validate_plan = lambda payload, _context, _allowed: payload
    bpy = types.ModuleType("bpy")
    bpy.app = SimpleNamespace(binary_path="", timers=SimpleNamespace())
    bpy.context = SimpleNamespace(window_manager=SimpleNamespace(windows=[]))

    replacements = {
        "bpy": bpy,
        "redrob_reblend": package,
        "redrob_reblend.protocol": protocol,
        "redrob_reblend.tools": tools,
    }
    previous = {name: sys.modules.get(name) for name in replacements}
    sys.modules.update(replacements)
    try:
        path = Path(__file__).resolve().parents[1] / "runtime.py"
        spec = importlib.util.spec_from_file_location("redrob_reblend.runtime", path)
        module = importlib.util.module_from_spec(spec)
        sys.modules["redrob_reblend.runtime"] = module
        spec.loader.exec_module(module)
        return module
    finally:
        for name, old_module in previous.items():
            if old_module is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = old_module
        sys.modules.pop("redrob_reblend.runtime", None)


class RuntimeRaceTests(unittest.TestCase):
    def test_start_cleans_up_when_initial_health_handshake_breaks(self):
        runtime_module = _load_runtime()

        class Pipe:
            def write(self, _payload):
                raise BrokenPipeError

            def flush(self):
                pass

            def readline(self, _limit):
                return b""

            def close(self):
                pass

        class Process:
            def __init__(self):
                self.stdin = Pipe()
                self.stdout = Pipe()
                self.stderr = Pipe()
                self.returncode = None

            def poll(self):
                return self.returncode

            def wait(self, timeout=None):
                self.returncode = 0
                return self.returncode

            def kill(self):
                self.returncode = -9

            def terminate(self):
                self.returncode = -15

        agent = runtime_module.AgentRuntime()
        agent.locate_executable = lambda: Path("/fake/redrob-reblend-agent")
        with mock.patch.object(runtime_module.subprocess, "Popen", return_value=Process()):
            self.assertFalse(agent.start())
        self.assertIsNone(agent.process)
        self.assertIn("handshake failed", agent.start_error)

    def test_health_error_invalidates_connection_and_pending_state(self):
        runtime_module = _load_runtime()
        agent = runtime_module.AgentRuntime()
        agent.requests["health-1"] = {"method": "health"}
        agent.requests["plan-1"] = {"method": "plan"}
        state = SimpleNamespace(
            status='IDLE',
            status_message="Checking",
            request_id="",
            sidecar_available=True,
        )
        context = SimpleNamespace(window_manager=SimpleNamespace(redrob_reblend=state))
        agent._handle_event(
            context,
            {
                "version": 1,
                "id": "health-1",
                "event": "error",
                "error": {"code": "bad", "message": "broken", "retryable": False},
            },
        )
        self.assertEqual(state.status, 'ERROR')
        self.assertFalse(state.sidecar_available)
        self.assertEqual(agent.requests, {})

    def test_invalid_raw_event_clears_pending_request(self):
        runtime_module = _load_runtime()
        runtime_module.protocol.decode_event = mock.Mock(
            side_effect=runtime_module.protocol.ProtocolError("event is not valid JSON")
        )
        agent = runtime_module.AgentRuntime()
        agent.requests["plan-raw"] = {"method": "plan"}
        agent.events.put({"generation": agent.generation, "line": b"invalid\n"})
        state = SimpleNamespace(
            status='WORKING',
            status_message="Planning",
            request_id="plan-raw",
            sidecar_available=True,
        )
        context = SimpleNamespace(window_manager=SimpleNamespace(redrob_reblend=state))
        agent.drain(context)
        self.assertEqual(state.status, 'ERROR')
        self.assertEqual(state.request_id, "")
        self.assertFalse(state.sidecar_available)
        self.assertEqual(agent.requests, {})

    def test_plan_validation_error_clears_working_request(self):
        runtime_module = _load_runtime()
        agent = runtime_module.AgentRuntime()
        plan_id = "plan-overflow"
        agent.requests[plan_id] = {
            "method": "plan",
            "scene_revision": "revision",
            "captured_objects": [],
        }
        runtime_module.tools.normalize_plan = mock.Mock(
            side_effect=runtime_module.tools.PlanValidationError("numeric value is outside range")
        )
        state = SimpleNamespace(
            status='WORKING',
            status_message="Planning",
            request_id=plan_id,
            scene_revision="revision",
            sidecar_available=True,
        )
        context = SimpleNamespace(window_manager=SimpleNamespace(redrob_reblend=state))
        agent._handle_event(
            context,
            {
                "version": 1,
                "id": plan_id,
                "event": "success",
                "result": {"summary": "Bad number", "actions": []},
            },
        )
        self.assertEqual(state.status, 'ERROR')
        self.assertEqual(state.request_id, "")
        self.assertIn("Rejected plan", state.status_message)

    def test_plan_result_cannot_win_after_cancel_was_requested(self):
        runtime_module = _load_runtime()
        agent = runtime_module.AgentRuntime()
        plan_id = "plan-1"
        cancel_id = "cancel-1"
        agent.requests[plan_id] = {
            "method": "plan",
            "scene_revision": "revision",
            "captured_objects": [],
        }
        agent.cancelled_requests.add(plan_id)
        state = SimpleNamespace(
            status='WORKING',
            status_message="Planning",
            request_id=plan_id,
            scene_revision="revision",
            plan_json="",
            allowed_objects_json="[]",
            plan_summary="",
            response_text="",
            sidecar_available=True,
        )
        context = SimpleNamespace(
            window_manager=SimpleNamespace(redrob_reblend=state),
        )
        agent._handle_event(
            context,
            {
                "version": 1,
                "id": plan_id,
                "event": "success",
                "result": {"summary": "Late plan", "actions": []},
            },
        )
        self.assertEqual(state.status, 'WORKING')
        self.assertEqual(state.plan_json, "")

        agent.requests[cancel_id] = {"method": "cancel", "target_request_id": plan_id}
        agent._handle_event(
            context,
            {
                "version": 1,
                "id": cancel_id,
                "event": "success",
                "result": {"request_id": plan_id, "cancelled": False},
            },
        )
        self.assertEqual(state.status, 'IDLE')
        self.assertEqual(state.request_id, "")
        self.assertNotIn(plan_id, agent.cancelled_requests)


if __name__ == "__main__":
    unittest.main()
