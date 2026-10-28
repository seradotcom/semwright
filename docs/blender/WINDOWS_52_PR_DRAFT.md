# Local PR draft — not submitted

## Proposed title

WIP: add pinned Blender 5.2 runtime profiles and Windows confinement diagnostics

## Problem

The first-party deep Blender driver has a Linux-verified runtime path. The owner has Blender 5.2.1 LTS on Windows, where an installed executable and an open GUI do not establish an isolated DriverProvider. Treating those states as integration would produce false provenance for downstream GLB assets.

## Changes

- Add exact host/version profiles and explicit 5.2 resource layouts; retain the Linux 4.5.14 profile.
- Add a Windows session-runner candidate with LPAC/Job identity checks, private pipes, suspended child launch and bounded handle inheritance.
- Keep authority with Driver Host: digest-pinned binaries, typed mounts/tool arguments, scoped output and fail-closed behavior.
- Separate preparation timeouts from session lifetime; preserve bounded startup, cancellation and exit diagnostics.
- Add Windows path-denial checks and private candidate-instance paths.
- Add sealed fixtures that reproduce nested launch denial and compare native SxS activation under the owner and LPAC.

## Result and current blocker

This is an experimental work-in-progress, not release-ready Windows support. The LPAC fixture reads the exact manifests and enumerates their directories, but CreateActCtxW returns error 5 even for a fixed minimal dependency-free manifest that succeeds with the owner. Nested child creation also remains denied. No broker-mediated Blender scene mutation, GLB export or Godot handoff has passed.

The implementation does not grant broad registry/network capabilities or fall back to owner execution. See WINDOWS_52_EXPERIMENTAL.md for the physical findings, exact hashes and reproduction command.

## Validation

Targeted Windows component and Python boundary tests have passed in the retained branch history. The decisive native-launch/SxS test fails and is preserved as such. Linux 4.5.14 and Linux 5.2.1 native runs have not been performed on this host; they remain required.

Independent original Blender-to-Godot art exports are deliberately excluded from Semwright acceptance evidence.

## Before publication or merge

Resolve the Host confinement contract, complete the Windows broker E2E and negative/recovery suite, run the retained Linux 4.5.14 regression, review shared Host/SDK/Godot changes separately, and update the compatibility/release matrix from actual results. The owner has not authorized pushing, opening a PR or running external CI.
