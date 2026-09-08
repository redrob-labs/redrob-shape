# Redrob Reblend

**A compatibility-first, agentic Blender distribution.**

Redrob Reblend keeps Blender's complete open-source 3D pipeline and adds a modern native workspace backed by Redrob. Existing modeling, sculpting, animation, simulation, rendering, compositing, tracking, editing, import/export, add-on, keymap, and `.blend` workflows remain Blender workflows.

> Reblend is an independent downstream project. Blender is developed by the Blender Foundation and its community; this repository preserves their copyright and license notices.

## What Reblend adds

- A focused **Reblend** workspace panel inside the native 3D View sidebar.
- A modern, reversible dark theme overlay with a restrained red accent.
- Bounded scene summaries that omit geometry, image pixels, file contents, and paths.
- Redrob planning through an isolated Rust sidecar rather than model code inside Blender.
- Review-before-apply action plans with stale-scene detection.
- One Blender Undo step for each approved plan.
- Four allow-listed tools: create primitive, transform object, add modifier, and set material.
- No arbitrary Python, shell, file-deletion, add-on-installation, or model-directed network tool.

If Redrob is not configured, has no balance, is offline, or the sidecar is absent, Blender continues to work normally.

## Architecture

```text
Blender + native Python workspace
          │ versioned JSON Lines over stdio
          ▼
redrob-reblend-agent (Rust)
          │ HTTPS / OpenAI-compatible chat completions
          ▼
Redrob API
```

Read the [architecture](doc/redrob_reblend/architecture.md), [security model](doc/redrob_reblend/security.md), and [build guide](doc/redrob_reblend/building.md).

## Build

Follow the official [Blender platform build instructions](https://developer.blender.org/docs/handbook/building_blender/), install Rust 1.88 or newer, then configure with the Reblend profile:

```sh
cmake -S . -B ../build_reblend \
  -C build_files/cmake/config/redrob_reblend.cmake
cmake --build ../build_reblend --config Release
```

A normal Blender configure remains unchanged and does not require Rust; `WITH_REDROB_AGENT` defaults to `OFF` unless the Reblend profile is selected.

## Connect Redrob

Create or approve a workspace key in [Redrob Console](https://console.redrob.ai/guide), then launch Blender with:

```sh
export REDROB_API_KEY="your-workspace-key"
```

The default API route is `https://console.redrob.ai/api/backend/v1/chat/completions` with model `auto`. The sidecar reads the key from its environment; the key is not sent over the Blender protocol, stored in `.blend` data, or written to logs.

[Redrob Code](https://console.redrob.ai/code) can be used independently for repository development and upstream modernization. It is not a runtime dependency of Reblend.

## Blender compatibility and upstream

The Blender executable names, CMake targets, file-format versions, platform application IDs, operators, keymaps, and startup precedence are intentionally retained. See [UPSTREAM.md](UPSTREAM.md) for the exact baseline and sync policy.

General Blender fixes should follow [Blender's official contribution process](https://developer.blender.org/docs/handbook/contributing/). Reblend-specific contributions should follow [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Blender and Redrob Reblend as a whole are licensed under the GNU General Public License, Version 3. Individual files and dependencies may use different compatible licenses. See [COPYING](COPYING), the existing Blender notices under `release/license`, and the sidecar's [third-party dependency inventory](intern/redrob_agent/THIRD_PARTY.md) and [complete license-text bundle](intern/redrob_agent/THIRD_PARTY-LICENSES.txt).
