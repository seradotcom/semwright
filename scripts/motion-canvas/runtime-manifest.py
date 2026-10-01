#!/usr/bin/env python3
"""Build the pinned Motion Canvas runtime manifest from installed locked bytes."""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import re

def sha(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()

def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, default=Path("."))
    parser.add_argument("--browser-rel", required=True)
    parser.add_argument("--output", default="runtime.json")
    args = parser.parse_args()

    root = args.root.resolve()
    browser_rel = Path(args.browser_rel)
    if browser_rel.is_absolute() or ".." in browser_rel.parts:
        parser.error("browser path must stay beneath the runtime root")

    required = {
        "node": Path(".semwright-tools/node"),
        "helper": Path("render.mjs"),
        "browser": browser_rel,
        "dependency_lock": Path("package-lock.json"),
    }
    for name, relative in required.items():
        path = root / relative
        if not path.is_file():
            parser.error(f"missing pinned runtime {name}: {relative}")

    css_files = [
        Path("node_modules/@fontsource-variable/instrument-sans/index.css"),
        Path("node_modules/@fontsource/ibm-plex-mono/400.css"),
    ]
    resources: dict[str, str] = {}
    for css_rel in css_files:
        css = root / css_rel
        if not css.is_file():
            parser.error(f"missing pinned font stylesheet: {css_rel}")
        resources[css_rel.as_posix()] = sha(css)
        text = css.read_text(encoding="utf-8")
        refs = re.findall(r"""url\((?:['"]?)([^)'"]+\.woff2)(?:['"]?)\)""", text)
        if not refs:
            parser.error(f"font stylesheet contains no WOFF2 resources: {css_rel}")
        for ref in refs:
            relative = css_rel.parent / ref
            if relative.is_absolute() or ".." in relative.parts:
                parser.error(f"unsafe pinned font reference: {relative}")
            path = root / relative
            if not path.is_file():
                parser.error(f"missing pinned font resource: {relative}")
            if path.stat().st_size > 16 * 1024 * 1024:
                parser.error(f"pinned font resource exceeds 16 MiB: {relative}")
            resources[relative.as_posix()] = sha(path)

    value = {
        "node": {"path": required["node"].as_posix(), "sha256": sha(root / required["node"])},
        "helper": {"path": required["helper"].as_posix(), "sha256": sha(root / required["helper"])},
        "browser": {"path": required["browser"].as_posix(), "sha256": sha(root / required["browser"])},
        "dependency_lock": {
            "path": required["dependency_lock"].as_posix(),
            "sha256": sha(root / required["dependency_lock"]),
        },
        "font_resources": [
            {"path": path, "sha256": digest}
            for path, digest in sorted(resources.items())
        ],
    }
    output = root / args.output
    output.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({
        "runtime_manifest": output.as_posix(),
        "font_resources": len(value["font_resources"]),
        "dependency_lock_sha256": value["dependency_lock"]["sha256"],
    }, sort_keys=True))
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
