"""Explicit job-level disposition; never relabel the failed original global run."""
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import zipfile

SOURCE = 'cd518748f742025a251b78028613aa1b16919e73'
ORIGINAL = 37074828959
FAILED_JOB = 111090595445
RETEST = 37085834067
RETEST_SUITE = '27924ecf3bba369e0560c6685693bac3aa1dcdc3'
REPO = 'seradotcom/semwright'
HERE = Path(__file__).resolve().parent


def api(path, raw=False):
    data = subprocess.check_output(['gh', 'api', f'repos/{REPO}/{path}'])
    return data if raw else json.loads(data)


def main():
    assert os.environ.get('GITHUB_ACTIONS') == 'true' and os.environ.get('RUNNER_ENVIRONMENT') == 'github-hosted'
    assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip() == SOURCE
    assert not subprocess.check_output(['git', 'status', '--porcelain', '--untracked-files=no'], text=True).strip()
    original = api(f'actions/runs/{ORIGINAL}')
    assert original['head_sha'] == SOURCE and original['status'] == 'completed' and original['conclusion'] == 'failure'
    page = api(f'actions/runs/{ORIGINAL}/jobs?per_page=100')
    jobs = page['jobs']
    assert len(jobs) == page['total_count']
    failures = [j for j in jobs if j['conclusion'] == 'failure']
    assert len(failures) == 1 and failures[0]['id'] == FAILED_JOB
    assert sum(j['conclusion'] == 'success' for j in jobs) == 62
    skipped = sorted(j['name'] for j in jobs if j['conclusion'] == 'skipped')
    assert skipped == sorted(['motion / domain', 'motion / portable-compile', 'motion / real-render-and-host', 'motion / fuzz', 'figma / fuzz', 'figma / authoring'])
    assert all(j['conclusion'] in ['success', 'skipped', 'failure'] for j in jobs)

    retest = api(f'actions/runs/{RETEST}')
    assert retest['head_sha'] == RETEST_SUITE and retest['status'] == 'completed' and retest['conclusion'] == 'success'
    retest_jobs = api(f'actions/runs/{RETEST}/jobs')['jobs']
    assert len(retest_jobs) == 1 and retest_jobs[0]['conclusion'] == 'success'
    assert retest_jobs[0]['labels'] == failures[0]['labels'] == ['windows-11-arm']
    steps = {s['name']: s for s in retest_jobs[0]['steps']}
    assert all(s['conclusion'] == 'success' for s in retest_jobs[0]['steps'])
    required = {s['name'] for s in failures[0]['steps']}
    assert required <= set(steps), ('The retest omitted original checks', sorted(required - set(steps)))
    assert 'Exact original UIA fixture with fresh readback, then fail fast' in steps
    diff = api(f'compare/{SOURCE}...{RETEST_SUITE}')
    assert {f['filename'] for f in diff['files']} == {
        '.github/workflows/semantic-creation-integration.yml',
        'integration-lab/i-windows-arm64-cd51874/uia_native_fixture.rs'}

    artifacts = api(f'actions/runs/{RETEST}/artifacts')['artifacts']
    assert len(artifacts) == 1 and artifacts[0]['id'] == 11261300160
    raw = api(f'actions/artifacts/{artifacts[0]["id"]}/zip', True)
    digest = hashlib.sha256(raw).hexdigest()
    assert artifacts[0]['digest'] == 'sha256:' + digest
    archive = zipfile.ZipFile(io.BytesIO(raw))
    identity = json.loads(archive.read('identity.json'))
    assert identity['source_sha'] == SOURCE and identity['suite_sha'] == RETEST_SUITE
    assert identity['run_id'] == str(RETEST)
    assert identity['production_source_modified'] is False and identity['interactive_windows_certified'] is False
    assert 'test result: ok. 1 passed; 0 failed; 0 ignored;' in archive.read('uia-freshness.log').decode()

    reports = {}
    for path in sorted((HERE / 'evidence').glob('*.json')):
        content = path.read_bytes()
        report = json.loads(content)
        assert report['source_sha'] == SOURCE, path.name
        run = api(f'actions/runs/{report["run_id"]}')
        assert run['status'] == 'completed' and run['conclusion'] == 'success', path.name
        if path.name == 'MAINTAINER_PRECHECK_cd51874.json':
            assert run['head_sha'] == '8b61cb360e767cd008504d3e65bed53b618cf367'
        else:
            assert run['head_sha'] == report.get('suite_sha', SOURCE), (path.name, run['head_sha'])
            assert report.get('conclusion', report.get('outcome')) in ['success', 'PASS'], path.name
        reports[path.name] = {'run_id': report['run_id'], 'suite_sha': run['head_sha'], 'sha256': hashlib.sha256(content).hexdigest()}
    assert len(reports) == 14, ('Missing required certificates', list(reports))
    assert json.loads((HERE / 'evidence/I_JOINT_CHAIN_VERIFIED_cd51874.json').read_text())['clean_chains_verified'] == 2
    assert json.loads((HERE / 'evidence/G_FINAL_INTEGRATED_CAMPAIGN_37074787747.json').read_text())['source_sha'] == SOURCE

    out = Path(os.environ['GITHUB_WORKSPACE']) / 'verification/global-disposition'
    out.mkdir(parents=True, exist_ok=True)
    result = {'schema_version': 1, 'source_sha': SOURCE, 'suite_sha': os.environ['GITHUB_SHA'],
              'run_id': int(os.environ['GITHUB_RUN_ID']), 'conclusion': 'success',
              'status': 'PASS_EXPLICIT_JOB_DISPOSITION_WITH_CORRECTED_TEST_FIXTURE',
              'original_global_run_id': ORIGINAL, 'original_global_conclusion': 'failure',
              'original_workflow_pass_claimed': False, 'original_jobs_passed': 62,
              'original_jobs_skipped': skipped, 'skips_count_as_executed': False,
              'original_failed_job': FAILED_JOB, 'corrected_job_retest_run_id': RETEST,
              'corrected_job_retest_suite_sha': RETEST_SUITE, 'corrected_job_artifact_digest': 'sha256:' + digest,
              'original_required_step_names_preserved': sorted(required), 'retest_job_id': retest_jobs[0]['id'],
              'production_source_modified': False, 'test_fixture_correction_in_product_tree': False,
              'fixture_disposition': 'Native UIA fixture reacquires its snapshot after the bounded placement search; production StaleReference checks and every original job check remain enforced. Review packet must carry the corrected external fixture and reproduction instructions.',
              'exact_source_certificates': reports, 'interactive_windows_certified': False,
              'native_scope': 'Scoped owner/native controls; no live Figma authoring or interactive Windows acceptance',
              'r16_closed': False, 'main_merge_performed': False}
    (out / 'I_GLOBAL_cd51874.json').write_text(json.dumps(result, indent=2) + '\n')
    (out / 'original-global-jobs.json').write_text(json.dumps(jobs, indent=2) + '\n')
    (out / 'corrected-arm64-job.json').write_text(json.dumps(retest_jobs, indent=2) + '\n')
    print(result['status'])


if __name__ == '__main__':
    main()
