#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Redrob Labs
#
# SPDX-License-Identifier: GPL-3.0-or-later

"""Refuse a release asset whose name Redrob Launcher cannot read.

    scripts/check-asset-names.py [--version 0.1.1] FILE-OR-NAME...
    scripts/check-asset-names.py [--version 0.1.1] --release-dir DIR
    scripts/check-asset-names.py --self-test

Redrob Launcher installs an asset only when its name is

    redrob-shape-<version>-<os>-<arch>[-portable].<ext>

with the os-arch token and the extension from the lists below (its parser: redrob-launcher,
crates/core/src/platform.rs, parse_asset_name and Platform::tokens). The first release carried
redrob-shape-linux-x64-0.1.0.tar.xz, which the launcher skips three times over: the version is in
the wrong place, "x64" is not a Linux token, and it installs no .tar.xz. Nothing in the pipeline
noticed, because a name is a string nobody runs. This check is that run: sign-and-publish.yml
calls it over every artefact it signs or collects, and again over the whole set before the CDN
upload and the release.

Checksum sidecars (*.sha256, SHA256SUMS.txt) are not installable and are accepted as such.

--release-dir checks a whole directory that is to become the release, and that directory must
also carry redrob-launcher.json, the manifest Redrob Launcher reads to learn that this release
uses its shared account (redrob-launcher, docs/release-manifest.md). Its content is checked too:
JSON, "schema": 1, "sharedAccount": true. The per-file calls do not require it, since the manifest
is written by the organization's shared publish job (redrob-labs/.github, release-publish.yml)
after they ran.
"""

import argparse
import json
import re
import sys
import unittest
from pathlib import Path

STEM = "redrob-shape"

# Platform::tokens, every token the launcher looks for on some machine.
TOKENS = (
    "linux-x86_64",
    "linux-aarch64",
    "linux-riscv64",
    "macos-universal",
    "macos-aarch64",
    "macos-arm64",
    "macos-x86_64",
    "windows-x64",
    "windows-arm64",
    "windows-x86",
    "freebsd-x86_64",
)

# PackageKind::suffix, the formats the launcher installs, per OS.
EXTENSIONS = {
    "windows": (".msi", "-portable.zip"),
    "macos": (".dmg",),
    "linux": (".AppImage", ".deb", ".rpm", ".flatpak", ".tar.gz"),
    "freebsd": (".tar.gz",),
}

SIDECARS = (".sha256",)
# The launcher's release manifest: one asset beside the installers, not installable itself.
MANIFEST_NAME = "redrob-launcher.json"
MANIFEST_SCHEMA = 1
SIDECAR_NAMES = ("SHA256SUMS.txt", MANIFEST_NAME)

# A version like 0.1.1 or 0.1.1-rc.1, no leading v.
VERSION = re.compile(r"^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$")


def parse(name):
    """(version, token, extension) for an installable name, or None."""
    for os_, exts in EXTENSIONS.items():
        for ext in exts:
            if not name.endswith(ext):
                continue
            base = name[: -len(ext)]
            if not base.startswith(STEM + "-"):
                return None
            rest = base[len(STEM) + 1 :]
            for token in TOKENS:
                if not token.startswith(os_ + "-"):
                    continue
                if rest.endswith("-" + token):
                    version = rest[: -len(token) - 1]
                    if VERSION.match(version):
                        return version, token, ext
            return None
    return None


def is_sidecar(name):
    return name in SIDECAR_NAMES or name.endswith(SIDECARS)


def manifest_problems(text):
    """One line per way the manifest's content is not what the launcher reads."""
    try:
        data = json.loads(text)
    except ValueError as exc:
        return [f"{MANIFEST_NAME}: not JSON ({exc})"]
    if not isinstance(data, dict):
        return [f"{MANIFEST_NAME}: not a JSON object"]
    out = []
    if data.get("schema") != MANIFEST_SCHEMA or isinstance(data.get("schema"), bool):
        out.append(f"{MANIFEST_NAME}: \"schema\" is {data.get('schema')!r}, expected {MANIFEST_SCHEMA}")
    if data.get("sharedAccount") is not True:
        out.append(f"{MANIFEST_NAME}: \"sharedAccount\" is {data.get('sharedAccount')!r}, expected true")
    return out


