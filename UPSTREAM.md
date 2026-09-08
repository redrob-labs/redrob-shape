# Blender upstream relationship

Redrob Reblend is based on the official [`blender/blender`](https://github.com/blender/blender) source tree. Blender remains the 3D core and retains its names, file compatibility, functionality, copyright, and licenses.

The initial downstream baseline is:

```text
acf2fda52b4f7e07a459136e99c80470f3a92725
Refactor: Node Wrangler: Create `Hotkey` dataclass
```

The repository should keep two remotes:

```text
origin    https://github.com/redrob-labs/redrob-reblend.git
upstream  https://github.com/blender/blender.git
```

When syncing upstream, review conflicts specifically around:

- `CMakeLists.txt`
- `source/creator/CMakeLists.txt`
- `scripts/modules/addon_utils.py`
- `scripts/addons_core/redrob_reblend/`

Never resolve a sync by changing Blender's blend-file compatibility constants or silently dropping upstream operators, keymaps, editors, importers, exporters, render engines, or tests.
