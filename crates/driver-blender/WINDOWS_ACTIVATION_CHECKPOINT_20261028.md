# Windows Blender activation diagnostics — 2026-10-28 checkpoint

**Experimental diagnostic checkpoint. Windows isolated Blender startup and the broker integration remain blocked.** This contribution preserves first-party fixtures, bounded observations and reproducible failures. Archipelago A05 remains BLOCKED; the native Linux 4.5.14 regression remains NOT_RUN.

## Evidence established on this host

Four selected initial-process fixtures passed under LPAC, with zero additional capabilities and verified Job membership. Their initial activation-context identities matched the owner controls. They exited successfully and were reaped.

| Selected test | Observed result | Scope |
|---|---|---|
| initial_host_embedded_manifest_activation_probe | PASS; one assembly | Fixed first-party manifest embedded in the fixture executable |
| initial_host_common_controls_probe | PASS; three assemblies | Common Controls dependency and resolved system comctl32.dll load |
| initial_host_private_assemblies_probe | PASS; five assemblies | Exact private manifest metadata for blender.crt and blender.shared |
| initial_host_exact_blender_manifest_probe | PASS; five assemblies | Exact Blender root-manifest bytes embedded in the first-party executable |

The combined regression recorded **4 passed, 0 failed**. This is the selected group, not the complete Windows native test file. Counts and servicing versions are observations of this host; the localized Common Controls resource contributes to the counts. See the [positive regression log](evidence/windows-activation-20261028/windows-positive-manifests-after-resource-probe.log) and [receipt](evidence/windows-activation-20261028/windows-positive-manifests-after-resource-probe.json).

These fixtures call the production Windows sandbox_spawn boundary directly and report broker_used=false. Resource editing affects only a newly copied first-party fixture executable. Blender executables, DLLs and manifests are not patched.

## CPU payload: readable and mappable; loading denied

The CPU diagnostic verified 51 pinned DLL files and two private manifests within the isolated fixture. A request to open the CPU payload for writing was denied with Win32 5; no file data was written. Four exact images passed read/execute, hash, SEC_IMAGE, mapping and unmapping checks. Image mapping does not establish loader initialization.

An explicit dependency-order load then requested:

1. blender.crt/vcruntime140.dll — loaded; exact module path matched.
2. blender.crt/vcruntime140_1.dll — loaded; exact module path matched.
3. blender.crt/msvcp140.dll — loaded; exact module path matched.
4. blender_cpu_check.dll — failed with Win32 5 and captured NT status c0000022.

All acquired CRT references were released in reverse order. No exports were called. The CPU request returned no module handle; the receipt does not establish which internal initialization steps, if any, were reached. This preload diagnostic is distinct from unmodified Blender startup. The fixture exited 2 and cargo exited 101. See the [CPU closure diagnostic](evidence/windows-activation-20261028/windows-initial-cpu-closure-order-resume.log).

A fixture-only ACL inheritance defect was corrected during investigation: staged DLLs and XML inherit the protected private root and the Host's per-container read/execute grant. This did not broaden system or actual-runtime ACLs, and it did not resolve the CPU failure. No additional capabilities, descendant-process allowance or Host policy relaxation was introduced.

Five mitigation queries describe the running fixture. System-call-disable flags 4 decode as FSCTL calls disallowed and Win32k calls not disallowed. Querying these flags did not change them and does not identify the cause of the denial.

## Exact CPU resource comparison

Read-only inspection found a 135-byte RT_MANIFEST resource, ID 2, in the pinned CPU DLL. Its XML has an assembly element with no dependency or assembly identity. The three inspected CRT DLLs had no enumerated RT_MANIFEST resources. Inspection used resource/data mappings without requesting DLL initialization. See the [inspection receipt](evidence/windows-activation-20261028/cpu-image-readonly/receipt.json).

The query-only fixture verifies the CPU hash and calls CreateActCtxW on that resource. It queries and releases any acquired context. It does not request CPU/CRT loading, call exports or activate the context on the thread. The isolated process runs first; owner controls run after its exit and reaping.

| Observation | Isolated fixture | Owner control afterward |
|---|---|---|
| Exact CPU image verification | PASS | PASS |
| CreateActCtxW, resource 2, flags 8 | FAIL: Win32 5 / NT c0000022 | PASS |
| Query | Not reached | Format 1; one assembly; empty root identity |
| Release | No reference acquired | ReleaseActCtx called |

ReleaseActCtx returns no status; the receipt records a call rather than inventing a success code. The fixture's initial executable context still contains the expected five assemblies. Both sides' observations were retained before the test assertion failed. See the [comparison log](evidence/windows-activation-20261028/windows-initial-cpu-resource-activation-resume.log) and [run receipt](evidence/windows-activation-20261028/windows-initial-cpu-resource-activation-resume.json).

An [earlier experiment](evidence/windows-activation-20261028/windows-blender-actctx-minimal-formatted.log) read and verified a 261-byte first-party minimal XML file within LPAC, then received error 5 from dynamic CreateActCtxW on that XML; the owner control passed. Failure to extract a PE resource is therefore not the only demonstrated failing input path. The earlier experiment used a different initial fixture context and is not a matched replacement for the current comparison.

## SxS capture and limits of attribution

