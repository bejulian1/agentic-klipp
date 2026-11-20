#!/usr/bin/env python3
"""Extract Klipp's Rust messages and update the gettext template and catalogs."""

import argparse
import ast
import json
from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
PO = ROOT / "po"
SOURCES = ("src", "crates/klipp-annotate/src", "crates/klipp-system/src")
CALLS = {"tr": 1, "trc": 2, "trn": 2, "tr_fmt": 1}

# Recognize non-code tokens first, so examples in comments and strings are ignored.
RUST = re.compile(
    r'(?P<comment>//[^\n]*|/\*)'
    r'|(?P<raw>(?:br|cr|r)(?P<hashes>\#*)".*?"(?P=hashes))'
    r'|(?P<string>(?:b|c)?"(?:[^"\\]|\\.)*")'
    r"|(?:b)?'(?:[^'\\\n]|\\(?:u\{[^}]+\}|x[0-9a-fA-F]{2}|.))'"
    r'|(?P<code>\w+|[^\s])',
    re.S,
)
ESCAPE = re.compile(r'\\(?:\n\s*|u\{([0-9a-fA-F_]+)\}|x([0-9a-fA-F]{2})|(.))', re.S)
PLACEHOLDER = re.compile(r'\{[a-zA-Z_][a-zA-Z0-9_]*\}')
ENTRY_FIELD = re.compile(r'^(msgctxt|msgid(?:_plural)?|msgstr(?:\[\d+\])?)\s+(.*)$')


def rust_string(token):
    if token.startswith("r"):
        start = token.index('"')
        return token[start + 1:len(token) - start]
    if not token.startswith('"'):
        return None

    def unescape(match):
        unicode, byte, escaped = match.groups()
        if unicode or byte:
            return chr(int((unicode or byte).replace("_", ""), 16))
        return {"n": "\n", "r": "\r", "t": "\t", "0": "\0", "\\": "\\",
                '"': '"', "'": "'", None: ""}[escaped]

    return ESCAPE.sub(unescape, token[1:-1])


def rust_tokens(source):
    line, end = 1, 0
    while match := RUST.search(source, end):
        line += source.count("\n", end, match.start())
        token, end = match.group(), match.end()
        if token == "/*":
            depth = 1
            for marker in re.finditer(r'/\*|\*/', source[end:]):
                depth += 1 if marker.group() == "/*" else -1
                if not depth:
                    end += marker.end()
                    break
            token = source[match.start():end]
        yield token, line, match.lastgroup == "comment"
        line += token.count("\n")


def extract():
    messages = {}
    for directory in SOURCES:
        for path in sorted((ROOT / directory).rglob("*.rs")):
            source = path.read_text(encoding="utf-8")
            tokens, comments = [], {}
            for token, line, comment in rust_tokens(source):
                if comment:
                    if token.startswith("// Oversettere:"):
                        comments[line] = token.removeprefix("// Oversettere:").strip()
                else:
                    tokens.append((token, line))

            for i, (name, line) in enumerate(tokens):
                if name not in CALLS or i + 1 >= len(tokens) or tokens[i + 1][0] != "(":
                    continue
                args = []
                for arg, separator in zip(tokens[i + 2:i + 2 + 2 * CALLS[name]:2],
                                          tokens[i + 3:i + 2 + 2 * CALLS[name]:2]):
                    literal = rust_string(arg[0]) if arg[0].startswith(('"', 'r"', 'r#')) else None
                    if literal is None or separator[0] not in (",", ")"):
                        break
                    args.append(literal)
                if len(args) != CALLS[name]:
                    continue
                context, msgid = args if name == "trc" else ("", args[0])
                plural = args[1] if name == "trn" else None
                entry = messages.setdefault((context, msgid), {
                    "plural": plural, "refs": [], "notes": set()
                })
                ref = f"{path.relative_to(ROOT).as_posix()}:{line}"
                if entry["plural"] != plural:
                    raise ValueError(f"{ref}: conflicting plural for {msgid!r}")
                if ref not in entry["refs"]:
                    entry["refs"].append(ref)
                note_line = line - 1
                while note_line in comments:
                    entry["notes"].add(comments[note_line])
                    note_line -= 1
                if placeholders := PLACEHOLDER.findall(msgid + (plural or "")):
                    names = ", ".join(sorted(set(placeholders)))
                    entry["notes"].add(f"Keep these placeholders where they occur: {names}.")
    return messages


