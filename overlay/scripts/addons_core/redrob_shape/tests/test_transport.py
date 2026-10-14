# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-2.0-or-later
"""Tests for credential storage and transport resolution.

Two properties matter more than the rest and are asserted in both directions:

  - A secret never reaches a log line or the UI. Storage is not enough; `redact` has to actually
    catch the shapes real keys have.
  - The subscription transport supplies NO credential to the agent. That is the whole basis on
    which the path is permitted: the vendor's own tool authenticates, and we never hold the token.
    A test that only checked "subscription works" would pass while violating the terms.
"""

from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


def _load(name):
    """Load a sibling module by path.

    The same approach the other tests in this directory use, and for the same reason: these modules
    live in a Blender add-on package whose __init__ imports bpy, so importing them as a package
    outside Blender fails. Loading the file directly keeps these unit tests runnable in plain
    CPython, which is what lets CI run them without building Blender.

    The sys.modules registration is required, not tidiness: transport.py uses dataclasses under
    `from __future__ import annotations`, and dataclasses resolves those string annotations by
    looking the class's module up in sys.modules. Without the entry it fails with a bare
    "'NoneType' object has no attribute '__dict__'" at decoration time.
    """
    module_name = f"redrob_shape_{name}_test"
    path = Path(__file__).resolve().parents[1] / f"{name}.py"
    spec = importlib.util.spec_from_file_location(module_name, path)
    module = importlib.util.module_from_spec(spec)
    sys.modules[module_name] = module
    spec.loader.exec_module(module)
    return module


credentials = _load("credentials")
transport = _load("transport")

CredentialStore = credentials.CredentialStore
describe = credentials.describe
redact = credentials.redact

GATEWAY_URL = transport.GATEWAY_URL
Kind = transport.Kind
agent_environment = transport.agent_environment
resolve = transport.resolve


class FakeKeyring:
    """Minimal keyring stand-in. Real keyring may prompt, so it is never used in tests."""

    def __init__(self):
        self.data = {}
        self.deleted = []

    def set_password(self, service, account, value):
        self.data[(service, account)] = value

    def get_password(self, service, account):
        return self.data.get((service, account))

    def delete_password(self, service, account):
        self.deleted.append(account)
        self.data.pop((service, account), None)


class RedactionTests(unittest.TestCase):
    def test_openai_key_shape_is_redacted(self):
        line = "request failed with key sk-abcdefghij1234567890"
        self.assertNotIn("abcdefghij", redact(line))
        self.assertIn("[redacted]", redact(line))

    def test_anthropic_key_shape_is_redacted(self):
        self.assertNotIn("XYZ123456789", redact("using sk-ant-XYZ123456789abc"))

    def test_bearer_header_is_redacted(self):
        self.assertEqual(redact("authorization: Bearer abc.def.ghi"), "authorization: Bearer [redacted]")

    def test_redrob_key_shape_is_redacted(self):
        self.assertNotIn("deadbeefcafe", redact("token rdb_deadbeefcafe0001"))

    def test_ordinary_text_survives(self):
        self.assertEqual(redact("no secrets in this line"), "no secrets in this line")

    def test_empty_is_safe(self):
        self.assertEqual(redact(""), "")

    def test_describe_never_returns_the_value(self):
        secret = "sk-abcdefghij1234567890"
        described = describe(secret)
        self.assertNotIn("abcdefghij", described)
        self.assertIn("chars", described)

    def test_describe_unset(self):
        self.assertEqual(describe(None), "not set")
        self.assertEqual(describe(""), "not set")


class CredentialStoreTests(unittest.TestCase):
    def setUp(self):
        self.keyring = FakeKeyring()
        self.store = CredentialStore(keyring_module=self.keyring)

    def test_roundtrip(self):
        self.store.set("openai", "sk-test-value-1234")
        self.assertEqual(self.store.get("openai"), "sk-test-value-1234")
        self.assertTrue(self.store.has("openai"))

    def test_stored_under_the_addon_service(self):
        self.store.set("redrob", "rdb_key_000000")
        self.assertIn((credentials.KEYRING_SERVICE, "redrob"), self.keyring.data)

    def test_unknown_account_is_refused(self):
        # A typo must be an error, not a key nothing can ever read back.
        with self.assertRaises(ValueError):
            self.store.set("openai-2", "x")
        with self.assertRaises(ValueError):
            self.store.get("gemini")

    def test_empty_value_is_refused(self):
        with self.assertRaises(ValueError):
            self.store.set("openai", "   ")

    def test_whitespace_is_trimmed(self):
        self.store.set("anthropic", "  sk-ant-padded  ")
        self.assertEqual(self.store.get("anthropic"), "sk-ant-padded")

    def test_delete(self):
        self.store.set("openai", "sk-gone-0000")
        self.store.delete("openai")
        self.assertIsNone(self.store.get("openai"))
        self.assertIn("openai", self.keyring.deleted)

    def test_status_describes_without_revealing(self):
        self.store.set("openai", "sk-abcdefghij1234567890")
        status = self.store.status()
        self.assertNotIn("abcdefghij", str(status))
        self.assertEqual(status["anthropic"], "not set")

    def test_session_fallback_when_no_keyring(self):
        # No keyring module and none importable: the key works for this session and the store says
        # it is not durable, rather than silently writing a file.
        store = CredentialStore(keyring_module=None)
        store._probed = True  # skip the import probe
        store._keyring = None
        store.set("openai", "sk-session-only-1")
        self.assertEqual(store.get("openai"), "sk-session-only-1")
        self.assertFalse(store.durable)

    def test_keyring_backed_store_is_durable(self):
        self.assertTrue(self.store.durable)


