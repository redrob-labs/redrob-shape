# AGENTS.md

Instructions for an agent working in `redrob-labs/redrob-shape`. Everything below was read out of
this tree or run against it.

## What this repository is

Redrob Shape is a Blender distribution with an AI agent sidecar, and this repository holds **only
our changes to Blender**. Upstream Blender is fetched at build time into `build/blender` from the
single commit pinned in `upstream.json` and is never committed, so `git log` here shows Redrob Shape work
instead of 164,952 upstream commits. Our two products are a Rust planning sidecar
(`overlay/intern/redrob_agent`) and a Blender add-on (`overlay/scripts/addons_core/redrob_shape`).

## The pin and the patch mechanism

Get this right first, because it is the shape of the whole repository and the thing that is easiest
to get wrong.

**The pin lives in `upstream.json`, and nowhere else.** It names `repo`
(`https://github.com/blender/blender.git`), `commit` (`acf2fda52b4f7e07a459136e99c80470f3a92725`)
and `version` (`5.3.0-alpha`). Every script reads those three values out of that file with `python3
-c 'import json'`, so bumping upstream is a one-line reviewable change rather than a hunt.

Our delta is split by the **kind** of change, because each kind has a storage format that survives
an upstream bump differently:

| Directory | Holds | Why not the other way |
|---|---|---|
| `patches/` | 5 patches modifying 42 existing upstream files | A modification has to be expressed as a diff so a bumped upstream can conflict on it visibly |
| `overlay/` | 51 files that exist only in this product, copied verbatim | A patch that creates a whole file is an unreadable copy of that file, and it conflicts on nothing |
| `brand/` | 3 binary icons copied over upstream's | A binary patch conflicts every time upstream revises its icon and shows a reviewer two blobs |

No patch creates a new file. Every one of the 42 targets already exists upstream, so a patch failing
means upstream moved the code it edits.

`scripts/` is the machinery, and the order matters:

1. `fetch-upstream.sh` clones the pinned commit shallow into `build/blender`, forcing `core.autocrlf
   false` and `core.eol lf` so a Linux-generated patch still applies on a Windows runner, and
   skipping the Git LFS smudge filter. It re-verifies that the checked-out `HEAD` equals the pinned
   commit and fails if not.
2. `apply-patches.sh` runs `git apply --3way` over `patches/*.patch` in numeric order and stops on
   the first failure, printing the conflicting hunk. `--3way` matters: without it a drifted patch can
   fail silently.
3. `apply-overlay.sh` copies `overlay/` in, then re-reads every file and `cmp`s it, because a silent
   copy failure yields a tree that looks like upstream and is branded as ours.
4. `apply-brand-assets.sh` copies `brand/` over upstream's icons and `cmp`s each one.
5. `scripts/verify-patches.sh` is **the gate**. It `rm -rf build/blender` first, then runs the four
   steps above, then asserts the result. The discard is the point: applying a patch to a
   half-patched tree reports success because the change is already there.

**Git LFS:** Blender tracks bundled assets such as `assets/brushes/*.blend` in LFS, and the GitHub
mirror cannot serve those objects, so the fetch sets the smudge filter to `--skip`. The gate asserts
the *outcome*, that a pointer file is still a pointer, rather than trusting the config, because a
machine with no `git-lfs` installed never runs the filter at all and cannot tell a working skip from
a missing tool. Nothing in `patches/` or `overlay/` touches an LFS asset. A real Blender **build**
does need them, which requires `git lfs pull` against `projects.blender.org`.

**What the gate asserts, beyond "the patches applied":** the add-on is registered in
`addon_utils.py` and not merely copied in; 12 named add-on modules and `blender_manifest.toml`
exist; `connection_ui` is registered and does **not** use `from __future__ import annotations`,
because PEP 563 stringifies annotations and breaks `bpy.props` registration inside Blender where CI
cannot see it; `WITH_REDROB_AGENT` is declared, attached to the `blender` target, and set `ON` by
the `redrob_shape` cmake preset (the option defaults to OFF, so without the preset a build ships
with no agent and no error); the window title, splash, animation player, desktop entry and macOS
`Info.plist` all name Redrob Shape, and the icons byte-match `brand/`; `COPYING`,
`release/license/` and Blender's metainfo credit are intact; the `version` in `upstream.json` equals
the version computed from `BKE_blender_version.h`; no patch line removes an upstream operator,
keymap, registration or test; and `scripts/sweep-strings.py --check` finds no user-visible string
still naming Blender.

