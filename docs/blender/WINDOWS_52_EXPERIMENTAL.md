# Blender 5.2.1 Windows candidate: demonstrated boundary

Status: **WIP / BLOCKED_WINDOWS_PROVIDER**. This branch is not verified Windows Blender support and is not ready for release. No native Blender scene mutation or GLB export has completed through the broker.

## Source and environment

- Upstream base: 41259eaeea54c6794e19378628de52e30e477834.
- Branch: feat/blender-52-windows-isolated-driver. Main and external repositories were not changed or published.
- Physical host: Windows 11 Home 10.0.26200, x64, Ryzen 5 4600H.
- Compiler used: Rust 1.98.1 x86_64-pc-windows-gnu, MSYS2 GCC 12.2.
- Owner-pinned Blender: 5.2.1 LTS, build 9e2066aef7ef.
- Executable: 113014232 bytes, SHA256 284f4041f98e113f3dc10654a7193ffaaa9bfdfec8b87fa116620a48b5f6d4cb.
- The original interactive Blender and the installed Semwright 1.0.0 broker were retained. The candidate broker is a private instance, not a replacement installation.
- GPU rendering of the independent Archipelago art is not evidence of the contained Blender driver.

## Implemented candidate work

The branch adds exact runtime profiles for Linux 4.5.14, Linux 5.2.1 and Windows 5.2.1, with distinct resource layouts and banner/version checks. Recognition of a profile is not native acceptance.

Windows work includes a Host-mediated session-runner candidate, attestation of LPAC identity and Job membership, private pipes, suspended child launch, a bounded inherited handle list, explicit preservation of the container SID, cancellation/reaping diagnostics, and separate preparation/lifetime timeouts. The driver does not expose generic Python or shell execution.

Related changes retain fail-closed Godot behavior on Windows, private daemon instance paths, canonical grant validation, executable ACL rights and a separate bounded budget for sealed applications. These are proposed first-party changes; all must receive upstream review.

## Physical findings on 2026-10-28

| Observation | Result | Scope |
|---|---|---|
| Sealed child image open for read/execute and SEC_IMAGE mapping | PASS | Rust diagnostic fixture only |
| Parent identity | LPAC, zero capabilities, Job verified | Same launcher thread |
| Thread impersonation query | ERROR_NO_TOKEN (1008) | No thread impersonation token |
| Self process-token QUERY, DUPLICATE and ASSIGN_PRIMARY handle opens | PASS | Handles immediately closed, no token changes |
| Self PROCESS_CREATE_PROCESS handle open | PASS | Immediately closed, no alternate parent used |
| Nested sealed Rust fixture launch, exact private stdio handle list | FAIL: CreateProcessW 5, NTSTATUS c0000022 | Not Blender |
| Nested fixture without stdio inheritance | FAIL: same denial | Not Blender |
| Direct Host-confined Blender --version | Process created, exits c0000022 before banner | No bootstrap or scene operation |
| Owner CreateActCtxW on exact Blender resource 1 | PASS | Resolve only, not activated |
| LPAC CreateActCtxW on same executable/hash/path | FAIL: error 5 | Resolve only |
| Full private manifest reads and hashes | PASS, equal to owner | Values below |
| Private manifest directory enumeration | PASS: 65 and 51 entries, expected files found | Bounded to 128 entries |
| Owner CreateActCtxW on a fixed minimal manifest | PASS | No dependencies, no activation |
| LPAC full minimal-manifest read/hash | PASS, equal to owner | Separate read-only, non-executable typed mount |
| LPAC CreateActCtxW on that minimal manifest | FAIL: error 5 | General SxS resolution requirement remains unidentified |

Manifest identities:

| File | Bytes | SHA256 |
|---|---:|---|
| blender.shared/blender.shared.manifest | 2527 | 655ad61bd460854680806f1fce59459daaf7fc309304fa17b3348c58cf745926 |
| blender.crt/blender.crt.manifest | 2841 | 43bdfdc2028539332fed3dc487474407d63e79f67394145397322dda58e455ee |
| Fixed minimal diagnostic manifest | 261 | c7fe00a9dd11885d38b9da689e356a10edc0bba0d431ee3696cb46822490886f |

The minimal manifest contains one assemblyIdentity and no dependencies, file references, DTD or entities. The fixture verifies the exact bytes before resolving it. Both activation context modes call ReleaseActCtx after success and never call ActivateActCtx.

The existing CodeIntegrity, AppLocker and selected Application event channels returned no matching events for the queried time window. That event-query step did not enable tracing, logging channels or policies. This absence is not proof that policy enforcement played no part. A later bounded SxS trace is recorded below.

## Interpretation and missing contract

The comparison excludes inability to read those particular private manifests, enumerate those directories or read the fixed minimal fixture. It does not identify the denied system resource. A registry, service, object-namespace or activation-subsystem dependency remains a hypothesis.

Host source already supplies its private AppContainer LOCALAPPDATA and Temp environment. No broad registry capability, network grant, owner-token execution fallback, alternate parent or system ACL change was added to obtain success.

