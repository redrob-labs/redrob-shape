<!--
SPDX-FileCopyrightText: 2026 Redrob Labs

SPDX-License-Identifier: GPL-3.0-or-later
-->

# Redrob Shape Agent

`redrob-shape-agent` is a separate Rust planning process for Redrob Shape. Process
separation limits the Blender integration surface, but it is not an operating-system sandbox. It reads
versioned JSON Lines requests from standard input, writes JSON Lines events to standard output,
and sends planning requests to Redrob's OpenAI-compatible chat-completions endpoint. It does not
load Blender, mutate scenes, execute model-generated code, or read API keys from protocol input.

## Configuration

- The key for `plan` comes from the OS keychain (`ai.redrob.shape` / `redrob`), put there by
  `connect_wait` or `store_key`. `REDROB_API_KEY` in the environment overrides it for development
  and CI. It is sent only as a bearer token and is never logged or returned.
- `REDROB_BASE_URL` optionally replaces the complete chat-completions endpoint. It defaults to
  `https://console.redrob.ai/api/backend/v1/chat/completions`. Only HTTPS URLs without embedded
  credentials or fragments are accepted; redirects are disabled.
- `REDROB_DEVICE_BASE_URL` optionally replaces the console root that holds `device/authorize` and
  `device/token`, default `https://console.redrob.ai/api/backend/v1`, with the same HTTPS rules.
- `REDROB_MODEL` optionally selects the model and defaults to `auto`.

`health` remains available when configuration is absent or invalid, allowing the parent process to
display a useful status without making a network request. It carries the connection status.

## Connecting

| Method | Answer |
|---|---|
| `connection_status` | `{connected, source: environment\|keychain\|session, key_hint, keychain_available}` |
| `connect_start` | `{user_code, verification_uri, verification_uri_complete, expires_in, interval}`. The device code stays in the agent. |
| `connect_wait` | Polls until approval, then stores the key and answers with the status plus `key_name` and `account_name`. Cancellable with `cancel`; errors `access_denied`, `expired_token`, `invalid_grant`, `no_pending_connection`. |
| `disconnect` | Removes the stored key and answers with the status. A key from the environment remains. |
| `store_key` | `{"key": "..."}`: stores a pasted key the same way. The key is never echoed. |

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
