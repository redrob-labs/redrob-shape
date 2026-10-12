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
    package = types.ModuleType("redrob_shape")
    package.__path__ = []
    protocol = types.ModuleType("redrob_shape.protocol")
    protocol.MAX_RESPONSE_BYTES = 256 * 1024
    protocol.ProtocolError = ValueError
    protocol.encode_request = lambda *_args: b"{}\n"
    tools = types.ModuleType("redrob_shape.tools")
    tools.PlanValidationError = ValueError
    tools.normalize_plan = lambda payload, _context: payload
    tools.validate_plan = lambda payload, _context, _allowed: payload
    bpy = types.ModuleType("bpy")
    bpy.app = SimpleNamespace(binary_path="", timers=SimpleNamespace())
    bpy.context = SimpleNamespace(window_manager=SimpleNamespace(windows=[]))
    i18n = types.ModuleType("redrob_shape.i18n")
    i18n.iface_ = lambda message: message
    conversation_spec = importlib.util.spec_from_file_location(
        "redrob_shape.conversation", Path(__file__).resolve().parents[1] / "conversation.py"
    )
    conversation = importlib.util.module_from_spec(conversation_spec)
    conversation_spec.loader.exec_module(conversation)
    chat_spec = importlib.util.spec_from_file_location(
        "redrob_shape.chat", Path(__file__).resolve().parents[1] / "chat.py"
    )
    chat = importlib.util.module_from_spec(chat_spec)
    chat_spec.loader.exec_module(chat)

    replacements = {
        "bpy": bpy,
        "redrob_shape": package,
        "redrob_shape.protocol": protocol,
        "redrob_shape.tools": tools,
        "redrob_shape.i18n": i18n,
        "redrob_shape.conversation": conversation,
        "redrob_shape.chat": chat,
    }
    previous = {name: sys.modules.get(name) for name in replacements}
    sys.modules.update(replacements)
    try:
        path = Path(__file__).resolve().parents[1] / "runtime.py"
        spec = importlib.util.spec_from_file_location("redrob_shape.runtime", path)
        module = importlib.util.module_from_spec(spec)
        sys.modules["redrob_shape.runtime"] = module
        spec.loader.exec_module(module)
        return module
    finally:
        for name, old_module in previous.items():
            if old_module is None:
                sys.modules.pop(name, None)
            else:
                sys.modules[name] = old_module
        sys.modules.pop("redrob_shape.runtime", None)


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
        agent.locate_executable = lambda: Path("/fake/redrob-shape-agent")
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
            connection='CONNECTED',
        )
        context = SimpleNamespace(window_manager=SimpleNamespace(redrob_shape=state))
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
            connection='CONNECTED',
        )
        context = SimpleNamespace(window_manager=SimpleNamespace(redrob_shape=state))
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
        context = SimpleNamespace(window_manager=SimpleNamespace(redrob_shape=state))
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
            window_manager=SimpleNamespace(redrob_shape=state),
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

    def test_a_plan_goes_up_for_review_and_joins_the_conversation(self):
        runtime_module = _load_runtime()
        agent = runtime_module.AgentRuntime()

        class Steps(list):
            def add(self):
                self.append(SimpleNamespace(enabled=True))

        earlier = {"summary": "A cube", "actions": [{"type": "x", "arguments": {}}]}
        history = runtime_module.conversation.add_turn([], "Add a cube", earlier)
        state = SimpleNamespace(
            status='WORKING',
            status_message="Planning",
            request_id="plan-2",
            scene_revision="revision",
            plan_json="",
            allowed_objects_json="[]",
            plan_summary="",
            plan_problem="",
            response_text="",
            sidecar_available=True,
            steps=Steps(),
            history_json=runtime_module.conversation.dump(history),
        )
        agent.requests["plan-2"] = {
            "method": "plan",
            "scene_revision": "revision",
            "captured_objects": ["Cube"],
            "instruction": "Rounder",
        }
        plan = {"summary": "Bevel it", "actions": [{"type": "a", "arguments": {}}, {"type": "b", "arguments": {}}]}
        context = SimpleNamespace(window_manager=SimpleNamespace(redrob_shape=state))
        agent._handle_event(context, {"version": 1, "id": "plan-2", "event": "success", "result": plan})

        self.assertEqual(state.status, 'READY')
        self.assertEqual(len(state.steps), 2)
        turns = runtime_module.conversation.load(state.history_json)
        self.assertEqual([t["outcome"] for t in turns], ["discarded", "pending"])
        self.assertEqual(turns[-1]["instruction"], "Rounder")
        self.assertEqual(turns[-1]["plan"], plan)
        self.assertNotIn("plan", turns[0])
        # The answer lands in the chat as Redrob's message, carrying the steps it names.
        answer = runtime_module.chat.load(state.chat_json)[-1]
        self.assertEqual((answer["role"], answer["kind"], answer["text"]), ("agent", "plan", "Bevel it"))
        self.assertEqual([step["type"] for step in answer["steps"]], ["a", "b"])

    def test_a_refused_plan_is_answered_in_the_chat(self):
        runtime_module = _load_runtime()
        agent = runtime_module.AgentRuntime()
        state = SimpleNamespace(status='WORKING', status_message="Planning", request_id="plan-3", chat_json="[]")
        agent.requests["plan-3"] = {"method": "plan"}
        context = SimpleNamespace(window_manager=SimpleNamespace(redrob_shape=state))
        error = {"code": "out_of_credit", "message": "This workspace is out of credit", "retryable": False}
        agent._handle_event(context, {"version": 1, "id": "plan-3", "event": "error", "error": error})
        self.assertEqual(state.status, 'ERROR')
        answer = runtime_module.chat.load(state.chat_json)[-1]
        self.assertEqual((answer["role"], answer["kind"], answer["text"]), ("agent", "error", error["message"]))