The missing contract is a public, tested route for initializing a native SxS application inside the approved confinement profile, with any required system resources identified explicitly. Separately, nested descendant creation must conform to the Host's declared child-process policy. The SxS comparison does not prove that both failures share a cause.

An owner-created activation context has not been injected or transferred into a child. No undocumented PEB/Rtl workaround is proposed.

## Reproduction

The fixture is opt-in and refuses an ordinary owner token before fixture filesystem access. The test itself prepares owner-controlled input, validates the owner comparison, and then invokes the production Windows sandbox spawn.

Set SEMWRIGHT_TEST_BLENDER_RUNTIME to the exact owner-approved private runtime and run:

    cargo test -p semwright-driver-blender --features windows-launch-test-fixture --test windows_native_launch --target x86_64-pc-windows-gnu --locked isolated_blender_activation_context_probe -- --ignored --nocapture

The observed outcome is a **failing test**, not an ignored success: zero passed, one failed, four filtered out. Its structured diagnostic contains the file hashes, enumeration results and error 5. The successful collection of diagnostic data is not successful Blender startup.

Closed logs are retained in the Archipelago integration evidence and in this checkout's local diagnostic logs:
- windows-native-launch-self-access.log
- windows-blender-actctx-probe.log (initial compile error, preserved)
- windows-blender-actctx-probe-fixed.log
- windows-blender-actctx-file-probe.log
- windows-blender-actctx-minimal.log
- windows-blender-actctx-minimal-formatted.log

## Acceptance still required

- Windows CLI/MCP -> broker/policy/audit -> isolated first-party Blender -> typed read/edit/readback -> collection GLB -> Godot import.
- Native negative and recovery tests: authority/path boundaries, unsupported versions, bad digests, cancellation, crash, stale generation, unknown mutation/no blind replay, network denial and process/resource limits.
- Linux 4.5.14 native regression: **NOT_RUN on this host**. Existing profile and component tests are retained; this is not a native regression claim.
- Linux 5.2.1 native acceptance: **NOT_RUN**.
- Windows confined GPU rendering: **NOT_RUN**.
- Owner artistic acceptance belongs to Archipelago and is independent of this engineering result.

## Primary API references

- https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createactctxw
- https://learn.microsoft.com/en-us/windows/win32/api/winbase/ns-winbase-actctxw

The implementation also uses the signatures and constants of the pinned windows-sys 0.61.2 dependency. These references describe APIs; the physical results above come from the retained local runs.

## Bounded SxS and locale diagnostics (same profile)

The built-in sxstrace tool ran for the existing opt-in fixture at ordinary rights. A session inventory first confirmed no existing SxsTrace session; a finally block stopped the newly started trace, and a later inventory verified it was gone. No elevation, capability, ACL, locale or policy change was made. The native fixture again failed (exit 101; zero passed, one failed). Filtered trace output records successful owner activation; no generation event for the failed LPAC activation was observed. The exact denied resource is still unidentified. Raw ETL is local diagnostic evidence, not proof of application startup.

GetUserPreferredUILanguages returned es-ES and en-US inside the same LPAC; GetUserDefaultLocaleName returned es-MX. The minimal CreateActCtxW operation still failed with error 5 before and after the queries and with explicit wLangId 0x0409. The owner comparison passed with both default and explicit language. No locale setting was changed. The locale test is preserved as zero passed, one failed, four filtered out.

These diagnostics do not establish that activation failure causes Blender's direct startup exit. They establish separate reproducible boundaries. The Host advertises descendant creation as denied; a future Host-owned initial-runtime session must honor that contract rather than silently enabling descendants. No owner activation handle injection, PEB modification, manifest removal, ordinary-token fallback or broadened capabilities are proposed.

Additional closed evidence: windows-blender-actctx-locale.log; sxs-20261028 filtered trace outputs and receipt. The source of the bounded trace is archived with the evidence.

Additional primary references:
- https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/sxstrace
- https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-getuserpreferreduilanguages
- https://learn.microsoft.com/en-us/windows/win32/api/winnls/nf-winnls-getuserdefaultlocalename

## Later initial-process activation checkpoint

The [2026-10-28 fixture checkpoint](../../crates/driver-blender/WINDOWS_ACTIVATION_CHECKPOINT_20261028.md)
separately records four passing initial-process manifest fixtures under the same
LPAC and Job confinement, including the exact Blender root manifest. This
narrows the earlier dynamic-activation finding: initial process activation can
succeed for the tested fixture even while dynamic CreateActCtxW fails.

The CPU DLL resource-2 resolve-only probe still fails inside LPAC with error 5
and NTSTATUS c0000022; its owner control succeeds after the isolated process is
reaped. The denied resource remains unidentified. A bounded, separately owned
SxS trace and two offline metadata readers preserve their coverage and
attribution limits. A successfully captured diagnostic is not a passing CPU
probe. No new Blender startup, typed scene mutation, GLB export or Godot E2E is
claimed. Windows acceptance remains BLOCKED; native Linux 4.5.14 is NOT_RUN.
