#!/usr/bin/env python3
"""Regenerate checked-in command documentation and the Blender command subset."""
import argparse
import json
from pathlib import Path
ROOT = Path(__file__).resolve().parents[1]

def generated():
    commands = json.loads((ROOT / "schemas/commands.json").read_text())
    lines = ["# Command reference", "", "Generated from `schemas/commands.json`; do not edit by hand.", "",
             f"{len(commands)} built-in descriptors. A descriptor is not proof of live backend support.",
             "Run `computerctl doctor` and consult `compatibility.md` and `../VERIFY.md`.", "",
             "Every command accepts only its documented properties. Use `commands describe NAME`",
             "for the authoritative input/output schema. Return schemas are intentionally broad",
             "for many backends in this development handoff; strengthening them is a release gate.", "",
             "| Command | Required capability | Risk | Timeout | Candidate backends |",
             "|---|---|---|---:|---|"]
    for c in commands:
        lines.append(f"| `{c['name']}` | {', '.join('`'+s+'`' for s in c['requires'])} | {c['risk']} | {c['timeout_ms']} ms | {', '.join(c['backends'])} |")
    for c in commands:
        lines += ["", f"## `{c['name']}`", "", c['description'], "",
                  f"Idempotency: `{c['idempotency']}`. Dry run: `{str(c['dry_run']).lower()}`.", "",
                  "```json", json.dumps(c['input_schema'], indent=2, ensure_ascii=False), "```"]
    return {
        ROOT / "docs/commands.md": "\n".join(lines) + "\n",
        ROOT / "adapters/blender/semwright_blender/commands.json": json.dumps({c['name']: c['input_schema'] for c in commands if c['name'].startswith('blender.')}, indent=2) + "\n",
    }

def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('--check',action='store_true');args=parser.parse_args()
    failed=[]
    for path, text in generated().items():
        if args.check:
            if not path.exists() or path.read_text()!=text: failed.append(str(path.relative_to(ROOT)))
        else: path.write_text(text)
    if failed: parser.exit(1,'Out-of-date generated files: '+', '.join(failed)+'\n')
    print('Command contracts/documentation are synchronized.')
if __name__ == '__main__': main()
