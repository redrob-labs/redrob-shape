# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Lifecycle and main-thread handoff for the local Redrob sidecar."""

import json
import os
import queue
import subprocess
import sys
import threading
import time
import uuid
from pathlib import Path

import bpy

from . import conversation, protocol, tools
from .i18n import iface_

_POLL_INTERVAL = 0.2
_MAX_EVENTS_PER_TICK = 32
_MAX_STDERR_LINE_BYTES = 4096

#: Requests about the Redrob Console connection. Their answers change the connection state, never the
#: plan status, and none of them carries the key back.
_CONNECTION_METHODS = frozenset(
    {"connection_status", "connect_start", "connect_wait", "disconnect", "store_key"}
)


def _idle_message(connected):
    return (
        iface_("Connected • you review every plan before it changes anything")
        if connected
        else iface_("Connect Redrob Console to create plans")
    )


def clear_connect_code(current):
    current.user_code = ""
    current.verification_uri = ""
    current.verification_uri_complete = ""
    current.connect_expires_at = 0.0
    current.connect_request_id = ""


def _open_url(url):
    if not url:
        return
    try:
        bpy.ops.wm.url_open(url=url)
    except (AttributeError, RuntimeError):
        # The panel still shows the code and a button that opens the page.
        pass


def show_plan(current, plan):
    """Put a checked plan up for review, every step switched on."""
    current.plan_json = json.dumps(plan, ensure_ascii=False, separators=(",", ":"))
    current.plan_summary = str(plan.get("summary", "Proposed changes"))
    current.response_text = current.plan_summary
    current.plan_problem = ""
    current.steps.clear()
    for _action in plan.get("actions", ()):
        current.steps.add()


