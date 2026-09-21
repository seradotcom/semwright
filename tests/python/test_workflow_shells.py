"""Real shell regressions and workflow contracts; not executed Rust fuzzing."""
from pathlib import Path
import subprocess
import tempfile
import unittest

import yaml

ROOT = Path(__file__).resolve().parents[2]


class WorkflowShellTests(unittest.TestCase):
    def run_fixture(self, *, strict, producer_exit):
        with tempfile.TemporaryDirectory(prefix='semwright-pipeline-test-') as directory:
            root = Path(directory)
            script = root / 'fixture.sh'
            script.write_text(
                f"(printf 'fixture producer exit {producer_exit}\\n'; exit {producer_exit}) | tee captured.log\n"
                "printf 'after pipeline\\n' > continued.txt\n"
            )
            shell = ['bash', '--noprofile', '--norc', '-eo', 'pipefail'] if strict else ['bash', '-e']
            result = subprocess.run([*shell, str(script)], cwd=root, capture_output=True, timeout=3)
            return result.returncode, (root / 'captured.log').read_text(), (root / 'continued.txt').exists()

    def test_unspecified_github_shell_can_mask_a_failed_producer(self):
        # Counterexample for the documented default: bash -e {0}.
        code, log, continued = self.run_fixture(strict=False, producer_exit=7)
        self.assertEqual(code, 0)
        self.assertIn('exit 7', log)
        self.assertTrue(continued)

    def test_explicit_bash_propagates_producer_failure_and_keeps_log(self):
        code, log, continued = self.run_fixture(strict=True, producer_exit=7)
        self.assertEqual(code, 7)
        self.assertIn('exit 7', log)
        self.assertFalse(continued)

    def test_explicit_bash_keeps_successful_pipelines_working(self):
        code, log, continued = self.run_fixture(strict=True, producer_exit=0)
        self.assertEqual(code, 0)
        self.assertIn('exit 0', log)
        self.assertTrue(continued)

    def test_linux_workflow_run_steps_use_explicit_bash(self):
        for path in sorted((ROOT / '.github/workflows').glob('*.yml')):
            workflow = yaml.safe_load(path.read_text())
            workflow_shell = workflow.get('defaults', {}).get('run', {}).get('shell')
            for name, job in workflow['jobs'].items():
                job_shell = job.get('defaults', {}).get('run', {}).get('shell', workflow_shell)
                for index, step in enumerate(job.get('steps', [])):
                    if 'run' not in step:
                        continue
                    with self.subTest(workflow=path.name, job=name, step=index):
                        self.assertEqual(step.get('shell', job_shell), 'bash')

    def test_required_workflow_jobs_do_not_hide_failures(self):
        for path in sorted((ROOT / '.github/workflows').glob('*.yml')):
            workflow = yaml.safe_load(path.read_text())
            for name, job in workflow['jobs'].items():
                with self.subTest(workflow=path.name, job=name):
                    self.assertIs(job.get('continue-on-error', False), False)
                    for step in job.get('steps', []):
                        self.assertIs(step.get('continue-on-error', False), False)


if __name__ == '__main__':
    unittest.main()
