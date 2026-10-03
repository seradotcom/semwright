#!/usr/bin/env python3
"""Build release tar/deb artifacts after fail-closed release admission."""
from __future__ import annotations

import argparse
import subprocess
import sys
from pathlib import Path

from package_lib import build_packages

ROOT = Path(__file__).resolve().parents[2]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument("--arch", choices=["x86_64", "aarch64"], required=True)
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    parser.add_argument("--deb", action="store_true")
    parser.add_argument("--candidate-sha", required=True)
    parser.add_argument("--review-report", type=Path, required=True)
    parser.add_argument("--distribution-manifest", type=Path, required=True)
    parser.add_argument("--maintainer-approved", action="store_true")
    args = parser.parse_args()

    admission = [sys.executable, str(ROOT / "scripts/release/assert-ready.py"), "--mode", "publish",
                 "--candidate-sha", args.candidate_sha, "--review-report", str(args.review_report),
                 "--distribution-manifest", str(args.distribution_manifest)]
    if args.maintainer_approved:
        admission.append("--maintainer-approved")
    subprocess.run(admission, check=True)
    artifacts = build_packages(
        ROOT,
        args.bin_dir,
        args.arch,
        args.output,
        deb=args.deb,
    )
    print("Built admitted release packages: " + ", ".join(sorted(artifacts)))


if __name__ == "__main__":
    main()
