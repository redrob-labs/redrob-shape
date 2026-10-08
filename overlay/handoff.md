<!--
SPDX-FileCopyrightText: 2026 Redrob Labs

SPDX-License-Identifier: GPL-3.0-or-later
-->

# Redrob Reblend handoff

> This handoff predates the 2026-10-08 rename from Redrob Reblend to Redrob Shape. It is kept
> as written: `redrob_reblend` is now `redrob_shape`, and the repository is
> `redrob-labs/redrob-shape`.

## Status at handoff

Redrob Reblend is a compatibility-first downstream of official Blender with an additive, native agent workspace. The initial safe tool surface is implemented and has passed unit, Rust, CMake, sidecar packaging/discovery, and real Blender-host validation.

- Canonical repository: `redrob-labs/redrob-reblend`
- Target canonical branch in this handoff: `main`
- Repository visibility: private
- Pinned Blender baseline: `acf2fda52b4f7e07a459136e99c80470f3a92725`

Primary downstream implementation commits before this handoff:

- `4a7c850f0b24a71f4adedd40982cabfceba851e3` — compatibility-first Reblend agent workspace
- `fb11a064d6c56b930bca036ef8fc1071d884f7c7` — real Blender-host integration validation

The complete Blender source tree remains present. Reblend does not replace Blender's executable, CMake target, file format, editors, keymaps, rendering pipeline, import/export stack, add-on system, application IDs, or startup precedence.

## Product behavior

Reblend adds a native **Reblend** panel to Blender's 3D View sidebar. A user can submit an instruction, review an exact locally validated plan, approve it, apply the plan on Blender's main thread, verify postconditions, and roll the transaction back with Blender Undo.

The initial allow-listed action surface is intentionally narrow:

1. Create a primitive: cube, UV sphere, cylinder, cone, plane, or torus.
2. Transform an existing object.
3. Add a bevel, subdivision, solidify, or mirror modifier.
4. Create a new material datablock and assign it to the target object's active material slot.

There is no arbitrary Python, shell, filesystem deletion, add-on installation, model-directed network fetch, or unrestricted `bpy` execution facility. Material datablocks created by the action are unique, but the current slot assignment can still be data-linked through shared object data; see **Known limitations and external blockers** before treating shared-mesh material operations as target-local.

The optional modern theme uses Blender's native theme properties. It snapshots previous values before mutation, persists the snapshot atomically in Blender's user configuration, tolerates Blender-version property differences, and can restore the prior theme across restarts.

## Architecture

```text
Blender core + bundled native Python add-on
                │
                │ versioned, bounded JSON Lines over stdio
                ▼
       redrob-reblend-agent (Rust)
                │
                │ HTTPS / OpenAI-compatible chat completions
                ▼
             Redrob API
```

### Blender side

The hidden core add-on lives under `scripts/addons_core/redrob_reblend`. `scripts/modules/addon_utils.py` auto-enables it in a packaged Reblend build.

The Blender-side flow is:

1. `context.py` serializes a deterministic, privacy-bounded scene summary and computes a scene revision.
2. `runtime.py` lazily discovers and starts the sidecar, communicates through pipes, and drains a bounded event queue from a Blender timer on the main thread.
3. `protocol.py` validates sidecar envelopes and response limits.
4. `tools.py` normalizes and validates every proposed action against the captured object scope and current scene.
5. `panels.py` displays every effective value before approval.
6. `operators.py` revalidates the live scene, executes the approved actions in a `REGISTER` + `UNDO` operator, and verifies postconditions.

Worker threads never access Blender RNA. If the sidecar is missing, unconfigured, offline, or crashes, ordinary Blender functionality remains available.

### Rust sidecar

The sidecar lives under `intern/redrob_agent` and is enabled only when `WITH_REDROB_AGENT=ON`. Normal Blender builds keep this option `OFF` and do not require Rust.

Protocol version 1 supports only:

- `health`
- `plan`
- `cancel`
- `shutdown`

