# Redrob Shape

Redrob Shape is an agentic 3D creation suite that shapes and shades your models by prompt, and
opens your existing .blend files.

[한국어](README.ko.md)

Written from scratch in pure Rust: a half-edge modeler, a wgpu viewport, a path tracer and an
egui editor, with a built-in agent that proposes edits and waits for your approval. No Blender,
no C++ toolchain, no platform-specific build machines -- `cargo build` is the whole build on
Linux, macOS and Windows.

## Status

Early. See [ROADMAP.md](ROADMAP.md) for the stage plan; the table there is the source of truth
for what works today.

## Build

```
cargo build --release -p redrob-shape
./target/release/redrob-shape
```

Linux needs the usual windowing headers (`libxkbcommon-dev libwayland-dev libx11-dev
libxrandr-dev libxi-dev libgl1-mesa-dev` on Debian/Ubuntu).

## Develop

```
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Branches follow gitflow: `develop` is the default, `main` is release, work branches are
`feat/…`, `fix/…`, `docs/…`, `chore/…`, `test/…`, `refactor/…`, `perf/…`, `hotfix/…`,
`release/…`, `sync/…`. See the org [CONTRIBUTING](https://github.com/redrob-labs/.github/blob/main/CONTRIBUTING.md).

## History

Until October 2026 this repository was a patch-based Blender distribution. That tree is kept at
the tag `blender-fork-final` and the branch `archive/blender-fork-history`; nothing from it is
carried here.

## License

MIT or Apache-2.0, at your option. See [NOTICE](NOTICE).
