# Contributing

Thanks for helping. This is the working agreement for the repository: how branches are named, what
has to be green before a merge, and the rule that follows from this being a **patch set over upstream
Blender** rather than a fork of its history.

## The rule that shapes everything

**This repository holds only our changes.** Upstream Blender is fetched at build time from the commit
pinned in `upstream.json` and is never committed: the previous shape carried Blender's entire history,
164,952 commits and 1.6 GB, which is what this layout replaced.

So:

- **`upstream.json` is the only place a baseline appears.** Bumping upstream is a change to that file
  plus whatever patches needed rebasing, in one branch.
- **`overlay/`** holds files that exist only in this product. They are copied in, never patched.
- **`patches/`** holds our modifications to upstream files, one patch per logical change.
- **Never edit the fetched checkout and stop there.** The edit disappears on the next fetch.

Blender tracks bundled assets in **Git LFS**, and the mirror this repository fetches from cannot serve
the smudge filter, so the fetch skips it. That is fine because no patch or overlay touches an
LFS-tracked asset, and the gate asserts the outcome (a pointer is still a pointer) rather than trusting
the configuration. If you find yourself needing to modify one, stop and raise it: it changes the fetch.

## Branch model

One long-lived branch, `main`. Short-lived branches off it, merged by pull request.

- **`main`** is the trunk: no direct pushes, no force pushes, no deletion.
- **Working branches** are `<type>/<short-slug>`, e.g. `fix/agent-panel-labels`,
  `feat/transport-selection`. Types: `feat`, `fix`, `chore`, `docs`, `test`, `refactor`, `perf`.
- **Upstream bumps** land on `sync/upstream-<date>`.
- Release tags are cut from `main`, formatted `v<major>.<minor>.<patch>`.

[redrob-code](https://github.com/redrob-labs/redrob-code) and
[redrob-console](https://github.com/mckinley-and-rice/redrob-console) run Git Flow with a `develop`
branch. This one does not, so cut from `main`.

## Day to day

```bash
git switch main && git pull
git switch -c fix/short-description

./scripts/fetch-upstream.sh
./scripts/apply-patches.sh
# change the tree, then capture it as a patch
./scripts/verify-patches.sh      # what CI runs

# open a pull request into main
```

The add-on tests need no Blender runtime and run straight out of the checkout, so run them too.

### Commits

Subject in the imperative. Use the body to say _why_: a patch's reason is what a future rebase needs
and the diff cannot carry.

## What CI checks

**`.github/workflows/verify-patches.yml`** applies every patch to the pinned upstream from a clean
tree, copies the overlay, and asserts the result. Applying patches to a tree that is already
half-patched proves nothing, which is why the gate starts clean: a stale patch would report success
because its changes were already there.

A second job runs the add-on tests.

Run `./scripts/verify-patches.sh` before pushing.