The service enforces bounded request IDs, instructions, active requests, actions, summaries, events, and HTTP bodies. Unknown versions, fields, methods, actions, non-finite numbers, out-of-range values, insecure endpoint configuration, and malformed model responses fail closed.

`REDROB_API_KEY` is inherited by the sidecar through its environment. It is not passed on a command line, sent over the Blender-side protocol, stored in `.blend` data, included in scene context, or intentionally logged.

## Safety and compatibility invariants

Do not weaken these constraints in follow-up work:

- Keep Blender's executable names, target names, blend-file version constants, minimum reader version, platform IDs, operators, keymaps, editors, render engines, import/export behavior, and startup/preferences precedence unchanged.
- Keep model output untrusted until it passes both Rust and Python validation.
- Keep scene mutation behind exact preview and explicit approval.
- Resolve object references and recompute the scene revision immediately before execution.
- Execute all `bpy` mutations on Blender's main thread.
- Keep every approved plan inside one Blender `REGISTER` + `UNDO` operator.
- Reject linked, read-only, cross-scene, stale, omitted, or otherwise out-of-scope object references.
- Never place secrets in source, logs, command lines, prompts, `.blend` files, or protocol payloads.
- Treat sidecar process separation as isolation, not as an OS sandbox.
- Add new tools one at a time with a strict schema, bounded values, exact preview, explicit scope, postcondition verification, and Undo coverage.

A mid-plan postcondition failure can leave partial mutations until the user invokes Undo. The operator deliberately preserves the Undo path and reports the failing action; it does not silently continue.

## Build and run

Follow Blender's normal platform dependency setup, and install Rust/Cargo 1.88 or newer when building the sidecar.

Configure with the Reblend profile:

```sh
cmake -S . -B ../build_reblend \
  -C build_files/cmake/config/redrob_reblend.cmake
cmake --build ../build_reblend --config Release
```

Equivalent explicit configuration:

```sh
cmake -S . -B ../build_reblend -DWITH_REDROB_AGENT=ON
```

Run with a Redrob workspace key:

```sh
export REDROB_API_KEY="your-workspace-key"
../build_reblend/bin/blender
```

Optional development configuration:

- `REDROB_MODEL` overrides the default `auto` model.
- `REDROB_BASE_URL` overrides the default HTTPS endpoint after strict URL validation.
- `REDROB_AGENT_BIN` points Blender to a development sidecar binary. Packaged builds discover the sidecar beside Blender without this variable.

The default endpoint is `https://console.redrob.ai/api/backend/v1/chat/completions`.

## Validation completed

The following checks passed before handoff:

- Python `compileall` for the Reblend add-on and license generator.
- Ruff checks for the Reblend add-on and sidecar tooling.
- Python unit suite: **18 passed**.
- Rust formatting and Clippy with warnings denied.
- Rust unit suite: **11 passed**.
- Deterministic third-party inventory/license drift check.
- Standalone sidecar CMake Release build and install.
- Standalone CTest suite: **3 passed**.
- Multi-config Debug/Release sidecar profile and copy-layout checks.
- Live JSON Lines sidecar health and shutdown checks.
- Clean `git diff --check` and clean worktree before handoff creation.

### Real Blender-host validation

A real host test passed against the official Blender `5.3.0 Alpha` Linux x64 daily binary, hash `a9beda8c0ff2`.

Downloaded archive SHA-256:

```text
0df28998e6a95a9b513cc8253278c5fdc756e782666d4186e8919a1c1fdf26da
```

The reusable host test validates:

- Add-on registration and Blender RNA/property integration.
- Normalized primitive preview defaults and cube/material execution postconditions.
- Torus behavior when Enter Edit Mode is enabled.
- Cross-scene object rejection.
- Material-slot scene-revision changes.
- Theme apply/restore round trip.
- Partial postcondition failure reporting.
- Operation-local background Undo restoration using pre/post memfile checkpoints and exact scene/object/mesh/material signatures.
- Production sidecar discovery beside the Blender executable with inherited `REDROB_AGENT_BIN` and `REDROB_API_KEY` deliberately isolated.
- Unconfigured health state and cooperative sidecar shutdown with exit code zero.
- No residual Blender or sidecar process after the test.

