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
| 4 | `ops` | extrude, inset, bevel, loop cut, smooth, Catmull-Clark and linear subdivide; Mirror/Array/Subdivision/Smooth modifiers | topology invariants |
| 5 | `ui-egui` | outliner, properties, tool shelf, G/R/S hotkeys, undo UI | driven-input recording |
| 6 | `scene`, `gpu` | PBR materials, lights, basic UV unwrap | viewport snapshots |
| 7 | `blend` | read-only `.blend` import (2.5x-5.x, zstd/gzip): objects, meshes, materials, lights, cameras | 3 real files load |
| 8 | `agent` | `redrob acp --attach` over ACP, MCP tools (scene_summary, add_primitive, transform, material, mesh_op, modifiers, lights, import/export), permission prompts, Agent panel | tools + protocol tests; real prompt needs Redrob Code installed |
| 9 | `render` | CPU path tracer (rayon, progressive, deterministic), Render window (F12), PNG save | Lambert formula, shadow, emission, determinism tests |
| 10 | packaging | macOS universal DMG (Developer ID, notarized), Windows x64/arm64 portable zip (Artifact Signing over OIDC), Linux tarball; cdn.redrob.ai/shape; launcher manifest | launcher-readable names, asset count = build count |
| later | | sculpt, keyframes, armatures, booleans, GPU path tracer | v0.2+ |

## Blender parity

After 0.3.0 the work is parity with Blender, feature by feature. Blender is GPL; Redrob Shape is
MIT/Apache and contains no Blender code. Every row below is ported from **behaviour**: the Blender
manual (CC-BY-SA 4.0) for what an operator does and its default key, and a local Blender build run
side by side for how the screen looks. Each stage ends with two screenshots of the same scene at the
same window size, Blender left and Shape right, and a keymap table checked against Blender's defaults.

| Stage | Area | Delivers | Gate |
|---|---|---|---|
| P1 | layout | Blender default screen: top bar with workspace tabs, editor headers with menus, T toolbar, N sidebar, Properties editor with tabbed icons, Outliner, Timeline, status bar; Blender dark theme colours | side-by-side screenshot, region rectangles within 2% |
| P2 | keymap | default keymap for object mode: click/box B/circle C/lasso select, Shift+A add, Shift+D duplicate, X delete menu, H/Alt+H, Ctrl+J, P, numpad views, Z shading pie, N/T, Ctrl+Space, F3 search, F12, Ctrl+Tab | keymap table, every key driven once |
| P3 | edit mode | vertex/edge/face modes (1/2/3), Alt+click loops, L linked, Ctrl+Numpad grow/shrink, K knife, F fill, M merge, dissolve/delete menu, Ctrl+B bevel with segments (scroll), bridge, spin, inset options panel | topology tests + driven recording |
| P4 | modifiers | Boolean, Solidify, Bevel (segments, profile), Decimate, Wireframe, Triangulate, Screw, Simple Deform, Shrinkwrap, Displace, Weld | per-modifier golden meshes |
| P5 | object mode | collections, hide/visibility, set origin, snapping, pivot points, transform orientations, proportional editing, parenting UI | outliner screenshot vs Blender |
| P6 | shading | Material Preview, shader node editor (Principled BSDF, image texture, mix), matcaps, cavity, viewport shading popover | node editor screenshot |
| P7 | sculpt | Draw, Clay, Smooth, Grab, Inflate, Flatten brushes; symmetry; dyntopo later | brush golden meshes |
| P8 | UV | UV editor, mark seams, angle-based unwrap, pack islands, live unwrap | UV editor screenshot |
| P9 | animation | Timeline, I keyframes, Dope Sheet, Graph Editor basics, playback, interpolation | playback recording |
| P10 | rigging | armatures, bones, parenting with automatic weights, weight paint | deformed golden mesh |
| P11 | render | Cycles-like features: GPU path tracer (wgpu compute), denoise, camera DOF, render/output properties; Eevee-like realtime view | reference PSNR |
| P12 | objects | curves (Bezier/NURBS), text, metaballs, lattice, empties, image planes | import/export round trips |
| P13 | formats | FBX (ASCII), USD (usda), Collada import/export; Alembic deferred (needs an Ogawa reader) | round trips against Blender's output |
| P14 | nodes | geometry nodes, compositor | node graph golden outputs |
| P15 | platform | Preferences, keymap editor, scripting (Shape Script console; Rhai deferred, not in offline registry), add-on model | scripted smoke test |

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
