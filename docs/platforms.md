# Platform architecture and support status

Semwright's architecture is **portable core + platform hosts**. Portability means the command, provider, policy, audit, recipe and driver semantics remain stable while OS-specific mechanics are implemented behind explicit contracts. It does not mean every operating system has identical security primitives or live support.

| Layer | Linux | macOS | Windows |
|---|---|---|---|
| Semantic platform contracts | implemented | implemented | future |
| Application/window discovery | native Linux backends | source foundation via native Apple APIs | future |
| Accessibility | AT-SPI | AXUIElement source foundation | future UI Automation |
| Synthetic input | Linux backend routes | CoreGraphics source foundation | future |
| Screen capture | portals/PipeWire routes | ScreenCaptureKit source foundation | future |
| Clipboard | Linux session backend | NSPasteboard source foundation | future |
| Scoped filesystem | openat2 pinned-root confinement | conservative descriptor-relative source foundation | future |
| Driver/plugin isolation | bubblewrap + Landlock | fail-closed while supported isolation model is unresolved | future |
| Service lifecycle | systemd user service | LaunchAgent/SMAppService packaging foundation | future |

## Evidence levels

Do not collapse these into one status:

1. **portable cross-check** — Rust-only shared crates type-check for a Darwin target;
2. **native CI** — code is compiled/linked/tested on GitHub-hosted macOS with an Apple SDK;
3. **live Mac acceptance** — Accessibility, input, ScreenCaptureKit, TCC, multi-display and service behaviour are exercised in an interactive authorized user session.

Linux is the verified runtime host. macOS remains experimental until levels 2 and 3 have adequate evidence. A green hosted build must not be represented as TCC/live-desktop certification.

## Security parity

The common layer expresses policy intent; the host uses the strongest supported enforcement available on that OS. Linux keeps openat2, bubblewrap and Landlock. macOS must not use private Seatbelt APIs, TCC database modification, SIP bypass or `sandbox-exec` as a claimed production equivalent. Where a third-party driver cannot be isolated with a supported mechanism, execution is denied rather than silently downgraded.

## Driver portability

Application protocol logic should avoid OS assumptions. Blender, LibreOffice, OBS, MLT and other drivers should carry application semantics independently of whether the Driver Host runs on Linux or a future validated macOS host. Binary verification, process launch, filesystem mounts and sandboxing belong to the platform host.
