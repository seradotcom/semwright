# Semwright Windows — UI Automation borrowed VARIANT / heap corruption

Date: 2026-10-31. Scope: **shared Windows runtime**, not a Figma, Blender, Godot, or OBS driver-specific workaround.

## Observations (Windows Event Viewer / WER)

Windows has repeatedly terminated `semwrightd.exe` with exception `0xc0000374` (heap corruption) and faulting module `ntdll.dll`. At least 14 archived WER records on the affected machine have the `PCH_99_FROM_ntdll+0x1628A4` signature, spanning multiple experimental daemon builds. Examples:

- 2026-10-28 15:37 local: two broker instances failed at the same time.
- 2026-10-29 21:56 local: four independent instances (Godot and OBS candidates) failed concurrently.
- 2026-10-31 01:14 local: three broker instances failed near-simultaneously.
- 2026-10-31 16:51 local: isolated Figma Lab broker failed after roughly 27 minutes of uptime.

No Windows shutdown/restart evidence appeared in the examined three-minute windows. None of 14 inspected WER archives contained a `.dmp` file. A consistent `ntdll.dll` crash site indicates where Windows detects damage, **not which source line caused it**.

## Known upstream bug with strong source-level match

Semwright pins `windows = 0.62.2` via `uiautomation = 0.25.1`. Upstream:

- [microsoft/windows-rs#3818](https://github.com/microsoft/windows-rs/issues/3818): UIA `IUIAutomationPropertyChangedEventHandler` receives `[in] VARIANT` values borrowed from UI Automation. The generated bindings in this version can clear/free the borrowed `VARIANT`; double-free of `SAFEARRAY` members demonstrably corrupts the heap.
- [microsoft/windows-rs#3886](https://github.com/microsoft/windows-rs/pull/3886): upstream changed the ownership behavior in newer source, but this is not in Semwright's pinned `0.62.2`.
- The previous Semwright workaround excluded `BoundingRectangle`/`RuntimeId` from the list but **kept** `IUIAutomationPropertyChangedEventHandler` registered, including BSTR-bearing `Name`, `HelpText`, `ValueValue`, and `ItemStatus`. Their involvement in each observed crash is still a hypothesis; the underlying ownership defect is confirmed upstream.

## Mitigation in this patch

Do **not** register the affected UIA property-change callback in `crates/platform-windows/src/uia.rs` while using these pinned bindings. Remove its handler, registration and retain-cycle field entirely. This avoids the known dangerous borrowed `VARIANT` conversion at the process boundary.

Preserved behaviors:
- UIA generic events including `Text_TextChanged`, `TextEdit_TextChanged`, `LayoutInvalidated`, window, selection and focus events.
- UIA structure-change events; structural changes invalidate cached stamps and require fresh snapshots.
- Interactive `ui.snapshot`, `ui.inspect`, `ui.hit_test`, `ui.invoke` and revalidation of targets before writes.

Tradeoff: a change *only* to a property that does not trigger any of those other events will not generate an immediate `PROPERTY_CHANGED` hint. Clients should use an explicit fresh read/snapshot to observe such a change. Do not claim property-level event parity until the upstream bindings are patched, shipped, and verified.

## Verification

- Target: `cargo check --offline --locked -p semwright-platform-windows` against existing shared E: Cargo target using MSVC + existing portable Windows SDK.
- Unit guard: `native_callbacks_keep_invalidation_without_borrowed_variant_ownership` **PASS**; prevents re-registration of the unsafe callback and confirms generic invalidation events remain.
- Native COM actor lifecycle `uia_actor_lifecycle_preserves_safe_event_subscriptions` **PASS**; actually initializes the UIA service, registers the remaining events, and removes them before releasing the COM apartment (no mock).
- Existing native Win32 fixture `real_win32_fixture_exercises_uia_without_pixel_fallback` was run **once, NOT PASS**: it encountered `StaleReference` at UIA inspect because a global structure/layout invalidation arrived between snapshot and inspect. This is a deliberate safety rejection, not heap corruption; the existing reference freshness contract was not weakened. Revalidate in a quieter isolated interactive environment later.
- **Important:** passing library tests and a short COM lifecycle do NOT certify the absence of *all* native memory defects. No post-mitigation broker soak and no memory dump of the original fault have yet proved universal root-cause closure.

## Follow-up to reach production-grade closure

1. A1/integrator builds the updated `semwrightd.exe` from this branch, confirms its hash, and rolls it out to one isolated candidate broker instance only. Do not replace a broker serving other active work.
2. Run one bounded Figma clone inspect/edit/readback/export cycle, plus native Blender/Godot/OBS smoke checks, then bounded runtime observation. Do not run duplicate Cargo targets or re-authorize unnecessarily.
3. On a further `0xc0000374`, capture **one** owner-controlled crash dump via per-image Windows Error Reporting LocalDumps at an encrypted/private E: folder (contains possible secrets, never commit or share raw). Requires the user's administrator approval; do not blanket-enable dumps for all applications.
4. Analyze dump in WinDbg `!analyze -v`, with symbols. If still ambiguous, opt-in targeted PageHeap/Application Verifier for isolated `semwrightd.exe` process only. Disable after diagnosis.
5. When a vetted `windows-rs` release containing #3886 is available and compatible with `uiautomation`, migrate dependencies, re-enable property event subscriptions with native tests, and remove this temporary mitigation.

**Release status:** Fix for a *known unsafe callback path*, not evidence that all historical crashes were caused by that one path. The source change is appropriate to integrate while deeper Windows testing proceeds.

## Recorded verification results

- `cargo check --locked --offline -p semwright-platform-windows`: **PASS** with MSVC and existing portable Windows SDK.
- `cargo test --locked --offline -p semwright-platform-windows --lib`: **10 PASS, 0 FAIL**, including property-handler exclusion and a real COM actor init/event subscription/shutdown.
- `cargo clippy --locked --offline -p semwright-platform-windows --lib -- -D warnings -A clippy::manual_is_multiple_of`: **PASS**. The narrowly waived lint originates in an unrelated, unmodified Windows sandbox source file (`crates/platform-windows-sys/src/launch/initial_lpac.rs`), not this fix.
- Isolated Win32 interactive fixture: **NOT PASS** due to expected `StaleReference` after a concurrent desktop-wide structure event, not a crash. No safety override or force-retry.
- A full daemon dependency-graph check was started against the existing Cargo target. Three unrelated concurrent edits appeared in protocol, platform services, and Windows paths while that check was running; they are **excluded from this change and its commit**. The daemon graph check alone must not be treated as validation of an unchanged production tree unless its source state is reconciled.
- No production daemon was replaced or restarted, and no Figma document was modified.