A separate staged-layout smoke check copied the downstream add-on and patched `addon_utils.py` into the extracted official Blender bundle, then verified that `redrob_reblend` loaded automatically and registered its `WindowManager` property. This is evidence for the intended hidden-core activation path, not a substitute for a generated full Reblend package from a source build.

Run the reusable host test with an official or built Blender and a sidecar placed beside its executable:

```sh
blender --background --factory-startup \
  --python scripts/addons_core/redrob_reblend/tests/blender_integration.py
```

Background Blender does not create the same UI-owned automatic Undo checkpoint as an interactive operator context. The integration test therefore brackets each operation with explicit memfile snapshots and separately asserts that the production operator declares `REGISTER` and `UNDO`. Interactive single-step Undo should still receive a dedicated UI-level test.

## Standard development checks

```sh
python -m compileall -q scripts/addons_core/redrob_reblend intern/redrob_agent/tools
python -m ruff check scripts/addons_core/redrob_reblend intern/redrob_agent/tools
python -m unittest discover \
  -s scripts/addons_core/redrob_reblend/tests \
  -p 'test_*.py'

cargo fmt --check --manifest-path intern/redrob_agent/Cargo.toml
cargo clippy \
  --manifest-path intern/redrob_agent/Cargo.toml \
  --all-targets --all-features -- -D warnings
cargo test \
  --manifest-path intern/redrob_agent/Cargo.toml \
  --all-targets --all-features

python intern/redrob_agent/tools/generate_third_party_licenses.py --check

cmake -S intern/redrob_agent -B ../build_reblend_agent
cmake --build ../build_reblend_agent --config Release
ctest --test-dir ../build_reblend_agent -C Release --output-on-failure
```

For relevant Blender core/build changes, also run the applicable upstream checks such as `make test`, `make check_cmake`, `make check_licenses`, and `make check_pep8`.

## Known limitations and external blockers

1. **Full source build dependency access:** initializing Blender's `lib/linux_x64` dependency submodule failed because `projects.blender.org` returned HTTP 403. The official Blender binary was used to validate the actual `bpy`, RNA, Undo, theme, packaging, and process boundaries instead. A full source build remains required once dependency access is available.
2. **Live Redrob planning:** no real `plan` request was sent because no `REDROB_API_KEY` was available. Missing-key, validation, protocol, timeout/error structure, and network-free health behavior are covered, but a live successful API round trip is still pending.
3. **Platform execution matrix:** CMake maps Windows/Linux/macOS x64 and arm64 targets, but native runtime evidence has only been recorded for Linux x64. Universal macOS is intentionally rejected; publish separate x64 and arm64 builds.
4. **Interactive Undo:** headless rollback is verified precisely, but interactive Blender's automatic single-step operator checkpoint needs UI-level coverage.
5. **CI:** no Reblend-specific checked-in CI workflow currently enforces the Python, Rust, CMake, license, host-Blender, or platform matrix checks.
6. **Tool scope:** animation, sculpting, node editing, lighting/camera control, collections, render settings, and edits to existing shared materials are outside the initial allow list.
7. **Large scenes:** scene context is intentionally capped at 48 objects and 40 KiB. Objects omitted from captured context cannot be targeted.
8. **Authentication:** environment-key setup is the current supported flow. Device authorization and OS-keychain storage require a stable public Console application contract.
9. **Partial failure:** rollback is user-driven through Blender Undo rather than an internal automatic rollback inside `execute_plan`.
10. **Shared object-data material scope:** `set_material` creates a unique material datablock, but assigning it through a DATA-linked slot—or adding a slot to shared mesh data—can affect the appearance or material-slot structure of other objects that share that data. The current preview does not enumerate that wider scope. Before production use on shared data, force an object-local assignment/copy or reject and clearly report shared/DATA-linked cases; add dedicated host coverage for the chosen behavior.

