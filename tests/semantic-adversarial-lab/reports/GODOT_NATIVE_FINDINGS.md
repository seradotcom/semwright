# Godot native adversarial findings

Exact product target: e6bd9489d7983d275fd1454dd7ab26916d3fbb15
Lab suite: a337709ffdd2e05f40da408e3a78183651dff113
Run: 36818122155
Godot-native job: 110227589092

## Result

- Requested: 13
- Executed: 12
- PASS: 10
- FAIL: 2
- BLOCKED: 1
- NOT_RUN: 0
- Native acceptance: false
- R16 closed: false
- Receipt SHA-256: 7c4b1dadc37c97c43e9e7a29f6af8b709cb4c94d21e2b01a557303259e59ea48
- Artifact SHA-256: 3813bc488c953f32740c1b8df43bfd7f369ae8f8c5a3cf496c1074c31881fcd7

Selftest suite on the same lab SHA passed 99/99.

## G-FIND-D-001 — persistence dependency stability

Case: G-GODOT-006.

Both save-candidate and reopen-candidate observations are from fresh Godot processes and individually pass the Godot decode_observation contract. The Godot persistence_value contract rejects the pair because normalized dependency sentinels differ.

Writer dependencies: 4. Reader dependencies: 3.

Writer-only sentinel:
- source: res://scenes/arena.tscn
- path: res://resources/arena_mesh_local_material_local.tres
- exists: true
- SHA-256: 30bbe11ba82e11ac91a3204f40a37a67042e8b19724b7faa8999a8d70ed2a3b7

Observed result: fresh_process=true, persistence_verified=false.

G-GODOT-007 remains BLOCKED because a dependency-tamper negative cannot be isolated while baseline persistence is red.

Reported on the Godot implementation on PR #176.

## G-FIND-D-002 — native observer false-PASS

Case: G-GODOT-013.

The product-generated fixture contains supported BoxMesh resources. D native_observer.gd enters the generic Mesh branch and invokes surface_get_array_len(index). Godot 4.7.2 emits:

SCRIPT ERROR: Invalid call. Nonexistent function surface_get_array_len in base BoxMesh.

The observer still exits 0 and publishes failures=[], so the structured receipt appears clean and D decode_observation admits it.

Observed:
- process_exit_zero=true
- observer_reported_failures_empty=true
- script_errors_absent=false

Reported on the Godot implementation on PR #176.

## Closed lab harness noise

Earlier failures before case execution were lab-owned and are not product findings: helper crate identity, font/xdg sandbox visibility, native address-space/file-size bounds, managed-store identity materialization, save/reopen process orchestration, and AppArmor execution paths.

G-GODOT-011 export and G-GODOT-012 standalone launch both PASS in the final run, so export harness noise is closed.

## Owner status

The Godot implementation published FIX_SHA 70bd7857e9700b6f03547a706ff0e6496ffd838e specifically addressing both G native findings. The closure retest below supersedes the earlier owner-open status.

## Closure retest

Godot FIX_SHA 70bd7857e9700b6f03547a706ff0e6496ffd838e was retested by the lab on suite 366772a088c786f8e9e1f4a1602d0b14778453d4, run 36940490321, Godot-native job 110636591865. Result: 13/13 PASS. G-GODOT-006, G-GODOT-007 and G-GODOT-013 all pass. Receipt SHA-256: 521da5cbe0c65bc84e38c9ab67f088385809f6bcb9f867a98d32c44900b4ddbb. Artifact SHA-256: 183d8686c45d774d1b0061eed28e892417f4dcf414831945c7f90a4b52e23dd6. G-FIND-D-001 and G-FIND-D-002 are CLOSED_RETEST_PASS.
