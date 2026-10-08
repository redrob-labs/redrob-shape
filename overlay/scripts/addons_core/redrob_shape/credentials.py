# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-2.0-or-later
"""Credential storage for Redrob Shape.

Secrets never go into Blender preferences. An add-on preference is stored in `userpref.blend`,
which is an ordinary file on disk with no protection -- a key put there is readable by anything
that can read the user's home directory, and it travels with a preferences backup. Blender has no
protected credential facility of its own, so this module uses the operating system keyring and,
when that is unavailable, keeps the key in memory for the session only and says so.

Two rules are enforced here rather than left to callers:

1. A secret is never returned in a form that gets logged. `redact()` is applied to anything that
   goes near a log line, and `describe()` is what the UI shows -- never the value.

2. We never read another product's credential file. OpenAI's own documentation says of
   `~/.codex/auth.json`: "Treat it like a password: it contains access tokens. Don't commit it,
   paste it into tickets, or share it in chat." Reading it to extract a token would also mean
   impersonating a first-party client, which is the thing that makes the subscription path a
   terms violation. When a vendor subscription is used, we invoke the vendor's own tool and let
   it read its own credentials -- see transport.py.
"""

from __future__ import annotations

import re
from typing import Dict, Optional

# Service name under which keys are stored in the OS keyring.
KEYRING_SERVICE = "ai.redrob.shape"

# The credentials this add-on can hold. Anything not in here cannot be stored, so a typo becomes
# an error instead of a silently unreachable key.
KNOWN_ACCOUNTS = (
    "redrob",     # Redrob gateway key, from the device-code flow or pasted
    "openai",     # the user's own OpenAI API key, billed to them
    "anthropic",  # the user's own Anthropic API key, billed to them
)

_SECRET_PATTERNS = (
    re.compile(r"\bsk-ant-[A-Za-z0-9._\-]{8,}", re.ASCII),
    re.compile(r"\bsk-[A-Za-z0-9._\-]{8,}", re.ASCII),
    re.compile(r"\brdb_[A-Za-z0-9._\-]{8,}", re.ASCII),
)

# Handled separately so the scheme survives: "Bearer [redacted]" reads as an auth header with its
# value removed, where a bare "[redacted]" loses what kind of thing was there. Matches what
# redrob-browser's RedrobService does, so log output looks the same across products.
_BEARER_PATTERN = re.compile(r"\b(Bearer)\s+\S+", re.ASCII | re.IGNORECASE)


def redact(text: str) -> str:
    """Replace anything that looks like a credential. Applied to every log line and error."""
    if not text:
        return text
    out = _BEARER_PATTERN.sub(r"\1 [redacted]", text)
    for pattern in _SECRET_PATTERNS:
        out = pattern.sub("[redacted]", out)
    return out


def describe(value: Optional[str]) -> str:
    """What the UI may show about a stored key: its shape, never its value."""
    if not value:
        return "not set"
    return f"set ({len(value)} chars, ending {value[-4:]})" if len(value) > 8 else "set"


class CredentialStore:
    """Keyring-backed store with an explicit in-memory fallback.

    `keyring` is imported lazily and its absence is not an error: Blender ships its own Python and
    a user may not have the module. The fallback is deliberately session-only rather than a file,
    because a file is exactly the thing this module exists to avoid.
    """

    def __init__(self, keyring_module=None) -> None:
        self._session: Dict[str, str] = {}
        self._keyring = keyring_module
        self._probed = keyring_module is not None

    def _backend(self):
        if not self._probed:
            self._probed = True
            try:
                import keyring  # type: ignore

                self._keyring = keyring
            except Exception:
                self._keyring = None
        return self._keyring

    @property
    def durable(self) -> bool:
        """True when a stored key survives restarting Blender."""
        return self._backend() is not None

    def set(self, account: str, value: str) -> None:
        if account not in KNOWN_ACCOUNTS:
            raise ValueError(f"unknown credential account: {account}")
        if not value or not value.strip():
            raise ValueError("refusing to store an empty credential")
        value = value.strip()

        backend = self._backend()
        if backend is not None:
            backend.set_password(KEYRING_SERVICE, account, value)
            # Also held for this session so a keyring that prompts does not prompt per request.
            self._session[account] = value
            return
        self._session[account] = value

    def get(self, account: str) -> Optional[str]:
        if account not in KNOWN_ACCOUNTS:
            raise ValueError(f"unknown credential account: {account}")
        if account in self._session:
            return self._session[account]
        backend = self._backend()
        if backend is None:
            return None
        try:
            return backend.get_password(KEYRING_SERVICE, account)
        except Exception:
            return None

    def delete(self, account: str) -> None:
        if account not in KNOWN_ACCOUNTS:
            raise ValueError(f"unknown credential account: {account}")
        self._session.pop(account, None)
        backend = self._backend()
        if backend is not None:
            try:
                backend.delete_password(KEYRING_SERVICE, account)
            except Exception:
                pass

    def has(self, account: str) -> bool:
        return bool(self.get(account))

    def status(self) -> Dict[str, str]:
        """Per-account state for the UI. Values are descriptions, never secrets."""
        return {account: describe(self.get(account)) for account in KNOWN_ACCOUNTS}
