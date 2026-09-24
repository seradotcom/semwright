#!/usr/bin/env python3
import pathlib
import re
import sys

root = pathlib.Path(__file__).resolve().parents[1]
rust_sources = [
    root / "src/main.rs",
    root / "src/semantic_more_ops.rs",
]
main = "\n".join(path.read_text(encoding="utf-8") for path in rust_sources)
plugin_sources = [
    root / "plugin/src/code.ts",
    root / "plugin/src/semantic_complete.ts",
    root / "plugin/src/semantic_more.ts",
]
plugin = "\n".join(path.read_text(encoding="utf-8") for path in plugin_sources)

advertised = set(re.findall(r'op\(\s*"([^"]+)"', main))
handlers = set(re.findall(r'case\s+"([^"]+)"', plugin))
local = {"doctor", "pairing.begin", "session.list"}

missing = sorted(advertised - handlers - local)
extra = sorted(handlers - advertised)
if missing:
    print("advertised without implementation:")
    for item in missing:
        print(" -", item)
    sys.exit(1)
if extra:
    print("plugin handlers without descriptor:")
    for item in extra:
        print(" -", item)
    sys.exit(1)

for forbidden in (r"\beval\s*\(", r"\bFunction\s*\(", r"app\.asar", r"remote-debugging-port"):
    if re.search(forbidden, plugin):
        raise SystemExit(f"forbidden production escape hatch matched: {forbidden}")

print(f"PASS advertised={len(advertised)} plugin_handlers={len(handlers)} local={len(local)}")
