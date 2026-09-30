#!/usr/bin/env python3
"""Classify changed repository paths into CI areas.

Normal PR iteration consumes the narrow affected-area outputs. Final certification
uses --full and therefore cannot silently skip a gate.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import sys

AREAS = (
    "core_rust",
    "native_chromium",
    "native_driver_conformance",
    "native_driver_distribution",
    "native_libreoffice",
    "native_blender",
    "native_figma",
    "native_kicad_mlt",
    "native_pipewire",
    "native_x11",
    "native_atspi",
    "native_sway",
    "security_dependencies",
    "security_fuzz",
    "runtime_tools",
    "platform_macos",
    "platform_windows",
    "driver_session",
    "godot",
    "motion",
    "mlt",
    "obs",
    "blender_semantic",
    "figma_semantic",
)

COMMON_HOST = (
    "crates/driver-host/**",
    "crates/driver-sdk/**",
    "crates/plugin-host/**",
    "crates/platform-api/**",
    "crates/platform-services/**",
    "scripts/dev/ci-driver-bwrap-profile.sh",
)
CORE = (
    "Cargo.toml",
    "Cargo.lock",
    ".cargo/**",
    "rust-toolchain*",
    "crates/types/**",
    "crates/core/**",
    "crates/protocol/**",
    "crates/policy/**",
    "crates/backend-api/**",
    "crates/platform-api/**",
    "crates/platform-common/**",
    "crates/platform-services/**",
    "crates/driver-sdk/**",
    "crates/driver-host/**",
    "crates/plugin-host/**",
    "crates/plugin-sdk/**",
    "crates/daemon/**",
    "crates/cli/**",
    "crates/mcp/**",
    "crates/tui/**",
    "crates/workflow/**",
    "crates/recipes/**",
    "crates/skills/**",
    "schemas/**",
)


def matches(path: str, patterns: tuple[str, ...]) -> bool:
    return any(fnmatch.fnmatchcase(path, pattern) for pattern in patterns)


def classify(paths: list[str], full: bool = False) -> dict[str, bool]:
    out = {area: bool(full) for area in AREAS}
    if full:
        return out

    for raw in paths:
        path = raw.strip().replace("\\", "/")
        if not path:
            continue

        common_host = matches(path, COMMON_HOST)
        if matches(path, CORE):
            out["core_rust"] = True

        if path in {"Cargo.toml", "Cargo.lock"} or fnmatch.fnmatchcase(path, "crates/*/Cargo.toml"):
            out["security_dependencies"] = True

        if path.startswith("fuzz/") or path.startswith("tests/fuzz/"):
            out["security_fuzz"] = True

        if common_host or matches(path, (
            "crates/platform-linux-sys/**",
            "crates/platform-windows-sys/**",
            "crates/platform-macos-sys/**",
            ".github/workflows/runtime-tools-portability.yml",
            "docs/runtime-tools.md",
            "scripts/verify-driver-runtime-tools.py",
        )):
            out["runtime_tools"] = True

        if common_host or matches(path, (
            "crates/adapters/**",
            "crates/adapters/tests/chromium_live.rs",
        )):
            out["native_chromium"] = True

        if common_host or matches(path, (
            "crates/federation/**",
            "scripts/dev/driver-conformance.sh",
            "scripts/dev/driver-broker-smoke.sh",
        )):
            out["native_driver_conformance"] = True

        if matches(path, (
            "crates/driver-registry/**",
            "crates/registry/**",
            "crates/cli/**",
            "scripts/dev/driver-registry-smoke.sh",
        )):
            out["native_driver_distribution"] = True

        if common_host or path.startswith("crates/driver-libreoffice/"):
            out["native_libreoffice"] = True

        if common_host or path.startswith("crates/driver-blender/") or path.startswith("adapters/blender/"):
            out["native_blender"] = True
            out["blender_semantic"] = True

        if common_host or path.startswith("crates/driver-figma/"):
            out["native_figma"] = True
            out["figma_semantic"] = True

        if common_host or matches(path, (
            "crates/driver-mlt-video/**",
            "crates/video-domain/**",
            "crates/kicad-ipc/**",
            "crates/driver-kicad/**",
            "integrations/kicad-driver/**",
        )):
            out["native_kicad_mlt"] = True
            if path.startswith("crates/driver-mlt-video/") or common_host:
                out["mlt"] = True

        if matches(path, (
            "crates/platform-linux/**",
            "crates/platform-linux-sys/**",
            "scripts/dev/pipewire-screencast-smoke.sh",
        )):
            out["native_pipewire"] = True

        if matches(path, (
            "crates/backends/**",
            "crates/platform-linux/**",
            "crates/platform-linux-sys/**",
        )):
            out["native_x11"] = True

        if matches(path, (
            "crates/platform-linux/**",
            "crates/platform-linux-sys/**",
            "scripts/dev/atspi-live-ci.sh",
            "fixtures/atspi/**",
        )):
            out["native_atspi"] = True

        if matches(path, (
            "crates/platform-linux/**",
            "crates/platform-linux-sys/**",
            "fixtures/sway/**",
        )):
            out["native_sway"] = True

        if common_host or matches(path, (
            "crates/platform-macos/**",
            "crates/platform-macos-sys/**",
            ".github/workflows/platformization-macos.yml",
        )):
            out["platform_macos"] = True

        if common_host or matches(path, (
            "crates/platform-windows/**",
            "crates/platform-windows-sys/**",
            "fixtures/windows-uia/**",
            "tests/python/test_windows_contracts.py",
            ".github/workflows/windows-platform.yml",
        )):
            out["platform_windows"] = True

        if common_host or matches(path, (
            "crates/driver-figma/**",
            "crates/driver-godot/**",
            "crates/driver-obs/**",
            "integrations/godot/**",
        )):
            out["driver_session"] = True

        if common_host or path.startswith("crates/driver-godot/") or path.startswith("integrations/godot/"):
            out["godot"] = True

        if common_host or path.startswith("crates/driver-motion-canvas/") or path.startswith("integrations/motion-canvas/"):
            out["motion"] = True

        if common_host or path.startswith("crates/driver-obs/"):
            out["obs"] = True

    return out


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--full", action="store_true")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("paths", nargs="*")
    args = parser.parse_args()

    paths = args.paths or [line.rstrip("\n") for line in sys.stdin]
    result = classify(paths, full=args.full)
    if args.json:
        print(json.dumps(result, sort_keys=True))
    else:
        for key in AREAS:
            print(f"{key}={'true' if result[key] else 'false'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
