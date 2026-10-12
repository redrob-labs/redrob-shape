# Contributing to Redrob Shape

Redrob Shape is a compatibility-first downstream of Blender. Changes should remain small, additive, and straightforward to replay when syncing Blender upstream.

## Choose the right project

- Redrob Shape UI, Redrob integration, agent protocol, safe tools, packaging, and downstream documentation belong in this repository.
- General Blender fixes should be reported and contributed through [Blender's official contribution process](https://developer.blender.org/docs/handbook/contributing/).

## Development checks

Run checks relevant to the files you changed:

```sh
python -m compileall -q scripts/addons_core/redrob_shape
ruff check scripts/addons_core/redrob_shape
cargo fmt --check --manifest-path intern/redrob_agent/Cargo.toml
cargo clippy --manifest-path intern/redrob_agent/Cargo.toml --all-targets --all-features -- -D warnings
cargo test --manifest-path intern/redrob_agent/Cargo.toml --all-targets --all-features
```

For Blender core or build changes, also use the appropriate upstream checks such as `make test`, `make check_cmake`, `make check_licenses`, and `make check_pep8`.

## Compatibility rules

- Do not change blend-file version constants for branding.
- Do not rename the Blender executable, CMake target, operators, or platform application IDs.
- Do not give the agent arbitrary Python, shell, file-deletion, or network-fetch tools.
- All model output remains untrusted until locally validated.
- Scene mutations require preview, explicit approval, main-thread execution, and Undo support.
- Keep API keys out of source, command lines, logs, `.blend` files, and prompts.

All new source files must carry an SPDX-compatible header and use a GPL-compatible license.
