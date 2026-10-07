#!/usr/bin/env python3
"""Validate release admission metadata. This is not an evidence attestation service."""
from __future__ import annotations

import argparse
import datetime
import json
import os
from pathlib import Path
import re
import stat
import subprocess
import sys
import tomllib
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
# Keep this independent of release-readiness.json: deriving the required keys from
# the submitted document would make omitted gates disappear from the policy.
REQUIRED_GATES = frozenset({
    'rust_build_and_tests',
    'reviewed_lockfile',
    'pinned_toolchain',
    'clippy_and_fmt',
    'dependency_audit_and_licenses',
    'rust_broker_integration',
    'plugin_sandbox_negative_tests',
    'application_adapter_validation',
    'release_packaging_validation',
    'security_review',
})
ENGINEERING_GATES = REQUIRED_GATES - {'security_review'}
READY_STATUS = 'READY_FOR_RELEASE_VALIDATION'
PENDING_STATUS = 'BLOCKED_PENDING_SECURITY_REVIEW'
DEFERRED = 'DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT'
POST_V1 = frozenset({'live_desktop_matrix', 'windows_interactive'})
REVIEW_AREAS = frozenset(f'R16-{i:02}' for i in range(1, 13))


def read_regular(path: Path, limit: int) -> str:
    """Read a bounded regular file without following its final symlink."""
    flags = os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC
    fd = os.open(path, flags)
    with os.fdopen(fd, 'rb') as stream:
        metadata = os.fstat(stream.fileno())
        if not stat.S_ISREG(metadata.st_mode) or metadata.st_size > limit:
            raise ValueError('expected a bounded regular file')
        body = stream.read(limit + 1)
        if len(body) > limit:
            raise ValueError('file grew beyond its limit')
        return body.decode('utf-8')


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('duplicate JSON key')
        result[key] = value
    return result


def reject_constant(value: str) -> Any:
    raise ValueError('non-finite JSON constant')


def validate(root: Path, mode: str = 'publish') -> list[str]:
    failures: list[str] = []
    try:
        readiness = json.loads(
            read_regular(root / 'release-readiness.json', 131072),
            object_pairs_hook=unique_object,
            parse_constant=reject_constant,
        )
        if not isinstance(readiness, dict):
            raise ValueError('root must be an object')
        if set(readiness) != {'schema_version', 'status', 'gates', 'post_v1_certification'}:
            failures.append('readiness document keys')
        if type(readiness.get('schema_version')) is not int or readiness['schema_version'] != 2:
            failures.append('readiness schema version')
        # The independent review is external to the immutable candidate. Both modes
        # validate honest source metadata; publication_errors requires the review.
        allowed_statuses = {READY_STATUS, PENDING_STATUS}
        if readiness.get('status') not in allowed_statuses:
            failures.append('readiness status')
        post_v1 = readiness.get('post_v1_certification')
        if (not isinstance(post_v1, dict) or set(post_v1) != POST_V1
                or any(value != DEFERRED for value in post_v1.values())):
            failures.append('post-v1 certification must retain the exact deferred, not PASS, states')
        gates = readiness.get('gates')
        if not isinstance(gates, dict):
            failures.append('gates must be an object')
        else:
            if set(gates) != REQUIRED_GATES:
                failures.append('required gate set is incomplete or contains unknown names')
            if any(type(value) is not bool for value in gates.values()):
                failures.append('gates must be literal booleans')
            failures.extend(name for name in sorted(ENGINEERING_GATES) if gates.get(name) is not True)
            if readiness.get('status') == READY_STATUS and gates.get('security_review') is not True:
                failures.append('ready status cannot hide pending security review')
            if readiness.get('status') == PENDING_STATUS and gates.get('security_review') is not False:
                failures.append('pending status must retain security_review=false')
    except (OSError, ValueError, TypeError, RecursionError):
        failures.append('readiness metadata unreadable or invalid')

    try:
        lock = tomllib.loads(read_regular(root / 'Cargo.lock', 4 * 1024 * 1024))
        packages = lock.get('package')
        if (type(lock.get('version')) is not int or lock['version'] not in (3, 4)
                or not isinstance(packages, list) or not packages
                or any(not isinstance(p, dict) or not isinstance(p.get('name'), str)
                       or not isinstance(p.get('version'), str) for p in packages)):
            raise ValueError('invalid lockfile structure')
    except (OSError, ValueError):
        failures.append('Cargo.lock present and parseable (cargo --locked must still verify it)')

    try:
        toolchain = tomllib.loads(read_regular(root / 'rust-toolchain.toml', 131072))
        channel = toolchain.get('toolchain', {}).get('channel')
        if not isinstance(channel, str) or re.fullmatch(r'[0-9]+\.[0-9]+\.[0-9]+', channel) is None:
            raise ValueError('toolchain must name an exact tested stable version')
    except (OSError, ValueError, AttributeError):
        failures.append('exact stable toolchain pin')
    return failures


