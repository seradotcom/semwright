#!/usr/bin/env python3
import pathlib
import re
import sys

root = pathlib.Path(__file__).resolve().parents[1]
rust_sources = [
    root / "src/main.rs",
    root / "src/semantic_more_ops.rs",
    root / "src/semantic_admin_ops.rs",
    root / "src/semantic_rest_ops.rs",
]
main = "\n".join(path.read_text(encoding="utf-8") for path in rust_sources)
plugin_sources = [
    root / "plugin/src/code.ts",
    *sorted((root / "plugin/src").glob("semantic_*.ts")),
]
plugin = "\n".join(path.read_text(encoding="utf-8") for path in plugin_sources)

advertised = set(re.findall(r'op\(\s*"([^"]+)"', main))
all_cases = set(re.findall(r'case\s+"([^"]+)"', plugin))
# Operation names are namespaced with a dot. Internal switches (for example
# VariableResolvedDataType cases like "COLOR") are implementation details,
# not bridge handlers, and must not inflate the capability surface.
handlers = {name for name in all_cases if "." in name or name in advertised}
local = {"doctor", "pairing.begin", "session.list"}
rest_coverage = __import__("json").loads(
    (root / "docs/REST_API_COVERAGE.json").read_text(encoding="utf-8")
)
rest = {
    item["capability"]
    for item in rest_coverage["operations"] + rest_coverage.get("documented_extras", [])
} | {"cloud.status"}

missing = sorted(advertised - handlers - local - rest)
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

print(
    f"PASS advertised={len(advertised)} plugin_handlers={len(handlers)} "
    f"local={len(local)} rest={len(rest)}"
)
