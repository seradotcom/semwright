#!/usr/bin/env python3
"""Validate release admission metadata. This is not an evidence attestation service."""
from __future__ import annotations

import json
import os
from pathlib import Path
import re
import stat
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
    'live_desktop_matrix',
    'application_adapter_validation',
    'release_packaging_validation',
    'security_review',
})
READY_STATUS = 'READY_FOR_RELEASE_VALIDATION'


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


def validate(root: Path) -> list[str]:
    failures: list[str] = []
    try:
        readiness = json.loads(
            read_regular(root / 'release-readiness.json', 131072),
            object_pairs_hook=unique_object,
            parse_constant=reject_constant,
        )
        if not isinstance(readiness, dict):
            raise ValueError('root must be an object')
        if set(readiness) != {'status', 'gates'}:
            failures.append('readiness document keys')
        if readiness.get('status') != READY_STATUS:
            failures.append('readiness status')
        gates = readiness.get('gates')
        if not isinstance(gates, dict):
            failures.append('gates must be an object')
        else:
            if set(gates) != REQUIRED_GATES:
                failures.append('required gate set is incomplete or contains unknown names')
            failures.extend(name for name in sorted(REQUIRED_GATES) if gates.get(name) is not True)
    except (OSError, ValueError, RecursionError):
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


def main() -> int:
    failures = validate(ROOT)
    if failures:
        print('Release blocked: ' + ', '.join(failures), file=sys.stderr)
        return 2
    print('Release admission metadata complete; CI must still run all gates on this commit.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
