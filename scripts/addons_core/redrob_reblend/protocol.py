# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Pure-Python JSON Lines protocol helpers for the local agent."""

import json

PROTOCOL_VERSION = 1
MAX_REQUEST_BYTES = 64 * 1024
MAX_RESPONSE_BYTES = 256 * 1024
_ALLOWED_METHODS = frozenset({"health", "plan", "cancel", "shutdown"})


class ProtocolError(ValueError):
    """Raised when a sidecar message violates the local protocol contract."""


def encode_request(request_id, method, params):
    if method not in _ALLOWED_METHODS:
        raise ProtocolError(f"unsupported method: {method}")
    if not isinstance(params, dict):
        raise ProtocolError("params must be an object")
    if not isinstance(request_id, (str, int)) or isinstance(request_id, bool):
        raise ProtocolError("request id must be a string or integer")
    if isinstance(request_id, str) and (not request_id or len(request_id.encode("utf-8")) > 128):
        raise ProtocolError("request id is empty or too large")
    payload = {
        "version": PROTOCOL_VERSION,
        "id": request_id,
        "method": method,
        "params": params,
    }
    encoded = json.dumps(payload, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    if len(encoded) > MAX_REQUEST_BYTES:
        raise ProtocolError("request exceeds 64 KiB")
    return encoded + b"\n"


def decode_event(line):
    if isinstance(line, str):
        raw = line.encode("utf-8")
    elif isinstance(line, bytes):
        raw = line
    else:
        raise ProtocolError("event must be text or bytes")
    if len(raw) > MAX_RESPONSE_BYTES:
        raise ProtocolError("event exceeds 256 KiB")
    try:
        event = json.loads(raw)
    except (UnicodeDecodeError, ValueError) as exc:
        raise ProtocolError("event is not valid JSON") from exc
    if not isinstance(event, dict) or event.get("version") != PROTOCOL_VERSION:
        raise ProtocolError("unsupported event envelope")
    if event.get("event") == "success":
        if set(event) != {"version", "id", "event", "result"} or not isinstance(event["result"], dict):
            raise ProtocolError("invalid success event")
    elif event.get("event") == "error":
        if set(event) != {"version", "id", "event", "error"} or not isinstance(event["error"], dict):
            raise ProtocolError("invalid error event")
        error = event["error"]
        if set(error) != {"code", "message", "retryable"}:
            raise ProtocolError("invalid error body")
        if not isinstance(error["code"], str) or not isinstance(error["message"], str):
            raise ProtocolError("invalid error strings")
        if not isinstance(error["retryable"], bool):
            raise ProtocolError("invalid retryable value")
    else:
        raise ProtocolError("unknown event type")
    return event
