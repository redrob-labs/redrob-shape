# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

import importlib.util
import json
import unittest
from pathlib import Path


def _load_protocol():
    path = Path(__file__).resolve().parents[1] / "protocol.py"
    spec = importlib.util.spec_from_file_location("redrob_reblend_protocol_test", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


protocol = _load_protocol()


class ProtocolTests(unittest.TestCase):
    def test_event_decoder_rejects_integer_tokens_above_python_limit(self):
        oversized_integer = b"9" * 5000
        raw = (
            b'{"version":1,"id":"p1","event":"success","result":{"value":'
            + oversized_integer
            + b"}}\n"
        )
        with self.assertRaises(protocol.ProtocolError):
            protocol.decode_event(raw)

    def test_request_is_compact_versioned_json_line(self):
        encoded = protocol.encode_request("request-1", "health", {})
        self.assertTrue(encoded.endswith(b"\n"))
        self.assertEqual(
            json.loads(encoded),
            {"version": 1, "id": "request-1", "method": "health", "params": {}},
        )

    def test_request_rejects_unknown_methods_and_oversized_payloads(self):
        with self.assertRaises(protocol.ProtocolError):
            protocol.encode_request("request-1", "execute", {})
        with self.assertRaises(protocol.ProtocolError):
            protocol.encode_request("request-1", "plan", {"instruction": "x" * (64 * 1024)})

    def test_event_decoder_fails_closed_on_unknown_fields(self):
        valid = b'{"version":1,"id":"h1","event":"success","result":{}}\n'
        self.assertEqual(protocol.decode_event(valid)["id"], "h1")
        invalid = b'{"version":1,"id":"h1","event":"success","result":{},"extra":true}\n'
        with self.assertRaises(protocol.ProtocolError):
            protocol.decode_event(invalid)


if __name__ == "__main__":
    unittest.main()
