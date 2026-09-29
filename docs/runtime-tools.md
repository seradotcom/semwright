# Secondary runtime tools

Semwright treats application runtimes and helper executables as explicit owner-granted tools, never as ambient programs discovered from PATH or platform install conventions.

For one-shot tools, drivers should call `DriverExecutionContext::execute_runtime_tool`. The SDK selects the platform-safe route by negotiated protocol: Linux v4 can consume a Host-materialized sealed executable, while protocol v5 uses Host mediation on Linux and Windows. Callers use a logical tool name rather than an installation path.

Portable runtimes may also require a read-only runtime directory for libraries/resources. That directory remains a separately named mount; executable authority stays in `Manifest.tools` and is SHA-256 pinned.

Protocol v5 adds an optional per-tool workspace allowlist and a logical working directory. The working directory is the root of one declared workspace mount; nested relative paths are deliberately rejected until every platform can resolve them without symlink/reparse races. On Windows, Driver Host constructs a short-lived AppContainer child with only the workspace mounts named by that tool and keeps the resolved working-directory path host-only.

Linux protocol v4 keeps the compatibility path that materializes sealed tools inside the driver sandbox. Protocol v5 switches Linux to Host-mediated invocation: Driver Host stages the verified tool separately, launches a short-lived nested sandbox, and grants only the workspace mounts listed for that tool. Windows v5 provides the same logical per-tool mount contract through Host-mediated AppContainer execution. macOS executable verification exists, but arbitrary driver/plugin sandbox execution remains fail-closed, so this document does not claim macOS runtime-tool acceptance.

Godot is the first production consumer of the v5 one-shot boundary: its runner requests the logical `godot` tool through Driver Host and production configuration resolves project/output/secret authority by logical grant names. This demonstrates the generic path without implying that real Godot acceptance has been certified on every host OS.

## Remaining runtime migration classes

| Driver/runtime | Lifetime / shape | Current legacy resolver | Generic primitive required |
| --- | --- | --- | --- |
| Blender | Persistent background application session | `tool_path("blender")` | Host-managed persistent runtime session |
| LibreOffice | Persistent soffice + UNO/Python bridge | fixed `/usr/bin/python3`, `/usr/bin/soffice`, `/usr/bin/sh` | Host-managed runtime bundle + persistent session |
| MLT | Render/probe work may outlive the initiating request | private `runtime.json` for melt/ffprobe/bwrap | Host-owned detached runtime job + multi-tool bundle |
| Motion Canvas | Async render job with Node helper + browser | private `runtime.json` | Host-owned detached runtime job + multi-tool bundle |
| Godot runner | Request-scoped one-shot tool | generic v5 runtime-tool boundary | migrated in the stacked runtime-tool work |

Persistent sessions and detached jobs are intentionally not emulated with request-scoped `execute_runtime_tool`: doing so would change cancellation and lifetime semantics. Those are distinct Host primitives that should reuse the same logical tool/mount authority rather than adding per-driver path resolvers.

`scripts/verify-driver-runtime-tools.py` is a ratchet. It rejects new production driver code that embeds common OS installation paths, calls `tool_path()` directly, reads a private `runtime.json`, or names `/plugin/tools/`. The exact six historical exceptions above are counted; they may disappear as migrations land but cannot grow silently. Test fixtures and developer scripts remain outside that production-source guard.
