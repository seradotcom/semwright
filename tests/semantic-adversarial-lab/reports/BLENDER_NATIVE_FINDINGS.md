# G Blender native adversarial findings

Exact product target: 3d04d8465dcfa94d6cbf548fcaf343f69ac5838f
G suite: 982e5698cc44c2adead9491a2f8590242fd1dbf3
Run: 36827742372
Blender-native job: 110257156669

## Result

- Requested: 14
- Executed: 14
- PASS: 13
- FAIL: 1
- BLOCKED: 0
- NOT_RUN: 0
- Native acceptance: false
- R16 closed: false
- Receipt SHA-256: 6bffdc74f0134dc68795652372366fd6c81a928b71987e69530b0031751d1c9e
- Artifact SHA-256: 5f89bf7aae9d63d5ff0011d3ce121b534632c9cbb65197c6b1dde7f3752d1d90

Selftest on the same G suite passed 102/102.

## G-FIND-E-001 — GLB export leaves managed source in drift

Case: G-BLENDER-010.

The isolated retest deliberately executes persistence/reopen, hostile symlink output,
cursor invalidation/restoration and shared-mesh negative planning before the successful
GLB export. Those independent cases pass.

Immediately before export, the managed island reports drift=false. The real GLB
export succeeds, produces glTF bytes, its returned SHA-256 matches the written file,
and the external synthetic sentinel is unchanged. A fresh composition.inspect
immediately after that export reports drift=true.

Observed case receipt:

- pre_export_drift_false=true
- glb_magic=true
- sha_matches=true
- sentinel_preserved=true
- source_drift_false=false

The prior cascading persist failures are closed as harness-ordering noise: in this
isolated run the two pre-persist snapshots are both drift=false with the same
fingerprint, composition.persist succeeds, duplicate persist rejection succeeds,
and fresh-process reopen succeeds.

Reported to owner E on PR #175:
https://github.com/seradotcom/semwright/pull/175#issuecomment-5926441241

G does not patch E or relax this oracle. Closure requires an owner fix SHA and an
exact-SHA G retest of the affected native family.

## Closed G harness noise

Earlier Blender runs blocked before case publication because G staged Driver Host
state outside the AppArmor executable allowlist and initially used outer resource
limits narrower than E's declared sandbox limits. Those G defects were corrected
without product changes. The final isolated run above has no infrastructure blocker.

## Owner status

E published two successive fixes. The first FIX_SHA 44780d977735ccc9417ec2992427833abffd0f3c still reproduced G-BLENDER-010; second FIX_SHA f492f13a028f781d9ca55631764578f5b327eb1b closed the finding in the retest documented below.

## Failed fix retest

E FIX_SHA 44780d977735ccc9417ec2992427833abffd0f3c was retested by G on suite ee512ea79840d880998045d1d373cfab94d4093b, run 36944138504, Blender-native job 110646109790. Result: 13/14 PASS; only G-BLENDER-010 still fails. Pre-export drift is false, the GLB has valid magic and matching SHA-256, and the external sentinel is preserved; the immediate post-export composition readback still reports drift=true. Receipt SHA-256: ca6edce0893bf6355534aca8b39978e8e25449701972f4d0e77ddf5f6591cb73. Artifact SHA-256: d59de226b39ea086a230d184e70474a17488291eb676b38f37b2dcb750596609. Status remains OPEN_OWNER_TRIAGE; another E FIX_SHA is required.

## Second fix closure retest

Owner E published second FIX_SHA f492f13a028f781d9ca55631764578f5b327eb1b after the first 44780d97 fix still reproduced G-BLENDER-010. G retested the unchanged 14-case Blender-native family on suite c913a26badc218b9e53756e68eb22398c52f5550, run 36946146627, job 110652860701. Result: 14/14 PASS, 0 FAIL/BLOCKED/NOT_RUN. G-BLENDER-010 observes pre_export_drift_false=true, source_drift_false=true, glb_magic=true, sha_matches=true and sentinel_preserved=true. Receipt SHA-256: 3bcdc35346f1e1b243312e605846adf44182447712721a811806885907a16740. Artifact SHA-256: e7e3d2774b3ba09c92cebfd2e898c3a8c18d6253d1c8b9cdabe852581c1f78b1. Status: CLOSED_RETEST_PASS.