## Upstream synchronization

Remotes are expected to remain:

```text
origin   redrob-labs/redrob-reblend
upstream blender/blender
```

Upstream has advanced beyond the pinned downstream baseline. Fetch and calculate the live relationship before planning a sync:

```sh
git fetch upstream
git rev-list --left-right --count HEAD...upstream/main
```

Follow `UPSTREAM.md`. Review these conflict hotspots deliberately:

- `CMakeLists.txt`
- `source/creator/CMakeLists.txt`
- `scripts/modules/addon_utils.py`
- `scripts/addons_core/redrob_reblend/`

After each upstream sync, rerun the complete Reblend validation suite and the relevant Blender upstream checks. Do not resolve conflicts by renaming Blender artifacts or changing compatibility constants.

## Recommended continuation order

1. Make `set_material` target-local for shared meshes/DATA-linked slots, or reject those cases with an exact wider-scope explanation; add dedicated host coverage.
2. Restore Blender dependency access and complete a full Reblend source build plus Blender test suite.
3. Sync current `upstream/main`, reviewing the documented hotspots and preserving all compatibility invariants.
4. Add secret-safe CI for Python, Ruff, Rust fmt/Clippy/tests, license drift, standalone CMake/CTest, and packaged Blender-host integration. CI workflow changes must be reviewed through a pull request.
5. Add native Windows, macOS, Linux, x64, and arm64 build/runtime evidence.
6. Add an interactive Blender UI test for the automatic one-step Undo boundary.
7. Add deterministic HTTPS integration tests for success, malformed/oversized responses, redirects, timeout, 429/5xx behavior, missing credit, crash recovery, and in-flight cancellation. Keep live API tests opt-in and secret-safe.
8. Implement device authorization and OS credential storage only after Redrob Console exposes a stable public contract.
9. Improve context selection for large scenes before expanding the tool surface.
10. Expand tools incrementally under the existing schema/preview/scope/verification/Undo rules.

## Important paths

| Path | Purpose |
| --- | --- |
| `README.md` | Product contract and quick start |
| `UPSTREAM.md` | Pinned Blender baseline and sync policy |
| `CONTRIBUTING.md` | Contributor checks and invariants |
| `doc/redrob_reblend/architecture.md` | Architecture and transaction design |
| `doc/redrob_reblend/security.md` | Trust boundaries and failure behavior |
| `doc/redrob_reblend/building.md` | Build, install, and run instructions |
| `build_files/cmake/config/redrob_reblend.cmake` | Reblend CMake cache profile |
| `scripts/modules/addon_utils.py` | Hidden core add-on activation |
| `scripts/addons_core/redrob_reblend/` | Native Blender UI, runtime, protocol, context, tools, theme, and tests |
| `intern/redrob_agent/` | Rust sidecar, CMake integration, tests, and license inventory |
| `source/creator/CMakeLists.txt` | Sidecar dependency/copy/install integration |

## Handoff acceptance checklist

A continuation is healthy when all of the following remain true:

- Blender starts and works normally with the sidecar disabled or unavailable.
- Existing `.blend` files, operators, keymaps, editors, render engines, and import/export paths remain Blender-compatible.
- The sidecar starts lazily and can shut down without leaving a child process.
- Missing credentials do not trigger a network request.
- Plans cannot expose Apply until local validation succeeds.
- Stale or out-of-scope plans are rejected before mutation.
- Every displayed preview value matches the effective execution arguments.
- Shared object-data material assignments are either made target-local or rejected with their wider effect clearly disclosed; this is a documented open item at this handoff.
- Approved actions execute only on the main thread and verify their postconditions.
- Undo restores the immediate pre-plan scene state.
- Theme restoration remains durable and reversible.
- Third-party inventory and bundled license texts remain deterministic and current.
- `.tasks/`, `.agents/tasks/`, and `semantic-review/` remain ignored and uncommitted.
