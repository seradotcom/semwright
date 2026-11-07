"""Run only three pure profile/Host regressions in an existing bounded CI unit."""
from pathlib import Path
import hashlib
import json
import os
import re
import selectors
import shutil
import signal
import subprocess
import sys
import time

LIMIT, RESERVE = 1536 * 1024**2, 512 * 1024**2
KERNEL_MAX, KERNEL_HIGH = 1472 * 1024**2, 1408 * 1024**2
FLOOR, GROWTH, LOG_LIMIT = 10 * 1024**3, 2 * 1024**3, 1024**2
EXPECTED_TESTS = (
    'tests::text_pixel_profile_absence_preserves_legacy_wire_bytes',
    'tests::text_pixel_profile_binds_ordered_authoring_samples_and_area',
    'renderer::text_pixel_host_config_tests::text_pixel_plan_selection_reaches_the_original_host_program',
)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def available():
    return int(next(line.split()[1] for line in Path('/proc/meminfo').read_text().splitlines()
                    if line.startswith('MemAvailable:'))) * 1024


def require_live_limits(limits):
    assert limits == {'memory.max': str(KERNEL_MAX), 'memory.high': str(KERNEL_HIGH),
                      'memory.swap.max': '0', 'pids.max': '256'}


def main():
    assert len(sys.argv) == 3
    repo, output, cargo = Path.cwd().resolve(), Path(sys.argv[1]), Path(sys.argv[2])
    assert output.is_absolute() and output.parent == Path(os.environ['RUNNER_TEMP']).resolve()
    assert output.name == 'native-text-pixels-host-focal' and not output.exists()
    assert cargo.is_absolute() and cargo.is_file()
    context = {key: os.environ[key] for key in ('GITHUB_RUN_ID','GITHUB_RUN_ATTEMPT','CANDIDATE_SHA')}
    assert re.fullmatch('[0-9a-f]{40}', context['CANDIDATE_SHA'])
    assert re.fullmatch('[1-9][0-9]{0,19}', context['GITHUB_RUN_ID']) and context['GITHUB_RUN_ATTEMPT'] == '1'
    assert subprocess.check_output(['git','rev-parse','HEAD'], timeout=10).decode().strip() == context['CANDIDATE_SHA']
    membership = Path('/proc/self/cgroup').read_text().splitlines()
    assert len(membership) == 1 and membership[0].startswith('0::/')
    cgroup = Path('/sys/fs/cgroup') / membership[0][3:].lstrip('/')
    limits = {name: (cgroup / name).read_text().strip() for name in ('memory.max','memory.high','memory.swap.max','pids.max')}
    require_live_limits(limits)
    assert available() >= LIMIT + RESERVE
    free = shutil.disk_usage(repo).free
    assert free >= FLOOR + GROWTH
    paths = ['Cargo.lock','Cargo.toml','rust-toolchain.toml','crates/motion-authoring/src/measurement.rs',
        'crates/driver-motion-canvas/Cargo.toml','crates/driver-motion-canvas/src/model.rs',
        'crates/driver-motion-canvas/src/renderer.rs','crates/driver-motion-canvas/src/validate.rs',
        'crates/driver-motion-canvas/src/tests.rs']
    def sources():
        return {name: {'bytes': (repo/name).stat().st_size, 'sha256': sha((repo/name).read_bytes())} for name in paths}
    before = sources()
    output.mkdir(mode=0o700)
    def retain(name, value):
        raw = (json.dumps(value, sort_keys=True, indent=2) + '\n').encode()
        assert len(raw) <= 128 * 1024
        with (output/name).open('xb') as stream:
            stream.write(raw); stream.flush(); os.fsync(stream.fileno())
    argv = [str(cargo), '+1.98.1', 'test', '--locked', '--jobs', '1', '-p',
        'semwright-driver-motion-canvas', '--lib', 'text_pixel', '--', '--test-threads=1']
    state = {'schema': 'semwright.native-text-pixels-host-focal/1', 'context': context, 'argv': argv,
        'source_originals': before, 'limits_direct': limits, 'physical_peak_budget_bytes': LIMIT,
        'reserve_physical_bytes': RESERVE,
        'disk_floor_bytes': FLOOR, 'maximum_new_disk_bytes': GROWTH, 'deadline_seconds': 300,
        'maximum_bytes_per_stream': LOG_LIMIT, 'expected_tests': EXPECTED_TESTS,
        'prior_four_ABI_tests_reused': True, 'prior_twenty_DOM_draws_reused': True,
        'Native_calls': 0, 'automatic_retry': False, 'G2_accepted': False, 'F_accepted': False}
    retain('intake.original.json', state)
    start, reason, child = time.monotonic(), None, None
    counts, descriptors = {'stdout': 0, 'stderr': 0}, {}
    env = os.environ.copy()
    env.update(CARGO_BUILD_JOBS='1', CARGO_INCREMENTAL='0', CARGO_PROFILE_DEV_DEBUG='0', CARGO_PROFILE_TEST_DEBUG='0')
    selector = selectors.DefaultSelector()
    logs = {name: (output/(name+'.original.log')).open('xb') for name in counts}
    try:
        child = subprocess.Popen(argv, cwd=repo, env=env, stdin=subprocess.DEVNULL,
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        for name, pipe in (('stdout', child.stdout), ('stderr', child.stderr)):
            os.set_blocking(pipe.fileno(), False); selector.register(pipe, selectors.EVENT_READ, name)
        while selector.get_map() or child.poll() is None:
            for key, _ in selector.select(0.1):
                chunk = os.read(key.fileobj.fileno(), 16384)
                if not chunk:
                    selector.unregister(key.fileobj); key.fileobj.close(); continue
                remaining = LOG_LIMIT - counts[key.data]
                logs[key.data].write(chunk[:remaining]); counts[key.data] += min(len(chunk), remaining)
                if len(chunk) > remaining: reason = 'bounded_stream_overflow'
            if time.monotonic() - start >= 285: reason = 'bounded_cargo_deadline'
            elif available() < RESERVE: reason = 'physical_reserve_lost'
            elif shutil.disk_usage(repo).free < max(FLOOR, free-GROWTH): reason = 'disk_budget_lost'
            if reason: break
    finally:
        if child is not None and child.poll() is None:
            os.killpg(child.pid, signal.SIGTERM)
            try: child.wait(timeout=5)
            except subprocess.TimeoutExpired:
                os.killpg(child.pid, signal.SIGKILL); child.wait(timeout=3)
        selector.close()
        if child is not None:
            for pipe in (child.stdout, child.stderr):
                if pipe is not None and not pipe.closed: pipe.close()
        for stream in logs.values(): stream.flush(); os.fsync(stream.fileno()); stream.close()
    remaining = [int(pid) for pid in (cgroup/'cgroup.procs').read_text().split()]
    only_reader = remaining == [os.getpid()]
    peak = int((cgroup/'memory.peak').read_text())
    event = dict(line.split() for line in (cgroup/'memory.events').read_text().splitlines())
    raw_out = (output/'stdout.original.log').read_bytes()
    for name in counts:
        raw = (output/(name+'.original.log')).read_bytes()
        descriptors[name] = {'bytes': len(raw), 'sha256': sha(raw), 'complete': reason is None}
    names = tuple(re.findall(rb'^test ([^\s]+) \.\.\. ok$', raw_out, flags=re.M))
    exact = sorted(name.decode() for name in names) == sorted(EXPECTED_TESTS)
    exact = exact and b'test result: ok. 3 passed; 0 failed; 0 ignored;' in raw_out
    same = sources() == before
    passed = (child is not None and child.returncode == 0 and reason is None and exact and same
              and only_reader and peak <= LIMIT and all(int(event.get(k,0)) == 0 for k in ('oom','oom_kill')))
    state.update(status='THREE_PROFILE_HOST_FOCALS_PASS' if passed else 'FOCAL_NEGATIVE_RETAINED',
        resource_stop=reason, exit_code=child.returncode if child else None, exact_three_tests_passed=exact,
        source_originals_unchanged=same, cgroup_only_reader_before_exit=only_reader,
        cgroup_peak_bytes=peak, cgroup_memory_events=event, streams=descriptors,
        elapsed_seconds=time.monotonic()-start)
    retain('terminal.original.json', state)
    print(json.dumps({'status': state['status'], 'reason': reason, 'exit_code': state['exit_code'],
                      'exact_three_tests_passed': exact, 'cgroup_peak_bytes': peak}), flush=True)
    return 0 if passed else 1


if __name__ == '__main__':
    raise SystemExit(main())