One additional execution of the same CPU resource test was captured with the built-in sxstrace tool. The runner checked existing sessions, identified its newly created session by handle, GUID, name and exclusive ETL path, and reconfirmed ownership before stopping that handle. Stop returned 0; a later query returned 4201, with owned_remaining=false. No elevation or Host policy change was requested. The receipt reports zero lost events and buffers. Cargo still exited 101: a successful capture is not a successful test.

The raw ETL remains under .local and is excluded from this commit. Its identity is **262144 bytes**, SHA-256 **2db670e8a1ba8449bc7bf43841a76de7c7615edcfb08674b3214f542623ff6ba**. The [capture receipt](evidence/windows-activation-20261028/sxs/receipt.json) and [evidence index](evidence/windows-activation-20261028/INDEX.json) retain its location and hash.

The [fixture-filtered parse](evidence/windows-activation-20261028/sxs/fixture-cpu-sxs.txt) reports successful generation of the initial context. Re-parsing the unchanged ETL for the [CPU DLL](evidence/windows-activation-20261028/sxs/cpu-dll-resource-sxs.txt) reports one successful CPU-context generation. These text reports have no client PID/TID or call timestamps. That CPU generation is compatible with the owner control afterward, but attribution is not established. The reports do not prove that the failed LPAC call occurred before SxS generation.

Two bounded metadata readers each returned 310 events, below their 2048-event limit. Records exposed no provider names or Properties values. The second reader also found zero EventData/Binary or Event/BinaryEventData nodes in all 310 XML records: NO_BINARY_ELEMENTS. Neither reader found the fixture-path payload anchor. This is a reader-coverage limitation: sxstrace itself rendered the paths from the same ETL. Emitter PID/TID must not be treated as client identity. These offline exports and reparses repeated neither cargo nor tracing. See the [v2 reader receipt](evidence/windows-activation-20261028/sxs/etl-metadata-v2-reader-receipt.json).

The separate Application/SideBySide query found no matching events in the original test interval, 18:22:18.485171–18:22:42.485171 UTC. This does not rule out Windows enforcement and does not describe every event channel or the later capture interval.

## Remaining acceptance

The exact protected resource or operation causing the denial remains unidentified. No particular capability, key, service, mitigation or filesystem object is established as the cause. The dynamic-context failure is not yet proven to explain the separate direct Blender startup failure.

The required path remains unverified: **broker → Host-owned initial Blender 5.2.1 process under approved confinement → typed scene mutation → readback → verified GLB → Godot import**.

A Host-owned initial-runtime session still needs a reviewed preparation, authorization, handshake, lifetime and audit contract. The diagnostic fixture is not that implementation. The descendant-process policy must remain intact. Linux 4.5.14 native regression and Windows confined GPU execution are also unverified. Archipelago artistic work is independent of these engineering observations.

## Reproduction gates

The Windows native test file requires the windows-launch-test-fixture feature. Private-manifest, CPU-load and CPU-resource tests are ignored by default. The runtime-dependent tests require SEMWRIGHT_TEST_BLENDER_RUNTIME to name an absolute, owner-approved directory containing the pinned files. The payload manifest records identities; it does not bundle or download runtime binaries.

Use the recorded Rust 1.98.1-x86_64-pc-windows-gnu toolchain and existing MSYS2 compiler path. Local capture runners also set CARGO_TARGET_DIR, TEMP and TMP below this checkout, CARGO_BUILD_JOBS=2, debug information off and incremental compilation off. Preserve the recorded environment and collect stdout, stderr, exit status, timeouts and source hashes. Scripts copied into evidence preserve the executed local capture sources and owner paths; they are evidence, not portable installers.

Selected positive regression:

    cargo +1.98.1-x86_64-pc-windows-gnu test -p semwright-driver-blender --features windows-launch-test-fixture --test windows_native_launch --target x86_64-pc-windows-gnu --locked --offline initial_host_ -- --include-ignored --skip initial_host_cpu_component_probe --skip initial_host_cpu_resource_activation_probe --nocapture --test-threads=1

Separate query-only CPU resource diagnostic, observed to fail:

    cargo +1.98.1-x86_64-pc-windows-gnu test -p semwright-driver-blender --features windows-launch-test-fixture --test windows_native_launch --target x86_64-pc-windows-gnu --locked --offline initial_host_cpu_resource_activation_probe -- --ignored --nocapture --test-threads=1

The code-loading diagnostic is the different opt-in test initial_host_cpu_component_probe. Offline ETL inspection does not require repeating either test.

## Sources

Physical outcomes are in the retained receipts and logs. API documentation does not substitute for execution.

- [Existing experimental boundary](../../docs/blender/WINDOWS_52_EXPERIMENTAL.md)
- [CreateActCtxW](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-createactctxw)
- [ReleaseActCtx](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-releaseactctx)
- [SxS manifest resources](https://learn.microsoft.com/en-us/windows/win32/sbscs/using-side-by-side-assemblies-as-a-resource)
- [ETW event header identity](https://learn.microsoft.com/en-us/windows/win32/api/evntcons/ns-evntcons-event_header)
- [Event XML binary fallback](https://learn.microsoft.com/en-us/windows/win32/wes/eventschema-eventtype-complextype)
