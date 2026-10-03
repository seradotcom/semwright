#!/usr/bin/env python3
"""Install/remove the actual Debian artifact, only in disposable GitHub-hosted CI."""
import argparse
import json
import os
from pathlib import Path
import subprocess

from bundle_contract import BINS, digest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', type=Path, required=True)
    args = parser.parse_args()
    if os.environ.get('GITHUB_ACTIONS') != 'true' or os.environ.get('RUNNER_ENVIRONMENT') != 'github-hosted':
        raise SystemExit('Debian package-manager acceptance is restricted to disposable GitHub-hosted runners')
    report = json.loads(args.report.read_text())
    names = [name for name in report['artifacts'] if name.endswith('.deb')]
    if len(names) != 1:
        raise SystemExit('one Debian artifact required')
    package = args.report.parent / names[0]
    if digest(package) != report['artifacts'][names[0]]:
        raise SystemExit('Debian artifact differs from the certified bytes')
    if any((Path('/usr/bin') / name).exists() for name in BINS):
        raise SystemExit('refusing to replace an existing Semwright system installation')
    subprocess.run(['sudo', 'apt-get', 'install', '--no-install-recommends', '-y', str(package.resolve())], check=True)
    try:
        for name in BINS[:4]:
            subprocess.run([str(Path('/usr/bin') / name), '--help'], check=True, capture_output=True, timeout=15)
    finally:
        subprocess.run(['sudo', 'apt-get', 'remove', '-y', 'semwright'], check=True)
    if any((Path('/usr/bin') / name).exists() for name in BINS):
        raise SystemExit('Debian removal left installed command files')
    report['deb_install_uninstall'] = True
    args.report.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    print('Debian install / installed smoke / removal: PASS')


if __name__ == '__main__':
    main()
