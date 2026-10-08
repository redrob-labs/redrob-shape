#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Build every brand asset under brand/ from the design system's published files, and pin them.

    scripts/build-brand-assets.py --design-system ~/workplace/redrob-design-system

Nothing here draws a mark. Every symbol pixel and every symbol path comes from a file the design
system ships, and the files those come from are pinned by sha256 in brand/SOURCES.json, together
with the sha256 of every output. scripts/check-brand.py, which the gate runs, refuses a brand/
tree that differs from the pins -- so a hand edit, a re-trace or a recolor fails CI.

Redrob Shape is not one of the seven suite products, so it has no product glyph and no product
color (20-color.md: "No eighth product color"). Its icon is therefore the group's icon: the
favicon, a white symbol on a Redrob Blue tile with an empty doorway (10-logo.md, Favicons).

What is derived, and how:
  - Icon sizes the design system ships (16, 32, 48, 64, 512) are copied byte for byte. 128 and 256
    exist only as resizes of the 512 file, whose spec (radius 22%, mark 66%) the design system
    gives for the whole 192-512 band; they are never upscaled.
  - The .ico, .icns and the X11 window-icon header are containers around those pixels.
  - The SVG icons reuse favicon.svg's own symbol path unchanged: the tile kept for the app icon,
    the bare symbol in white (Solid White) for Blender's own UI logo, in black for the symbolic icon.
  - The splash is Redrob Black with the Primary White lockup and the product name set by the
    product-lockup rule (assets/Products/README.md): Pretendard SemiBold at 70% of the lockup's
    height, 40% of its height after the wordmark, on the wordmark's baseline.

