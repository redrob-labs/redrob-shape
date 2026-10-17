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

## Screenshots


| | |
|---|---|
| ![Layout workspace](docs/screenshots/layout.png)<br>Layout workspace | ![Add menu (Shift+A)](docs/screenshots/add-menu.png)<br>Add menu (Shift+A) |
| ![Edit mode](docs/screenshots/edit-mode.png)<br>Edit mode | ![Modifiers](docs/screenshots/modifiers.png)<br>Modifiers |
| ![Outliner](docs/screenshots/outliner.png)<br>Outliner | ![Shader editor](docs/screenshots/shader-editor.png)<br>Shader editor |
| ![Shading popover](docs/screenshots/shading-popover.png)<br>Shading popover | ![Sculpt mode](docs/screenshots/sculpt.png)<br>Sculpt mode |
| ![UV editing](docs/screenshots/uv-editing.png)<br>UV editing | ![Animation](docs/screenshots/animation.png)<br>Animation |
| ![Pose mode](docs/screenshots/pose-mode.png)<br>Pose mode | ![Weight paint](docs/screenshots/weight-paint.png)<br>Weight paint |
| ![Render (path tracer)](docs/screenshots/render.png)<br>Render (path tracer) | ![Curves](docs/screenshots/curves.png)<br>Curves |
| ![COLLADA import](docs/screenshots/collada-import.png)<br>COLLADA import | ![Geometry Nodes](docs/screenshots/geometry-nodes.png)<br>Geometry Nodes |
| ![Preferences / keymap](docs/screenshots/preferences.png)<br>Preferences / keymap | ![Scripting console](docs/screenshots/scripting-console.png)<br>Scripting console |

## Install

Releases ship for Linux (tar.gz), macOS (notarized DMG) and Windows (`-setup.exe` per-user
installer, or `-portable.zip` to unzip anywhere). Redrob Launcher installs the portable build.

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

## Control from Redrob Code

While a Redrob Shape window is open it registers itself with the shared Redrob Code daemon,
so the agent in any Redrob surface sees the window as MCP server `app:shape` and can drive
the scene the user is looking at. Every call runs on the UI thread with the same code the menus
use, and edits land in the undo history. Without a daemon the app runs as usual.

- `scene_info` -- file path (or untitled), object count, selection, viewport and scene camera, render size.
- `open_scene {path}` -- open a scene file in the window, replacing the current scene.
- `list_objects {max?}` -- one line per object: id, name, type, parent, visible or hidden.
- `select {ids}` -- select objects by id or name, replacing the selection.
- `transform {id, translate?, rotate_deg?, scale?}` -- move, rotate and/or scale one object relative to where it is.
- `set_visible {id, visible}` -- show or hide one object.
- `render {path?, width?, height?}` -- render with the render panel's settings and write a PNG; answers the path.
- `save` -- save the scene to its file; fails when the scene is untitled.

## History

Until October 2026 this repository was a patch-based Blender distribution. That tree is kept at
the tag `blender-fork-final` and the branch `archive/blender-fork-history`; nothing from it is
carried here.

## License

MIT or Apache-2.0, at your option. See [NOTICE](NOTICE).
