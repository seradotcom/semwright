#!/usr/bin/env python3
"""Build development certification packages without granting release admission."""
from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts/release"))

from package_lib import build_packages, source_date_epoch  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--arch", choices=("x86_64", "aarch64"), required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--report", type=Path, required=True)
    args = parser.parse_args()

    epoch = source_date_epoch(ROOT)
    artifacts = build_packages(
        ROOT,
        args.bin_dir.resolve(),
        args.arch,
        args.output.resolve(),
        deb=True,
        epoch=epoch,
    )
    report = {
        "status": "PASS",
        "purpose": "development_supply_chain_certification",
        "release_admission": False,
        "arch": args.arch,
        "source_date_epoch": epoch,
        "artifacts": artifacts,
    }
    args.report.parent.mkdir(parents=True, exist_ok=True)
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n")
    print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
