#!/usr/bin/env python3
"""Rewrites Blender's product name to ours inside user-visible strings in the fetched checkout.

Narrow by necessity. UPSTREAM.md commits this repository to Blender keeping "its names, file
compatibility, functionality, copyright, and licenses", and in Blender the name is load-bearing in
more places than usual: RNA identifiers, operator idnames, `bpy` module paths, the .blend format's
own strings, and the `BLENDER_*` build and version macros. Renaming any of those breaks scripts,
add-ons and files.

So only strings a user READS are rewritten, found by where they sit, never by a blind search:

  C/C++ (source/)
    N_("...") IFACE_("...") TIP_("...") DATA_ RPT_ and their CTX_ forms -- the translation macros
    ot->name = "..." / ot->description = "..." -- operator names and tooltips
    BKE_report / WM_report and their f-forms -- messages shown in the status bar
  RNA (source/blender/makesrna/intern/) -- every literal: property, struct and enum names and
    tooltips. Identifiers there are never the bare word "Blender", which is never rewritten.
  Python UI (scripts/startup/)
    text= label= name= description= heading= title= keyword arguments, bl_label / bl_description
    and label / text / name / description assignments, operator class docstrings (their tooltip),
    and the arguments of iface_ / tip_ / rpt_ / pgettext* / report.
  Translations (locale/po/*.po)
    A string we rewrote in the sources gets the same new msgid, so its translation is not lost.
    Without this a Korean user sees English for every renamed string. msgstr follows: the Latin
    name everywhere, and Korean's own spellings (Blender, 블렌더) become 레드롭 셰이프.

Holdbacks, because renaming them would make the string false:

  * URLs -- blender.org links stay valid links.
  * The bare word "Blender" -- in Python and RNA that is an identifier (the keymap, an engine).
  * Phrases that name the real Blender: its files, Foundation, manual, community, Studio, ID,
    Extensions platform and website, its version numbers ("Blender 2.8", "before Blender 5.0"),
    and the Blender unit. Our builds read Blender files and keep Blender's units and history.

Usage: sweep-strings.py <checkout-root>            rewrite in place
       sweep-strings.py <checkout-root> --check    fail if anything is left to rewrite
"""

from __future__ import annotations

import argparse
import ast
import re
import sys
from pathlib import Path

PRODUCT = "Redrob Shape"
PRODUCT_KO = "레드롭 셰이프"

# Whole word only, and never the build macros or the Python module. The edges are ASCII on purpose:
# Python's \b treats Korean and Japanese letters as word characters, so "Blender를" or "Blenderを"
# would have no word boundary and a translation would keep the old name.
NAME = re.compile(r"(?<![A-Za-z0-9_])Blender(?![A-Za-z0-9_])")

MACROS = re.compile(
    r'\b(?:N_|IFACE_|TIP_|DATA_|RPT_|CTX_N_|CTX_IFACE_|CTX_TIP_|CTX_DATA_)\('
    r'\s*(?:[A-Za-z_][A-Za-z0-9_]*\s*,\s*)?'
    r'"((?:[^"\\]|\\.)*)"',
)
# The CTX_ variants take a translation CONTEXT first; the optional group above consumes it.

OPERATOR_FIELDS = re.compile(r'\bot->(?:name|description)\s*=\s*"((?:[^"\\]|\\.)*)"')
REPORTS = re.compile(
    r'\b(?:BKE_reportf?|WM_reportf?|WM_global_reportf?)\s*\([^;"]*?"((?:[^"\\]|\\.)*)"'
)
LITERAL = re.compile(r'"((?:[^"\\]|\\.)*)"')

URL = re.compile(r"https?://\S*")

KEEP = re.compile(
    r"Blender (?:Foundation|[Ff]ile|manual|Manual|community|Studio|studio|ID|Extensions|Website|"
    r"Store|[Uu]nits?|space unit|[Oo]riginal|RNA|render|binary|\d|\{|%)"
    r"|Blender's (?:official|source)"
    r"|Donate to Blender"
    r"|Filter Blender"
    r"|(?:newer|future|older|Big Endian) versions? of Blender"
    r"|[Vv]ersion of Blender the"
    r"|(?:removed from|written for|[Bb]efore|[Ss]ince|[Pp]rior to) Blender"
    r"|\.blend\b"
)
KEEP_KO = re.compile(r"(?:Blender|블렌더) ?(?:파일|재단|단위|\d)|\.blend\b")


