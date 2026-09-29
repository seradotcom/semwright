# Secondary runtime tools

Semwright treats application runtimes and helper executables as explicit owner-granted tools, never as ambient programs discovered from PATH or platform install conventions.

For one-shot tools, drivers should call `DriverExecutionContext::execute_runtime_tool`. The SDK selects the platform-safe route: a Host-materialized sealed executable on Linux, or Driver Protocol v4 Host-mediated execution on Windows. Callers use a logical tool name and never receive a Windows installation path.

Portable runtimes may also require a read-only runtime directory for libraries/resources. That directory remains a separately named mount; executable authority stays in `Manifest.tools` and is SHA-256 pinned.

Linux currently materializes sealed tools inside the driver sandbox. Windows executes them through Driver Host and does not expose a direct executable path to the driver. macOS executable verification exists, but arbitrary driver/plugin sandbox execution remains fail-closed, so this document does not claim macOS runtime-tool acceptance.

Blender already avoids ambient discovery but still needs a persistent runtime session; it therefore remains on `tool_path("blender")` until a Host-managed session primitive exists. Godot, MLT/ffprobe and other one-shot runtime consumers should migrate to `execute_runtime_tool` as their invocation requirements are normalized.

`scripts/verify-driver-runtime-tools.py` rejects new production driver code that embeds common Linux, Windows or macOS application installation paths or shell-style executable discovery. Test fixtures and developer scripts are outside that production-source guard.
