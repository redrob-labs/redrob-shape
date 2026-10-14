# Roadmap

Redrob Shape is built from scratch in Rust, one layer per stage. Each stage is one pull request
into `develop` and lands only when its gate is green. Model: the ArtCraft "craft" apps
(photocraft, cadcraft) -- pure Rust, egui/wgpu, small crates with a fixed layering.

| Stage | Crates | Delivers | Gate |
|---|---|---|---|
| 0 | all (stubs), `apps/redrob-shape` | workspace skeleton, CI, brand, editor shell with empty viewport | CI green, window opens |
| 1 | `math`, `geom` | half-edge mesh, BVH, normals, topology queries | golden meshes, Euler characteristic |
| 2 | `scene`, `undo`, `format` | object tree, command stack, `.rshape`, OBJ/STL/glTF/PLY | import -> export -> import round trip |
| 3 | `gpu` | wgpu viewport: orbit camera, grid, ID-buffer picking, wire/solid | kittest snapshots |
| 4 | `ops` | extrude, inset, loop cut, smooth, Catmull-Clark and linear subdivide; Mirror/Array/Subdivision/Smooth modifiers | topology invariants |
| 5 | `ui-egui` | outliner, properties, tool shelf, G/R/S hotkeys, undo UI | driven-input recording |
| 6 | `scene`, `gpu` | PBR materials, lights, basic UV unwrap | viewport snapshots |
| 7 | `blend` | read-only `.blend` import (2.5x-5.x, zstd/gzip): objects, meshes, materials, lights, cameras | 3 real files load |
| 8 | `agent` | `redrob acp --attach` over ACP, MCP tools (scene_summary, add_primitive, transform, material, mesh_op, modifiers, lights, import/export), permission prompts, Agent panel | tools + protocol tests; real prompt needs Redrob Code installed |
| 9 | `render` | CPU path tracer (rayon, progressive, deterministic), Render window (F12), PNG save | Lambert formula, shadow, emission, determinism tests |
| 10 | packaging | macOS universal, Windows x64/arm64, Linux; OIDC signing; launcher manifest | asset count = build count |
| later | | bevel, sculpt, keyframes, armatures, booleans, GPU path tracer | v0.2+ |

Layering (a crate may depend only on crates above it):

```
math
geom
scene  undo
ops    format  blend
gpu    render
agent
ui-egui
apps/redrob-shape
```