def rewrite(text: str, product: str = PRODUCT, keep: re.Pattern = KEEP, name=NAME) -> str:
    if text.strip() == "Blender":
        return text
    stash: list[str] = []

    def hide(m: re.Match) -> str:
        stash.append(m.group(0))
        return f"\x00{len(stash) - 1}\x00"

    body = URL.sub(hide, text)
    body = keep.sub(hide, body)
    body = name.sub(product, body)
    return re.sub(r"\x00(\d+)\x00", lambda m: stash[int(m.group(1))], body)


NAME_KO = re.compile(r"(?<![A-Za-z0-9_])Blender(?![A-Za-z0-9_])|블렌더")


def rewrite_ko(text: str) -> str:
    return rewrite(text, PRODUCT_KO, KEEP_KO, NAME_KO)


# --- C / C++ -------------------------------------------------------------------------


def sweep_c(text: str, everything: bool) -> tuple[str, int]:
    changes = 0

    def on_literal(match: re.Match) -> str:
        nonlocal changes
        original = match.group(1)
        rewritten = rewrite(original)
        if rewritten == original:
            return match.group(0)
        changes += 1
        start, end = match.span(1)
        whole = match.group(0)
        offset = match.start(0)
        return whole[: start - offset] + rewritten + whole[end - offset :]

    if everything:
        return LITERAL.sub(on_literal, text), changes
    for pattern in (MACROS, OPERATOR_FIELDS, REPORTS):
        text = pattern.sub(on_literal, text)
    return text, changes


# --- Python UI -----------------------------------------------------------------------

UI_KEYWORDS = {"text", "label", "name", "description", "heading", "title", "message"}
UI_ASSIGN = {"bl_label", "bl_description", "label", "text", "name", "description"}
UI_CALLS = {
    "iface_", "tip_", "rpt_", "pgettext", "pgettext_iface", "pgettext_tip", "pgettext_rpt",
    "pgettext_n", "report",
}


def _callee(node: ast.Call) -> str:
    func = node.func
    if isinstance(func, ast.Name):
        return func.id
    if isinstance(func, ast.Attribute):
        return func.attr
    return ""


def _ui_strings(tree: ast.AST) -> list[ast.Constant]:
    found: list[ast.Constant] = []

    def take(node) -> None:
        if isinstance(node, ast.Constant) and isinstance(node.value, str):
            found.append(node)

    for node in ast.walk(tree):
        if isinstance(node, ast.keyword) and node.arg in UI_KEYWORDS:
            take(node.value)
        elif isinstance(node, (ast.Assign, ast.AnnAssign)):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            for t in targets:
                tname = t.id if isinstance(t, ast.Name) else getattr(t, "attr", None)
                if tname in UI_ASSIGN and node.value is not None:
                    take(node.value)
        elif isinstance(node, ast.Call) and _callee(node) in UI_CALLS:
            for arg in node.args:
                take(arg)
        elif isinstance(node, ast.ClassDef) and node.body:
            first = node.body[0]
            if isinstance(first, ast.Expr):
                take(first.value)
    return found


def sweep_py(text: str) -> tuple[str, int]:
    try:
        tree = ast.parse(text)
    except SyntaxError:
        return text, 0
    lines = text.splitlines(keepends=True)
    starts = [0]
    for line in lines:
        starts.append(starts[-1] + len(line))

    def offset(lineno: int, col: int) -> int:
        # ast columns are UTF-8 byte offsets; convert within the line.
        line = lines[lineno - 1]
        return starts[lineno - 1] + len(line.encode("utf-8")[:col].decode("utf-8", "replace"))

    spans = []
    for node in _ui_strings(tree):
        if "Blender" not in node.value or node.value.strip() == "Blender":
            continue
        a = offset(node.lineno, node.col_offset)
        b = offset(node.end_lineno, node.end_col_offset)
        source = text[a:b]
        new = rewrite(source)
        if new != source:
            spans.append((a, b, new))
    for a, b, new in sorted(set(spans), reverse=True):
        text = text[:a] + new + text[b:]
    return text, len(set(spans))


# --- translations --------------------------------------------------------------------


def _unescape(s: str) -> str:
    return re.sub(r"\\(.)", lambda m: {"n": "\n", "t": "\t"}.get(m.group(1), m.group(1)), s)


def _escape(s: str) -> str:
    return s.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\t", "\\t")