**When the pin moves:** edit `commit` and `version` in `upstream.json` together, run the gate, and
regenerate whichever patch reports a conflict, committing the pin and the patches in one branch.
Changing `commit` alone fails the version assertion. Never edit `build/blender` and stop there: it
is gitignored, and the gate deletes it.

Never resolve an upstream conflict by dropping upstream operators, keymaps, editors, importers,
exporters, render engines or tests, and never change Blender's blend-file compatibility constants.
The gate enforces both structurally.

## Branch model, as configured now

- **`develop` is the default branch and the base of every pull request.** Cut your branch from it.
- **`main` is released state.** It moves by merging `develop` into it, and release tags are cut from
  `main`.
- **A hotfix** branches from `main`, merges into `main`, releases, and then `main` is merged back
  into `develop`. That back-merge is not bookkeeping: the sibling repository `redrob-code` spent a
  month with a lockfile its own default branch could not install from because the step was skipped.
- **Both branches are protected:** pull requests only, force pushes and deletions blocked, 0
  required reviews, admin enforcement off.
- **Required checks:** `Add-on unit tests` and `Patches apply to pinned upstream`.

**Working branch names are `<type>/<short-slug>`, and the allowed types are `feat`, `fix`, `docs`,
`chore`, `test`, `refactor`, `perf`, `hotfix`, `release` and `sync`.**
`.github/workflows/gitflow.yml` now enforces that on every pull request, and its second job fails a
push to `main` that `develop` does not yet contain. Both rules were already prose and both were
broken anyway: every branch in this repository's history so far carries a `kiro/` prefix that no
document defines, and a name says what the change is, not which tool made it. `develop` and `main`
are exempt because a promotion or a back-merge pull request is not named after a type.

**Live caveat:** pull request #7 is open from another workstream and targets `main`, not `develop`.
When it merges, `main` will be ahead of `develop` and needs a back-merge before anything else is
based on `develop`. That pull request also adds a `CONTRIBUTING.md` describing a `main`-only branch
model, which the configuration above has since replaced.

## Commands

Python is the only toolchain needed to run a check locally; the add-on tests use stdlib `unittest`
and there is no `requirements.txt`, `package.json` or lockfile anywhere in the tree.

```bash
# The add-on unit tests. Fast, needs no Blender and no network.
cd overlay/scripts/addons_core/redrob_shape
python3 -m unittest discover -s tests -p 'test_*.py'
# 73 tests, OK, exit 0 (verified on python 3.14.4)

# The gate. Network-bound: it shallow-clones Blender before asserting anything.
./scripts/verify-patches.sh
```

The Rust sidecar is built by Blender's own CMake behind `WITH_REDROB_AGENT`, enabled by the
`redrob_shape` cmake preset at
`overlay/build_files/cmake/config/redrob_shape.cmake`. `Cargo.toml` pins `rust-version = "1.88"`
and edition 2021.

**`__pycache__` is tracked in this repository (16 `.pyc` files).** Running the add-on tests rewrites
them, so a clean tree becomes dirty as a side effect of testing. Restore them with `git checkout --
overlay/scripts/addons_core/redrob_shape/__pycache__
overlay/scripts/addons_core/redrob_shape/tests/__pycache__` and never commit them.

`README.md` states 4 patches, 6 modified upstream files and 18 add-on tests. The tree now has 5
patches, 42 modified files and 73 tests, so count from the tree rather than the prose.

## CI

Two workflows, `.github/workflows/verify-patches.yml` and `.github/workflows/gitflow.yml`.

- Triggers: `pull_request` with **no branch filter**, so it runs on every pull request including
  those into `develop`; `push` restricted to `main`; and `workflow_dispatch`. Note the gap: a merge
  into `develop` produces no push run, so the pull request run is the only signal before it lands.
- Job `verify`, named **Patches apply to pinned upstream**, runs `./scripts/verify-patches.sh` on
  `ubuntu-latest`.
- Job `addon-tests`, named **Add-on unit tests**, runs the `unittest discover` command above out of
  `overlay/` without fetching upstream at all.
- Both job names are the two required checks, so both must pass to merge.
- The `addon-tests` job contains a "Setup Python" step that is `actions/setup-node` guarded by `if:
  false`. It never executes, and the job uses the runner's system `python3`.

Concurrency is grouped per workflow and pull request with `cancel-in-progress: true`, so pushing
again cancels the previous run.
