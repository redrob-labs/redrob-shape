# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-2.0-or-later
"""Transport resolution for Redrob Shape.

Four ways to reach a model, resolved in a fixed order with an explicit reason for whichever is
chosen, so the UI can say *why* it is using what it is using rather than silently degrading.

    1. LOCAL      the redrob-code engine on loopback. Nothing leaves the machine except what that
                  separately configured engine sends, and it already carries provider support --
                  including subscription-backed providers -- so a user who runs it needs no key here.
    2. GATEWAY    the Redrob console gateway, with a Redrob key from the device-code flow. This is
                  the same contract redrob-browser uses.
    3. BYO        the user's own OpenAI or Anthropic API key, billed to them. Always permitted by
                  both vendors; the baseline that never depends on a policy staying put.
    4. SUBSCRIPTION  the user's Claude or ChatGPT subscription, reached by invoking the VENDOR'S OWN
                  tool. Off by default.

Why the subscription path is shaped that way, and why it is last:

Neither vendor permits a third-party app to hold a consumer subscription credential and call the
model with it. Anthropic's terms restrict subscription OAuth to its own interfaces, and it
suspended accounts over this in early 2026; OpenAI's documented "Sign in with ChatGPT" is an
identity provider, and its subscription sign-in is scoped to the ChatGPT desktop app, the Codex
CLI and the IDE extension.

What IS permitted is the vendor's own tool doing the authenticating. Anthropic's support
documentation states that Claude Agent SDK and third-party app usage draw from the subscription's
usage limits; OpenAI documents the Codex SDK, App Server and non-interactive mode as programmatic
surfaces that reuse the credentials `codex login` cached. So we run the tool the user installed and
logged into themselves, and we never touch the credential.

The rule that follows, and that this module enforces: we detect the tool on PATH and execute it. We
do not read `~/.codex/auth.json` or Claude's credential store, we do not mint or refresh a token,
and we do not send a subscription credential anywhere. Anything else would be the violation.

This path is also the one that can be withdrawn by a vendor without notice -- Anthropic changed its
position three times in 2026 and has said another update is coming -- so `POLICY_VOLATILE` marks it
for a UI warning, and resolution always has BYO to fall back to.
"""

from __future__ import annotations

import os
import shutil
from dataclasses import dataclass, field, replace
from enum import Enum
from typing import Callable, List, Optional, Sequence

LOCAL_ENGINE_URL = "http://127.0.0.1:4096"
GATEWAY_URL = "https://console.redrob.ai/api/backend"


class Kind(str, Enum):
    LOCAL = "local"
    GATEWAY = "gateway"
    BYO = "byo"
    SUBSCRIPTION = "subscription"


#: Transports whose availability depends on a vendor policy that has moved before and may move
#: again. The UI must say so rather than presenting them as settled.
POLICY_VOLATILE = frozenset({Kind.SUBSCRIPTION})

#: What the agent in this build can actually plan through. It speaks the Redrob gateway's
#: OpenAI-compatible endpoint only: it has no loopback-HTTP path for the local engine, reads no
#: OpenAI or Anthropic key, and runs no vendor tool. The other kinds stay resolvable so the panel
#: can explain them, but they are never chosen while this says so.
AGENT_KINDS = frozenset({Kind.GATEWAY})
UNSUPPORTED_REASON = "not used by this version: plans go through the Redrob gateway"

#: The vendor tools we are willing to invoke, and the subcommand that makes them non-interactive.
#: The executable name is all we look for -- presence on PATH means the user installed it.
VENDOR_TOOLS = {
    "claude": {
        "executable": "claude",
        "label": "Claude Agent SDK",
        "vendor": "anthropic",
        # Anthropic documents Agent SDK / `claude -p` usage as drawing from the plan.
        "probe": ("--version",),
    },
    "codex": {
        "executable": "codex",
        "label": "Codex CLI",
        "vendor": "openai",
        # OpenAI documents non-interactive mode as a programmatic surface.
        "probe": ("--version",),
    },
}


@dataclass(frozen=True)
class Transport:
    kind: Kind
    label: str
    detail: str
    #: Set when this transport is usable right now.
    available: bool
    #: Why it is or is not usable, shown in the UI verbatim.
    reason: str
    #: True when a vendor policy could withdraw it.
    volatile: bool = False
    #: Provider name for BYO, tool name for SUBSCRIPTION; empty otherwise.
    provider: str = ""


@dataclass
class Resolution:
    chosen: Optional[Transport]
    considered: List[Transport] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return self.chosen is not None

    def summary(self) -> str:
        if self.chosen is None:
            return "no transport available"
        suffix = " (policy may change)" if self.chosen.volatile else ""
        return f"{self.chosen.label}: {self.chosen.reason}{suffix}"


def _probe_local(health_check: Callable[[str], bool]) -> Transport:
    healthy = False
    try:
        healthy = bool(health_check(LOCAL_ENGINE_URL))
    except Exception:
        healthy = False
    return Transport(
        kind=Kind.LOCAL,
        label="redrob-code (local)",
        detail=LOCAL_ENGINE_URL,
        available=healthy,
        reason=(
            "local engine is healthy; requests stay on loopback"
            if healthy
            else "local engine is not reachable on 127.0.0.1:4096"
        ),
    )