PO_FIELD = re.compile(r'^(msgctxt|msgid|msgid_plural|msgstr(?:\[\d+\])?) "(.*)"\s*$')
PO_CONT = re.compile(r'^"(.*)"\s*$')


def _po_entries(lines: list[str]):
    """Yields (field, start, end, value) for each field; end is exclusive."""
    i = 0
    while i < len(lines):
        m = PO_FIELD.match(lines[i])
        if not m:
            i += 1
            continue
        field, parts, j = m.group(1), [m.group(2)], i + 1
        while j < len(lines) and (c := PO_CONT.match(lines[j])):
            parts.append(c.group(1))
            j += 1
        yield field, i, j, _unescape("".join(parts))
        i = j


def sweep_po(text: str, renamed: set[str], korean: bool) -> tuple[str, int]:
    lines = text.splitlines(keepends=True)
    out: list[str] = []
    changes = 0
    pending = False  # the msgid of the current entry was rewritten
    last = 0
    for field, a, b, value in _po_entries(lines):
        out.extend(lines[last:a])
        last = b
        new = value
        if field == "msgctxt":
            pending = False
        elif field in ("msgid", "msgid_plural"):
            if field == "msgid":
                pending = False
            candidate = rewrite(value)
            if candidate != value and candidate in renamed:
                new, pending = candidate, True
        elif field.startswith("msgstr") and pending:
            new = rewrite_ko(value) if korean else rewrite(value)
        if new == value:
            out.extend(lines[a:b])
            continue
        changes += 1
        out.append(f'{field} "{_escape(new)}"\n')
    out.extend(lines[last:])
    return "".join(out), changes


# --- driver --------------------------------------------------------------------------


def _literal_values(text: str, python: bool) -> set[str]:
    """Every string a user could see that names the product -- the new msgids po must carry."""
    values: set[str] = set()
    if python:
        try:
            tree = ast.parse(text)
        except SyntaxError:
            return values
        for node in _ui_strings(tree):
            if PRODUCT in node.value:
                values.add(node.value)
        return values
    # Adjacent C literals concatenate, and the msgid is the concatenation.
    for run in re.finditer(r'"(?:[^"\\]|\\.)*"(?:\s*"(?:[^"\\]|\\.)*")*', text):
        joined = _unescape("".join(LITERAL.findall(run.group(0))))
        if PRODUCT in joined:
            values.add(joined)
    return values


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("root", type=Path)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()

    root = args.root
    source = root / "source"
    if not source.is_dir():
        sys.exit(f"error: {source} not found -- is {root} a fetched checkout?")
    rna = source / "blender" / "makesrna" / "intern"

    counts = {"c": [0, 0], "py": [0, 0], "po": [0, 0]}
    renamed: set[str] = set()

    def handle(path: Path, kind: str, rewritten: str, changes: int) -> None:
        if changes:
            counts[kind][0] += 1
            counts[kind][1] += changes
            if not args.check:
                path.write_text(rewritten, encoding="utf-8", errors="surrogateescape")

    for path in sorted(source.rglob("*")):
        if path.suffix not in {".c", ".cc", ".hh", ".h"}:
            continue
        rel = path.relative_to(source).parts
        if "tests" in rel[-2:] or "test" in rel[-2:] or path.stem.endswith("_test"):
            continue
        text = path.read_text(encoding="utf-8", errors="surrogateescape")
        new, n = sweep_c(text, everything=path.parent == rna)
        handle(path, "c", new, n)
        renamed |= _literal_values(new, python=False)

    for path in sorted((root / "scripts" / "startup").rglob("*.py")):
        text = path.read_text(encoding="utf-8")
        new, n = sweep_py(text)
        handle(path, "py", new, n)
        renamed |= _literal_values(new, python=True)

    po_dir = root / "locale" / "po"
    if po_dir.is_dir():
        for path in sorted(po_dir.glob("*.po")):
            text = path.read_text(encoding="utf-8", errors="surrogateescape")
            new, n = sweep_po(text, renamed, korean=path.stem == "ko")
            handle(path, "po", new, n)

    verb = "would rewrite" if args.check else "rewrote"
    for kind, label in (("c", "C/C++"), ("py", "Python UI"), ("po", "translation")):
        files, total = counts[kind]
        print(f"string sweep {verb} {total} {label} string(s) in {files} file(s) -> {PRODUCT!r}")

    if args.check and any(t for _, t in counts.values()):
        print("error: user-visible strings still name Blender; run scripts/sweep-strings.py "
              "<checkout> to rewrite them", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
