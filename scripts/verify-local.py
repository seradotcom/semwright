#!/usr/bin/env python3
"""Run the complete local quality plan with bounded jobs and durable evidence."""
from __future__ import annotations

import argparse
import math
from datetime import datetime, timezone
import os
from pathlib import Path
import signal
import sys
import tempfile
import uuid

from verification import Report, run_check

ROOT = Path(__file__).resolve().parents[1]
# These are compiler/quality gates, not a replacement for the live, federation,
# coverage, fuzz, sandbox or release acceptance matrices.
RUST_CHECKS = (
    ('rustc', ('rustc', '--version'), ()),
    ('cargo', ('cargo', '--version'), ()),
    ('rustfmt-version', ('rustfmt', '--version'), ()),
    ('clippy-version', ('cargo', 'clippy', '--version'), ('cargo-clippy',)),
    ('metadata', ('cargo', 'metadata', '--locked', '--format-version', '1'), ()),
    ('rustfmt', ('cargo', 'fmt', '--all', '--', '--check'), ('rustfmt',)),
    ('check', ('cargo', 'check', '--locked', '--workspace', '--all-targets', '--all-features'), ()),
    ('build', ('cargo', 'build', '--locked', '--workspace', '--all-targets', '--all-features'), ()),
    ('clippy', ('cargo', 'clippy', '--locked', '--workspace', '--all-targets', '--all-features', '--', '-D', 'warnings'), ('cargo-clippy',)),
    ('rust-tests', ('cargo', 'test', '--locked', '--workspace', '--all-targets', '--all-features'), ()),
    ('doctests', ('cargo', 'test', '--locked', '--workspace', '--all-features', '--doc'), ()),
    ('rust-doc', ('cargo', 'doc', '--locked', '--workspace', '--all-features', '--no-deps'), ()),
    ('release', ('cargo', 'build', '--locked', '--workspace', '--all-features', '--release'), ()),
    ('audit', ('cargo', 'audit', '--deny', 'warnings'), ('cargo-audit',)),
    ('deny', ('cargo', 'deny', 'check'), ('cargo-deny',)),
)


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    result.add_argument('--with-chromium', action='store_true', help='Run the real Python CDP probe; not the Rust adapter')
    result.add_argument('--output', type=Path, help='Fresh evidence directory; existing reports are never overwritten')
    result.add_argument('--timeout-seconds', type=float, default=900, help='Per-command elapsed-time budget')
    result.add_argument('--max-log-bytes', type=int, default=16 * 1024 * 1024, help='Per-command captured-output budget')
    return result


def main() -> int:
    argument_parser = parser()
    args = argument_parser.parse_args()
    if not math.isfinite(args.timeout_seconds) or args.timeout_seconds <= 0 or args.max_log_bytes <= 0:
        argument_parser.error('verification budgets must be finite and positive')
    stamp = datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%SZ')
    output = (args.output or ROOT / 'verification/runs' / (stamp + '-' + uuid.uuid4().hex[:8])).absolute()
    checks = ['source', 'python', 'javascript', 'native-compile', 'native-kernel']
    if args.with_chromium:
        checks.append('chromium')
    checks.extend(name for name, _, _ in RUST_CHECKS)
    checks.extend(['shellcheck', 'actionlint', 'ruff'])
    report = Report(output, checks, source_root=ROOT)
    report.document['not_in_this_plan'] = [
        'Rust adapter live tests', 'MCP server/federation E2E', 'plugin sandbox conformance',
        'coverage', 'fuzzing', 'benchmarks', 'binary packaging/install', 'live desktop matrix',
        'GitHub Actions execution',
    ]
    report.document['chromium_python_probe_requested'] = args.with_chromium
    report.persist()
    interrupted = False

    def run(name: str, argv: tuple[str, ...] | list[str], *, tools: tuple[str, ...] = (), docs: bool = False) -> bool:
        environment = dict(os.environ)
        if docs:
            environment['RUSTDOCFLAGS'] = (environment.get('RUSTDOCFLAGS', '') + ' -D warnings').strip()
        entry = run_check(
            ROOT, output, name, argv, timeout_seconds=args.timeout_seconds,
            max_log_bytes=args.max_log_bytes, environment=environment,
            required_executables=tools,
        )
        report.record(entry)
        print(name + ': ' + entry['status'], flush=True)
        if entry.get('interrupted'):
            raise KeyboardInterrupt
        return entry['status'] == 'PASS'

    def interrupt(_signum: int, _frame: object) -> None:
        raise KeyboardInterrupt

    previous_term = signal.signal(signal.SIGTERM, interrupt)
    try:
        run('source', [sys.executable, 'scripts/verify-source.py'])
        run('python', [sys.executable, '-m', 'unittest', 'discover', '-s', 'tests/python', '-v'])
        run('javascript', ['node', '--test', 'tests/js/bridge.test.mjs'])
        with tempfile.TemporaryDirectory(prefix='semwright-native-') as temporary:
            executable = str(Path(temporary) / 'openat2-check')
            if run('native-compile', ['gcc', '-Wall', '-Wextra', '-Werror', '-O2', 'tests/native/openat2.c', '-o', executable]):
                fixture = Path(temporary) / 'fixture'
                fixture.mkdir(mode=0o700)
                run('native-kernel', [executable, str(fixture)])
            else:
                report.record({'name': 'native-kernel', 'status': 'BLOCKED', 'exit_code': None,
                               'reason': 'Native fixture was not built; execution was not attempted'})
        if args.with_chromium:
            run('chromium', [sys.executable, 'tests/python/cdp_live.py'])
        for name, argv, tools in RUST_CHECKS:
            run(name, argv, tools=tools, docs=name == 'rust-doc')
        shell_files = sorted(path for path in ROOT.rglob('*.sh')
                             if not any(part in {'.git', 'target', 'node_modules'} for part in path.parts))
        run('shellcheck', ['shellcheck', *map(str, shell_files)])
        run('actionlint', ['actionlint'])
        run('ruff', ['ruff', 'check', 'adapters/blender', 'tests/python', 'scripts'])
    except KeyboardInterrupt:
        interrupted = True
    finally:
        signal.signal(signal.SIGTERM, previous_term)
        complete = report.finish(interrupted=interrupted)
        print('Overall: ' + report.document['overall'])
        print('Evidence: ' + str(report.path))
    return 0 if complete else 130 if interrupted else 1


if __name__ == '__main__':
    try:
        sys.exit(main())
    except (OSError, ValueError) as error:
        print('Verification could not safely record evidence: ' + str(error), file=sys.stderr)
        sys.exit(2)
