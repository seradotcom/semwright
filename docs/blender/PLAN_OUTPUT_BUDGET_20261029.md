# Plan output preflight — Windows physical regression, 29 October 2026

Status: bounded regression PASS. This does not complete G6, certify the artistic result, or run Linux 4.5.14.

## Defect and change

Candidate10 accepted the original 1,062-vertex / 2,112-triangle lighthouse recipe as input, but its complete PlanOutput duplicated enough typed intent/operation data to exceed the registry's 16,384-node output budget. The driver had already issued the plan in PlanVault and inserted its record before the registry rejected that response. An unreturned authority record could therefore occupy the bounded cache.

The driver now serializes the exact PlanOutput once and checks its existing value budget and descriptor output schema before publishing it. Cancellation is checked immediately before issuance. A duplicate owner/reference key is rejected before issuance. Vault issuance and record insertion remain adjacent synchronous operations, and the exact prevalidated Value is returned. Both root and repair planning use this helper; repair still retains the parent's convergence budget.

The existing value-budget function moved unchanged from registry/bounds.rs into semwright-types/json_budget.rs and is re-exported from registry. The registry still independently checks inputs and outputs. Node, depth, pending-walk and byte limits were not increased. The patch does not add executable authoring, capabilities, filesystem grants, network access, or alternative owner-token execution.

Files:
- crates/types/src/json_budget.rs and lib.rs.
- crates/registry/src/bounds.rs.
- crates/driver-blender/src/authoring_runtime.rs.
- Original regression fixtures in crates/driver-blender/tests/fixtures/authoring_plan_output_{budget,valid}.json.
- Physical test in crates/driver-blender/tests/windows_broker_authoring.rs.

## Verification

| Verification | Actual result |
| --- | --- |
| Driver binary contract and publication regressions | 20 passed, 0 failed |
| Registry and shared types tests | 60 passed, 0 failed |
| Known oversized input | Input budget passes; exact full output fails ResourceExhausted |
| Failed publication | Pre-issue gate not reached, no new record or matching vault authority; previous plan still usable |
| Valid follow-up publication | Accepted after rejection |
| Cancellation / bad output schema / duplicate reference | Reject before new publication |
| Production candidate build | PASS, unchanged source overlay during build |
| Physical isolated Windows regression | PASS in native-profile12/output-budget-attempt02 |
| Linux 4.5.14 native regression | NOT_RUN |

The shared-budget suite checks the existing 16,384/16,385-node, depth 64/65 and byte boundaries. It also runs the existing registry and types suites. Physical-only tests are ignored in ordinary cargo test; compiling them is not reported as physical success.

Build logs and exact source overlays are archived in checkpoint04:
- publication-contract-attempt01: 20 tests, cargo log 8b02157cf8e92b1291e36c8dc38a07ac919e19d187ba9d02fc7631830c55c12d.
- shared-budget-contract-attempt01: 60 tests, cargo log 15aa764b9151fe763089f9f1155590977178b2bb42efcf27a3faae5731271ad9.
- authoring-proof-build-attempt20: compiled physical harness.
- build-attempt20: production CLI, daemon and Blender driver.

## Physical proof

Candidate12 used the production broker and Driver Host. The driver token had LPAC and zero added capabilities. Native Blender had LPAC with the exact previously authorized registryRead capability, an attested Job, one-process limit and no network grant. This is not the earlier zero-capability minimal-manifest fixture, and not owner-token Blender.

The oversized plan request c41abcb6aea64ed2bd210d48cc320f97 received ResourceExhausted originating in the driver, with the Host's redacted driver-error message. It was not the former post-dispatch registry output rejection. No plan result was returned. Independent native observations before and after matched exactly, including the native session.

Without restarting that session, the same official memory-only broker client planned and applied the two-mesh/two-material static fixture. Effect Conformance and independent native readback passed. The fixture then passed library persistence, checkpoint close, fresh hash-bound reopen with scripts disabled, further readback and GLB export. Both native processes were reaped by the Host after saved checkpoints. The idle candidate broker and its confined driver were then retired by exact process identity.

Zero internal vault/cache publication is covered by the unit regression. It is not directly introspected through the public broker. The physical proof establishes real error provenance, no observed native-scene effect, and continued valid execution in the same native session.

The first candidate12 test started before the Host published readiness and could not connect; it sent no requests. Its failure is preserved. The runner now requires the broker-ready record before submitting the proof.

## Pins and archive

- Driver: 825559f5ac297df17fdfb8a506fe048cff25e13a803acbfb54000ab443303943.
- Daemon: b6d7322114b822f8d14ff5ba2ead3bf663ccd9a467510cdba311829be0d199fc.
- CLI: 6760955fd7bf45d6e74773295a40aaefd94e1480b0e68686ffda445d0c968db5.
- Physical receipt: 86ce6ba88a9ba004690fcd5f15503a397a9c5a8b5b258e58f96fc91cbaa035e4.
- Independent GLB: 3,208 bytes, ee59d4724ed2a8e0d9f70e13209ba8d1b808d607b3bb0f406791132c0ee9f11b.
- Scene checkpoint: 64091f63068fc68ec952dd7974f2fc26a6e0c4013db432c9cf289675ce1f1004.
- Archive: crates/driver-blender/evidence/windows-native-profile-20261029/checkpoint04.
- Archive manifest: 39a05bf1ebce19ade2147aeb0a101dafe1c4024688150dade31a40b108cbb167.
- Archive has 210 payloads, 3,853,886 bytes, 36 audit records and 18 matched request/envelope pairs, zero unmatched.

The local audit chain proves internal hash consistency and request/provenance correspondence. It does not claim resistance to malicious rewriting by the same owner. No external publication, push or CI was performed.
