#!/usr/bin/env python3
"""Rewrites Blender's product name to ours inside user-visible strings in the fetched checkout.

Narrow by necessity. UPSTREAM.md commits this repository to Blender keeping "its names, file
compatibility, functionality, copyright, and licenses", and in Blender the name is load-bearing in
more places than usual: RNA identifiers, operator idnames, `bpy` module paths, the .blend format's
own strings, and the `BLENDER_*` build and version macros. Renaming any of those breaks scripts,
add-ons and files.

So only strings inside Blender's own translation macros are rewritten:

    N_("...")       marked for translation, not translated at that point
    IFACE_("...")   interface label
    TIP_("...")     tooltip

Those macros ARE the definition of "a user reads this". Everything else keeps Blender's naming.

Two further holdbacks, because renaming them would make the string false:

  * URLs -- blender.org links stay valid links.
  * ".blend", "Blender file", and references to the Blender Foundation, manual and community. Our
    builds read and write Blender files; the manual and the Foundation are not ours.

Usage: sweep-strings.py <checkout-root> [--check]
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

PRODUCT = "Redrob Reblend"

# Whole word only, and never the build macros or the Python module.
NAME = re.compile(r"\bBlender\b(?!_VERSION|_FILE|_STARTUP|Foundation)")

MACROS = re.compile(
    r'\b(?:N_|IFACE_|TIP_|DATA_|CTX_N_|CTX_IFACE_|RPT_)\(\s*"((?:[^"\\]|\\.)*)"',
)

URL = re.compile(r"https?://\S*")

KEEP = (
    "Blender Foundation",
    "Blender file",
    "Blender manual",
    "Blender community",
    "Blender Studio",
    ".blend",
)


def rewrite(text: str) -> str:
    stash: list[str] = []

    def hide(value: str) -> str:
        stash.append(value)
        return f"\x00{len(stash) - 1}\x00"

    body = URL.sub(lambda m: hide(m.group(0)), text)
    for phrase in KEEP:
        if phrase in body:
            body = body.replace(phrase, hide(phrase))
    body = NAME.sub(PRODUCT, body)
    return re.sub(r"\x00(\d+)\x00", lambda m: stash[int(m.group(1))], body)


def sweep(path: Path) -> tuple[str, int]:
    text = path.read_text(encoding="utf-8", errors="surrogateescape")
    changes = 0

    def on_macro(match: re.Match) -> str:
        nonlocal changes
        original = match.group(1)
        rewritten = rewrite(original)
        if rewritten == original:
            return match.group(0)
        changes += 1
        return match.group(0).replace(original, rewritten, 1)

    return MACROS.sub(on_macro, text), changes


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    source = args.root / "source"
    if not source.is_dir():
        sys.exit(f"error: {source} not found -- is {args.root} a fetched checkout?")

    files = 0
    total = 0
    for path in sorted(source.rglob("*")):
        if path.suffix not in {".c", ".cc", ".hh", ".h"}:
            continue
        if "tests" in path.parts or "test" in path.parts:
            continue
        rewritten, changes = sweep(path)
        if changes == 0:
            continue
        files += 1
        total += changes
        if not args.check:
            path.write_text(rewritten, encoding="utf-8", errors="surrogateescape")

    verb = "would rewrite" if args.check else "rewrote"
    print(f"string sweep {verb} {total} user-visible string(s) in {files} file(s) -> {PRODUCT!r}")

    if args.check and total:
        print("error: user-visible strings still name Blender; run without --check and regenerate "
              "the patch", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