class AgentRuntime:
    def __init__(self):
        self.process = None
        self.events = queue.Queue(maxsize=256)
        self.writer_lock = threading.Lock()
        self.reader_thread = None
        self.stderr_thread = None
        self.requests = {}
        self.cancelled_requests = set()
        self.stopping = False
        self.start_error = ""
        self.generation = 0
        self.last_countdown_redraw = 0.0

    @property
    def running(self):
        return self.process is not None and self.process.poll() is None

    def locate_executable(self):
        executable_name = "redrob-shape-agent.exe" if sys.platform == "win32" else "redrob-shape-agent"
        candidates = []
        configured = os.environ.get("REDROB_AGENT_BIN")
        if configured:
            candidates.append(Path(configured).expanduser())
        if bpy.app.binary_path:
            binary_directory = Path(bpy.app.binary_path).resolve().parent
            candidates.extend((binary_directory / executable_name, binary_directory / "bin" / executable_name))
        repository = Path(__file__).resolve().parents[3]
        candidates.extend(
            (
                repository / "intern" / "redrob_agent" / "target" / "release" / executable_name,
                repository / "intern" / "redrob_agent" / "target" / "debug" / executable_name,
            )
        )
        for candidate in candidates:
            if candidate.is_file():
                return candidate
        return None

    def start(self):
        if self.running:
            return True
        self.stop(force=True)
        executable = self.locate_executable()
        if executable is None:
            self.start_error = "Agent binary not found; build with WITH_REDROB_AGENT=ON"
            return False
        creationflags = subprocess.CREATE_NO_WINDOW if sys.platform == "win32" else 0
        try:
            process = subprocess.Popen(
                [str(executable)],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                bufsize=0,
                creationflags=creationflags,
            )
        except OSError as exc:
            self.start_error = f"Could not start local agent: {exc}"
            return False
        self.process = process
        self.stopping = False
        self.start_error = ""
        self.generation += 1
        generation = self.generation
        self.reader_thread = threading.Thread(
            target=self._read_stdout,
            args=(process, generation),
            name="RedrobShapeAgentReader",
            daemon=True,
        )
        self.stderr_thread = threading.Thread(
            target=self._read_stderr,
            args=(process, generation),
            name="RedrobShapeAgentStderr",
            daemon=True,
        )
        self.reader_thread.start()
        self.stderr_thread.start()
        try:
            self.send("health", {})
        except RuntimeError as exc:
            self.stop(force=True)
            self.start_error = f"Local agent handshake failed: {exc}"
            return False
        return True

    def send(self, method, params, metadata=None):
        process = self.process
        if not self.running or process is None or process.stdin is None:
            raise RuntimeError(self.start_error or "Local agent is not running")
        request_id = uuid.uuid4().hex
        encoded = protocol.encode_request(request_id, method, params)
        request = {"method": method}
        if metadata:
            request.update(metadata)
        self.requests[request_id] = request
        with self.writer_lock:
            try:
                process.stdin.write(encoded)
                process.stdin.flush()
            except (BrokenPipeError, OSError) as exc:
                self.requests.pop(request_id, None)
                raise RuntimeError("Local agent connection closed") from exc
        return request_id

    def submit_plan(self, instruction, scene_context, scene_revision, captured_objects, turns=()):
        """Ask for a plan. ``turns`` is the conversation so far; as much of it is sent as fits."""
        params = {"instruction": instruction, "context": scene_context}
        base = len(protocol.encode_request("0" * 32, "plan", params))
        # Room for the ``"history":[...]`` key and the envelope's own growth.
        budget = protocol.MAX_REQUEST_BYTES - base - 64
        history = conversation.request_history(list(turns), budget) if budget > 0 else []
        if history:
            params["history"] = history
        return self.send(
            "plan",
            params,
            {
                "scene_revision": scene_revision,
                "captured_objects": sorted(captured_objects),
                "instruction": instruction,
            },
        )

    def connection_status(self):
        return self.send("connection_status", {})

    def connect_start(self):
        return self.send("connect_start", {})

    def connect_wait(self):
        return self.send("connect_wait", {})

    def disconnect(self):
        return self.send("disconnect", {})

    def store_key(self, key):
        # Straight to the agent, which keeps it in the system keychain. Nothing here holds on to it.
        return self.send("store_key", {"key": key})

    def cancel(self, request_id):
        if not request_id or not self.running:
            return None
        if request_id in self.cancelled_requests:
            return None
        self.cancelled_requests.add(request_id)
        try:
            return self.send("cancel", {"request_id": request_id}, {"target_request_id": request_id})
        except RuntimeError:
            self.cancelled_requests.discard(request_id)
            raise

    def _read_stdout(self, process, generation):
        stdout = process.stdout
        if stdout is None:
            return
        try:
            while True:
                line = stdout.readline(protocol.MAX_RESPONSE_BYTES + 1)
                if not line:
                    return
                if len(line) > protocol.MAX_RESPONSE_BYTES or not line.endswith(b"\n"):
                    self._put_event({"local_error": "Agent emitted an oversized or incomplete event"}, generation)
                    process.kill()
                    return
                self._put_event({"line": line}, generation)
        except OSError:
            if not self.stopping:
                self._put_event({"local_error": "Lost connection to local agent"}, generation)

    def _read_stderr(self, process, generation):
        stderr = process.stderr
        if stderr is None:
            return
        try:
            while True:
                line = stderr.readline(_MAX_STDERR_LINE_BYTES + 1)
                if not line:
                    return
                oversized = len(line) > _MAX_STDERR_LINE_BYTES and not line.endswith(b"\n")
                message = line[:_MAX_STDERR_LINE_BYTES].decode("utf-8", errors="replace").strip()
                if message:
                    suffix = "…" if oversized else ""
                    self._put_event({"diagnostic": f"{message}{suffix}"}, generation)
        except OSError:
            return

    def _put_event(self, event, generation):
        event["generation"] = generation
        try:
            self.events.put_nowait(event)
        except queue.Full:
            try:
                self.events.get_nowait()
                self.events.put_nowait(
                    {"local_error": "Agent event queue overflow", "generation": generation}
                )
            except queue.Empty:
                pass

    def drain(self, context):
        current = context.window_manager.redrob_shape
        handled = 0
        while handled < _MAX_EVENTS_PER_TICK:
            try:
                raw = self.events.get_nowait()
            except queue.Empty:
                break
            handled += 1
            if raw.get("generation") != self.generation:
                continue
            if "local_error" in raw:
                if current.status == 'WORKING':
                    current.status = 'ERROR'
                    current.status_message = raw["local_error"]
                    current.request_id = ""
                continue
            if "diagnostic" in raw:
                if not self.running and current.status == 'WORKING':
                    current.status = 'ERROR'
                    current.status_message = raw["diagnostic"]
                continue
            try:
                event = protocol.decode_event(raw["line"])
            except protocol.ProtocolError as exc:
                self._connection_failed(current, f"Rejected invalid agent event: {exc}")
                break
            self._handle_event(context, event)

        if self.process is not None and self.process.poll() is not None and not self.stopping:
            if current.status == 'WORKING':
                current.status = 'ERROR'
                current.status_message = iface_("Local agent exited before completing the request")
                current.request_id = ""
            if current.connection in {'STARTING', 'WAITING'}:
                clear_connect_code(current)
                current.connection = 'ERROR'
                current.connection_message = "Local agent exited before the connection finished"
            current.sidecar_available = False
            self._cleanup_exited_process()
        ticking = current.connection == 'WAITING' or current.status == 'WORKING'
        if handled or (ticking and self._countdown_due()):
            _redraw()

    def _countdown_due(self):
        now = time.monotonic()
        if now - self.last_countdown_redraw < 1.0:
            return False
        self.last_countdown_redraw = now
        return True

    def _connection_failed(self, current, message):
        self.stop(force=True)
        self.requests.clear()
        self.cancelled_requests.clear()
        current.sidecar_available = False
        current.status = 'ERROR'
        current.status_message = message
        current.request_id = ""
        if current.connection in {'UNKNOWN', 'STARTING', 'WAITING'}:
            clear_connect_code(current)
            current.connection = 'ERROR'
            current.connection_message = message

    def _handle_event(self, context, event):
        current = context.window_manager.redrob_shape
        request_id = str(event.get("id", ""))
        request = self.requests.pop(request_id, None)
        if request is None:
            return
        method = request["method"]

        if method in _CONNECTION_METHODS:
            self.cancelled_requests.discard(request_id)
            self._handle_connection_event(current, method, request_id, event)
            return

        if method == "plan" and request_id in self.cancelled_requests:
            if (
                event["event"] == "error"
                and event["error"]["code"] == "cancelled"
                and request_id == current.request_id
            ):
                current.status = 'IDLE'
                current.status_message = iface_("Request cancelled")
                current.request_id = ""
            return

        if event["event"] == "error":
            error = event["error"]
            if method == "health":
                self._connection_failed(current, f"Local agent health check failed: {error['message']}")
                return
            if method == "plan" and request_id != current.request_id:
                return
            if method == "cancel":
                target = request.get("target_request_id")
                self.cancelled_requests.discard(target)
                if target != current.request_id:
                    return
            if error["code"] == "cancelled" and method in {"plan", "cancel"}:
                current.status = 'IDLE'
                current.status_message = iface_("Request cancelled")
                current.request_id = ""
                return
            current.status = 'ERROR'
            current.status_message = error["message"]
            if request_id == current.request_id:
                current.request_id = ""
            return

        result = event["result"]
        if method == "health":
            if (
                result.get("service") != "redrob-shape-agent"
                or not isinstance(result.get("configured"), bool)
            ):
                self._connection_failed(current, "Local agent returned an invalid health response")
                return
            if isinstance(result.get("connection"), dict):
                self._apply_connection(current, result["connection"])
            current.sidecar_available = result["configured"]
            if current.status == 'IDLE':
                current.status_message = _idle_message(current.sidecar_available)
        elif method == "plan":
            if request_id != current.request_id:
                return
            if request.get("scene_revision") != current.scene_revision:
                current.status = 'ERROR'
                current.status_message = iface_("The scene changed while planning; create a new plan")
                current.request_id = ""
                return
            allowed_objects = set(request.get("captured_objects", ()))
            raw_plan = result
            try:
                result = tools.normalize_plan(result, context)
                tools.validate_plan(result, context, allowed_objects)
            except tools.PlanValidationError as exc:
                current.status = 'ERROR'
                current.status_message = iface_("Rejected plan: {}").format(exc)
                current.request_id = ""
                return
            current.request_id = ""
            current.allowed_objects_json = json.dumps(sorted(allowed_objects), separators=(",", ":"))
            show_plan(current, result)
            # The conversation keeps the plan as the agent sent it: that is the form the agent
            # re-checks when a refinement sends it back.
            turns = conversation.add_turn(
                conversation.load(current.history_json), request.get("instruction", ""), raw_plan
            )
            current.history_json = conversation.dump(turns)
            current.status = 'READY'
            current.status_message = iface_("Plan ready • switch off any step you do not want, then apply")
        elif method == "cancel":
            target = request.get("target_request_id")
            self.cancelled_requests.discard(target)
            if target == current.request_id:
                current.status = 'IDLE'
                current.status_message = iface_("Request cancelled")
                current.request_id = ""
        elif method == "shutdown":
            current.sidecar_available = False

    @staticmethod
    def _apply_connection(current, status):
        connected = bool(status.get("connected"))
        current.connection = 'CONNECTED' if connected else 'DISCONNECTED'
        current.connection_source = str(status.get("source") or "")
        current.connection_hint = str(status.get("key_hint") or "")
        current.connection_keychain = bool(status.get("keychain_available", True))
        current.sidecar_available = connected
        if current.status == 'IDLE':
            current.status_message = _idle_message(connected)

    def _handle_connection_event(self, current, method, request_id, event):
        if event["event"] == "error":
            error = event["error"]
            if method == "connect_wait" and request_id != current.connect_request_id:
                return  # an older attempt that a newer one replaced
            if method in {"connect_start", "connect_wait"}:
                clear_connect_code(current)
                if error["code"] == "cancelled":
                    current.connection = 'DISCONNECTED'
                    current.connection_message = "Connecting was cancelled"
                else:
                    current.connection = 'ERROR'
                    current.connection_message = error["message"]
                return
            current.connection_message = error["message"]
            return

        result = event["result"]
        if method == "connect_start":
            if current.connection != 'STARTING':
                return  # cancelled while the code was on its way
            current.user_code = str(result.get("user_code", ""))
            current.verification_uri = str(result.get("verification_uri", ""))
            current.verification_uri_complete = str(
                result.get("verification_uri_complete") or current.verification_uri
            )
            current.connect_expires_at = time.time() + float(result.get("expires_in", 0))
            current.connection = 'WAITING'
            current.connection_message = "Approve the code on the page that opened"
            try:
                current.connect_request_id = self.connect_wait()
            except RuntimeError as exc:
                clear_connect_code(current)
                current.connection = 'ERROR'
                current.connection_message = str(exc)
                return
            _open_url(current.verification_uri_complete)
            return

        if method == "connect_wait":
            if request_id != current.connect_request_id:
                return
            clear_connect_code(current)
            current.key_name = str(result.get("key_name", ""))
            current.account_name = str(result.get("account_name", ""))
        self._apply_connection(current, result)
        if method == "connect_wait":
            current.connection_message = f"Connected to {current.account_name or 'Redrob Console'}"
        elif method == "disconnect":
            current.key_name = ""
            current.account_name = ""
            current.connection_message = (
                "Still connected through REDROB_API_KEY in the environment"
                if current.connection == 'CONNECTED'
                else "Disconnected. Revoke the key in Redrob Console if you no longer need it"
            )
        elif method == "store_key":
            current.connection_message = "Key stored"

    def _cleanup_exited_process(self):
        process = self.process
        if process is None:
            return
        for stream in (process.stdin, process.stdout, process.stderr):
            if stream is not None:
                try:
                    stream.close()
                except OSError:
                    pass
        for thread in (self.reader_thread, self.stderr_thread):
            if thread is not None and thread.is_alive():
                thread.join(timeout=0.5)
        self.process = None
        self.reader_thread = None
        self.stderr_thread = None
        self.requests.clear()
        self.cancelled_requests.clear()
        while not self.events.empty():
            try:
                self.events.get_nowait()
            except queue.Empty:
                break

    def stop(self, force=False):
        process = self.process
        if process is None:
            return
        self.stopping = True
        if process.poll() is None and not force:
            try:
                self.send("shutdown", {})
                process.wait(timeout=1.5)
            except (RuntimeError, subprocess.TimeoutExpired):
                process.terminate()
        if process.poll() is None:
            try:
                process.wait(timeout=1.0)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=1.0)
        self._cleanup_exited_process()
        self.stopping = False
        self.generation += 1


