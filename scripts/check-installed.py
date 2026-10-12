#!/usr/bin/env python3
"""Checks an INSTALLED Redrob Shape build: the thing a user would get, not the source tree.

    python3 scripts/check-installed.py <install dir> [--window]

<install dir> is what `cmake --build <out> --target install` wrote: on macOS the directory holding
`Redrob Shape.app`, on Windows and Linux the directory holding `blender(.exe)`.

Four checks, each against the built artifact rather than against a file we copied in:

1. Brand: our splash is compiled INTO the binary. Blender embeds datafiles as raw byte arrays, so
   brand/release/datafiles/splash.png has to appear verbatim in the executable. A build that kept
   upstream's splash fails here. On Windows every image of brand/.../winblender.ico has to be in
   the .exe (the resource compiler stores each image's bytes unchanged); on macOS the bundle's
   .icns has to be ours byte for byte.
2. The agent binary sits beside Blender, where the add-on looks for it.
3. The agent answers a `health` request over the add-on's own protocol (protocol.py, imported from
   the overlay, so the check speaks exactly what the add-on speaks).
4. Blender starts, in the background with factory settings, and the redrob_shape add-on is loaded.
5. With --window: the real app opens its window (a timer prints a marker from inside it, then quits).

Exit status 0 only when all four pass. Uses only the standard library, so it runs on every
runner's stock python.
"""

import hashlib
import json
import os
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BRAND = ROOT / "brand"
ADDON = ROOT / "overlay" / "scripts" / "addons_core" / "redrob_shape"

failures = []


def check(ok, message):
    print(("ok    " if ok else "FAIL  ") + message, flush=True)
    if not ok:
        failures.append(message)
    return ok


def layout(install):
    """(blender executable, agent executable, platform name)."""
    if sys.platform == "darwin":
        app = install / "Redrob Shape.app"
        return app / "Contents" / "MacOS" / "Redrob Shape", app / "Contents" / "MacOS" / "redrob-shape-agent", "macos"
    if sys.platform == "win32":
        return install / "blender.exe", install / "redrob-shape-agent.exe", "windows"
    return install / "blender", install / "redrob-shape-agent", "linux"


def ico_images(path):
    """The raw bytes of every image inside an .ico file."""
    data = path.read_bytes()
    _reserved, kind, count = struct.unpack_from("<HHH", data, 0)
    if kind != 1:
        raise ValueError(f"{path} is not an icon file")
    images = []
    for index in range(count):
        size, offset = struct.unpack_from("<II", data, 6 + index * 16 + 8)
        images.append(data[offset:offset + size])
    return images


def check_brand(blender, platform):
    binary = blender.read_bytes()
    splash = (BRAND / "release" / "datafiles" / "splash.png").read_bytes()
    check(splash in binary, f"our splash.png ({len(splash)} bytes) is compiled into {blender.name}")
    if platform == "windows":
        images = ico_images(BRAND / "release" / "windows" / "icons" / "winblender.ico")
        missing = [len(image) for image in images if image not in binary]
        check(not missing, f"all {len(images)} images of our winblender.ico are in {blender.name}"
              + (f" (missing sizes {missing})" if missing else ""))
    if platform == "macos":
        ours = BRAND / "release" / "darwin" / "Blender.app" / "Contents" / "Resources" / "blender_icon_legacy.icns"
        shipped = blender.parents[1] / "Resources" / "blender_icon_legacy.icns"
        same = shipped.is_file() and hashlib.sha256(shipped.read_bytes()).digest() == hashlib.sha256(ours.read_bytes()).digest()
        check(same, f"the bundle's {shipped.name} is ours")


def check_agent(agent):
    if not check(agent.is_file(), f"agent binary at {agent}"):
        return
    sys.path.insert(0, str(ADDON))
    import protocol  # the add-on's own encoder and decoder

    request = protocol.encode_request("0" * 32, "health", {})
    env = dict(os.environ)
    env.pop("REDROB_API_KEY", None)
    try:
        completed = subprocess.run(
            [str(agent)], input=request, capture_output=True, timeout=60, env=env
        )
    except subprocess.TimeoutExpired:
        check(False, "agent answered health within 60 s")
        return
    lines = [line for line in completed.stdout.splitlines() if line.strip()]
    if not check(bool(lines), "agent wrote a reply to health"):
        print(completed.stderr.decode("utf-8", "replace")[-2000:])
        return
    try:
        event = protocol.decode_event(lines[0])
    except protocol.ProtocolError as exc:
        check(False, f"agent reply decodes with the add-on's protocol ({exc})")
        return
    result = event.get("result", {})
    check(event.get("event") == "success" and result.get("service") == "redrob-shape-agent",
          f"agent health: service={result.get('service')!r}")


