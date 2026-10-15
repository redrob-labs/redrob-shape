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
