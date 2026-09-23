# macOS host status

Semwright is being platformized around a shared semantic core and explicit operating-system
hosts. Linux remains the mature host. The macOS implementation in this branch is an
integration target, **not a claim of production macOS support**.

## Shared architecture

Agent-facing commands, provider identity, policy semantics, audit, registry descriptors,
recipes, Driver Protocol and MCP contracts remain shared. Operating-system mechanics are
selected behind the platform boundary:

```text
CLI / MCP / SDK
      |
    Broker
policy / audit / refs
      |
Provider Runtime
      |
platform contracts
   /         \
Linux       macOS
```
Linux keeps its stronger existing mechanisms such as `openat2`, bubblewrap and Landlock.
Portability must not reduce those guarantees.

The macOS host uses public Apple APIs and keeps platform-native handles out of command
schemas. Its source covers application/window discovery, Accessibility semantics,
CoreGraphics input, ScreenCaptureKit capture, NSPasteboard, Mach-O inspection,
per-user paths/IPC, and descriptor-relative filesystem confinement.

## Verification levels

The following evidence is intentionally separated:

1. **Linux regression** — the platformized tree must continue to pass the normal Rust
   workspace gates.
2. **Darwin cross-check** — shared Rust contracts can be typechecked for
   `aarch64-apple-darwin` and `x86_64-apple-darwin` without treating that as native
   execution. Native C/Swift linking remains an Apple-host gate.
3. **Native hosted macOS CI** — ARM64 and Intel jobs compile/link against the installed
   Apple SDK, execute noninteractive unit tests, and run the safe native smoke fixture.
4. **Interactive Mac acceptance** — TCC and real desktop interaction require a dedicated
   graphical Mac session and remain separate from hosted CI.
## Permissions and consent

Semwright does not bypass TCC. Accessibility and Screen Recording are user-controlled
permissions. Synthetic input is a fallback after semantic Accessibility operations, not
the primary interface. The product must report missing permission rather than editing the
TCC database, driving System Settings to approve itself, disabling SIP, or using private
Apple APIs.

No global keylogger or Input Monitoring dependency is introduced merely to post input.

## Driver and plugin isolation

Linux first-party/community driver execution remains digest-pinned and isolated with
bubblewrap + Landlock. macOS does not have a proven drop-in equivalent for dynamically
installed arbitrary child processes.

Accordingly, the macOS platform launcher remains fail-closed for arbitrary driver/plugin
execution until a supported isolation and code-identity model is demonstrated. Candidate
future designs may use signed bundled helpers/XPC/App Sandbox where those mechanisms are
compatible with the helper's authority, but the project does not label them equivalent to
Landlock/bubblewrap without evidence.
## Filesystem

Linux retains its `openat2` confinement. The initial macOS implementation uses a pinned
directory descriptor and currently provides a deliberately narrower guarantee for immediate
children. Nested paths that cannot be proven safe are rejected rather than authorized via
string canonicalization.

## What hosted CI does not prove

Even a green ARM64/Intel macOS workflow does **not** by itself prove:

- live AX behavior across real applications;
- TCC grant/revoke UX;
- CGEvent focus/delivery behavior;
- ScreenCaptureKit capture after user consent;
- multi-monitor/Retina coordinate behavior;
- sleep/wake and login-session recovery;
- installed LaunchAgent/SMAppService lifecycle;
- Developer ID signing/notarization;
- safe arbitrary third-party driver/plugin execution.

Those remain explicit live-Mac acceptance work.
