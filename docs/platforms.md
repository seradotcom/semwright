# Platform architecture and support status

Current policy separates **native package availability**, **hosted automated verification** and
**physical/interactive certification**. The initial-v1 package set targets Linux x86_64/aarch64,
Windows x86_64/ARM64 and macOS arm64/x86_64. Candidate availability is not a public release or a
universal desktop-support guarantee. R06/R18 residuals are OPEN/deferred post-v1 and no longer
initial-v1 admission prerequisites; independent security review remains mandatory. See
[release policy](release-policy.md) and [installation](installation.md).

## Recorded implementation/evidence matrix


Semwright has a portable core and Linux, macOS and Windows hosts. Implementation, native
CI and interactive desktop certification are separate claims. This matrix describes source
`6491c0d838fa066938a494524d69ed507aa0dbe8`, not every later revision.

| Surface | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Shared broker/policy/provider contracts | Implemented | Implemented | Implemented |
| Desktop/application interfaces | AT-SPI and compositor/X11 backends | AX/CoreGraphics host | UIA/native host |
| Capture/clipboard | Portal/PipeWire/session routes | ScreenCaptureKit/NSPasteboard, subject to consent | Native capture/clipboard; interactive acceptance separate |
| Filesystem services | Pinned-root/openat2 | Native descriptor-relative mechanisms and limits | Native root-relative mechanisms and limits |
| Driver/plugin execution | Bubblewrap + Landlock admission | Arbitrary payload execution remains fail-closed | Restricted/AppContainer profiles with native authority tests |
| Secondary runtime tools | Host-mediated tools/jobs/sessions | Verification does not authorize execution | Host-mediated profiles, explicit unsupported combinations |
| Real desktop evidence | Hosted and historical real-login cases; R06 physical residuals | Interactive TCC matrix pending | Unlocked-desktop R18 certification pending |

Windows is not a future-only platform. Run
[37096430846](https://github.com/seradotcom/semwright/actions/runs/37096430846) passed at the
exact snapshot, covering x64/ARM64 and selected sealed-tool compatibility jobs. It does not
close R18. The earlier ARM64 fixture failure remains historical evidence; a corrected run
does not rewrite that failure.

Engineering source `cd518748f742025a251b78028613aa1b16919e73` has separate domain/application
certificates. Read the [integration ledger](semantic-creation/INTEGRATION.md) without
relabeling them as executions on this snapshot. Linux evidence never certifies macOS or
Windows. A hosted UI fixture does not certify physical mixed-DPI displays.

## Evidence levels

Source review establishes what a revision implements and requires. Build/cross-checks
establish compilation for their recorded toolchain/target. Native hosted execution proves
only the exercised runtime and fixtures. Interactive acceptance requires an authorized
login session, native consent and actual display/input conditions. Physical hardware and
model-based productivity evaluation are separate scopes.

A job can be skipped while its workflow concludes successfully. Inspect job status,
checkout SHA, test selection and artifact provenance rather than a badge or workflow name.

## Unsupported boundaries

macOS must not modify TCC databases, disable SIP or use private sandbox facilities to
manufacture parity. Verification is not execution isolation. Linux retains required
confinement instead of falling back to direct launch. Windows external MCP mounts remain
blocked where transparent path virtualization is not proven. Runtime sessions cannot
approximate unsupported aggregate per-operation CPU accounting.

See [runtime tools](runtime-tools.md), [Windows verification](windows/WINDOWS_VERIFY.md),
[release blockers](../RELEASE_BLOCKERS.md), [compatibility](compatibility.md) and
[manual acceptance](manual-testing.md). Support is development-only; no production SLA
or universal application certification is implied.