Needs ImageMagick 7 (`magick`) for resizing, pixel dumps and the splash; the check does not.
"""

import argparse
import hashlib
import json
import re
import struct
import subprocess
import sys
import tempfile
import textwrap
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
BRAND = ROOT / "brand"
SOURCES_JSON = BRAND / "SOURCES.json"

FAVICONS = "assets/Favicons"
SOURCES = {
    "favicon-16": f"{FAVICONS}/favicon-16x16.png",
    "favicon-32": f"{FAVICONS}/favicon-32x32.png",
    "favicon-48": f"{FAVICONS}/favicon-48x48.png",
    "favicon-64": f"{FAVICONS}/favicon-64x64.png",
    "icon-512": f"{FAVICONS}/icon-512.png",
    "favicon-svg": f"{FAVICONS}/favicon.svg",
    "lockup-white": "assets/Logos/redrob-logo-primary-white.png",
    "pretendard-semibold": "fonts/Pretendard-SemiBold.woff2",
    "tokens": "tokens.css",
}

# Outputs, relative to brand/. Each maps onto the same path in the Blender tree, except the
# freedesktop size directories, which apply-brand-assets.sh places under release/freedesktop/icons/.
OUT_ICO = "release/windows/icons/winblender.ico"
OUT_ICNS = "release/darwin/Blender.app/Contents/Resources/blender_icon_legacy.icns"
OUT_PNG_512 = "512x512/apps/blender.png"
OUT_SVG_APP = "release/freedesktop/icons/scalable/apps/blender.svg"
OUT_SVG_SYMBOLIC = "release/freedesktop/icons/symbolic/apps/blender-symbolic.svg"
OUT_SVG_UI = "release/datafiles/icons_svg/blender.svg"
OUT_SVG_ABOUT = "release/datafiles/icons_svg/blender_logo_large.svg"
OUT_SPLASH = "release/datafiles/splash.png"
OUT_X11 = "intern/ghost/intern/GHOST_IconX11.hh"

SPLASH_SIZE = (1000, 500)
LOCKUP_HEIGHT = 56  # px; the lockup's minimum on screen is 18
# Measured from redrob-logo-primary-white.png (3150 x 630): the wordmark's ink ends at row 523,
# and "redrob" has no descender, so that row is its baseline.
LOCKUP_BASELINE = 523 / 630


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def magick(*args, capture=False):
    result = subprocess.run(["magick", *map(str, args)], check=True, capture_output=True)
    return result.stdout if capture else None


def png_size(data):
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise ValueError("not a PNG")
    return struct.unpack(">II", data[16:24])


THEME_PY = ROOT / "overlay/scripts/addons_core/redrob_shape/theme.py"


def token(tokens_css, name):
    """A token's hex value, as the dark theme resolves it (Redrob Shape's UI is dark)."""
    dark = tokens_css[tokens_css.index('[data-theme="dark"]') :]
    dark = dark[: dark.index("}")]
    for block in (dark, tokens_css):
        match = re.search(rf"--{re.escape(name)}:\s*(#[0-9a-fA-F]{{6}}|var\(--[\w-]+\))", block)
        if match:
            value = match.group(1)
            if value.startswith("var("):
                return token(tokens_css, value[6:-1])
            return value.lower()
    sys.exit(f"tokens.css has no --{name}")


def theme_token_names():
    return re.findall(r'^    "([\w-]+)": "#[0-9a-f]{6}",', THEME_PY.read_text(encoding="utf-8"), re.M)


def write_ico(path, pngs):
    """ICO with PNG entries (Windows Vista and later), largest last as Windows expects none."""
    header = struct.pack("<HHH", 0, 1, len(pngs))
    entries, blobs = b"", b""
    offset = 6 + 16 * len(pngs)
    for data in pngs:
        width, height = png_size(data)
        entries += struct.pack(
            "<BBBBHHII", width % 256, height % 256, 0, 0, 1, 32, len(data), offset + len(blobs)
        )
        blobs += data
    path.write_bytes(header + entries + blobs)


def write_icns(path, entries):
    """ICNS of PNG elements: (type, png bytes) pairs."""
    body = b"".join(kind.encode("ascii") + struct.pack(">I", 8 + len(data)) + data for kind, data in entries)
    path.write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)


def write_x11_header(path, pngs, scratch):
    """GHOST's window icon: width, height, then 0xAARRGGBB per pixel, rows top to bottom."""
    values = []
    for index, data in enumerate(pngs):
        width, height = png_size(data)
        source = scratch / f"x11-{index}.png"
        source.write_bytes(data)
        raw = magick(source, "-depth", "8", "rgba:-", capture=True)
        if len(raw) != width * height * 4:
            sys.exit("unexpected pixel dump size")
        values += [f"{width}", f"{height}"]
        for offset in range(0, len(raw), 4):
            red, green, blue, alpha = raw[offset : offset + 4]
            values.append(f"0x{(alpha << 24) | (red << 16) | (green << 8) | blue:x}")
    body = textwrap.fill(", ".join(values), width=100, initial_indent="    ", subsequent_indent="    ")
    path.write_text(
        "/* SPDX-FileCopyrightText: 2001-2002 NaN Holding BV. All rights reserved.\n"
        " * SPDX-FileCopyrightText: 2026 Redrob Labs\n"
        " *\n"
        " * SPDX-License-Identifier: GPL-2.0-or-later */\n\n"
        "/** \\file\n * \\ingroup GHOST\n * Icon image data for X11.\n */\n\n"
        "#pragma once\n\n"
        "/**\n"
        " * The Redrob Shape window icon at 16, 32, 48 and 64 px: the design system's favicon files,\n"
        " * pixel for pixel. Generated by scripts/build-brand-assets.py in the redrob-shape\n"
        " * repository; do not edit by hand.\n"
        " *\n"
        " * \\note Using 'unsigned' to avoid `-Wnarrowing` warning.\n"
        " */\n"
        "static const unsigned long BLENDER_ICONS_WM_X11[] = {\n"
        f"{body},\n"
        "};\n",
        encoding="utf-8",
    )


def symbol_path(favicon_svg):
    """The white symbol path from favicon.svg, unchanged."""
    paths = re.findall(r'<path fill="(#[0-9A-Fa-f]+)" d="([^"]+)"/>', favicon_svg)
    white = [d for fill, d in paths if fill.upper() in {"#FFF", "#FFFFFF"}]
    if len(white) != 1:
        sys.exit("favicon.svg no longer has exactly one white symbol path")
    return white[0]


def symbol_svg(d, fill, label):
    # The symbol keeps favicon.svg's own 64-unit coordinates, drawn at 1600 units: Blender's SVG
    # icon rasterizer (blf_glyph.cc) treats 1600 document units as the requested size, so a 64-unit
    # document came out as a dot. The space around the ink is wider than the 0.3x clear space.
    return (
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="1600" height="1600"'
        f' role="img" aria-label="{label}"><path fill="{fill}" d="{d}"/></svg>\n'
    )