_RUNTIME = AgentRuntime()


def runtime():
    return _RUNTIME


def _redraw():
    for window in bpy.context.window_manager.windows:
        for area in window.screen.areas:
            if area.type == 'VIEW_3D':
                area.tag_redraw()


redraw = _redraw


def _poll():
    try:
        if hasattr(bpy.context.window_manager, "redrob_shape"):
            _RUNTIME.drain(bpy.context)
    except (AttributeError, ReferenceError, RuntimeError):
        pass
    return _POLL_INTERVAL


def _start_soon():
    # One shot, after Blender has finished starting, so the Connection panel shows the real state
    # instead of "Checking". Background runs (rendering, tests) have no panel and skip it.
    if not _RUNTIME.running:
        _RUNTIME.start()
    return None


def register():
    if not bpy.app.timers.is_registered(_poll):
        bpy.app.timers.register(_poll, first_interval=0.1, persistent=True)
    if not bpy.app.background and not bpy.app.timers.is_registered(_start_soon):
        bpy.app.timers.register(_start_soon, first_interval=1.0)


def unregister():
    if bpy.app.timers.is_registered(_start_soon):
        bpy.app.timers.unregister(_start_soon)
    if bpy.app.timers.is_registered(_poll):
        bpy.app.timers.unregister(_poll)
    _RUNTIME.stop()