def _connection_state(**overrides):
    state = SimpleNamespace(
        status='IDLE',
        status_message="",
        request_id="",
        sidecar_available=False,
        connection='UNKNOWN',
        connection_message="",
        connection_source="",
        connection_hint="",
        connection_keychain=True,
        user_code="",
        verification_uri="",
        verification_uri_complete="",
        connect_request_id="",
        connect_expires_at=0.0,
        key_name="",
        account_name="",
    )
    for name, value in overrides.items():
        setattr(state, name, value)
    return state


_STARTED = {
    "user_code": "TY78-Y8VS",
    "verification_uri": "https://console.redrob.ai/connect",
    "verification_uri_complete": "https://console.redrob.ai/connect?code=TY78-Y8VS",
    "expires_in": 600,
    "interval": 5,
}


class ConnectionTests(unittest.TestCase):
    def setUp(self):
        self.runtime = _load_runtime()
        self.agent = self.runtime.AgentRuntime()

    def _deliver(self, state, request_id, method, event):
        self.agent.requests[request_id] = {"method": method}
        context = SimpleNamespace(window_manager=SimpleNamespace(redrob_shape=state))
        self.agent._handle_event(context, {"version": 1, "id": request_id, **event})

    def test_health_reports_the_connection(self):
        state = _connection_state()
        self._deliver(
            state,
            "h",
            "health",
            {
                "event": "success",
                "result": {
                    "service": "redrob-shape-agent",
                    "configured": True,
                    "connection": {
                        "connected": True,
                        "source": "keychain",
                        "key_hint": "cdef",
                        "keychain_available": True,
                    },
                },
            },
        )
        self.assertEqual(state.connection, 'CONNECTED')
        self.assertEqual(state.connection_hint, "cdef")
        self.assertTrue(state.sidecar_available)
        self.assertEqual(state.status_message, self.runtime._idle_message(True))

    def test_a_started_connection_shows_the_code_and_waits_for_approval(self):
        state = _connection_state(connection='STARTING')
        self.agent.connect_wait = mock.Mock(return_value="wait-1")
        self._deliver(state, "s", "connect_start", {"event": "success", "result": _STARTED})
        self.assertEqual(state.connection, 'WAITING')
        self.assertEqual(state.user_code, "TY78-Y8VS")
        self.assertEqual(state.verification_uri_complete, _STARTED["verification_uri_complete"])
        self.assertEqual(state.connect_request_id, "wait-1")
        self.assertGreater(state.connect_expires_at, 0)
        self.agent.connect_wait.assert_called_once_with()

    def test_a_code_that_arrives_after_cancel_is_ignored(self):
        state = _connection_state(connection='DISCONNECTED')
        self.agent.connect_wait = mock.Mock()
        self._deliver(state, "s", "connect_start", {"event": "success", "result": _STARTED})
        self.assertEqual(state.connection, 'DISCONNECTED')
        self.assertEqual(state.user_code, "")
        self.agent.connect_wait.assert_not_called()

    def test_approval_connects_and_keeps_only_a_hint(self):
        state = _connection_state(connection='WAITING', connect_request_id="wait-1", user_code="TY78-Y8VS")
        self._deliver(
            state,
            "wait-1",
            "connect_wait",
            {
                "event": "success",
                "result": {
                    "connected": True,
                    "source": "keychain",
                    "key_hint": "cdef",
                    "keychain_available": True,
                    "key_name": "Redrob Shape (2026-10-08)",
                    "account_name": "Acme",
                },
            },
        )
        self.assertEqual(state.connection, 'CONNECTED')
        self.assertEqual(state.user_code, "")
        self.assertEqual(state.connect_request_id, "")
        self.assertEqual(state.key_name, "Redrob Shape (2026-10-08)")
        self.assertIn("Acme", state.connection_message)
        self.assertTrue(state.sidecar_available)

    def test_a_declined_code_says_why(self):
        state = _connection_state(connection='WAITING', connect_request_id="wait-1", user_code="TY78-Y8VS")
        self._deliver(
            state,
            "wait-1",
            "connect_wait",
            {
                "event": "error",
                "error": {
                    "code": "access_denied",
                    "message": "the connection was declined in Redrob Console",
                    "retryable": False,
                },
            },
        )
        self.assertEqual(state.connection, 'ERROR')
        self.assertIn("declined", state.connection_message)
        self.assertEqual(state.user_code, "")

    def test_a_replaced_attempt_cannot_overwrite_the_current_one(self):
        state = _connection_state(connection='WAITING', connect_request_id="wait-2", user_code="NEW1-CODE")
        self._deliver(
            state,
            "wait-1",
            "connect_wait",
            {
                "event": "error",
                "error": {"code": "cancelled", "message": "connecting was cancelled", "retryable": False},
            },
        )
        self.assertEqual(state.connection, 'WAITING')
        self.assertEqual(state.user_code, "NEW1-CODE")

    def test_disconnect_forgets_and_points_at_revocation(self):
        state = _connection_state(connection='CONNECTED', key_name="Redrob Shape (2026-10-08)")
        self._deliver(
            state,
            "d",
            "disconnect",
            {
                "event": "success",
                "result": {"connected": False, "source": None, "key_hint": None, "keychain_available": True},
            },
        )
        self.assertEqual(state.connection, 'DISCONNECTED')
        self.assertEqual(state.key_name, "")
        self.assertIn("Revoke", state.connection_message)
        self.assertFalse(state.sidecar_available)

    def test_disconnect_cannot_remove_an_environment_key_and_says_so(self):
        state = _connection_state(connection='CONNECTED', connection_source="environment")
        self._deliver(
            state,
            "d",
            "disconnect",
            {
                "event": "success",
                "result": {
                    "connected": True,
                    "source": "environment",
                    "key_hint": "cdef",
                    "keychain_available": True,
                },
            },
        )
        self.assertEqual(state.connection, 'CONNECTED')
        self.assertIn("REDROB_API_KEY", state.connection_message)

    def test_connection_answers_never_touch_the_plan_status(self):
        state = _connection_state(status='READY', status_message="Validated locally")
        self._deliver(
            state,
            "c",
            "connection_status",
            {"event": "error", "error": {"code": "x", "message": "keychain locked", "retryable": False}},
        )
        self.assertEqual(state.status, 'READY')
        self.assertEqual(state.status_message, "Validated locally")
        self.assertEqual(state.connection_message, "keychain locked")

    def test_an_agent_that_exits_mid_connection_is_reported(self):
        state = _connection_state(connection='WAITING', connect_request_id="wait-1", user_code="TY78-Y8VS")

        class Exited:
            stdin = stdout = stderr = None

            def poll(self):
                return 1

        self.agent.process = Exited()
        context = SimpleNamespace(window_manager=SimpleNamespace(redrob_shape=state))
        self.agent.drain(context)
        self.assertEqual(state.connection, 'ERROR')
        self.assertEqual(state.user_code, "")
        self.assertIn("exited", state.connection_message)


if __name__ == "__main__":
    unittest.main()
