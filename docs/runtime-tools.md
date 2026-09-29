# Secondary runtime tools

Semwright treats application runtimes and helper executables as explicit owner-granted tools, never as ambient programs discovered from PATH or platform install conventions.

For one-shot tools, drivers should call `DriverExecutionContext::execute_runtime_tool`. The SDK selects the platform-safe route by negotiated protocol: Linux v4 can consume a Host-materialized sealed executable, while protocol v5 uses Host mediation on Linux and Windows. Callers use a logical tool name rather than an installation path.

Portable runtimes may also require a read-only runtime directory for libraries/resources. That directory remains a separately named mount; executable authority stays in `Manifest.tools` and is SHA-256 pinned.

Protocol v5 adds an optional per-tool workspace allowlist and a logical working directory. The working directory is the root of one declared workspace mount; nested relative paths are deliberately rejected until every platform can resolve them without symlink/reparse races. On Windows, Driver Host constructs a short-lived AppContainer child with only the workspace mounts named by that tool and keeps the resolved working-directory path host-only.

Linux protocol v4 keeps the compatibility path that materializes sealed tools inside the driver sandbox. Protocol v5 switches Linux to Host-mediated invocation: Driver Host stages the verified tool separately, launches a short-lived nested sandbox, and grants only the workspace mounts listed for that tool. Windows v5 provides the same logical per-tool mount contract through Host-mediated AppContainer execution. macOS executable verification exists, but arbitrary driver/plugin sandbox execution remains fail-closed, so this document does not claim macOS runtime-tool acceptance.

Blender already avoids ambient discovery but still needs a persistent runtime session; it therefore remains on `tool_path("blender")` until a Host-managed session primitive exists. Godot, MLT/ffprobe and other one-shot runtime consumers should migrate to `execute_runtime_tool` as their invocation requirements are normalized.

`scripts/verify-driver-runtime-tools.py` rejects new production driver code that embeds common Linux, Windows or macOS application installation paths or shell-style executable discovery. Test fixtures and developer scripts are outside that production-source guard.
