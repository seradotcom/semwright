#!/usr/bin/env python3
import pathlib
import re
import sys

root=pathlib.Path(__file__).resolve().parents[1]
main=(root/"src/main.rs").read_text(encoding="utf-8")
plugin=(root/"plugin/src/code.ts").read_text(encoding="utf-8")
match=re.search(r"const SUPPORTED_OPERATIONS: &\[&str\] = &\[(.*?)\];", main, re.S)
if not match:
    raise SystemExit("SUPPORTED_OPERATIONS not found")
advertised=set(re.findall(r'"([^"]+)"', match.group(1)))
handlers=set(re.findall(r'case\s+"([^"]+)"', plugin))
local={"doctor","pairing.begin","session.list"}
missing=sorted(advertised-handlers-local)
if missing:
    print("advertised without implementation:")
    for item in missing:
        print(" -", item)
    sys.exit(1)
extra=sorted(handlers-advertised)
print(f"PASS advertised={len(advertised)} plugin_handlers={len(handlers)} local={len(local)}")
if extra:
    print("non-advertised plugin handlers:", ",".join(extra))
