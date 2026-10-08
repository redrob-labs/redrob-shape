#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Refuse a brand/ tree that is not exactly what build-brand-assets.py made from the design system.

    scripts/check-brand.py            # the repository's brand/ tree
    scripts/check-brand.py TREE       # also: TREE (a patched Blender checkout) carries those files

No dependencies: the gate and CI run it without ImageMagick. A build reads no pixels, so a
hand-drawn or recolored mark passes every other check for as long as nobody looks; this one
compares bytes against pins written when the files were built from the design system.
"""

import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BRAND = ROOT / "brand"
SIZE_DIR = re.compile(r"^\d+x\d+/")


def sha256(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def destination(rel):
    """Where apply-brand-assets.sh puts a brand/ file in the Blender tree."""
    return f"release/freedesktop/icons/{rel}" if SIZE_DIR.match(rel) else rel


def main():
    errors = []
    pins = json.loads((BRAND / "SOURCES.json").read_text(encoding="utf-8"))
    outputs = pins["outputs"]

    present = {
        str(path.relative_to(BRAND)) for path in BRAND.rglob("*") if path.is_file() and path.name != "SOURCES.json"
    }
    for rel in sorted(present - set(outputs)):
        errors.append(f"brand/{rel} is not built from the design system (not in SOURCES.json)")
    for rel, digest in sorted(outputs.items()):
        path = BRAND / rel
        if not path.is_file():
            errors.append(f"brand/{rel} is missing")
        elif sha256(path) != digest:
            errors.append(f"brand/{rel} was changed by hand: rebuild it with scripts/build-brand-assets.py")

    # Files the design system ships are carried unchanged, so their pins must agree.
    for rel, source in pins["copied_unchanged"].items():
        if outputs.get(rel) != pins["sources"].get(source):
            errors.append(f"brand/{rel} is not byte-identical to the design system's {source}")

    # Every SVG mark carries favicon.svg's own symbol path, untouched.
    for rel in outputs:
        if rel.endswith(".svg"):
            for d in re.findall(r'<path fill="#FFF" d="([^"]+)"|<path fill="#000" d="([^"]+)"', (BRAND / rel).read_text()):
                path = next(part for part in d if part)
                if hashlib.sha256(path.encode()).hexdigest() != pins["symbol_path_sha256"]:
                    errors.append(f"brand/{rel} carries a symbol path that is not the design system's")
            if re.search(r"\b(filter|mask|opacity|transform)=", (BRAND / rel).read_text()):
                errors.append(f"brand/{rel} applies an effect to the mark (filter, mask, opacity or transform)")

    # The theme's colours are design-system tokens, by name and value.
    theme_py = (ROOT / "overlay/scripts/addons_core/redrob_shape/theme.py").read_text(encoding="utf-8")
    theme_tokens = dict(re.findall(r'^    "([\w-]+)": "(#[0-9a-f]{6})",', theme_py, re.M))
    if not theme_tokens:
        errors.append("theme.py has no TOKENS table")
    if theme_tokens != pins["tokens"]:
        errors.append(f"theme.py TOKENS differ from tokens.css as pinned: {theme_tokens} != {pins['tokens']}")

    if len(sys.argv) > 1:
        tree = Path(sys.argv[1])
        for rel, digest in sorted(outputs.items()):
            placed = tree / destination(rel)
            if not placed.is_file() or sha256(placed) != digest:
                errors.append(f"{destination(rel)} in {tree} is not our brand asset")

    if errors:
        print("\n".join(f"error: {error}" for error in errors), file=sys.stderr)
        sys.exit(1)
    print(f"brand assets match the design system pins: {len(outputs)} files")


if __name__ == "__main__":
    main()