def publication_errors(root: Path, candidate_sha: str | None, approved: bool,
                       review_path: Path | None, manifest_path: Path | None) -> list[str]:
    """Check record bindings, not reviewer authenticity; the maintainer must verify that."""
    failures = []
    try:
        actual = subprocess.check_output(
            ['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True, stderr=subprocess.DEVNULL
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        actual = None
    if not candidate_sha or re.fullmatch(r'[0-9a-f]{40}', candidate_sha) is None or candidate_sha != actual:
        failures.append('explicit candidate SHA must match the exact checkout')
    if not approved:
        failures.append('explicit maintainer publication authorization required')
    try:
        if review_path is None:
            raise ValueError('missing independent review')
        report = json.loads(read_regular(review_path, 131072), object_pairs_hook=unique_object,
                            parse_constant=reject_constant)
        if not isinstance(report, dict):
            raise ValueError('review must be an object')
        if (report.get('reviewed_sha') != candidate_sha
                or report.get('independent') is not True
                or report.get('conclusion') != 'APPROVED_FOR_PUBLIC_RELEASE'
                or report.get('unresolved_blocking_findings') != []
                or not isinstance(report.get('reviewer'), str) or not report['reviewer'].strip()
                or not isinstance(report.get('report_reference'), str) or not report['report_reference'].strip()
                or not isinstance(report.get('areas'), list)
                or len(report['areas']) != 12 or set(report['areas']) != REVIEW_AREAS):
            raise ValueError('review is incomplete or bound to another commit')
        reviewed = datetime.date.fromisoformat(report['reviewed_at'])
        if reviewed > datetime.datetime.now(datetime.timezone.utc).date():
            raise ValueError('future review date')
    except (OSError, ValueError, TypeError, KeyError, RecursionError):
        failures.append('dated independent exact-SHA security review required')
    try:
        if manifest_path is None:
            raise ValueError('missing distribution validation')
        manifest = json.loads(read_regular(manifest_path, 131072), object_pairs_hook=unique_object,
                              parse_constant=reject_constant)
        expected = {'linux-x86_64', 'linux-aarch64', 'windows-x86_64', 'windows-arm64',
                    'macos-arm64', 'macos-x86_64'}
        if (not isinstance(manifest, dict) or manifest.get('status') != 'PASS'
                or manifest.get('source_sha') != candidate_sha
                or manifest.get('release_admission') is not False
                or manifest.get('package_count') != 8
                or not isinstance(manifest.get('packages'), list) or len(manifest['packages']) != 8
                or set(manifest.get('verified_platforms', [])) != expected
                or manifest.get('install_uninstall_verified') is not True):
            raise ValueError('incomplete exact-SHA distribution validation')
    except (OSError, ValueError, TypeError, RecursionError):
        failures.append('final exact-SHA six-platform install/uninstall manifest required')
    return failures


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--mode', choices=('staging', 'publish'), default='publish')
    parser.add_argument('--candidate-sha')
    parser.add_argument('--maintainer-approved', action='store_true')
    parser.add_argument('--review-report', type=Path)
    parser.add_argument('--distribution-manifest', type=Path)
    args = parser.parse_args()
    failures = validate(ROOT, args.mode)
    if args.mode == 'publish':
        failures.extend(publication_errors(ROOT, args.candidate_sha, args.maintainer_approved,
                                           args.review_report, args.distribution_manifest))
    if failures:
        print('Release blocked: ' + ', '.join(failures), file=sys.stderr)
        return 2
    if args.mode == 'staging':
        print('STAGING_ADMITTED: engineering metadata complete. NOT PUBLIC RELEASE AUTHORIZATION. '
              'CI must still run native package and installation checks on this commit.')
    else:
        print('Publication records are bound to this SHA. This validator cannot establish reviewer '
              'authenticity; the maintainer must verify the independent report and CI evidence.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