def build_splash(path, design, black, scratch):
    width, height = SPLASH_SIZE
    lockup = scratch / "lockup.png"
    magick(design / SOURCES["lockup-white"], "-filter", "Lanczos", "-resize", f"x{LOCKUP_HEIGHT}", lockup)
    lockup_width = int(magick(lockup, "-format", "%w", "info:", capture=True))
    name_size = 0.7 * LOCKUP_HEIGHT
    gap = round(0.4 * LOCKUP_HEIGHT)
    font = design / SOURCES["pretendard-semibold"]
    name_width = int(
        magick(
            "-background", "none", "-fill", "white", "-font", font, "-pointsize", f"{name_size:g}",
            "label:Shape", "-trim", "-format", "%w", "info:", capture=True,
        )
    )
    unit = lockup_width + gap + name_width
    left = (width - unit) // 2
    top = (height - LOCKUP_HEIGHT) // 2
    baseline = top + round(LOCKUP_BASELINE * LOCKUP_HEIGHT)
    magick(
        "-size", f"{width}x{height}", f"xc:{black}",
        lockup, "-geometry", f"+{left}+{top}", "-composite",
        "-font", font, "-pointsize", f"{name_size:g}", "-fill", "white",
        "-annotate", f"+{left + lockup_width + gap}+{baseline}", "Shape",
        "-alpha", "off", "-define", "png:exclude-chunks=date,time", "-strip", f"PNG24:{path}",
    )


def build(design):
    missing = [rel for rel in SOURCES.values() if not (design / rel).is_file()]
    if missing:
        sys.exit(f"design system files missing: {missing}")
    read = {key: (design / rel).read_bytes() for key, rel in SOURCES.items()}
    tokens_css = read["tokens"].decode("utf-8")
    blue, black = token(tokens_css, "blue-6"), token(tokens_css, "redrob-black")
    favicon_svg = read["favicon-svg"].decode("utf-8")
    if blue.upper() not in favicon_svg.upper():
        sys.exit("favicon.svg's tile is not tokens.css --blue-6")
    d = symbol_path(favicon_svg)

    with tempfile.TemporaryDirectory(dir=None) as temporary:
        scratch = Path(temporary)
        resized = {}
        for size in (128, 256):
            out = scratch / f"icon-{size}.png"
            magick(design / SOURCES["icon-512"], "-filter", "Lanczos", "-resize", f"{size}x{size}",
                   "-define", "png:exclude-chunks=date,time", "-strip", f"PNG32:{out}")
            resized[size] = out.read_bytes()

        outputs = {
            OUT_PNG_512: lambda p: p.write_bytes(read["icon-512"]),
            OUT_ICO: lambda p: write_ico(p, [read["favicon-16"], read["favicon-32"], read["favicon-48"],
                                             read["favicon-64"], resized[128], resized[256]]),
            OUT_ICNS: lambda p: write_icns(p, [
                ("icp4", read["favicon-16"]), ("icp5", read["favicon-32"]), ("icp6", read["favicon-64"]),
                ("ic07", resized[128]), ("ic08", resized[256]), ("ic09", read["icon-512"]),
                ("ic11", read["favicon-32"]), ("ic12", read["favicon-64"]), ("ic13", resized[256]),
                ("ic14", read["icon-512"]),
            ]),
            OUT_SVG_APP: lambda p: p.write_bytes(read["favicon-svg"]),
            OUT_SVG_UI: lambda p: p.write_text(symbol_svg(d, "#FFF", "Redrob"), encoding="utf-8"),
            OUT_SVG_ABOUT: lambda p: p.write_text(symbol_svg(d, "#FFF", "Redrob"), encoding="utf-8"),
            OUT_SVG_SYMBOLIC: lambda p: p.write_text(symbol_svg(d, "#000", "Redrob"), encoding="utf-8"),
            OUT_SPLASH: lambda p: build_splash(p, design, black, scratch),
            OUT_X11: lambda p: write_x11_header(
                p, [read["favicon-16"], read["favicon-32"], read["favicon-48"], read["favicon-64"]], scratch
            ),
        }
        for rel, make in outputs.items():
            target = BRAND / rel
            target.parent.mkdir(parents=True, exist_ok=True)
            make(target)

    pins = {
        "//": "Written by scripts/build-brand-assets.py; checked by scripts/check-brand.py. Do not edit.",
        "design_system": "redrob-design-system (the published distribution)",
        "sources": {rel: sha256(design / rel) for rel in SOURCES.values()},
        "symbol_path_sha256": hashlib.sha256(d.encode("utf-8")).hexdigest(),
        "tokens": {name: token(tokens_css, name) for name in theme_token_names()},
        "copied_unchanged": {OUT_PNG_512: SOURCES["icon-512"], OUT_SVG_APP: SOURCES["favicon-svg"]},
        "outputs": {rel: sha256(BRAND / rel) for rel in sorted(outputs)},
    }
    SOURCES_JSON.write_text(json.dumps(pins, indent=2) + "\n", encoding="utf-8")
    for rel in sorted(outputs):
        print(f"  {rel}  {pins['outputs'][rel][:12]}")


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--design-system", required=True, type=Path)
    args = parser.parse_args()
    build(args.design_system.expanduser().resolve())


if __name__ == "__main__":
    main()
