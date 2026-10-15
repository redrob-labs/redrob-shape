# Changelog

All notable changes to Redrob Shape. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed
- The default screen follows Blender's Layout workspace: top bar with File/Edit/Render/Window/Help
  and workspace tabs (Layout, Modeling, Rendering), a 3D viewport header (mode, View/Select/Add/
  Object or Mesh menus, shading), the T tool column, the N sidebar (Item transform, View), a right
  column with the Outliner over a tabbed Properties editor (Render, Output, World, Object,
  Modifiers, Data, Material), and a status bar. Blender's dark theme colours and region sizes.
- Keys: T toolbar, N sidebar, Ctrl+Space maximize viewport, Shift+D duplicate, H / Alt+H hide and
  reveal, Alt+A select none, Ctrl+I invert selection.
- Viewport background is Blender's #3f3f3f.
- Edit mode the Blender way: vertex / edge / face select modes (1 / 2 / 3, header buttons) with
  vertex dots and orange selection in Blender's colours, click / Shift+click / box select per mode,
  Alt+click edge and face loops, L select linked, Ctrl+Numpad +/- grow and shrink, F fill, M merge
  menu (center / first / last), X delete menu (vertices, edges, faces, dissolve edges / faces).
- Edit mode tools: C circle select (paint, Shift erases, wheel sizes), K knife (two edge points
  of one face), Ctrl+B bevel with wheel segments (rounded profile), Alt+S shrink/fatten, Bridge
  Edge Loops, Spin, and the Ctrl+V / Ctrl+E / Ctrl+F vertex / edge / face menus.
- Modifiers with Blender's names and parameters: Boolean (union / difference / intersect with
  another object, BSP clipping), Shrinkwrap (nearest surface of another object, offset), Solidify
  (thickness, offset), Decimate (collapse to a ratio), Wireframe (strut thickness), Triangulate,
  Screw (axis, angle, steps, screw), Simple Deform (twist / bend / taper / stretch), Displace
  (noise strength, scale), Weld (distance); Bevel gained Segments. All available to the agent's
  `add_modifier`.
- Object mode the Blender way: a 3D cursor (Shift+right click places it; new objects appear there),
  pivot point (median / active / individual origins / cursor) and transform orientation (global /
  local / view) dropdowns in the header, Ctrl snapping during G/R/S, G/R/S on vertices in edit mode
  with proportional editing (O, wheel sizes the falloff), Ctrl+P / Alt+P parenting that keeps the
  world placement, Object > Set Origin (geometry to origin, origin to geometry / cursor, cursor to
  selected), collections in the outliner with eye toggles and M > Move to Collection.
- Shading like Blender's: Solid is studio grey for every surface; Material Preview shows materials,
  the scene's lights and image textures. Materials take a Base Color image (PNG/JPEG; box UVs are
  generated when the mesh has none) that the viewport samples through the mesh UVs and the path
  tracer samples at the hit. Textures reload from their path when a scene is opened. The agent's
  `set_material` takes a `texture` path; `--shading wire|solid|material` picks the start mode.
- Shader Editor in the new Shading workspace tab (Material Preview viewport above, node canvas
  below): Use Nodes builds a Principled BSDF into a Material Output from the flat material; nodes
  Principled BSDF, Image Texture, Mix Color, RGB and Value; drag headers to move, drag from an
  output socket onto an input to link (same type only, no cycles; dragging off a linked input picks
  the link up), Shift+A or right-click adds at the pointer, X / Delete removes, Home frames, middle
  drag or scroll pans. Every change evaluates the tree back into the material the viewport and the
  path tracer read; the material panel's fields write the BSDF constants and its Image button
  adds the Image Texture node. Node trees are saved with the material.
- Object-mode keymap (Blender defaults): Shift+A add menu at the pointer, Z shading menu,
  Shift+Z wireframe toggle, X asks before deleting (Delete key deletes at once), F3 operator
  search (also Edit > Operator Search), Ctrl+Tab edit mode, B box select and drag-to-box with the
  Select tool, numpad 1/3/7 (+Ctrl) axis views in orthographic with auto-perspective, numpad 5
  perspective toggle, numpad . frame selected, Shift+C frame all, Ctrl+J join, P separate.

## [0.3.0] - 2026-10-15

### Added
- Bevel: chamfer every edge (Edit > Mesh > Bevel, agent `mesh_op` bevel), the edges of the selected
  faces in edit mode (Ctrl+B, pointer-driven offset), and a Bevel modifier.

### Changed
- macOS: signed with the Developer ID certificate, notarized and stapled; ships as a
  drag-to-Applications disk image (`redrob-shape-<version>-macos-universal.dmg`).
- Windows: the archive is named `-portable.zip`, the name Redrob Launcher installs.
- Releases are published to cdn.redrob.ai/shape/ and through the organization's shared publish
  job, with `SHA256SUMS.txt` and `redrob-launcher.json` beside the installers.

## [0.2.0] - 2026-10-14

The first release of Redrob Shape written from scratch in Rust. Nothing from the earlier
Blender-based distribution (0.1.x, kept at the tag `blender-fork-final`) is carried here.

### Added
- Half-edge mesh core with primitives (cube, plane, UV sphere, torus, cylinder), BVH picking.
- Scene with objects, materials, lights, cameras, parenting and a snapshot undo history.
- Files: native `.rshape`; OBJ, STL, PLY and glTF/GLB import and export; read-only `.blend`
  import for Blender 2.5x through 5.x (zstd/gzip, modern attribute storage and legacy layouts).
- wgpu viewport: orbit/pan/zoom, grid, solid/wire shading, scene lighting, gizmos for lights and
  cameras, orange face highlight in edit mode.
- Modeling: extrude, inset, loop cut, smooth, subdivide (Catmull-Clark and linear), triangulate,
  flip normals; Mirror, Array, Subdivision and Smooth modifiers; UV box/planar/sphere projection.
- Editing: G/R/S modals with axis locks, tool bar, edit mode with face selection, E/I/Ctrl+R.
- Materials (colour, metallic, roughness, emissive), point/sun/spot lights, cameras.
- Built-in agent: Redrob Code over ACP with MCP tools for the scene; every call is an undo step.
- Path tracer with a progressive Render window (F12) and PNG output.
- Release archives for Linux x86_64, macOS universal, Windows x64 and arm64.
