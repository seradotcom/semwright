# Driver / Plugin sandbox on Windows

Desired secure sequence: verify trusted source bytes -> stage controlled immutable copy -> create child suspended with explicit handle list/environment/cwd -> use AppContainer or LPAC/restricted token as justified -> assign a kill-on-close/resource-limited Windows Job Object -> apply compatible process mitigations -> resume -> speak the unchanged Driver Protocol over stdio -> terminate entire process tree on host loss.

The source contains PE verification and Job Object primitives, but does not pretend that those alone are a sandbox. The current shared Semwright launcher API returns a `tokio::process::Command`; at that point it cannot guarantee job assignment and token construction before first instruction. Accordingly `WindowsSandbox::command` returns SandboxDenied and reports mechanism `unavailable:windows-appcontainer-lpac-job-preexec-contract`.

The generic fix should introduce a platform-owned secure-spawn abstraction rather than special-case Windows inside Driver Protocol. Linux bubblewrap/Landlock and macOS behavior must not be weakened. Once secure-spawn exists, AppContainer/LPAC capability grants, network denial by default, workspace grants, handle inheritance and Job limits can be wired and tested natively.
