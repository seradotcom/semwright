#!/usr/bin/env python3
"""Bind all eight native packages to six exact-SHA installation certificates."""
from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path
import re

EXPECTED = frozenset({'linux-x86_64', 'linux-aarch64', 'windows-x86_64',
                      'windows-arm64', 'macos-arm64', 'macos-x86_64'})
INSTALL_CHECKS = ('install', 'installed_smoke', 'uninstall', 'cleanup', 'overwrite_refused',
                  'changed_files_preserved', 'unknown_files_preserved')


def digest(path: Path) -> str:
    with path.open('rb') as stream:
        return hashlib.file_digest(stream, 'sha256').hexdigest()


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate certificate key')
        result[key] = value
    return result


def build_manifest(root: Path, source_sha: str) -> dict:
    if re.fullmatch(r'[0-9a-f]{40}', source_sha) is None:
        raise ValueError('exact source SHA required')
    reports = sorted(root.rglob('certification-*.json'))
    if len(reports) != 6:
        raise ValueError('six certification reports required')
    platforms = set()
    packages = {}
    certificates = []
    certified_paths = set()
    for report in reports:
        if report.is_symlink() or report.stat().st_size > 131072:
            raise ValueError('invalid certificate file')
        row = json.loads(report.read_text(encoding='utf-8'), object_pairs_hook=unique_object)
        identity = f"{row.get('platform')}-{row.get('arch')}"
        if identity not in EXPECTED or identity in platforms:
            raise ValueError('unknown or duplicate native platform certificate')
        platforms.add(identity)
        if (row.get('status') != 'PASS' or row.get('source_sha') != source_sha
                or row.get('release_admission') is not False or row.get('reproducible') is not True
                or row.get('internal_checksums_verified') is not True
                or row.get('binary_architecture') != 'PASS'
                or not isinstance(row.get('user_install'), dict)
                or any(row['user_install'].get(key) is not True for key in INSTALL_CHECKS)):
            raise ValueError(f'incomplete or stale native installation certificate: {identity}')
        if row['platform'] == 'linux':
            if row.get('deb_install_uninstall') is not True:
                raise ValueError('Debian package manager install/remove evidence required')
            expected_files = row.get('artifacts')
            if not isinstance(expected_files, dict) or len(expected_files) != 2:
                raise ValueError('two Linux artifacts required')
            if sum(name.endswith('.deb') for name in expected_files) != 1 or sum(name.endswith('.tar.gz') for name in expected_files) != 1:
                raise ValueError('Linux certificate must name tar and deb')
        else:
            expected_files = {row.get('artifact'): row.get('artifact_sha256')}
            extension = '.zip' if row['platform'] == 'windows' else '.tar.gz'
            if not isinstance(row.get('artifact'), str) or not row['artifact'].endswith(extension):
                raise ValueError('incorrect native archive format')
        for name, sha in expected_files.items():
            if (not isinstance(name, str) or re.fullmatch(r'[A-Za-z0-9._-]+', name) is None
                    or name in packages or not isinstance(sha, str) or re.fullmatch(r'[0-9a-f]{64}', sha) is None):
                raise ValueError('unsafe or duplicate package identity')
            path = report.parent / name
            if path.is_symlink() or not path.is_file() or digest(path) != sha:
                raise ValueError(f'package checksum mismatch: {name}')
            certified_paths.add(path.resolve())
            packages[name] = {'name': name, 'platform': identity, 'bytes': path.stat().st_size, 'sha256': sha}
        certificates.append({'platform': identity, 'path': report.relative_to(root).as_posix(),
                             'sha256': digest(report)})
    actual = {path.resolve() for path in root.rglob('*') if path.is_file() and path.name.endswith(('.tar.gz', '.deb', '.zip'))}
    if platforms != EXPECTED or len(packages) != 8 or len(actual) != 8 or actual != certified_paths:
        raise ValueError('expected precisely eight certified packages and six native platforms')
    return {'schema_version': 2, 'status': 'PASS', 'source_sha': source_sha,
            'release_admission': False, 'package_count': 8, 'verified_platforms': sorted(platforms),
            'install_uninstall_verified': True, 'certification_reports': certificates,
            'packages': sorted(packages.values(), key=lambda p: p['name']),
            'release_readiness': 'Independent security review and explicit maintainer authorization still required.'}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input', type=Path, required=True)
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    manifest = build_manifest(args.input, args.source_sha)
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output / 'V1_DISTRIBUTION_MANIFEST.json').write_text(json.dumps(manifest, indent=2, sort_keys=True) + '\n')
    (args.output / 'SHA256SUMS').write_text(''.join(f'{p["sha256"]}  {p["name"]}\n' for p in manifest['packages']))
    print(json.dumps(manifest, indent=2, sort_keys=True))


if __name__ == '__main__':
    main()
