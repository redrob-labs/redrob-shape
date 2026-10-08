# Redrob Shape architecture

Redrob Shape is a compatibility-first Blender distribution with an additive, native agent workspace. The upstream Blender source remains the product core; Redrob Shape adds a bundled Python integration and an isolated Rust agent process without changing `.blend` file compatibility, operator identifiers, keymaps, rendering engines, or platform executable names.

## Project structure

```text
redrob-shape/
├── scripts/addons_core/redrob_shape/   # Native Blender UI and safe action executor
│   ├── __init__.py                       # Lifecycle and class registration
│   ├── state.py                          # Session-only UI and job state
│   ├── context.py                        # Bounded scene summaries
│   ├── protocol.py                       # Versioned sidecar message types
│   ├── runtime.py                        # Sidecar process and main-thread queue
│   ├── tools.py                          # Allow-listed Blender actions
│   ├── operators.py                     # Submit, cancel, approve, apply, theme
│   ├── panels.py                        # Modern native View3D sidebar
│   ├── theme.py                         # Reversible Redrob Shape visual preset
│   └── blender_manifest.toml             # Extension metadata and permissions
├── intern/redrob_agent/                  # Cross-platform Rust sidecar
│   ├── Cargo.toml / Cargo.lock
│   ├── CMakeLists.txt                    # Optional isolated Cargo target
│   └── src/                              # Redrob API client and JSON-lines IPC
└── doc/redrob_shape/                   # Architecture, security, and operation docs
```

## Compatibility boundaries

The following upstream contracts are deliberately preserved:

- Blender C/C++ target and executable names.
- `BLENDER_VERSION`, blend-file version, and minimum reader version.
- All existing operators, keymaps, editors, workspaces, render engines, imports, and exports.
- Existing startup-file and user-preference precedence.
- Existing Linux desktop IDs, Windows launcher behavior, and macOS bundle identifiers.
- A normal Blender build when `WITH_REDROB_AGENT=OFF`.

Redrob Shape is additive. If the agent is unavailable, disabled, offline, or out of credit, Blender continues to operate normally.

## Runtime topology

```text
Blender main process
  Native Python Panel / Operators
       │ JSON Lines over stdin/stdout
       ▼
redrob-shape-agent (Rust child process)
       │ HTTPS, OpenAI Chat Completions compatible
       ▼
https://console.redrob.ai/api/backend/v1
```

The API key never reaches Blender. **Connect Redrob Console** runs the console's device flow inside the sidecar: Blender shows a short code and opens the approval page, and once the user approves, the sidecar collects the key and keeps it in the operating-system keychain under `ai.redrob.shape`. Where no keychain answers, the key lasts for the session and the panel says so. `REDROB_API_KEY` in the environment overrides the keychain for development and CI. The key is never passed on the command line, stored in a `.blend` file, included in prompts, or written to logs. The sidecar is a separate process, not an operating-system sandbox; safety comes from the narrow protocol, local validation, approval gate, and allow-listed Blender executor.

## Agent transaction

Every mutating request follows this state machine:

1. **Observe** — collect a bounded, non-binary scene summary.
2. **Plan** — ask Redrob for a strict, versioned action plan.
3. **Validate** — reject unknown tools, stale objects, invalid values, and oversized plans.
4. **Preview** — show the exact operations in Blender without mutation.
5. **Approve** — require an explicit user action.
6. **Execute** — run the whole plan through one `REGISTER` + `UNDO` operator on Blender's main thread.
7. **Verify** — compare resulting objects and report partial or complete success.

Workers and the sidecar never access `bpy`. Background messages contain plain JSON-compatible values and are drained by a guarded `bpy.app.timers` callback.

## Tool policy

The first tool surface is intentionally narrow:

- `create_primitive`
- `transform_object`
- `add_modifier`
- `set_material`

Arbitrary Python execution, shell execution, add-on installation, network fetching, file deletion, and overwrite exports are not agent tools. New tools require schema validation, deterministic previews, explicit mutation boundaries, and verification.

## Modern UI strategy

Redrob Shape uses Blender's native UI toolkit so accessibility, scaling, keymaps, context, and platform behavior remain intact. Modernization consists of:

- A focused Redrob Shape sidebar with clear status, prompt, plan preview, approval, cancellation, and activity sections.
- An optional reversible dark visual preset with red accent colors.
- Existing editor, workspace, menu, and operator behavior left in place.

No Electron, embedded browser, CSS overlay, or replacement window system is introduced.

## Build strategy

`WITH_REDROB_AGENT` gates the Rust process and is disabled by default for upstream-compatible builds. Enabling it requires Cargo, builds into the CMake binary tree, and installs the sidecar next to the Blender executable using existing platform destinations. Cargo never writes build artifacts into the source tree.
