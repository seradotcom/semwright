# Permissions and external confirmation

Owner configuration is read only at startup. It must be a single-link, user-owned regular
file with mode `0600`. Unknown settings are rejected. An agent cannot replace the profile,
provide a grant, choose a more powerful backend, or approve a request in the wire protocol.

| Profile | Baseline grant in this source |
|---|---|
| `observe` | Desktop/application/window/UI/process metadata |
| `desktop` | Observe + window management, semantic UI mutation, notifications |
| `workspace` | Same baseline; named filesystem roots must still be explicitly configured |
| `developer` | Same baseline; does not silently enable shell, plugins, or broad home access |

This is deliberately stricter than the blueprint's shorthand “desktop includes input”.
Input, clipboard, screen capture, app launching and application adapters require separate
`policy.allow` entries. `policy.deny` wins over baseline/explicit grants. `shell.exec` is
rejected because this build has no unrestricted-shell implementation. There is no unsafe
full-user profile.

```toml
[policy]
profile = "desktop"
apps = ["org.gnome.TextEditor"]
allow = ["input.keyboard", "input.pointer", "screen.capture"]
confirm_mutations = true
```

An application scope is exact, not fuzzy. Discover actual app IDs before configuring it.
No app list means no per-app restriction, not a magical automatic selection. Browser
commands use `org.semwright.Chromium`; Blender uses `org.blender.Blender` in this model.
Some native window managers expose class identifiers rather than desktop-file IDs.

Named filesystem grants use `[[policy.filesystem]]`, with `name`, canonical absolute
`path`, `read` and `write`. Roots may not overlap daemon runtime, state, configuration,
`/proc`, `/sys`, `/dev` or `/run`. A root must not be `/`. The Linux implementation uses
FD-relative resolution and atomic writes; it does not merely compare string prefixes.
See [security](security.md) for trusted-root and application-path limitations.

Risk classes `destructive`, `secret_access`, `code_execution` and `privilege_sensitive`
always need human approval after capability checks. `confirm_mutations=true` adds a gate
for ordinary mutations. Run the daemon **in the foreground** with `--approval-console`.
The operator types the exact challenge on that terminal. Other broker I/O is blocked
while the prompt is active. No `confirmed:true`, `--yes`, MCP elicitation response or
recipe input substitutes for this decision. A systemd user service has no approval
console and returns `ConsentRequired`.

Portal permission is an additional boundary. Approving a broker action does not dismiss
a compositor's chooser; approving a chooser does not grant every command in the broker.
Do not give a model direct access to the operator terminal or a separate unrestricted
shell and then expect this boundary to contain it.
