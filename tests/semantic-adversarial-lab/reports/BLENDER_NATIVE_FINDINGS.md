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

Current E PR #175 remains on the attacked certified product SHA 3d04d8465dcfa94d6cbf548fcaf343f69ac5838f. No product fix SHA for G-FIND-E-001 has been published to G, so no retest or closure is claimed.

## Failed fix retest

E FIX_SHA 44780d977735ccc9417ec2992427833abffd0f3c was retested by G on suite ee512ea79840d880998045d1d373cfab94d4093b, run 36944138504, Blender-native job 110646109790. Result: 13/14 PASS; only G-BLENDER-010 still fails. Pre-export drift is false, the GLB has valid magic and matching SHA-256, and the external sentinel is preserved; the immediate post-export composition readback still reports drift=true. Receipt SHA-256: ca6edce0893bf6355534aca8b39978e8e25449701972f4d0e77ddf5f6591cb73. Artifact SHA-256: d59de226b39ea086a230d184e70474a17488291eb676b38f37b2dcb750596609. Status remains OPEN_OWNER_TRIAGE; another E FIX_SHA is required.
