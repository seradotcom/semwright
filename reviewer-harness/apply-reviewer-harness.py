#!/usr/bin/env python3
"""Overlay exclusively reviewer test sources; preserve and record the production tree identity."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument('--source', type=Path, required=True)
parser.add_argument('--reviewed-sha', required=True)
parser.add_argument('--output', type=Path, required=True)
args = parser.parse_args()
source = args.source.resolve(strict=True)
harness = Path(__file__).resolve().parent
def git(*parts):
    return subprocess.check_output(['git', '-C', str(source), *parts], text=True).strip()
if git('rev-parse', 'HEAD') != args.reviewed_sha or git('status', '--porcelain'):
    raise SystemExit('Exact clean reviewed source required before overlay')
digest = lambda data: hashlib.sha256(data).hexdigest()
portal_path = source/'crates/platform-linux/src/portal_fixture_tests.rs'
before_portal = portal_path.read_bytes()
audit_target = source/'crates/core/tests/reviewer_audit_faults.rs'
if audit_target.exists():
    raise SystemExit('Reviewer test filename already exists')
audit_target.write_bytes((harness/'reviewer_audit_faults.rs').read_bytes())
portal_path.write_bytes(before_portal + b'\n' + (harness/'reviewer_portal_revocation.rs').read_bytes())
record = {
    'reviewed_sha':args.reviewed_sha,
    'reviewed_tree':git('rev-parse', 'HEAD^{tree}'),
    'classification':'REVIEWER_DEFENSIVE_TEST_OVERLAY_ONLY_NOT_PRODUCTION_SOURCE',
    'harness_files':{p.name:digest(p.read_bytes()) for p in sorted(harness.iterdir()) if p.is_file()},
    'portal_fixture_before_sha256':digest(before_portal),
    'portal_fixture_overlay_sha256':digest(portal_path.read_bytes()),
    'overlay_targets':['crates/core/tests/reviewer_audit_faults.rs','crates/platform-linux/src/portal_fixture_tests.rs'],
    'tracked_diff':git('diff', '--stat'),
}
args.output.mkdir(parents=True, exist_ok=True)
(args.output/'HARNESS_BINDING.json').write_text(json.dumps(record, indent=2, sort_keys=True)+'\n')
print(json.dumps(record, indent=2, sort_keys=True))