def release_dir_problems(directory, version=None):
    """problems() over every file in the directory, plus the manifest and its content."""
    directory = Path(directory)
    files = sorted(p for p in directory.iterdir() if p.is_file())
    out = problems([p.name for p in files], version)
    manifest = directory / MANIFEST_NAME
    if not manifest.is_file():
        out.append(f"{MANIFEST_NAME}: missing from {directory}")
    else:
        out.extend(manifest_problems(manifest.read_text(encoding="utf-8")))
    return out


def problems(names, version=None):
    """One line per name the launcher would skip (or that carries the wrong version)."""
    out = []
    installable = 0
    for name in names:
        if is_sidecar(name):
            continue
        parsed = parse(name)
        if parsed is None:
            out.append(f"{name}: not {STEM}-<version>-<os>-<arch>[-portable].<ext> with a token and extension the launcher installs")
            continue
        installable += 1
        if version is not None and parsed[0] != version:
            out.append(f"{name}: carries version {parsed[0]}, expected {version}")
    if installable == 0:
        out.append("no installable asset among the names given")
    return out


class Tests(unittest.TestCase):
    def test_the_names_this_pipeline_publishes(self):
        for name, want in [
            ("redrob-shape-0.1.1-linux-x86_64.tar.gz", ("0.1.1", "linux-x86_64", ".tar.gz")),
            ("redrob-shape-0.1.1-macos-aarch64.dmg", ("0.1.1", "macos-aarch64", ".dmg")),
            ("redrob-shape-0.1.1-windows-x64-portable.zip", ("0.1.1", "windows-x64", "-portable.zip")),
            ("redrob-shape-0.2.0-rc.1-windows-x64.msi", ("0.2.0-rc.1", "windows-x64", ".msi")),
        ]:
            self.assertEqual(parse(name), want, name)

    def test_the_first_release_names_are_refused(self):
        # Each of the 0.1.0 names, and each of its three mistakes on its own.
        for name in [
            "redrob-shape-linux-x64-0.1.0.tar.xz",
            "redrob-shape-macos-arm64-0.1.0.zip",
            "redrob-shape-windows-x64-0.1.0.zip",
            "redrob-shape-linux-x64-0.1.0.tar.gz",  # version after the token
            "redrob-shape-0.1.0-linux-x64.tar.gz",  # x64 is not a Linux token
            "redrob-shape-0.1.0-linux-x86_64.tar.xz",  # no .tar.xz
            "redrob-shape-0.1.0-macos-aarch64.zip",  # a zip is not installed on macOS
            "redrob-shape-0.1.0-windows-x64.zip",  # only -portable.zip
            "redrob-shape-v0.1.0-linux-x86_64.tar.gz",  # no leading v
            "redrob-shape-0.1.0-linux-x86_64-portable.zip",  # not a Linux format
            "redrob-shape-0.1.0-macos-aarch64-unsigned.zip",
            "redrob-cad-0.1.0-linux-x86_64.tar.gz",  # another stem
        ]:
            self.assertIsNone(parse(name), name)

    def test_problems_report_each_bad_name_and_the_version(self):
        names = [
            "redrob-shape-0.1.1-linux-x86_64.tar.gz",
            "redrob-shape-0.1.1-linux-x86_64.tar.gz.sha256",
            "SHA256SUMS.txt",
            "redrob-shape-macos-arm64-unsigned.zip",
        ]
        self.assertEqual(problems(names[:3], "0.1.1"), [])
        found = problems(names, "0.1.1")
        self.assertEqual(len(found), 1)
        self.assertIn("macos-arm64-unsigned.zip", found[0])
        self.assertEqual(problems(names[:3], "0.1.2"), ["redrob-shape-0.1.1-linux-x86_64.tar.gz: carries version 0.1.1, expected 0.1.2"])
        self.assertEqual(problems(["SHA256SUMS.txt"]), ["no installable asset among the names given"])
        self.assertEqual(problems(names[:3] + [MANIFEST_NAME], "0.1.1"), [])

    def test_the_manifest_content_the_launcher_reads(self):
        # The exact bytes the publish job writes (docs/release-manifest.md in redrob-launcher).
        self.assertEqual(manifest_problems('{ "schema": 1, "sharedAccount": true }\n'), [])
        self.assertEqual(manifest_problems('{"sharedAccount": true, "schema": 1, "later": "ignored"}'), [])
        for text, fragment in [
            ("", "not JSON"),
            ("{ schema: 1 }", "not JSON"),
            ("[1]", "not a JSON object"),
            ('{ "schema": 2, "sharedAccount": true }', '"schema" is 2'),
            ('{ "schema": "1", "sharedAccount": true }', '"schema" is \'1\''),
            ('{ "schema": true, "sharedAccount": true }', '"schema" is True'),
            ('{ "sharedAccount": true }', '"schema" is None'),
            ('{ "schema": 1, "sharedAccount": false }', '"sharedAccount" is False'),
            ('{ "schema": 1, "sharedAccount": "true" }', "\"sharedAccount\" is 'true'"),
            ('{ "schema": 1 }', '"sharedAccount" is None'),
        ]:
            found = manifest_problems(text)
            self.assertTrue(found, text)
            self.assertIn(fragment, found[0], text)
        self.assertEqual(len(manifest_problems("{}")), 2)

    def test_a_release_directory_without_the_manifest_is_refused(self):
        import tempfile

        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / "redrob-shape-0.1.1-linux-x86_64.tar.gz").write_bytes(b"")
            (d / "redrob-shape-0.1.1-macos-aarch64.dmg").write_bytes(b"")
            self.assertEqual(release_dir_problems(d, "0.1.1"), [f"{MANIFEST_NAME}: missing from {d}"])
            (d / MANIFEST_NAME).write_text('{ "schema": 1, "sharedAccount": false }\n')
            self.assertEqual(release_dir_problems(d, "0.1.1"), [f'{MANIFEST_NAME}: "sharedAccount" is False, expected true'])
            (d / MANIFEST_NAME).write_text('{ "schema": 1, "sharedAccount": true }\n')
            self.assertEqual(release_dir_problems(d, "0.1.1"), [])
            # The manifest alone is not a release.
            (d / "redrob-shape-0.1.1-linux-x86_64.tar.gz").unlink()
            (d / "redrob-shape-0.1.1-macos-aarch64.dmg").unlink()
            self.assertEqual(release_dir_problems(d, "0.1.1"), ["no installable asset among the names given"])


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--version", help="every installable asset has to carry this version")
    ap.add_argument("--self-test", action="store_true")
    ap.add_argument("--release-dir", metavar="DIR",
                    help=f"check every file in DIR as the whole release; DIR must carry {MANIFEST_NAME}")
    ap.add_argument("names", nargs="*", help="asset file paths or bare names")
    args = ap.parse_args(argv)
    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(Tests)
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1
    if args.release_dir:
        if args.names:
            ap.error("--release-dir takes no asset names")
        if not Path(args.release_dir).is_dir():
            ap.error(f"{args.release_dir} is not a directory")
        names = sorted(p.name for p in Path(args.release_dir).iterdir() if p.is_file())
        found = release_dir_problems(args.release_dir, args.version)
    else:
        if not args.names:
            ap.error("give asset names, --release-dir DIR, or --self-test")
        names = [Path(n).name for n in args.names]
        found = problems(names, args.version)
    for line in found:
        print(f"error: {line}", file=sys.stderr)
    if not found:
        print(f"{sum(1 for n in names if not is_sidecar(n))} asset name(s) the launcher can read"
              + (f", {MANIFEST_NAME} present and valid" if args.release_dir else ""))
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
