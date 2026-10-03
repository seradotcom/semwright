"""Small synthetic packaging fixtures, not native application certification."""
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'scripts/release'))
from bundle_contract import BINS, add_bundle_files, verify_checksums, write_checksums  # noqa: E402
from certify_install import certify_install  # noqa: E402
from distribution_manifest import EXPECTED, INSTALL_CHECKS, build_manifest  # noqa: E402


class BundleContractTests(unittest.TestCase):
    def make_bundle(self, root):
        root.mkdir()
        (root / 'bin').mkdir()
        for name in BINS:
            target = root / 'bin' / name
            if name == 'semwright':
                target.write_text('''#!/bin/sh
set -eu
if [ "${1:-}" = "--dry-run" ] && [ "${2:-}" = "--json" ] && [ "${3:-}" = "setup" ]; then
  printf '%s\\n' '{"setup":"dry-run","authority_changed":false,"service_started":false,"permissions_granted":false,"config":{"status":"would-create"},"mcp_client_snippet":{"status":"would-create"}}'
  exit 0
fi
if [ "${1:-}" = "--json" ] && [ "${2:-}" = "setup" ]; then
  D="$HOME/.config/semwright"; C="$D/daemon.toml"; M="$D/mcp-client.json"
  if [ -f "$C" ]; then CS=kept-existing; else mkdir -p "$D"; chmod 700 "$D"; printf '# fixture\\n[policy]\\nprofile = "observe"\\n' > "$C"; chmod 600 "$C"; CS=created; fi
  if [ -f "$M" ]; then MS=kept-existing; else MCP="$(dirname "$0")/semwright-mcp"; printf '{"mcpServers":{"semwright":{"command":"%s"}}}\\n' "$MCP" > "$M"; chmod 600 "$M"; MS=created; fi
  printf '{"setup":"complete","authority_changed":false,"service_started":false,"permissions_granted":false,"config":{"status":"%s"},"mcp_client_snippet":{"status":"%s"}}\\n' "$CS" "$MS"
  exit 0
fi
printf 'synthetic fixture only\\n'
''')
            else:
                target.write_text('#!/bin/sh\nprintf "synthetic fixture only\\n"\n')
            target.chmod(0o755)
        add_bundle_files(ROOT, root, 'linux', 1_700_000_000)
        write_checksums(root, 1_700_000_000)
        return root

    def test_complete_allowlisted_payload_and_checksums(self):
        with tempfile.TemporaryDirectory() as temp:
            stage = self.make_bundle(Path(temp) / 'bundle')
            records = verify_checksums(stage)
            self.assertIn('install.sh', records)
            self.assertIn('uninstall.sh', records)
            self.assertIn('config/observe.toml', records)
            self.assertIn('LICENSE-MIT', records)
            self.assertIn('BUNDLE-CONTRACT.json', records)
            self.assertFalse((stage / 'packaging/nix').exists())
            self.assertEqual(set(BINS), {'semwright', 'semwrightd', 'semwright-mcp', 'semwright-inspect', 'semwright-sandbox'})

    def test_checksum_tamper_omission_and_duplicate_refused(self):
        with tempfile.TemporaryDirectory() as temp:
            stage = self.make_bundle(Path(temp) / 'bundle')
            manifest = stage / 'SHA256SUMS'
            original = manifest.read_text()
            first_line = original.splitlines(keepends=True)[0]
            for text in (original + first_line, original.removeprefix(first_line), 'invalid\n'):
                manifest.write_text(text)
                with self.assertRaises(ValueError):
                    verify_checksums(stage)
            manifest.write_text(original)
            (stage / 'README.md').write_text('synthetic tamper\n')
            with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
                verify_checksums(stage)

    @unittest.skipIf(os.name == 'nt' or (hasattr(os, 'getuid') and os.getuid() == 0), 'POSIX normal-user helper test')
    def test_shipped_posix_helpers_receipt_roundtrip_with_synthetic_binaries(self):
        with tempfile.TemporaryDirectory() as temp:
            stage = self.make_bundle(Path(temp) / 'bundle')
            result = certify_install(stage, 'linux')
            self.assertTrue(all(result.values()))

    def test_helpers_never_request_os_security_bypass(self):
        for path in (ROOT / 'packaging/portable').iterdir():
            body = path.read_text().lower()
            self.assertNotIn('executionpolicy bypass', body)
            self.assertNotIn('set-executionpolicy', body)
            self.assertNotIn('xattr -d', body)
            self.assertNotIn('spctl --master-disable', body)
            self.assertNotIn('remove-item -recurse', body)


class DistributionManifestTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.sha = 'a' * 40
        for identity in EXPECTED:
            platform, arch = identity.split('-', 1)
            directory = self.root / identity
            directory.mkdir()
            suffixes = ['.tar.gz', '.deb'] if platform == 'linux' else ['.zip' if platform == 'windows' else '.tar.gz']
            artifacts = {}
            for suffix in suffixes:
                name = f'synthetic-{identity}{suffix}'
                body = f'synthetic fixture for {name}'.encode()
                (directory / name).write_bytes(body)
                artifacts[name] = hashlib.sha256(body).hexdigest()
            row = {'status': 'PASS', 'source_sha': self.sha, 'platform': platform, 'arch': arch,
                   'release_admission': False, 'reproducible': True, 'internal_checksums_verified': True,
                   'binary_architecture': 'PASS', 'user_install': {key: True for key in INSTALL_CHECKS}}
            if platform == 'linux':
                row.update(artifacts=artifacts, deb_install_uninstall=True)
            else:
                name, digest = next(iter(artifacts.items()))
                row.update(artifact=name, artifact_sha256=digest)
            (directory / f'certification-{identity}.json').write_text(json.dumps(row))

    def test_all_six_exact_sha_certificates_bind_eight_package_hashes(self):
        manifest = build_manifest(self.root, self.sha)
        self.assertEqual(manifest['package_count'], 8)
        self.assertEqual(set(manifest['verified_platforms']), EXPECTED)
        self.assertTrue(manifest['install_uninstall_verified'])
        self.assertFalse(manifest['release_admission'])

    def test_missing_or_stale_or_incomplete_install_certification_is_rejected(self):
        path = next(self.root.rglob('certification-*.json'))
        original = json.loads(path.read_text())
        for key, value in [('source_sha', 'b'*40), ('user_install', {}), ('status', 'PENDING'),
                           ('internal_checksums_verified', False), ('binary_architecture', 'SKIPPED')]:
            row = dict(original, **{key: value})
            path.write_text(json.dumps(row))
            with self.assertRaises(ValueError):
                build_manifest(self.root, self.sha)
        path.unlink()
        with self.assertRaises(ValueError):
            build_manifest(self.root, self.sha)

    def test_actual_package_bytes_must_match_native_certificate(self):
        path = next(self.root.rglob('*.zip'))
        path.write_text('different synthetic bytes')
        with self.assertRaisesRegex(ValueError, 'checksum mismatch'):
            build_manifest(self.root, self.sha)

    def test_extra_uncertified_package_refused(self):
        (self.root / 'extra.zip').write_bytes(b'not certified')
        with self.assertRaises(ValueError):
            build_manifest(self.root, self.sha)


if __name__ == '__main__':
    unittest.main()
