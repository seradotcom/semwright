# Platform architecture and support status

Current policy separates **native package availability**, **hosted automated verification** and
**physical/interactive certification**. The initial-v1 package set targets Linux x86_64/aarch64,
Windows x86_64/ARM64 and macOS arm64/x86_64. Candidate availability is not a public release or a
universal desktop-support guarantee. R06/R18 residuals are OPEN/deferred post-v1 and no longer
initial-v1 admission prerequisites; independent security review remains mandatory. See
[release policy](release-policy.md) and [installation](installation.md).

## Current implementation/evidence matrix

Semwright has a portable core and Linux, macOS and Windows hosts. Implementation, native CI and
interactive desktop certification are separate claims. Exact execution evidence is source-scoped;
the current ledger is [VERIFY.md](../VERIFY.md), with platform-specific detail under `docs/windows/`
and the subsystem documentation.

| Surface | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Shared broker/policy/provider contracts | Implemented | Implemented | Implemented |
| Desktop/application interfaces | AT-SPI and compositor/X11 backends | AX/CoreGraphics host | UIA/native host |
| Capture/clipboard | Portal/PipeWire/session routes | ScreenCaptureKit/NSPasteboard, subject to consent | Windows.Graphics.Capture/clipboard plumbing; interactive acceptance separate |
| Filesystem services | Pinned-root/openat2 | Native descriptor-relative mechanisms and limits | HANDLE-relative confinement with reparse/hardlink defenses |
| Driver/plugin execution | Bubblewrap + Landlock admission | Arbitrary payload execution remains fail-closed | Platform-owned AppContainer/LPAC secure spawn + Job containment for supported profiles |
| Secondary runtime tools | Host-mediated tools/jobs/sessions | Verification does not authorize unsupported execution | Host-mediated profiles with explicit unsupported combinations |
| Native SDK portable cooperation | Native CI + Linux real-Host E2E | Portable library CI; Host E2E not claimed | Portable library CI; Host E2E not claimed |
| Real desktop evidence | Hosted and historical real-login cases; R06 physical residuals | Interactive TCC matrix pending | Unlocked-desktop certification remains environment-dependent post-v1 |

Windows is not a future-only platform. On source
`04cf0ef7062d134b71206d832375be9545553bb4`, public run
[37228279724](https://github.com/seradotcom/semwright/actions/runs/37228279724) passed native
x64, native ARM64 and both sealed-tool compatibility jobs. That is noninteractive native-host
evidence, not unlocked-desktop certification. Historical Windows failures remain in the review
records as evidence of what was corrected; they are not the current support statement.

The Native SDK's public six-runner portability milestone
[37179820287](https://github.com/seradotcom/semwright/actions/runs/37179820287) passed Ubuntu
x64/ARM64, Windows x64/ARM64 and macOS arm64/x64. Its real daemon/Broker/Driver Host application
path is separately exercised on Linux; portable library success is not inferred to mean equivalent
Host/application certification on Windows or macOS.

## Evidence levels

Source review establishes what a revision implements and requires. Build/cross-checks establish
compilation for their recorded toolchain/target. Native hosted execution proves only the exercised
runtime and fixtures. Interactive acceptance requires an authorized login session, native consent
and actual display/input conditions. Physical hardware and model-based productivity evaluation are
separate scopes.

A job can be skipped while its workflow concludes successfully. Inspect job status, checkout SHA,
test selection and artifact provenance rather than a badge or workflow name.

## Unsupported boundaries

macOS must not modify TCC databases, disable SIP or use private sandbox facilities to manufacture
parity. Verification is not execution isolation. Linux retains required confinement instead of
falling back to direct launch. Windows external MCP filesystem mounts remain
`BLOCKED_PORTABLE_PATH_VIRTUALIZATION` where transparent path virtualization is not proven.
Runtime sessions cannot approximate unsupported aggregate per-operation CPU accounting.

See [runtime tools](runtime-tools.md), [Windows verification](windows/WINDOWS_VERIFY.md),
[Native SDK compatibility](native-sdk/COMPATIBILITY.md), [release blockers](../RELEASE_BLOCKERS.md),
[compatibility](compatibility.md) and [manual acceptance](manual-testing.md). Support is
development-only; no production SLA or universal application certification is implied.
