<!--
SPDX-FileCopyrightText: 2026 Redrob Labs

SPDX-License-Identifier: GPL-3.0-or-later
-->

# Redrob Shape Agent

`redrob-shape-agent` is a separate Rust planning process for Redrob Shape. Process
separation limits the Blender integration surface, but it is not an operating-system sandbox. It reads
versioned JSON Lines requests from standard input, writes JSON Lines events to standard output,
and sends planning requests to the shared **Redrob Code** daemon on this computer, over its
OpenAI-compatible `/v1/chat/completions`. It does not load Blender, mutate scenes, execute
model-generated code, hold a Redrob Console key, or read API keys from protocol input.

## Requires Redrob Code

Plans go through one `redrob serve --register` per user, the engine every Redrob app attaches to
(redrob-code `docs/LOCAL-ENGINE-API.md`, section 3). The engine holds the Redrob Console key,
applies masking, and talks to the Console; this sidecar never does. There is deliberately no
direct path to the Console: two paths would hide whether masking applied to a plan.

- The daemon is found through its record (`~/.redrob/run/server.json`, or `REDROB_RUN_DIR`) and
  started with `redrob service start --json` when it is not up (`REDROB_CODE_BIN`,
  `~/.redrob/bin/redrob`, then `redrob` on `PATH`). A daemon this sidecar starts stays up for 10
  minutes with nobody attached.
- Every request to the daemon carries its `Authorization`, `x-redrob-app: shape`, and
  `x-redrob-session`: the id the add-on passes in `REDROB_SHAPE_SESSION`, one per Blender session.
- The daemon's secret rotates on every start. A 401 or a refused connection makes the sidecar read
  the record again once and repeat the request; a second 401 is the Console refusing the key the
  daemon holds, reported as `invalid_api_key`.
- No engine binary: `plan`, `connection_status`, `disconnect` and `store_key` fail with
  `engine_not_installed` ("Redrob Code is not installed. Install it with Redrob Launcher."), and
  `health` reports it under `engine` with `install_url`. An engine that will not start fails with
  `engine_start_failed` and the reason.

## Configuration

- `REDROB_DEVICE_BASE_URL` optionally replaces the console root that holds `device/authorize` and
  `device/token`, default `https://console.redrob.ai/api/backend/v1`. Only HTTPS URLs without
  embedded credentials or fragments are accepted; redirects are disabled. The device flow is how a
  person obtains a key; the key then goes to the engine.
- `REDROB_MODEL` optionally selects the model, in the engine's `<provider>/<model>` form, and
  defaults to `redrob/auto`.
- `REDROB_SHAPE_SESSION` is the session id the add-on sets. Absent, the sidecar mints one.
- `REDROB_RUN_DIR` and `REDROB_CODE_BIN` are the engine client's own overrides, above.

There is no `REDROB_API_KEY` and no `REDROB_BASE_URL` any more: the engine holds the key
(`redrob providers login`, or Connect from Blender), and the engine's record says where plans go.

`health` remains available when configuration is absent or invalid, allowing the parent process to
display a useful status without making a network request to anything but the local engine. It
carries the connection status and an `engine` block (`available`, `version`, or `code`, `message`,
`install_url`).

## Connecting

| Method | Answer |
|---|---|
| `connection_status` | `{connected, source: engine\|null, provider_source: api\|env\|launcher\|config\|null, key_hint, engine_version}`, from the engine's `GET /provider`. |
| `connect_start` | `{user_code, verification_uri, verification_uri_complete, expires_in, interval}`. The device code stays in the agent. |
| `connect_wait` | Polls until approval, then hands the key to the engine (`PUT /auth/redrob`) and answers with the status plus `key_name` and `account_name`. Cancellable with `cancel`; errors `access_denied`, `expired_token`, `invalid_grant`, `no_pending_connection`. |
| `disconnect` | The engine forgets the key (`DELETE /auth/redrob`), which signs out every Redrob app on the computer, and answers with the status. A key the engine's own environment or configuration supplies, and Redrob Launcher's sign-in, remain. |
| `store_key` | `{"key": "..."}`: hands a pasted key to the engine the same way. The key is never echoed. |

A key an earlier version kept in the OS keychain (`ai.redrob.shape` / `redrob`) is moved into
the engine the first time the engine answers, and deleted from the keychain. That is the only
thing this sidecar still does with a keychain.

## Protocol

Every input line must be one UTF-8 JSON object no larger than 64 KiB. Unknown envelope fields,
methods, parameters, protocol versions, and malformed JSON fail closed. Request IDs are unsigned
integers or non-empty strings of at most 128 bytes.

```json
{"version":1,"id":"h1","method":"health","params":{}}
{"version":1,"id":"p1","method":"plan","params":{"instruction":"Add a beveled cube","context":{"objects":[]}}}
{"version":1,"id":"c1","method":"cancel","params":{"request_id":"p1"}}
{"version":1,"id":"s1","method":"shutdown","params":{}}
```

`instruction` is non-empty and at most 16 KiB. `context` is optional JSON and defaults to `null`.
At most 128 plans may be active. `cancel` targets an active plan request ID. `shutdown` acknowledges
the request, cancels active plans, drains their terminal events, and exits. End-of-file also cancels
active work.

Success and failure are always structured events:

```json
{"version":1,"id":"h1","event":"success","result":{"service":"redrob-shape-agent","configured":true}}
{"version":1,"id":"p1","event":"error","error":{"code":"cancelled","message":"request was cancelled","retryable":false}}
```

Standard output is protocol-only. Fatal process diagnostics go to standard error and never include
request bodies, model responses, authorization headers, or API keys. Output lines are bounded to
256 KiB. HTTP responses are streamed into a buffer capped at 1 MiB, with connect and total request
timeouts.

A successful `plan` result contains only a locally validated object with `summary` and `actions`.
The only accepted action types are `create_primitive`, `transform_object`, `add_modifier`, and
`set_material`. Unknown fields and action types are rejected. Names, action counts, modifier-specific
arguments, finite numbers, coordinates, transforms, and material ranges are validated locally.
Both plain JSON model content and a single `json` Markdown fence are accepted.

## Build and test

Direct Cargo workflow:

```sh
cargo build --locked
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
```

The local CMake file maps the selected Windows, Linux, or macOS architecture to an explicit
Rust target and keeps all Cargo output under `CMAKE_BINARY_DIR/redrob_agent_cargo/<target>`.
Universal macOS builds are rejected explicitly; configure separate x64 and arm64 builds instead:

```sh
cmake -S intern/redrob_agent -B build/redrob-agent \
  -DREDROB_AGENT_INSTALL_DESTINATION=libexec/blender
cmake --build build/redrob-agent --config Release
cmake --install build/redrob-agent --prefix /desired/prefix --config Release
```

`REDROB_AGENT_INSTALL_DESTINATION` is caller-provided when the embedding application needs the
binary beside its own executable. If omitted, CMake safely installs to `CMAKE_INSTALL_BINDIR`
(normally `bin`). Debug CMake configurations use Cargo's debug profile; other configurations use
Cargo's release profile. The install also packages `THIRD_PARTY.md` and the generated
`THIRD_PARTY-LICENSES.txt` bundle. After changing `Cargo.lock`, regenerate and check the bundle:

```sh
python tools/generate_third_party_licenses.py
python tools/generate_third_party_licenses.py --check
```
