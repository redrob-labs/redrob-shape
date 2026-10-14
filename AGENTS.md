# Working in this repository

- Pure Rust, `unsafe` forbidden by the workspace lints. egui/eframe 0.36, wgpu via `egui-wgpu`.
- One crate per layer; respect the order in `ROADMAP.md`. A lower crate never imports a higher one.
- Every change passes `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` before it is pushed. CI runs exactly those.
- UI is tested with `egui_kittest`; keep `ShapeApp::ui` callable without a window so tests can drive it.
- Branch names: `feat/ fix/ docs/ chore/ test/ refactor/ perf/ hotfix/ release/ sync/` (checked by
  `.github/workflows/gitflow.yml`). Pull requests target `develop`.
- Brand files in `assets/brand` are copied from the published Redrob design system and pinned in
  `assets/brand/SOURCES.json`; never recolor or redraw them.
- Nothing from Blender (code, data files, icons) may enter this tree. The old distribution is at
  the tag `blender-fork-final`.
