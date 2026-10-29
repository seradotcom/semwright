# Local PR draft — not submitted

## Proposed title

WIP: add owner-gated initial Blender 5.2.1 sessions on Windows

## Problem

The first-party Blender driver has a Linux-qualified runtime route. Windows cannot use the same descendant-launch arrangement inside the tested LPAC. An installed Blender or open GUI does not establish a confined DriverProvider; direct bpy exports do not establish broker provenance.

## Changes

- Add an explicit, disabled-by-default owner grant for a closed Windows Blender 5.2.1 initial-process profile.
- Keep Host control of pinned runtime identity, private snapshot, fixed arguments/environment/bootstrap, typed mounts, actual token/Job attestation and lifetime controls.
- Preserve LPAC and the one-process native Job; request the separately authorized registryRead capability without network or GPU.
- Route the first-party bridge through production broker/DriverProvider; keep generic RNA mutation on 5.2 restricted.
- Add readiness cancellation, late-start rejection and correlated reaping evidence.
- Add new-only checkpoint, advisory inspection, save-before-close and fresh hash-checked initial open while retaining legacy destructive-operation consent.
- Make the Host package self-contained with exact vendored sources, a read-only consistency gate and explicit synchronization.
- Retain Linux profiles and update manifest constructors with the optional Windows profile.

## Observed behavior

Production candidates execute authenticated broker requests against confined Blender 5.2.1: typed creation/transform, independent readback and GLB export. Duplicate output and traversal requests were rejected with preservation checks. Native Godot imported/rendered the exact candidate03 GLB; this consumer did not use the Godot broker.

Owner opt-in denial and real readiness cancellation have physical evidence. Recovery remains incomplete: preserved failures exposed factory data and Blender 5.2 API differences. Full save/close/reopen and composition acceptance remain pending.

See [the current checkpoint](WINDOWS_NATIVE_PROFILE_20261029.md) and [historical probes](WINDOWS_52_EXPERIMENTAL.md).

## Authority and compatibility limits

registryRead allows Windows ACL-authorized registry reads; it is not an individual SideBySide-key whitelist. No owner-token fallback, arbitrary Python/eval/shell capability, global ACL relaxation, third-party Blender MCP or automatic human consent response is added.

The public manifest type gains an optional field. Serialized manifests without it retain their default route; external Rust struct-literal call sites may need an update. Shared Host/SDK/platform changes require upstream review.

Checkpointing preserves a declared scene-data subset with intentional orphan retention in its recovery copy. It does not preserve undo history or all process state. Initial file open uses an owner-controlled workspace and hashes, not a sealed file-handle import protocol.

Linux 4.5.14/5.2.1 native regression, confined Windows GPU rendering, full Godot Windows-driver qualification and registry installation remain unverified. The retained Linux native lane was not spoofed or run locally.

## Validation and release boundary

Source/binary-pinned records cover unit/build/package checks, native broker fixture, owner denial, cancellation and native Godot consumption. Failed/incomplete runs remain visible. Diagnostic success is not authoring acceptance; fixture preservation is not human-edit preservation.

This contribution is experimental and not release-ready. Complete lifecycle/composition proofs, audit correlation and Linux regression before advancing compatibility claims. Archipelago art and G6 remain independent and incomplete.

The owner authorized local development/checkpoints, not external push, PR, merge, publication or CI.
