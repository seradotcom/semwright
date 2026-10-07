"""Synthetic metadata/authorization fixtures; never a Semwright security attestation."""
import copy
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
# Independent policy expectation: do not derive required gates from the submitted document.
GATES = ('rust_build_and_tests', 'reviewed_lockfile', 'pinned_toolchain', 'clippy_and_fmt',
         'dependency_audit_and_licenses', 'rust_broker_integration', 'plugin_sandbox_negative_tests',
         'application_adapter_validation', 'release_packaging_validation', 'security_review')
DEFERRED = 'DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT'


class ReleaseAdmissionTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='semwright-admission-test-')
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.script = self.root / 'scripts/release/assert-ready.py'
        self.script.parent.mkdir(parents=True)
        shutil.copyfile(ROOT / 'scripts/release/assert-ready.py', self.script)
        (self.root / 'Cargo.lock').write_text('version = 4\n[[package]]\nname="fixture"\nversion="0.0.0"\n')
        (self.root / 'rust-toolchain.toml').write_text('[toolchain]\nchannel="1.98.1"\n')
        subprocess.run(['git', 'init', '-q', str(self.root)], check=True, capture_output=True)
        subprocess.run(['git', '-C', str(self.root), '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid',
                        '-c', 'commit.gpgsign=false', 'commit', '--allow-empty', '-qm', 'synthetic fixture'],
                       check=True, capture_output=True)
        self.sha = subprocess.check_output(['git', '-C', str(self.root), 'rev-parse', 'HEAD'], text=True).strip()
        self.document = {'schema_version': 2, 'status': 'READY_FOR_RELEASE_VALIDATION',
                         'gates': {name: True for name in GATES},
                         'post_v1_certification': {name: DEFERRED for name in ('live_desktop_matrix', 'windows_interactive')}}
        self.review = {'reviewed_sha': self.sha, 'independent': True, 'reviewer': 'SYNTHETIC TEST ONLY',
                       'reviewed_at': '2026-01-01', 'report_reference': 'synthetic fixture, not evidence',
                       'conclusion': 'APPROVED_FOR_PUBLIC_RELEASE', 'unresolved_blocking_findings': [],
                       'areas': [f'R16-{i:02}' for i in range(1, 13)]}
        self.manifest = {'source_sha': self.sha, 'status': 'PASS', 'release_admission': False,
                         'package_count': 8, 'packages': [{} for _ in range(8)],
                         'verified_platforms': ['linux-x86_64', 'linux-aarch64', 'windows-x86_64',
                                                'windows-arm64', 'macos-arm64', 'macos-x86_64'],
                         'install_uninstall_verified': True}

    def invoke(self, document=None, raw=None, mode='publish', approved=True, records=True, candidate=None):
        (self.root / 'release-readiness.json').write_text(raw if raw is not None else json.dumps(
            self.document if document is None else document))
        args = [sys.executable, '-I', '-S', str(self.script), '--mode', mode,
                '--candidate-sha', candidate or self.sha]
        if approved:
            args.append('--maintainer-approved')
        if records:
            (self.root / 'review.json').write_text(json.dumps(self.review))
            (self.root / 'manifest.json').write_text(json.dumps(self.manifest))
            args += ['--review-report', str(self.root / 'review.json'),
                     '--distribution-manifest', str(self.root / 'manifest.json')]
        return subprocess.run(args, cwd=self.root, capture_output=True, text=True, timeout=5)

    def assert_blocked(self, result):
        self.assertEqual(result.returncode, 2, result.stdout + result.stderr)
        self.assertIn('Release blocked:', result.stderr)
        self.assertNotIn('Traceback', result.stderr)

    def test_complete_synthetic_records_only_check_binding_not_reviewer_authenticity(self):
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('cannot establish reviewer authenticity', result.stdout)

    def test_staging_accepts_pending_security_but_never_authorizes_publication(self):
        self.document['status'] = 'BLOCKED_PENDING_SECURITY_REVIEW'
        self.document['gates']['security_review'] = False
        result = self.invoke(mode='staging', approved=False, records=False)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('NOT PUBLIC RELEASE AUTHORIZATION', result.stdout)
        self.assert_blocked(self.invoke(records=False))

    def test_external_review_satisfies_pending_security_without_changing_source_sha(self):
        self.document['status'] = 'BLOCKED_PENDING_SECURITY_REVIEW'
        self.document['gates']['security_review'] = False
        (self.root / 'release-readiness.json').write_text(json.dumps(self.document))
        subprocess.run(['git', '-C', str(self.root), 'add', 'release-readiness.json'], check=True)
        subprocess.run(['git', '-C', str(self.root), '-c', 'user.name=Fixture',
                        '-c', 'user.email=fixture@example.invalid', '-c', 'commit.gpgsign=false',
                        'commit', '-qm', 'immutable pending candidate'], check=True)
        self.sha = subprocess.check_output(
            ['git', '-C', str(self.root), 'rev-parse', 'HEAD'], text=True).strip()
        self.review['reviewed_sha'] = self.sha
        self.manifest['source_sha'] = self.sha
        result = self.invoke()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(subprocess.check_output(
            ['git', '-C', str(self.root), 'rev-parse', 'HEAD'], text=True).strip(), self.sha)
        self.assert_blocked(self.invoke(approved=False))
        self.assert_blocked(self.invoke(records=False))
        self.review['reviewed_sha'] = 'b' * 40
        self.assert_blocked(self.invoke())

    def test_pending_source_cannot_bypass_independent_review_requirements(self):
        self.document['status'] = 'BLOCKED_PENDING_SECURITY_REVIEW'
        self.document['gates']['security_review'] = False
        for key, value in [('independent', False), ('reviewer', ''), ('areas', []),
                           ('conclusion', 'UNREVIEWED'), ('reviewed_at', '2999-01-01'),
                           ('unresolved_blocking_findings', ['OPEN'])]:
            previous = self.review[key]
            self.review[key] = value
            self.assert_blocked(self.invoke())
            self.review[key] = previous

    def test_ready_and_pending_status_must_agree_with_security_boolean_in_both_modes(self):
        for status, security in [('READY_FOR_RELEASE_VALIDATION', False),
                                 ('BLOCKED_PENDING_SECURITY_REVIEW', True)]:
            self.document['status'] = status
            self.document['gates']['security_review'] = security
            for mode in ('staging', 'publish'):
                self.assert_blocked(self.invoke(mode=mode))

    def test_empty_gate_set_is_rejected_in_both_modes(self):
        self.document['gates'] = {}
        for mode in ('staging', 'publish'):
            self.assert_blocked(self.invoke(mode=mode))

    def test_each_required_gate_cannot_be_omitted_even_in_staging(self):
        for name in GATES:
            doc = copy.deepcopy(self.document)
            del doc['gates'][name]
            for mode in ('staging', 'publish'):
                with self.subTest(name=name, mode=mode):
                    self.assert_blocked(self.invoke(doc, mode=mode))

    def test_unknown_gate_cannot_change_contract(self):
        self.document['gates']['invented_gate'] = True
        self.assert_blocked(self.invoke())

    def test_false_and_non_boolean_engineering_values_are_rejected(self):
        for value in (False, 1, 'true', None, [], {}, DEFERRED):
            self.document['gates'][GATES[0]] = value
            for mode in ('staging', 'publish'):
                self.assert_blocked(self.invoke(mode=mode))

    def test_post_v1_cannot_masquerade_as_pass_or_be_omitted(self):
        for value in ({}, None, {'live_desktop_matrix': True},
                      {'live_desktop_matrix': 'PASS', 'windows_interactive': DEFERRED},
                      {'live_desktop_matrix': DEFERRED, 'windows_interactive': False},
                      {'live_desktop_matrix': DEFERRED, 'windows_interactive': DEFERRED, 'unknown': DEFERRED}):
            self.document['post_v1_certification'] = value
            for mode in ('staging', 'publish'):
                self.assert_blocked(self.invoke(mode=mode))

    def test_pending_security_must_still_be_a_literal_boolean(self):
        self.document['status'] = 'BLOCKED_PENDING_SECURITY_REVIEW'
        for value in (0, 'false', None, DEFERRED):
            self.document['gates']['security_review'] = value
            self.assert_blocked(self.invoke(mode='staging'))

    def test_unknown_or_inconsistent_status_fails(self):
        for status in ('BLOCKED_DEVELOPMENT_SOURCE', 'PASS', [], None):
            self.document['status'] = status
            self.assert_blocked(self.invoke(mode='staging'))

    def test_malformed_documents_fail_without_traceback(self):
        for raw in ('{', 'null', '[]', '{"gates": []}', '{"status": "READY_FOR_RELEASE_VALIDATION"}'):
            self.assert_blocked(self.invoke(raw=raw))

    def test_duplicate_json_keys_are_rejected(self):
        raw = json.dumps(self.document).replace('"schema_version": 2', '"schema_version": 2, "schema_version": 2')
        self.assert_blocked(self.invoke(raw=raw))

    def test_lockfile_is_required(self):
        (self.root / 'Cargo.lock').unlink()
        self.assert_blocked(self.invoke())

    def test_empty_or_malformed_lockfile_is_rejected(self):
        for content in ('', 'not TOML {', 'version=4\n'):
            (self.root / 'Cargo.lock').write_text(content)
            self.assert_blocked(self.invoke())

    def test_lockfile_symlink_is_rejected(self):
        path = self.root / 'Cargo.lock'
        path.rename(self.root / 'actual.lock')
        path.symlink_to(self.root / 'actual.lock')
        self.assert_blocked(self.invoke())

    def test_floating_toolchain_is_rejected(self):
        for channel in ('stable', 'nightly', '1.98', 'beta'):
            (self.root / 'rust-toolchain.toml').write_text(f'[toolchain]\nchannel="{channel}"\n')
            self.assert_blocked(self.invoke())

    def test_oversized_readiness_is_rejected(self):
        self.assert_blocked(self.invoke(raw=' ' * 131073 + json.dumps(self.document)))

    def test_publication_requires_explicit_maintainer_approval(self):
        self.assert_blocked(self.invoke(approved=False))

    def test_publication_requires_external_review_and_distribution_records(self):
        self.assert_blocked(self.invoke(records=False))

    def test_publication_rejects_stale_candidate(self):
        self.assert_blocked(self.invoke(candidate='a' * 40))

    def test_security_report_requires_all_independent_exact_sha_fields(self):
        for key, value in [('reviewed_sha', 'b' * 40), ('independent', False), ('reviewer', ''),
                           ('reviewed_at', '2999-01-01'), ('report_reference', ''), ('areas', []),
                           ('unresolved_blocking_findings', ['OPEN']), ('conclusion', 'PENDING')]:
            previous = self.review[key]
            self.review[key] = value
            self.assert_blocked(self.invoke())
            self.review[key] = previous

    def test_final_validation_requires_exact_sha_and_all_platform_installations(self):
        for key, value in [('source_sha', 'b' * 40), ('status', 'PENDING'), ('package_count', 7),
                           ('install_uninstall_verified', False), ('verified_platforms', ['linux-x86_64'])]:
            previous = self.manifest[key]
            self.manifest[key] = value
            self.assert_blocked(self.invoke())
            self.manifest[key] = previous

    def test_actual_checkout_stages_but_remains_publication_blocked(self):
        for mode, expected in [('staging', 0), ('publish', 2)]:
            result = subprocess.run([sys.executable, '-I', '-S', str(ROOT / 'scripts/release/assert-ready.py'),
                                     '--mode', mode], capture_output=True, text=True, timeout=5)
            self.assertEqual(result.returncode, expected, result.stdout + result.stderr)


class PublicationWorkflowTests(unittest.TestCase):
    def test_publication_keeps_external_review_bound_to_frozen_source(self):
        text = (ROOT / '.github/workflows/release.yml').read_text()
        for required in (
            "inputs.publish == true", "startsWith(github.ref, 'refs/tags/v')",
            'test "$CANDIDATE_SHA" = "$(git rev-parse HEAD)"',
            '--json headSha --jq .headSha)" = "$CANDIDATE_SHA"',
            '--json conclusion --jq .conclusion)" = success',
            '-n independent-security-review', '--mode publish',
            '--candidate-sha "$CANDIDATE_SHA" --maintainer-approved',
            '--review-report review/security-review.json',
            '--distribution-manifest validated-manifest/V1_DISTRIBUTION_MANIFEST.json',
            'gh release view "$GITHUB_REF_NAME"',
        ):
            self.assertIn(required, text)
        self.assertNotIn('git commit', text)
        self.assertNotIn('security_review=true', text)


if __name__ == '__main__':
    unittest.main()