def read_catalog(path):
    """Read existing translations and translator-owned comments and flags."""
    entries = {}
    fields, comments = {}, []
    current = None
    for line in [*path.read_text(encoding="utf-8").splitlines(), ""]:
        line = line.strip()
        if "msgid" in fields and (not line or line.startswith(("#", "msgctxt ", "msgid "))):
            entries[(fields.get("msgctxt", ""), fields["msgid"])] = fields, comments
            fields, comments, current = {}, [], None
        if line.startswith("#") and (line == "#" or line.startswith(("# ", "#,", "#|"))):
            comments.append(line)
        elif match := ENTRY_FIELD.match(line):
            current = match[1]
            fields[current] = ast.literal_eval(match[2])
        elif line.startswith('"') and current:
            fields[current] += ast.literal_eval(line)
    return entries


def quote(s):
    return json.dumps(s, ensure_ascii=False)


def field(name, value):
    if "\n" not in value:
        return [f"{name} {quote(value)}"]
    return [f'{name} ""', *(quote(line) for line in value.splitlines(keepends=True))]


def header(language, old):
    headers = {
        "Project-Id-Version": "Agentic Klipp",
        "MIME-Version": "1.0",
        "Content-Type": "text/plain; charset=UTF-8",
        "Content-Transfer-Encoding": "8bit",
    }
    headers.update(line.split(": ", 1) for line in old.splitlines() if ": " in line)
    if language:
        condition = "n > 1" if language in ("fr", "pt_BR") else "n != 1"
        headers["Language"] = language
        headers["Plural-Forms"] = f"nplurals=2; plural=({condition});"
    value = "".join(f"{key}: {value}\n" for key, value in headers.items())
    return ["msgid \"\"", *field("msgstr", value)]


def render(messages, language="", existing=None):
    existing = existing or {}
    header_fields, header_comments = existing.get(("", ""), ({}, []))
    lines = [*header_comments, *header(language, header_fields.get("msgstr", ""))]
    for (context, msgid), entry in messages.items():
        old, comments = existing.get((context, msgid), ({}, []))
        lines.extend(["", *comments])
        lines.extend(f"#. {note}" for note in sorted(entry["notes"]))
        lines.extend(f"#: {ref}" for ref in entry["refs"])
        if context:
            lines.extend(field("msgctxt", context))
        lines.extend(field("msgid", msgid))
        if entry["plural"] is None:
            lines.extend(field("msgstr", old.get("msgstr", old.get("msgstr[0]", ""))))
        else:
            lines.extend(field("msgid_plural", entry["plural"]))
            lines.extend(field("msgstr[0]", old.get("msgstr[0]", old.get("msgstr", ""))))
            lines.extend(field("msgstr[1]", old.get("msgstr[1]", "")))
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="check generated files without editing")
    args = parser.parse_args()
    messages = extract()
    files = {PO / "klipp.pot": render(messages)}
    for path in sorted(PO.glob("*.po")):
        files[path] = render(messages, path.stem, read_catalog(path))
    stale = []
    for path, content in files.items():
        if not path.exists() or path.read_text(encoding="utf-8") != content:
            if args.check:
                stale.append(path.name)
            else:
                path.write_text(content, encoding="utf-8")
    if stale:
        sys.exit(f"Outdated catalogs ({', '.join(stale)}). Run python3 -I scripts/update-po.py")


if __name__ == "__main__":
    main()