class _Store:
    """Credential store stub for transport tests."""

    def __init__(self, **held):
        self._held = {k: v for k, v in held.items() if v}

    def has(self, account):
        return account in self._held

    def get(self, account):
        return self._held.get(account)


def _no_tools(_name):
    return None


def _all_tools(name):
    return f"/usr/local/bin/{name}"


class ResolutionOrderTests(unittest.TestCase):
    def test_the_engine_wins_when_the_sidecar_says_it_is_connected(self):
        r = resolve(_Store(redrob="k", openai="k"), engine_connected=True, which=_no_tools)
        self.assertIs(r.chosen.kind, Kind.LOCAL)
        self.assertIn("Redrob Code", r.chosen.reason)

    def test_gateway_next(self):
        r = resolve(_Store(redrob="k", openai="k"), engine_connected=False, which=_no_tools)
        self.assertIs(r.chosen.kind, Kind.GATEWAY)

    def test_byo_when_no_gateway_key(self):
        r = resolve(_Store(anthropic="k"), engine_connected=False, which=_no_tools)
        self.assertIs(r.chosen.kind, Kind.BYO)
        self.assertEqual(r.chosen.provider, "anthropic")

    def test_nothing_available_is_reported_not_raised(self):
        r = resolve(_Store(), engine_connected=False, which=_no_tools)
        self.assertFalse(r.ok)
        self.assertIsNone(r.chosen)
        self.assertEqual(r.summary(), "no transport available")
        # Every option is still described, so the UI can tell the user what to set up.
        self.assertEqual(len(r.considered), 6)

    def test_an_unanswered_engine_is_not_available_and_says_it_is_being_asked(self):
        r = resolve(_Store(), engine_connected=None, which=_no_tools)
        local = [c for c in r.considered if c.kind is Kind.LOCAL][0]
        self.assertFalse(local.available)
        self.assertIn("asking", local.reason)

    def test_subscription_is_off_by_default(self):
        r = resolve(_Store(), engine_connected=False, which=_all_tools)
        self.assertFalse(r.ok)
        subs = [c for c in r.considered if c.kind is Kind.SUBSCRIPTION]
        self.assertTrue(subs)
        self.assertTrue(all(not c.available for c in subs))
        self.assertIn("disabled", subs[0].reason)

    def test_subscription_used_when_enabled_and_tool_present(self):
        r = resolve(_Store(), engine_connected=False, subscription_enabled=True, which=_all_tools)
        self.assertIs(r.chosen.kind, Kind.SUBSCRIPTION)
        self.assertTrue(r.chosen.volatile)
        self.assertIn("policy may change", r.summary())

    def test_subscription_unavailable_when_tool_absent(self):
        r = resolve(_Store(), engine_connected=False, subscription_enabled=True, which=_no_tools)
        self.assertFalse(r.ok)
        subs = [c for c in r.considered if c.kind is Kind.SUBSCRIPTION]
        self.assertIn("not installed", subs[0].reason)

    def test_byo_is_preferred_over_subscription(self):
        # BYO never depends on a policy staying put, so it outranks the volatile path.
        r = resolve(_Store(openai="k"), engine_connected=False, subscription_enabled=True, which=_all_tools)
        self.assertIs(r.chosen.kind, Kind.BYO)

    def test_explicit_preference_is_honoured(self):
        r = resolve(_Store(redrob="k"), engine_connected=True, preferred=[Kind.GATEWAY], which=_no_tools)
        self.assertIs(r.chosen.kind, Kind.GATEWAY)

    def test_unavailable_preference_does_not_strand_the_session(self):
        r = resolve(_Store(redrob="k"), engine_connected=True, preferred=[Kind.BYO], which=_no_tools)
        self.assertTrue(r.ok)
        self.assertIsNot(r.chosen.kind, Kind.BYO)


