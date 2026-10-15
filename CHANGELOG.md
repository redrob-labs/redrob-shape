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
- Viewport shading popover (the arrow next to the shading buttons), after Blender's: Lighting
  Studio / MatCap (Clay, Metal, Red Wax, Ceramic, drawn in the shader from the view-space normal) /
  Flat; Color Material (the material's new Viewport Display colour) / Object / Random / Single;
  Backface Culling, X-Ray with alpha (Alt+Z, header toggle) and Cavity (crease darkening).
- Sculpt Mode (Ctrl+Tab mode menu, header mode dropdown, Sculpting workspace tab): brushes Draw
  (X), Clay (C), Smooth (S, or Shift while painting), Grab (G), Inflate (I), Flatten (Shift+T) in
  the toolbar and header; the brush is a red ring of `Radius` pixels, F / Shift+F adjust radius /
  strength by pointer distance (click confirms, Esc restores), Ctrl inverts, Shift+X/Y/Z toggle
  symmetry. One stroke is one undo step. Dabs are spaced at 10 % of the radius with Blender's
  smooth falloff; the Sculpting workspace switches to MatCap lighting.
- UV editing: the UV Editing workspace (UV editor on the left, edit mode with everything
  selected on the right) draws the active mesh's UV faces over the 0..1 grid with the selected
  faces in orange and counts islands and seams; edit-mode U menu with Unwrap (islands split at
  seams, each unfolded face by face across shared edges, then packed), Smart UV Project (66 degree
  normal groups), Cube / Cylinder / Sphere Projection and Pack Islands; Ctrl+E Mark Seam / Clear
  Seam on the selected edges or faces, seams drawn red in the viewport. Seams are saved with the
  mesh.
- Keyframe animation: objects carry an Action of F-curves (Location / Rotation / Scale X Y Z) with
  Constant, Linear or Bezier (auto-clamped, Blender's default ease) interpolation; the scene keeps
  frame, range and fps and is evaluated into the transforms on every frame change. Object-mode I
  opens the Insert Keyframe menu (Location, Rotation, Scale, Location & Rotation, all), Alt+I
  deletes the keys at the frame. The Animation workspace adds the Timeline: playback buttons, frame
  / start / end / fps fields, a ruler with the active object's keys as diamonds (summary plus one
  row per channel), a blue playhead you can scrub; Space plays, Left / Right step, Shift+Left /
  Right jump to the ends, Up / Down jump to the next / previous key. Saved in .rshape.
- Rigging (part 1): Armature objects (Shift+A > Armature: one bone; bones have head, tail, roll,
  parent and a pose transform), drawn as octahedral bones, blue when selected. Pose Mode (Ctrl+Tab
  menu, header dropdown): click picks a bone, G / R move / rotate the selected bones' pose with
  LMB / Enter to confirm and Esc to cancel, Alt+G / Alt+R clear, A selects all, Clear Pose in the
  header. Ctrl+P now opens Set Parent To: Object (Keep Transform), and with an armature active
  Armature Deform / With Automatic Weights, which parent the meshes, fill vertex groups from bone
  distance and add an Armature modifier that skins the mesh by those weights. Vertex groups and
  poses are saved. Fix: the outliner panicked on parented objects (indent inside a row).
- Rigging (part 2): Tab on an armature enters Armature Edit Mode (rest bones drawn): click picks,
  E extrudes a child bone from the active tail and the pointer places it, G moves the selected
  bones, X deletes (children re-parent), A selects all; Tab from Pose Mode goes to Edit Mode. In
  Pose Mode, I keys the selected bones (bone location / rotation F-curves, shown in the Timeline)
  and playback poses them. Bone picking accepts clicks on the bone body, not only its axis.
  `SHAPE_TRACE_STATUS=1` echoes the status line to stderr for scripted testing.
- Weight Paint mode (Ctrl+Tab menu / header dropdown on a mesh with vertex groups): the viewport
  colours the vertices of the active group with Blender's blue-cyan-green-yellow-red ramp, LMB
  strokes add weight under the brush on the deformed mesh (Ctrl subtracts; Radius / Weight in the
  header), the group is picked from a header dropdown, one stroke is one undo step.
- Rendering: cameras gain Depth of Field (F-Stop, Focus Distance; thin-lens sampling in the path
  tracer), the Render tab gains a Denoise toggle (edge-aware bilateral filter tuned to the image's
  noise level, used for the window and saved PNGs), Output gains Resolution %, the frame range and
  an output path, and Render > Render Animation (Ctrl+F12) renders every frame of the range to
  `<path>####.png` on a worker thread with progress in the Output tab.
- Curve objects: Shift+A > Curve > Bezier / Circle add Bezier splines (control points with
  Automatic / Vector / Aligned / Free handles, Resolution Preview U, Cyclic U); a Bevel Depth and
  Resolution sweep a tube that the viewport, the renderer and the exporters use, and Object >
  Convert to Mesh (also in the Data tab) turns it into mesh data. Curve Edit Mode (Tab): click picks
  a control point (points and handles drawn), G moves, E extrudes from the end of the spline, X
  deletes, V sets the handle type, A selects all. Fix: pressing Tab no longer parks keyboard focus
  on a panel widget, so hotkeys keep working after a mode change.
- Text objects (Shift+A > Text): the body is set in the bundled Ubuntu Light font, glyph outlines
  are triangulated (holes kept) and optionally extruded into a solid; Data tab with Body, Size and
  Extrude. Metaballs (Shift+A > Metaball): elements with radius and stiffness, polygonised at a
  threshold with marching tetrahedra so nearby balls merge; Data tab edits elements, resolution and
  threshold. Add > Image as Plane makes a textured quad with the image's aspect. Text and metaballs
  convert to plain meshes (Object > Convert to Mesh).
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
