# Troubleshooting without guessing

| Symptom | Check | Safe next action |
|---|---|---|
| `cargo` not found | Toolchain actually installed and in PATH | Install an official toolchain; no binary is bundled here |
| `--locked` fails | Cargo.lock is absent in this development archive | Run the explicit bootstrap, review lock/toolchain, retry full gates |
| Root startup rejected | UID and whether a real or fake daemon was requested | Run as the graphical login user; do not remove the safety check |
| IPC unavailable | Broker running, XDG runtime, exact socket, UID/mode | Use `computerctl config paths`; start foreground broker and inspect stderr |
| Socket already exists | Another live broker/session bus owner | Stop your own prior broker; do not blindly delete the socket |
| Configuration denied | Regular single-link file, owner and `0600`; correct top-level keys | Fix your explicit config; unknown keys are rejected |
| Read works, mutation fails | `policy.allow`, application scope and risk | Review owner configuration, not model-provided confirmation fields |
| `ConsentRequired` in a service | No controlling operator console | Stop service; use an explicitly foreground approved operation |
| Portal chooser missing/cancelled | Matching portal/backend in your real login session | Inspect portal services and retry only after explicit user intent |
| AT-SPI empty/partial tree | Accessibility service/app support, node/depth budget | Use a known accessible fixture; do not infer there are no controls |
| `StaleReference` | Same session, time elapsed, app/tree generation | Re-observe and resolve intent; never force-use or rewrite a stale ID |
| Duplicate `Save` controls | Discovery count/candidates and ancestry | Refine exact semantics or choose a verified ref; do not pick the first |
| Window input refused | Expected window focus and explicit input grants | Re-observe focus; do not silently route to global input |
| KWin snapshot stale | Broker/script lifecycle and heartbeat | Restart your own script after broker startup; inspect current snapshot |
| Chromium origin denied | Exact scheme/host/port in owner allowlist | Use a trusted local test origin, not a wildcard/normal-profile debugger |
| Plugin `SandboxDenied` | bubblewrap, user namespaces, Landlock and helper | Validate platform support; never replace with direct execution |
| Unknown action outcome | Timeout/cancel/disconnect after dispatch | Inspect application state before retrying; no success/rollback assumption |
| Audit cannot be written | Owner/mode, space and bounded rotation | Restore the audit sink before authorizing more work |

`doctor` reads runtime conditions, but the uncompiled source and compatibility matrix
remain separate verification facts. Do not “fix” an unavailable feature by returning fake
results. Review only redacted metadata in bug reports. Browser cookies, clipboard text,
session tickets and private screenshots should not appear in issue trackers.