class AgentSupportTests(unittest.TestCase):
    def test_unsupported_transports_are_listed_but_never_chosen(self):
        r = resolve(
            _Store(redrob="k", openai="k", anthropic="k"),
            engine_connected=False,
            subscription_enabled=True,
            which=_all_tools,
            supported=transport.AGENT_KINDS,
        )
        self.assertIsNone(r.chosen)
        unsupported = [c for c in r.considered if c.kind is not Kind.LOCAL]
        self.assertEqual(len(unsupported), 5)
        for candidate in unsupported:
            self.assertFalse(candidate.available)
            self.assertEqual(candidate.reason, transport.UNSUPPORTED_REASON)

    def test_the_sidecars_answer_decides_the_engine(self):
        connected = resolve(_Store(), engine_connected=True, which=_no_tools, supported=transport.AGENT_KINDS)
        self.assertIs(connected.chosen.kind, Kind.LOCAL)
        # A Redrob key in the add-on's own store is not a path any more: only the engine's answer counts.
        disconnected = resolve(
            _Store(redrob="k"), engine_connected=False, which=_no_tools, supported=transport.AGENT_KINDS
        )
        self.assertIsNone(disconnected.chosen)

    def test_the_direct_gateway_is_never_the_agents_path(self):
        # Two paths would hide whether masking applied; the engine is the only one.
        self.assertEqual(transport.AGENT_KINDS, frozenset({Kind.LOCAL}))
        self.assertIn("Redrob Code", transport.UNSUPPORTED_REASON)


class AgentEnvironmentTests(unittest.TestCase):
    def _resolve(self, store, **kw):
        return resolve(store, kw.pop("engine", False), which=kw.pop("which", _no_tools), **kw)

    def test_the_engine_gets_no_key_and_no_endpoint(self):
        # The sidecar finds the shared engine through its record and the engine holds the key. A
        # key or an endpoint in the environment would be a second path.
        store = _Store(redrob="rdb_key_123456")
        env = agent_environment(
            self._resolve(store, engine=True).chosen,
            store,
            base={"REDROB_BASE_URL": "https://example.invalid/stale", "REDROB_API_KEY": "rdb_stale"},
        )
        self.assertNotIn("REDROB_API_KEY", env)
        self.assertNotIn("REDROB_BASE_URL", env)

    def test_gateway_leaves_the_key_and_the_endpoint_alone_too(self):
        store = _Store(redrob="rdb_key_123456")
        env = agent_environment(self._resolve(store).chosen, store, base={"REDROB_API_KEY": "rdb_stale"})
        self.assertIs(self._resolve(store).chosen.kind, Kind.GATEWAY)
        self.assertNotIn("REDROB_API_KEY", env)
        self.assertNotIn("REDROB_BASE_URL", env)

    def test_byo_openai_gets_only_its_own_key(self):
        store = _Store(openai="sk-byo-openai-1")
        env = agent_environment(self._resolve(store).chosen, store, base={})
        self.assertEqual(env["OPENAI_API_KEY"], "sk-byo-openai-1")
        self.assertNotIn("ANTHROPIC_API_KEY", env)
        self.assertNotIn("REDROB_API_KEY", env)

    def test_subscription_supplies_no_credential_at_all(self):
        # The basis on which this path is permitted: the vendor's own tool authenticates and we
        # never hold or forward the token. Any credential appearing here would be the violation.
        store = _Store()
        chosen = self._resolve(store, subscription_enabled=True, which=_all_tools).chosen
        env = agent_environment(chosen, store, base={})
        for name in ("REDROB_API_KEY", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"):
            self.assertNotIn(name, env)

    def test_an_inherited_key_for_another_provider_is_stripped(self):
        # A stray key in the user's shell must not bill them when plans go through the engine.
        store = _Store()
        env = agent_environment(
            self._resolve(store, engine=True).chosen,
            store,
            base={"OPENAI_API_KEY": "sk-inherited-from-shell", "PATH": "/usr/bin"},
        )
        self.assertNotIn("OPENAI_API_KEY", env)
        self.assertEqual(env["PATH"], "/usr/bin")

    def test_unrelated_environment_survives(self):
        store = _Store()
        env = agent_environment(self._resolve(store, engine=True).chosen, store, base={"HOME": "/home/u"})
        self.assertEqual(env["HOME"], "/home/u")


if __name__ == "__main__":
    unittest.main()
