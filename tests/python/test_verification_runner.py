"""Executed process/report contracts for the verification runner, not Rust tests."""
from __future__ import annotations

import ast
import hashlib
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

ROOT = Path(__file__).resolve().parents[2]
SPEC = importlib.util.spec_from_file_location('semwright_verification_tested', ROOT / 'scripts/verification.py')
assert SPEC is not None and SPEC.loader is not None
VERIFICATION = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(VERIFICATION)


class VerificationRunnerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='semwright-runner-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.output = self.root / 'evidence'

    def run_python(self, code, **kwargs):
        return VERIFICATION.run_check(self.root, self.output, 'fixture',
                                      [sys.executable, '-I', '-S', '-c', code], **kwargs)

    def test_success_retains_exit_output_and_hash(self):
        result = self.run_python('print("fixture ok")')
        self.assertEqual(result['status'], 'PASS')
        self.assertEqual(result['exit_code'], 0)
        body = (self.output / 'fixture.log').read_bytes()
        self.assertEqual(body, b'fixture ok\n')
        self.assertEqual(result['log_sha256'], hashlib.sha256(body).hexdigest())
        self.assertEqual(result['log_bytes'], len(body))

    def test_failure_is_not_relabelled_as_blocked(self):
        result = self.run_python('import sys; print("failure"); sys.exit(7)')
        self.assertEqual(result['status'], 'FAIL')
        self.assertEqual(result['exit_code'], 7)

    def test_missing_executable_has_a_real_log(self):
        result = VERIFICATION.run_check(self.root, self.output, 'missing', ['/does-not-exist/semwright-test'])
        self.assertEqual(result['status'], 'BLOCKED')
        self.assertIsNone(result['exit_code'])
        self.assertTrue((self.output / 'missing.log').is_file())
        self.assertGreater(result['log_bytes'], 0)

    def test_missing_subcommand_dependency_is_blocked(self):
        result = self.run_python('print("must not run")', required_executables=['/missing/fixture-tool'])
        self.assertEqual(result['status'], 'BLOCKED')
        self.assertNotIn('must not run', (self.output / 'fixture.log').read_text())

    def test_timeout_is_bounded_and_reaped(self):
        started = time.monotonic()
        result = self.run_python('import time; time.sleep(30)', timeout_seconds=0.2)
        self.assertEqual(result['status'], 'FAIL')
        self.assertTrue(result['reason'].startswith('TIMEOUT:'))
        self.assertLess(time.monotonic() - started, 3)
        self.assertIsNotNone(result['exit_code'])

    def test_output_limit_is_enforced_in_actual_capture(self):
        result = self.run_python('import os\nwhile True: os.write(1, b"x" * 65536)', max_log_bytes=1024)
        self.assertEqual(result['status'], 'FAIL')
        self.assertTrue(result['reason'].startswith('OUTPUT_LIMIT:'))
        self.assertEqual((self.output / 'fixture.log').stat().st_size, 1024)

    def test_reusing_a_log_path_cannot_erase_previous_evidence(self):
        self.run_python('print("first")')
        with self.assertRaises(FileExistsError):
            self.run_python('print("second")')
        self.assertEqual((self.output / 'fixture.log').read_text(), 'first\n')

    def test_invalid_name_and_budgets_are_rejected(self):
        for name in ('../outside', '', 'name/escape'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                VERIFICATION.run_check(self.root, self.output, name, [sys.executable])
        with self.assertRaises(ValueError):
            self.run_python('pass', timeout_seconds=0)

    def test_non_finite_time_budgets_are_rejected(self):
        for budget in (float('nan'), float('inf'), float('-inf')):
            with self.subTest(budget=budget), self.assertRaises(ValueError):
                self.run_python('pass', timeout_seconds=budget)

    def test_boolean_time_budgets_are_rejected(self):
        for budget in (True, False):
            with self.subTest(budget=budget), self.assertRaises(ValueError):
                self.run_python('pass', timeout_seconds=budget)

    def test_output_budgets_are_positive_integers(self):
        for budget in (0, -1, True, False, 1.5, float('nan')):
            with self.subTest(budget=budget), self.assertRaises(ValueError):
                self.run_python('pass', max_log_bytes=budget)

    @staticmethod
    def active(pid):
        path = Path(f'/proc/{pid}/stat')
        if not path.exists():
            return False
        # A zombie is no longer an executing descendant; init may reap it later.
        return path.read_text().rsplit(')', 1)[1].split()[0] not in ('Z', 'X')

    def test_timeout_stops_child_in_the_owned_process_group(self):
        pidfile = self.root / 'child.pid'
        code = ("import subprocess,sys,time\n"
                "from pathlib import Path\n"
                "child=subprocess.Popen([sys.executable,'-I','-S','-c','import time; time.sleep(30)'])\n"
                f"Path({str(pidfile)!r}).write_text(str(child.pid))\n"
                "time.sleep(30)\n")
        result = self.run_python(code, timeout_seconds=0.5)
        self.assertEqual(result['status'], 'FAIL')
        self.assertTrue(pidfile.exists())
        pid = int(pidfile.read_text())
        deadline = time.monotonic() + 1
        while self.active(pid) and time.monotonic() < deadline:
            time.sleep(0.01)
        self.assertFalse(self.active(pid), f'owned child still active: {pid}')

    def test_descendant_holding_output_after_parent_exit_is_not_a_pass(self):
        code = ("import subprocess,sys\n"
                "subprocess.Popen([sys.executable,'-I','-S','-c','import time; time.sleep(30)'])\n")
        result = self.run_python(code, timeout_seconds=2)
        self.assertEqual(result['status'], 'FAIL')
        self.assertTrue(result['reason'].startswith('DESCENDANT_PIPE:'))

    def test_report_persists_each_check_and_keeps_unexecuted_work_explicit(self):
        report = VERIFICATION.Report(self.output, ['done', 'pending'], source_root=self.root)
        report.record({'name': 'done', 'status': 'PASS', 'exit_code': 0})
        intermediate = json.loads((self.output / 'summary.json').read_text())
        self.assertEqual(intermediate['overall'], 'RUNNING')
        self.assertEqual(intermediate['results'][1]['status'], 'NOT_RUN')
        self.assertFalse(report.finish())
        self.assertEqual(json.loads((self.output / 'summary.json').read_text())['overall'], 'INCOMPLETE_OR_FAILED')

    def test_report_cannot_be_overwritten_or_silently_rerun(self):
        report = VERIFICATION.Report(self.output, ['only'], source_root=self.root)
        with self.assertRaises(FileExistsError):
            VERIFICATION.Report(self.output, ['only'], source_root=self.root)
        report.record({'name': 'only', 'status': 'PASS', 'exit_code': 0})
        with self.assertRaises(ValueError):
            report.record({'name': 'only', 'status': 'FAIL', 'exit_code': 1})
        self.assertTrue(report.finish())

    def test_interruption_cannot_report_pass(self):
        report = VERIFICATION.Report(self.output, ['only'], source_root=self.root)
        report.record({'name': 'only', 'status': 'PASS', 'exit_code': 0})
        self.assertFalse(report.finish(interrupted=True))

    def test_report_rejects_duplicate_check_names(self):
        with self.assertRaises(ValueError):
            VERIFICATION.Report(self.output, ['same', 'same'], source_root=self.root)

    def test_report_rejects_empty_plans(self):
        with self.assertRaises(ValueError):
            VERIFICATION.Report(self.output, [], source_root=self.root)

    def test_report_rejects_invalid_names(self):
        for name in ('../outside', '', 'bad/name', 'UPPER'):
            with self.subTest(name=name), self.assertRaises(ValueError):
                VERIFICATION.Report(self.output, [name], source_root=self.root)

    def test_report_rejects_pass_without_a_real_success_exit(self):
        report = VERIFICATION.Report(self.output, ['only'], source_root=self.root)
        for code in (None, True, False, 7, '0'):
            with self.subTest(code=code), self.assertRaises(ValueError):
                report.record({'name': 'only', 'status': 'PASS', 'exit_code': code})
        self.assertEqual(report.document['results'][0]['status'], 'NOT_RUN')

    def test_report_rejects_unknown_or_unexecuted_result_status(self):
        report = VERIFICATION.Report(self.output, ['only'], source_root=self.root)
        for status in ('SUCCESS', None, 'NOT_RUN'):
            with self.subTest(status=status), self.assertRaises(ValueError):
                report.record({'name': 'only', 'status': status, 'exit_code': None})
        self.assertEqual(report.document['results'][0]['status'], 'NOT_RUN')

    def test_rust_plan_keeps_workspace_features_build_and_doctests(self):
        # Verify only the runner's command contract. This does not execute Rust.
        tree = ast.parse((ROOT / 'scripts/verify-local.py').read_text())
        assignment = next(node for node in tree.body if isinstance(node, ast.Assign)
                          and any(isinstance(t, ast.Name) and t.id == 'RUST_CHECKS'
                                  for t in node.targets))
        checks = {name: argv for name, argv, _tools in ast.literal_eval(assignment.value)}
        for name in ('check', 'build', 'clippy', 'rust-tests'):
            with self.subTest(name=name):
                for flag in ('--locked', '--workspace', '--all-targets', '--all-features'):
                    self.assertIn(flag, checks[name])
        self.assertEqual(checks['clippy'][-3:], ('--', '-D', 'warnings'))
        self.assertIn('--doc', checks['doctests'])
        self.assertIn('--all-features', checks['doctests'])
        self.assertNotIn('--all-targets', checks['doctests'])
        self.assertIn('--release', checks['release'])
        self.assertEqual(checks['audit'], ('cargo', 'audit', '--deny', 'warnings'))

    def test_cli_rejects_non_finite_budget_before_creating_evidence(self):
        result = subprocess.run(
            [sys.executable, '-S', str(ROOT / 'scripts/verify-local.py'),
             '--timeout-seconds', 'nan', '--output', str(self.output)],
            capture_output=True, text=True, timeout=5,
        )
        self.assertEqual(result.returncode, 2)
        self.assertIn('finite', result.stderr)
        self.assertFalse(self.output.exists())


if __name__ == '__main__':
    unittest.main()
