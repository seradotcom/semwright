"""Release metadata validation, using disposable fixtures, never release evidence."""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
GATES = json.loads((ROOT / 'release-readiness.json').read_text())['gates']


class ReleaseAdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='semwright-admission-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.script = self.root / 'scripts/release/assert-ready.py'
        self.script.parent.mkdir(parents=True)
        shutil.copyfile(ROOT / 'scripts/release/assert-ready.py', self.script)
        # Synthetic test metadata. This is not Semwright's real resolved lockfile.
        (self.root / 'Cargo.lock').write_text(
            'version = 4\n[[package]]\nname = "test-only-fixture"\nversion = "0.0.0"\n'
        )
        (self.root / 'rust-toolchain.toml').write_text('[toolchain]\nchannel = "1.97.0"\n')
        self.document = {'status': 'READY_FOR_RELEASE_VALIDATION',
                         'gates': {name: True for name in GATES}}

    def invoke(self, document=None, raw=None):
        path = self.root / 'release-readiness.json'
        path.write_text(raw if raw is not None else json.dumps(
            self.document if document is None else document))
        return subprocess.run([sys.executable, "-I", "-S", str(self.script)], cwd=self.root,
                              capture_output=True, text=True, timeout=5)

    def assert_blocked(self, result):
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn('Release blocked:', result.stderr)
        self.assertNotIn('Traceback', result.stderr)

    def test_complete_synthetic_metadata_is_only_admission_not_certification(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('CI must still run', result.stdout)

    def test_empty_gate_set_is_rejected(self):
        self.document['gates'] = {}
        self.assert_blocked(self.invoke())

    def test_each_required_gate_cannot_be_omitted(self):
        for name in GATES:
            with self.subTest(name=name):
                doc = {'status': self.document['status'], 'gates': dict(self.document['gates'])}
                del doc['gates'][name]
                self.assert_blocked(self.invoke(doc))

    def test_unknown_gate_cannot_change_contract(self):
        self.document['gates']['invented_gate'] = True
        self.assert_blocked(self.invoke())

    def test_false_and_non_boolean_gate_values_are_rejected(self):
        name = next(iter(GATES))
        for value in (False, 1, 'true', None, [], {}):
            with self.subTest(value=value):
                self.document['gates'][name] = value
                self.assert_blocked(self.invoke())

    def test_blocked_status_cannot_be_accepted(self):
        self.document['status'] = 'BLOCKED_DEVELOPMENT_SOURCE'
        self.assert_blocked(self.invoke())

    def test_malformed_documents_fail_without_traceback(self):
        for raw in ('{', 'null', '[]', '{"gates": []}', '{"status": "READY_FOR_RELEASE_VALIDATION"}'):
            with self.subTest(raw=raw):
                self.assert_blocked(self.invoke(raw=raw))

    def test_duplicate_json_keys_are_rejected(self):
        gates = json.dumps(self.document['gates'])
        raw = '{"status":"READY_FOR_RELEASE_VALIDATION","gates":{},"gates":' + gates + '}'
        self.assert_blocked(self.invoke(raw=raw))

    def test_lockfile_is_required(self):
        (self.root / 'Cargo.lock').unlink()
        self.assert_blocked(self.invoke())

    def test_empty_or_malformed_lockfile_is_rejected(self):
        for content in ('', 'not TOML {', 'version=4\n'):
            with self.subTest(content=content):
                (self.root / 'Cargo.lock').write_text(content)
                self.assert_blocked(self.invoke())

    def test_lockfile_symlink_is_rejected(self):
        path = self.root / 'Cargo.lock'
        path.rename(self.root / 'actual.lock')
        path.symlink_to(self.root / 'actual.lock')
        self.assert_blocked(self.invoke())

    def test_floating_toolchain_is_rejected(self):
        for channel in ('stable', 'nightly', '1.97', 'beta'):
            with self.subTest(channel=channel):
                (self.root / 'rust-toolchain.toml').write_text(f'[toolchain]\nchannel="{channel}"\n')
                self.assert_blocked(self.invoke())

    def test_oversized_readiness_is_rejected(self):
        self.assert_blocked(self.invoke(raw=' ' * 131073 + json.dumps(self.document)))

    def test_actual_development_checkout_remains_blocked(self):
        result = subprocess.run([sys.executable, '-I', '-S', str(ROOT / 'scripts/release/assert-ready.py')],
                                capture_output=True, text=True, timeout=5)
        self.assert_blocked(result)


if __name__ == '__main__':
    unittest.main()
