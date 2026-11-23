#!/usr/bin/env python3
"""Oppdater README-versjonsloggen fra den engelske CHANGELOG.md som appen også viser."""

from pathlib import Path
import re

root = Path(__file__).resolve().parent.parent
readme = root / "README.md"
start = "<!-- BEGIN VERSION HISTORY -->"
end = "<!-- END VERSION HISTORY -->"
text = readme.read_text(encoding="utf-8")
if text.count(start) != 1 or text.count(end) != 1:
    raise SystemExit("README må ha nøyaktig ett sett med versjonsloggmarkører")
before, rest = text.split(start)
_, after = rest.split(end)
history = (root / "CHANGELOG.md").read_text(encoding="utf-8")
# Dokumenttittelen står allerede i README, så bare versjonene tas med.
history = re.sub(r"\A# [^\n]*\n", "", history).strip()
history = re.sub(r"^(#{2,5}) ", r"\1# ", history, flags=re.MULTILINE)
readme.write_text(f"{before}{start}\n\n{history}\n\n{end}{after}", encoding="utf-8")