def _probe_gateway(store, connected: Optional[bool] = None) -> Transport:
    # The key lives in the agent's keychain entry, so the agent's answer is the authority; the
    # local store is the fallback for callers that have not asked the agent.
    has_key = connected if connected is not None else store.has("redrob")
    return Transport(
        kind=Kind.GATEWAY,
        label="Redrob gateway",
        detail=GATEWAY_URL,
        available=has_key,
        reason="connected to Redrob Console" if has_key else "connect Redrob Console, or paste a Redrob key",
        provider="redrob",
    )


def _probe_byo(store) -> List[Transport]:
    out = []
    for provider, label in (("anthropic", "Anthropic (your API key)"), ("openai", "OpenAI (your API key)")):
        has_key = store.has(provider)
        out.append(
            Transport(
                kind=Kind.BYO,
                label=label,
                detail=f"{provider} API, billed to your account",
                available=has_key,
                reason="your API key is stored" if has_key else "no API key stored for this provider",
                provider=provider,
            )
        )
    return out


def _probe_subscription(which: Callable[[str], Optional[str]], enabled: bool) -> List[Transport]:
    out = []
    for name, spec in VENDOR_TOOLS.items():
        path = which(spec["executable"])
        if not enabled:
            reason = "disabled; enable it in Redrob Shape preferences to use your subscription"
        elif path:
            reason = f"{spec['label']} found on PATH; it authenticates with your own subscription"
        else:
            reason = f"{spec['label']} is not installed, or not on PATH"
        out.append(
            Transport(
                kind=Kind.SUBSCRIPTION,
                label=spec["label"],
                detail=path or spec["executable"],
                available=bool(enabled and path),
                reason=reason,
                volatile=True,
                provider=name,
            )
        )
    return out


def resolve(
    store,
    health_check: Callable[[str], bool],
    subscription_enabled: bool = False,
    which: Callable[[str], Optional[str]] = shutil.which,
    preferred: Optional[Sequence[Kind]] = None,
    gateway_connected: Optional[bool] = None,
    supported: Optional[frozenset] = None,
) -> Resolution:
    """Pick a transport, and record everything considered so the UI can explain the choice.

    `preferred` overrides the order for a user who has made an explicit choice. An unavailable
    preference does not strand the session -- resolution continues down the default order -- but the
    reason it was skipped is kept in `considered`.

    `gateway_connected` is the agent's own answer about the Redrob key, which it keeps. `supported`
    restricts the choice to the kinds the agent can plan through (`AGENT_KINDS`); the others are
    still listed, as unavailable, with the reason.
    """
    candidates: List[Transport] = []
    candidates.append(_probe_local(health_check))
    candidates.append(_probe_gateway(store, gateway_connected))
    candidates.extend(_probe_byo(store))
    candidates.extend(_probe_subscription(which, subscription_enabled))
    if supported is not None:
        candidates = [
            candidate
            if candidate.kind in supported
            else replace(candidate, available=False, reason=UNSUPPORTED_REASON)
            for candidate in candidates
        ]

    order = list(preferred) if preferred else [Kind.LOCAL, Kind.GATEWAY, Kind.BYO, Kind.SUBSCRIPTION]
    # Any kind the caller did not rank still gets tried, after the ones they did.
    for kind in (Kind.LOCAL, Kind.GATEWAY, Kind.BYO, Kind.SUBSCRIPTION):
        if kind not in order:
            order.append(kind)

    for kind in order:
        for candidate in candidates:
            if candidate.kind is kind and candidate.available:
                return Resolution(chosen=candidate, considered=candidates)
    return Resolution(chosen=None, considered=candidates)


def agent_environment(transport: Transport, store, base: Optional[dict] = None) -> dict:
    """The environment for the agent sidecar, for one resolved transport.

    Only the variables that transport needs are set, and any inherited credential for a DIFFERENT
    provider is removed -- so a stray OPENAI_API_KEY in the user's shell cannot end up billing them
    when they chose the gateway.
    """
    env = dict(base if base is not None else os.environ)

    for name in ("REDROB_API_KEY", "REDROB_BASE_URL", "OPENAI_API_KEY", "ANTHROPIC_API_KEY"):
        env.pop(name, None)

    if transport.kind is Kind.GATEWAY:
        # Nothing to add. The agent keeps the Redrob key in the system keychain itself, and its
        # default endpoint IS the gateway's completion URL. Setting REDROB_BASE_URL to GATEWAY_URL,
        # as this did, pointed the agent at /api/backend, which it treats as the full endpoint.
        pass
    elif transport.kind is Kind.LOCAL:
        env["REDROB_BASE_URL"] = LOCAL_ENGINE_URL
    elif transport.kind is Kind.BYO:
        if transport.provider == "openai":
            env["OPENAI_API_KEY"] = store.get("openai") or ""
        elif transport.provider == "anthropic":
            env["ANTHROPIC_API_KEY"] = store.get("anthropic") or ""
    elif transport.kind is Kind.SUBSCRIPTION:
        # Deliberately nothing. The vendor tool reads its own credentials; we must not supply,
        # forward, or even look at them. Putting a token here would be the violation.
        pass

    return env
