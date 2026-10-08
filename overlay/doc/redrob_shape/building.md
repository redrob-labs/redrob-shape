# Building Redrob Shape

Redrob Shape keeps Blender's normal build unchanged. The Python workspace is part of `scripts/addons_core`; the isolated Rust process is included only when `WITH_REDROB_AGENT` is enabled.

## Requirements

Install Blender's platform prerequisites and dependencies using the official [Blender build instructions](https://developer.blender.org/docs/handbook/building_blender/). Redrob Shape additionally requires Rust 1.88 or newer and Cargo when the agent is enabled.

## Configure

Use the Redrob Shape cache initializer:

```sh
cmake -S . -B ../build_shape \
  -C build_files/cmake/config/redrob_shape.cmake
cmake --build ../build_shape --config Release
```

Or enable only the sidecar explicitly:

```sh
cmake -S . -B ../build_shape -DWITH_REDROB_AGENT=ON
```

A normal configuration without either option remains an upstream-compatible Blender build and does not require Rust.

## Installation layout

| Platform | Sidecar destination |
|---|---|
| Windows | Beside `blender.exe` |
| macOS | `Blender.app/Contents/MacOS/redrob-shape-agent` |
| Linux portable | Beside `blender` |
| Linux system | `bin/redrob-shape-agent` |

The sidecar is not linked into Blender. CMake maps Windows x64/arm64, Linux x64/arm64, and macOS x64/arm64 to explicit Rust targets; universal macOS configurations fail clearly because one sidecar binary cannot represent both architectures. During a build, the selected artifact is copied beside the build-tree Blender executable so it works without an install step. If the sidecar is absent or fails to start, every non-agent Blender function remains available. The install also includes `THIRD_PARTY.md` and the complete `THIRD_PARTY-LICENSES.txt` bundle in Blender's platform text-license destination.

## Connect Redrob

Set the API key in the environment that launches Blender:

```sh
export REDROB_API_KEY="your-workspace-key"
```

Optional variables:

- `REDROB_MODEL` defaults to `auto`.
- `REDROB_BASE_URL` defaults to `https://console.redrob.ai/api/backend/v1/chat/completions` and must be HTTPS.
- `REDROB_AGENT_BIN` overrides sidecar discovery for development builds.

Use [Redrob Console](https://console.redrob.ai/) to manage the workspace key, balance, logs, and revocation. Secrets are consumed by the sidecar and are never sent through Blender's JSON protocol.

## Sidecar-only checks

```sh
cargo fmt --check --manifest-path intern/redrob_agent/Cargo.toml
cargo clippy --manifest-path intern/redrob_agent/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path intern/redrob_agent/Cargo.toml --all-targets --all-features
python -m unittest discover -s scripts/addons_core/redrob_shape/tests -p 'test_*.py'
python intern/redrob_agent/tools/generate_third_party_licenses.py --check
```

With a built Blender executable, run the host integration and Undo checks:

```sh
blender --background --factory-startup \
  --python scripts/addons_core/redrob_shape/tests/blender_integration.py
```

Regenerate `THIRD_PARTY-LICENSES.txt` without `--check` after a locked dependency update.

A standalone CMake build is also supported:

```sh
cmake -S intern/redrob_agent -B ../build_shape_agent
cmake --build ../build_shape_agent --config Release
ctest --test-dir ../build_shape_agent --output-on-failure -C Release
```
