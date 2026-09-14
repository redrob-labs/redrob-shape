# Redrob Reblend — patch-based customization of Blender

A compatibility-first, agentic Blender distribution. This repository holds **only our changes**.
Upstream Blender is fetched at build time, never vendored, so `git log` here shows Reblend work
rather than being buried under upstream history.

```
upstream.json     the pinned upstream repo + commit -- the only place a baseline appears
overlay/          files that exist only in this product (copied in, never patched)
patches/          our modifications to upstream files, one patch per logical change
scripts/          fetch, apply, and THE GATE
```

Build:

```
./scripts/verify-patches.sh     # fetch upstream, apply everything, assert the result
```

That leaves a complete, customized Blender tree in `build/blender`, built with Blender's own CMake
toolchain and the `redrob_reblend` config preset.

## Why this shape

The previous shape carried Blender's **entire history** — 164,952 commits, 1.6 GB, root commit
`12315f4d0e0` "Initial revision" (Hans Lambermont, 2002-10-12), with authentic Blender release
hashes. Unlike the sibling `redrob-cad`, nothing was broken about it: a real merge base existed,
`UPSTREAM.md` recorded the downstream baseline, and upstream could have been merged as soon as a
remote was added.

What it cost was proportion. **Three commits out of 164,952 were ours.** Our work was invisible in
the log and every clone paid 1.6 GB for history that upstream already hosts.

| Kind | Count | Where it lives now |
|---|---|---|
| Files only we have | 43 | `overlay/` |
| Upstream files we modify | 6 | `patches/` — 4 patches, grouped by intent |
| Upstream files we delete | 0 | — |

Blender's history is preserved on `archive/blender-fork-history` and remains available from
upstream, so this shape discards nothing that cannot be fetched again.

## What is ours

- `intern/redrob_agent/` — the agent sidecar, in Rust, built by CMake behind `WITH_REDROB_AGENT`.
  The option defaults to OFF; the `redrob_reblend` cmake preset is what turns it on, which the gate
  asserts, because otherwise a build silently ships with no agent and no error.
- `scripts/addons_core/redrob_reblend/` — the Blender add-on: panels, operators, runtime,
  protocol, tools, theme, state, context, and 18 unit tests.

## Branding, and the line it does not cross

`patches/0004` renames the **application shell** — the window title and the desktop entry — to
Redrob Reblend. Blender is GPL and its trademark policy asks that a modified redistribution not
present itself as official Blender, so renaming the shell is what compliance looks like, not an
optional flourish.

It stops there deliberately. The gate asserts what must *not* have changed:

- `COPYING` and `release/license/` ship intact.
- The upstream metainfo still credits Blender.
- The blend-file subversion matches the pin, so our builds cannot silently write files upstream
  cannot read.
- **No patch touches `tests/`, `source/blender/editors/` or `scripts/startup/bl_operators/`.**
  `UPSTREAM.md` forbids resolving an upstream sync by dropping upstream operators, keymaps,
  editors, importers, exporters, render engines or tests; this makes that structural rather than a
  rule someone has to remember.

Blender's 3D core keeps its own names throughout. What is rebranded is the product wrapping it.

## Bumping upstream

1. Edit `commit` (and `version`) in `upstream.json`.
2. Run `./scripts/verify-patches.sh`.
3. A patch that no longer applies fails with the conflicting hunk named. Regenerate it against the
   new upstream and commit both together.

The gate discards and refetches before applying anything. Verifying against a tree that is already
half-patched proves nothing — a stale patch would report success because its changes were already
there.

## CI

This repository had no GitHub CI: the add-on's 18 tests existed with no workflow to execute them,
and Blender's own pipeline is Gitea-based and belongs to upstream. Now `verify-patches.yml` runs
the gate, and a second job runs the add-on tests that need no Blender runtime — straight out of
`overlay/`, without fetching upstream at all.
