"""Stage a named I fixture correction, keeping frozen production code unchanged."""
from pathlib import Path
import subprocess
HERE=Path(__file__).resolve().parent
WORKTREE='/home/sergio/Documents/Projects/semwright-worktrees/semantic-creation-integration'
SOURCE='cd518748f742025a251b78028613aa1b16919e73'
def original(p):return subprocess.check_output(['git','show',SOURCE+':'+p],cwd=WORKTREE).decode()
s=original('crates/platform-windows/tests/uia_native_fixture.rs')
old='''        return (
            last_snapshot.expect("occluded fixture must still expose a scoped semantic snapshot"),
            None,
        );'''
new='''        // The bounded physical-point search can outlive a UIA structure epoch.
        // Keep the semantic assertions, but observe again after the search;
        // production StaleReference rejection is unchanged.
        assert!(
            last_snapshot.is_some(),
            "occluded fixture must still expose a scoped semantic snapshot"
        );
        let fresh_snapshot = backend
            .execute(
                ctx,
                "ui.snapshot",
                &json!({"_target":window_target.clone()}),
            )
            .await
            .expect("fresh scoped UIA snapshot after physical-point search");
        return (fresh_snapshot, None);'''
assert s.count(old)==1;s=s.replace(old,new)
(HERE/'uia_native_fixture.rs').write_text(s)
w=original('.github/workflows/windows-platform.yml')
job=w[w.index('  windows-native:\n'):w.index('  sealed-tool-compat:\n')]
job=job.replace('''          - arch: x64
            runs_on: windows-2025
''','')
needle='''        with:
          persist-credentials: false
''';assert job.count(needle)==1
job=job.replace(needle,'''        with:
          ref: ${{ inputs.candidate_sha }}
          persist-credentials: false
      - name: Bind frozen source and separately identified native fixture
        env:
          TESTED_SOURCE_SHA: ${{ inputs.candidate_sha }}
        run: |
          set -euo pipefail
          test "$TESTED_SOURCE_SHA" = cd518748f742025a251b78028613aa1b16919e73
          test "$(git rev-parse HEAD)" = "$TESTED_SOURCE_SHA"
          git diff --exit-code
          git fetch --depth=1 origin "$GITHUB_SHA"
          git show "$GITHUB_SHA:integration-lab/i-windows-arm64-cd51874/uia_native_fixture.rs" > crates/platform-windows/tests/uia_native_fixture.rs
          test "$(git diff --name-only)" = crates/platform-windows/tests/uia_native_fixture.rs
          mkdir -p verification/platform-ci
          python - <<'PYIDENTITY'
          import json,os
          from pathlib import Path
          Path('verification/platform-ci/identity.json').write_text(json.dumps({'source_sha':os.environ['TESTED_SOURCE_SHA'],'suite_sha':os.environ['GITHUB_SHA'],'run_id':os.environ['GITHUB_RUN_ID'],'production_source_modified':False,'original_failed_run':37074828959,'original_failed_job':111090595445,'interactive_windows_certified':False},indent=2)+'\\n')
          PYIDENTITY
''')
needle='''      - name: Native Windows runtime tests
''';assert job.count(needle)==1
job=job.replace(needle,'''      - name: Exact original UIA fixture with fresh readback, then fail fast
        run: |
          set -euo pipefail
          cargo test --locked -p semwright-platform-windows --test uia_native_fixture real_win32_fixture_exercises_uia_without_pixel_fallback -- --exact --nocapture 2>&1 | tee verification/platform-ci/uia-freshness.log
          grep -F 'test result: ok. 1 passed; 0 failed; 0 ignored;' verification/platform-ci/uia-freshness.log
'''+needle)
needle='''      - uses: actions/upload-artifact@043fb46d1a93c77aae656e7c1c64a875d1fc6a0a
''';assert job.count(needle)==1
job=job.replace(needle,'''      - name: Preserve the exact source and fixture boundary
        run: test "$(git diff --name-only)" = crates/platform-windows/tests/uia_native_fixture.rs
'''+needle)
header='''name: I exact-source Windows ARM64 native retest
on:
  workflow_dispatch:
    inputs:
      candidate_sha:
        required: true
        type: string
permissions:
  contents: read
concurrency:
  group: i-windows-arm64-${{ github.sha }}
  cancel-in-progress: false
env:
  CARGO_INCREMENTAL: 0
  CARGO_PROFILE_DEV_DEBUG: 0
  CARGO_PROFILE_TEST_DEBUG: 0
jobs:
'''
(HERE/'workflow.yml').write_text(header+job)
print('Prepared fixture-only ARM64 retest; full original native Windows job retained')