PROBE = r"""
import json, sys
import addon_utils
import bpy
enabled = "redrob_shape" in bpy.context.preferences.addons
loaded = addon_utils.check("redrob_shape")
panels = sorted(name for name in dir(bpy.types) if name.startswith("REDROB_PT_"))
print("REDROB_PROBE " + json.dumps({"enabled": enabled, "loaded": list(loaded), "panels": panels,
                                     "version": bpy.app.version_string}), flush=True)
"""


def check_blender(blender):
    if not check(blender.is_file(), f"blender executable at {blender}"):
        return
    with tempfile.TemporaryDirectory() as tmp:
        probe = Path(tmp) / "probe.py"
        probe.write_text(PROBE, encoding="utf-8")
        env = dict(os.environ)
        # A fresh config directory, so nothing on the runner decides what is enabled.
        env["BLENDER_USER_RESOURCES"] = tmp
        try:
            completed = subprocess.run(
                [str(blender), "--background", "--factory-startup", "--python-exit-code", "1",
                 "--python", str(probe)],
                capture_output=True, timeout=300, env=env,
            )
        except subprocess.TimeoutExpired:
            check(False, "Blender ran the probe within 300 s")
            return
    output = completed.stdout.decode("utf-8", "replace")
    found = [line for line in output.splitlines() if line.startswith("REDROB_PROBE ")]
    if not check(completed.returncode == 0 and bool(found), f"Blender started in the background (exit {completed.returncode})"):
        print(output[-3000:])
        print(completed.stderr.decode("utf-8", "replace")[-3000:])
        return
    probe = json.loads(found[0].split(" ", 1)[1])
    print(f"      Blender {probe['version']}, panels {', '.join(probe['panels'])}")
    # A hidden core add-on (patch 0001) is loaded without an entry in preferences.addons, so the
    # loaded flag of addon_utils.check() is the one that says the user gets it.
    check(probe["loaded"][1], f"the redrob_shape add-on is loaded with factory settings {probe['loaded']}")
    check("REDROB_PT_workspace" in probe["panels"], "the Redrob Shape panel is registered")


WINDOW_PROBE = r"""
import os
import bpy
def report():
    window = bpy.context.window_manager.windows[0]
    print(f"REDROB_WINDOW {window.width}x{window.height}", flush=True)
    shot = os.environ.get("REDROB_WINDOW_SHOT")
    if shot:
        # Blender's own framebuffer, so the picture is the window that opened, on any platform.
        with bpy.context.temp_override(window=window, screen=window.screen):
            bpy.ops.screen.screenshot(filepath=shot)
        print(f"REDROB_SHOT {shot}", flush=True)
    bpy.ops.wm.quit_blender()
bpy.app.timers.register(report, first_interval=8.0)
"""


def check_window(blender):
    """Start the real app WITH its window. A timer prints a marker once the window exists; a
    build that cannot open a window (no GPU backend, a crash in drawing) never reaches it."""
    with tempfile.TemporaryDirectory() as tmp:
        probe = Path(tmp) / "window_probe.py"
        probe.write_text(WINDOW_PROBE, encoding="utf-8")
        env = dict(os.environ)
        env["BLENDER_USER_RESOURCES"] = tmp
        try:
            completed = subprocess.run(
                [str(blender), "--factory-startup", "--python", str(probe)],
                capture_output=True, timeout=240, env=env,
            )
        except subprocess.TimeoutExpired as exc:
            output = (exc.stdout or b"").decode("utf-8", "replace")
            check(False, "Blender opened a window and quit within 240 s")
            print(output[-3000:])
            return
    output = completed.stdout.decode("utf-8", "replace")
    found = [line for line in output.splitlines() if line.startswith("REDROB_WINDOW ")]
    if not check(bool(found), f"Blender opened a window ({found[0].split(' ', 1)[1] if found else 'no marker'})"):
        print(output[-3000:])
        print(completed.stderr.decode("utf-8", "replace")[-3000:])


def main():
    args = sys.argv[1:]
    window = "--window" in args
    args = [arg for arg in args if arg != "--window"]
    if len(args) != 1:
        print(__doc__)
        return 2
    install = Path(args[0]).resolve()
    blender, agent, platform = layout(install)
    print(f"checking the {platform} install at {install}")
    if blender.is_file():
        check_brand(blender, platform)
    check_agent(agent)
    check_blender(blender)
    if window and blender.is_file():
        check_window(blender)
    if failures:
        print(f"\n{len(failures)} check(s) failed")
        return 1
    print("\nall checks passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
