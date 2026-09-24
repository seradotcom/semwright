# Third-platform findings

## Raw AX role mapping in platform-api
Old assumption: two Unix-like hosts made a macOS role mapper look harmless in common code. Windows evidence: UIA has an entirely different ControlType vocabulary. Minimal change: normalized roles stay portable; AX mapping lives in platform-macos and UIA mapping in platform-windows. Linux impact: none intended. macOS impact: move only, preserve mapping.

## Sandbox destinations encoded as `/workspace` and `/etc`
Old assumption: sandbox target strings were Linux paths. Windows evidence: AppContainer/LPAC does not materialize a Linux namespace. Minimal change in the replacement API: `MountClass + logical_name`; each host chooses materialization. Linux must render Workspace as its existing `/workspace/<logical>` and SystemConfig as its existing `/etc/<logical>` while retaining all old validation. Windows does not invent `C:\workspace`; arbitrary driver spawn stays fail-closed.

## Unix identity in platform-services
Old assumption: `uid` and Unix peer credentials were universal. Windows evidence: identity is SID + access token + session and local transport is Named Pipe. Minimal change: compile Unix helpers only on Unix and expose Windows semantic principal/Windows transport primitives separately. Linux checks remain uid/mode/nlink based.

## Driver secure spawn
Old assumption: a launcher that returns `Command` is enough. Windows evidence: a safe child boundary needs pre-first-instruction token/job/handle-list ordering. Minimal generic change still required: platform-owned `SecureChild`/spawn contract. Until then Windows returns SandboxDenied; no Linux/macOS weakening.

## Filesystem
Old assumption: pathname validation plus Unix primitives could be described uniformly. Windows evidence: reparse points, ADS, DOS devices, case/aliasing and HANDLE identity are distinct. Minimal result: new explicit confinement level `WindowsPinnedRootSingleChildReadOnly`; no false equivalence to openat2.
