# Driver / Plugin sandbox on Windows

Windows untrusted-child execution is platform-owned. The secure sequence is: verify pinned source bytes -> stage a controlled copy -> construct a unique AppContainer/LPAC profile -> prepare transactional filesystem/tool grants -> create the child suspended with an explicit inherited-handle list, environment and working directory -> attach the child to a kill-on-close/resource-limited Job Object -> resume only after containment exists -> speak the unchanged Driver/Plugin/MCP stdio protocol -> revoke grants/delete the profile/terminate the process tree on shutdown or failure.

`WindowsSandbox::command` deliberately returns `SandboxDenied`; Windows confinement is available only through the platform-owned `spawn` path so no caller can run untrusted code before token/AppContainer attributes and Job containment are installed.

Native x64 and ARM64 CI currently proves secure spawn for Driver, Plugin and governed stdio MCP children. Driver authority additionally covers workspace, system-config, secrets, Host-mediated loopback, sealed tools and per-operation CPU budgets. Plugin workspace mounts are supported through the host-controlled `SEMWRIGHT_SANDBOX_MOUNTS_V1` materialization table. Ambient network is default-deny and owner-gated.

External MCP filesystem mounts remain fail-closed. Third-party MCP binaries expect the portable `/workspace/<name>` contract but do not consume the Semwright SDK mount table; Windows does not currently provide a transparent per-process POSIX-style mount namespace for that path. Do not replace this with global `C:\workspace`, junctions shared across children, DLL injection or path-rewrite hooks.

Hosted native CI is not evidence of UIA/input/capture interaction with a real desktop. Those rows are certified separately by the interactive Windows harness.
