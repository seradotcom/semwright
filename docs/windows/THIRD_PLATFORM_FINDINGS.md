# Third-platform findings

## Raw AX role mapping in platform-api
Old assumption: two Unix-like hosts made a macOS role mapper look harmless in common code. Windows evidence: UIA has an entirely different ControlType vocabulary. Minimal change: normalized roles stay portable; AX mapping lives in platform-macos and UIA mapping in platform-windows. Linux impact: none intended. macOS impact: move only, preserve mapping.

## Sandbox destinations encoded as `/workspace` and `/etc`
Old assumption: sandbox target strings were Linux paths. Windows evidence: AppContainer/LPAC does not materialize a Linux namespace. The portable replacement is `MountClass + logical_name`; each host chooses materialization. Linux retains `/workspace/<logical>` and `/etc/<logical>`. Windows Driver and Plugin children consume the host-controlled `SEMWRIGHT_SANDBOX_MOUNTS_V1` table and receive only the owner-approved paths proven by native tests. Windows does not invent a shared `C:\workspace`. External MCP filesystem mounts remain blocked because third-party MCP binaries cannot transparently consume Semwright's mount table as the portable `/workspace/<name>` namespace.

## Unix identity in platform-services
Old assumption: `uid` and Unix peer credentials were universal. Windows evidence: identity is SID + access token + session and local transport is Named Pipe. Minimal change: compile Unix helpers only on Unix and expose Windows semantic principal/Windows transport primitives separately. Linux checks remain uid/mode/nlink based.

## Driver secure spawn
Old assumption: a launcher that returns `Command` is enough. Windows evidence: a safe child boundary needs pre-first-instruction AppContainer/LPAC capability setup, explicit inherited-handle lists, suspended creation, Job assignment before resume and process-tree cleanup. The shared platform spawn contract now returns a platform-owned sandbox process instead of exposing an unrestricted `Command`. Driver, Plugin and governed stdio MCP secure spawn have native Windows x64/ARM64 evidence for their supported authority profiles; unsupported profiles remain fail-closed without weakening Linux/macOS.

## Filesystem
Old assumption: pathname validation plus Unix primitives could be described uniformly. Windows evidence: reparse points, ADS, DOS devices, case/aliasing and HANDLE identity are distinct. Minimal result: new explicit confinement level `WindowsPinnedRootSingleChildReadOnly`; no false equivalence to openat2.
