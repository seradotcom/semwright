"""Bounded Linux verification jobs and append-only-per-run evidence.

This controls trusted developer tools, not hostile plugins. A child that creates
its own session can leave the managed process group; this is not a sandbox.
"""
from __future__ import annotations

from datetime import datetime, timezone
import hashlib
import json
import math
import os
from pathlib import Path
import re
import selectors
import shutil
import signal
import stat
import subprocess
import time
from typing import Any, Mapping, Sequence


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat()


def _leader_exited(process: subprocess.Popen[bytes]) -> bool:
    # Observe without reaping. The unreaped leader reserves its PID while its
    # process group is signalled, avoiding a PID-reuse cleanup race.
    return os.waitid(os.P_PID, process.pid, os.WEXITED | os.WNOHANG | os.WNOWAIT) is not None


def _cleanup_group(process: subprocess.Popen[bytes]) -> None:
    try:
        os.killpg(process.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    time.sleep(0.05)
    try:
        os.killpg(process.pid, signal.SIGKILL)
    except ProcessLookupError:
        pass
    process.wait(timeout=3)


def run_check(
    root: Path,
    output: Path,
    name: str,
    argv: Sequence[str],
    *,
    timeout_seconds: float = 300,
    max_log_bytes: int = 16 * 1024 * 1024,
    environment: Mapping[str, str] | None = None,
    required_executables: Sequence[str] = (),
) -> dict[str, Any]:
    """Run one foreground check, retaining bounded output and its actual exit code."""
    if not isinstance(name, str) or re.fullmatch(r'[a-z0-9][a-z0-9-]{0,63}', name) is None:
        raise ValueError('invalid check name')
    if not argv or any(not isinstance(arg, str) or '\x00' in arg for arg in argv):
        raise ValueError('argv must contain non-NUL strings')
    if (type(timeout_seconds) not in (int, float) or not math.isfinite(timeout_seconds)
            or timeout_seconds <= 0 or type(max_log_bytes) is not int or max_log_bytes <= 0):
        raise ValueError('budgets must be finite and positive; output budget must be an integer')
    output.mkdir(parents=True, exist_ok=True, mode=0o700)
    log = output / (name + '.log')
    env = dict(os.environ if environment is None else environment)
    env['PYTHONDONTWRITEBYTECODE'] = '1'
    started = time.monotonic()
    entry: dict[str, Any] = {
        'name': name, 'command': list(argv), 'started_at_utc': utc_now(),
        'log': str(log.relative_to(root)) if log.is_relative_to(root) else str(log),
        'timeout_seconds': timeout_seconds, 'max_log_bytes': max_log_bytes,
        'status': 'FAIL', 'exit_code': None,
    }
    process: subprocess.Popen[bytes] | None = None
    selector = selectors.DefaultSelector()
    written = 0
    flags = os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW | os.O_CLOEXEC
    fd = os.open(log, flags, 0o600)
    try:
        with os.fdopen(fd, 'wb') as stream:
            missing = sorted({command for command in (argv[0], *required_executables)
                              if shutil.which(command, path=env.get('PATH', '')) is None})
            if missing:
                entry.update(status='BLOCKED', reason='Executable unavailable: ' + ', '.join(missing))
                stream.write((entry['reason'] + '\n').encode()[:max_log_bytes])
            else:
                reason: str | None = None
                try:
                    process = subprocess.Popen(
                        list(argv), cwd=root, env=env, stdin=subprocess.DEVNULL,
                        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                        start_new_session=True, close_fds=True, bufsize=0,
                    )
                    assert process.stdout is not None
                    os.set_blocking(process.stdout.fileno(), False)
                    selector.register(process.stdout, selectors.EVENT_READ)
                    deadline = started + timeout_seconds
                    exit_seen: float | None = None
                    while True:
                        now = time.monotonic()
                        if now >= deadline:
                            reason = 'TIMEOUT: command exceeded its elapsed-time budget'
                            break
                        if _leader_exited(process):
                            if not selector.get_map():
                                break
                            if exit_seen is None:
                                exit_seen = now
                            elif now - exit_seen > 0.25:
                                reason = 'DESCENDANT_PIPE: output remained open after the command exited'
                                break
                        if not selector.get_map():
                            time.sleep(min(0.02, max(0, deadline - now)))
                            continue
                        for key, _ in selector.select(min(0.05, max(0, deadline - now))):
                            try:
                                chunk = os.read(key.fd, 65536)
                            except BlockingIOError:
                                continue
                            if not chunk:
                                selector.unregister(key.fileobj)
                                continue
                            remaining = max_log_bytes - written
                            stream.write(chunk[:remaining])
                            written += min(len(chunk), remaining)
                            if len(chunk) > remaining:
                                reason = 'OUTPUT_LIMIT: command exceeded its captured-output budget'
                                break
                        if reason:
                            break
                except KeyboardInterrupt:
                    reason = 'INTERRUPTED: verification was interrupted'
                    entry['interrupted'] = True
                except (OSError, ValueError):
                    reason = 'EXECUTION_ERROR: command could not be started or observed'
                finally:
                    if process is not None:
                        _cleanup_group(process)
                        entry['exit_code'] = process.returncode
                        entry['process_group_cleanup'] = 'owned group signalled and leader reaped'
                        if process.stdout is not None:
                            process.stdout.close()
                if reason:
                    entry.update(status='FAIL', reason=reason)
                else:
                    entry['status'] = 'PASS' if entry['exit_code'] == 0 else 'FAIL'
                    if entry['status'] == 'FAIL':
                        entry['reason'] = 'Command exited unsuccessfully'
    finally:
        selector.close()
    entry['finished_at_utc'] = utc_now()
    entry['duration_seconds'] = round(time.monotonic() - started, 6)
    entry['log_bytes'] = log.stat().st_size
    entry['log_sha256'] = hashlib.sha256(log.read_bytes()).hexdigest()
    return entry


class Report:
    """Persist each transition; refuse to overwrite an earlier run's report."""
    def __init__(self, output: Path, checks: Sequence[str], *, source_root: Path) -> None:
        if not checks or any(not isinstance(name, str) or re.fullmatch(r'[a-z0-9][a-z0-9-]{0,63}', name) is None
                             for name in checks):
            raise ValueError('report requires a nonempty plan of valid check names')
        if len(set(checks)) != len(checks):
            raise ValueError('duplicate check name')
        output.mkdir(parents=True, exist_ok=True, mode=0o700)
        if not stat.S_ISDIR(output.lstat().st_mode):
            raise ValueError('report output must be a real directory, not a symlink')
        self.path = output / 'summary.json'
        fd = os.open(self.path, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        os.close(fd)
        self.document: dict[str, Any] = {
            'started_at_utc': utc_now(), 'scope': 'local quality gates; not release certification',
            'source_root': str(source_root), 'overall': 'RUNNING',
            'results': [{'name': name, 'status': 'NOT_RUN', 'exit_code': None} for name in checks],
        }
        self.persist()

    def persist(self) -> None:
        temporary = self.path.with_suffix('.tmp')
        fd = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
        try:
            with os.fdopen(fd, 'w', encoding='utf-8') as stream:
                json.dump(self.document, stream, indent=2, allow_nan=False)
                stream.write('\n')
                stream.flush()
                os.fsync(stream.fileno())
            os.replace(temporary, self.path)
        finally:
            temporary.unlink(missing_ok=True)

    def record(self, entry: dict[str, Any]) -> None:
        if entry.get('status') not in ('PASS', 'FAIL', 'BLOCKED'):
            raise ValueError('result must have an executed or blocked status')
        if entry['status'] == 'PASS' and (type(entry.get('exit_code')) is not int or entry['exit_code'] != 0):
            raise ValueError('PASS requires an actual zero exit code')
        for index, existing in enumerate(self.document['results']):
            if existing['name'] == entry['name']:
                if existing['status'] != 'NOT_RUN':
                    raise ValueError('a check cannot be silently rerun in the same report')
                self.document['results'][index] = entry
                self.persist()
                return
        raise ValueError('unplanned check')

    def finish(self, *, interrupted: bool = False) -> bool:
        self.document['finished_at_utc'] = utc_now()
        self.document['interrupted'] = interrupted
        complete = not interrupted and all(item['status'] == 'PASS' for item in self.document['results'])
        self.document['overall'] = 'PASS' if complete else 'INCOMPLETE_OR_FAILED'
        self.persist()
        return complete
