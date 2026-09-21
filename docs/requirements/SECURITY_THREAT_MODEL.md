# Security and Threat Model

## Security objective

An LLM or automation client must not automatically inherit all privileges of the interactive Linux user.

The project should make **least privilege usable**.

## Threat actors / failure modes

1. A model chooses the wrong command.
2. Prompt injection inside a document/webpage attempts to trigger unrelated actions.
3. A plugin is malicious or compromised.
4. A command argument attempts path traversal or symlink escape.
5. A stale UI reference targets a different control.
6. A low-level input fallback acts on the wrong focused window.
7. Audit logs accidentally store secrets.
8. An MCP client is compromised.
9. A portal/session permission changes during execution.
10. A recipe performs a destructive action after state drift.

## Trust boundaries

```text
LLM/MCP client
      │ untrusted intent
      ▼
CLI/MCP adapter
      │
      ▼
Broker ───────── policy/audit ───── trusted core
      │
      ├── system backends
      ├── desktop backends
      └── plugins ───────────────── less trusted / sandboxed
```

The broker is the authority.

## No root by default

The daemon runs as the current user.

Do not ship:
- setuid binary;
- always-on root daemon;
- root shell helper.

If optional uinput setup needs elevated one-time configuration, document it and make it separable from normal runtime.

## Permission profiles

Built-in profiles:

### `observe`

Allowed:
- app/window metadata;
- AT-SPI tree;
- process metadata within sensible limits.

Denied:
- keyboard/pointer;
- mutations;
- clipboard contents;
- filesystem contents unless explicitly scoped;
- shell.

### `desktop`

Adds:
- window management;
- UI invoke/type;
- pointer/keyboard through approved backends;
- clipboard write/read with explicit policy.

### `workspace`

Adds:
- filesystem only under configured workspace roots;
- process execution from allowlist;
- no unrestricted home access.

### `developer`

Can opt into:
- restricted shell;
- plugin development;
- broader filesystem.

### `unsafe-full-user`

If implemented, it must be explicit and visually noisy. Never default.

## Capability grants

Grants should be:
- session-scoped by default;
- app-scoped where possible;
- path-scoped for filesystem;
- time-bounded optionally;
- revocable.

Example policy:

```toml
[profile.workspace]
allow = [
  "desktop.observe",
  "window.manage",
  "ui.observe",
  "ui.invoke",
  "input.keyboard"
]

[[filesystem]]
path = "/home/user/projects/foo"
read = true
write = true

[shell]
enabled = false
```

## Risk classes

- `read_only`
- `mutating_reversible`
- `mutating`
- `destructive`
- `secret_access`
- `code_execution`
- `privilege_sensitive`

Commands declare their class.

Policy may require confirmations by class.

## Confirmation gates

Confirmation must be enforced by the broker.

Examples:
- delete files;
- close app with unsaved work when detectable;
- read clipboard if policy marks it sensitive;
- start screen capture;
- launch low-level input helper;
- enable arbitrary shell;
- install untrusted plugin.

Support:
- once;
- for this command;
- for this app;
- for this session.

Never let the model answer its own confirmation prompt.

## Filesystem safety

Requirements:
- path normalization;
- canonicalization where appropriate;
- prevent `..` escape;
- detect symlink escape relative to scoped roots;
- use directory FDs / openat-style safe operations where feasible;
- safe temp files;
- restrictive permissions;
- atomic writes where appropriate;
- size limits.

Sensitive default-deny examples:
- `~/.ssh`
- browser profiles/cookies;
- password stores;
- cloud credentials;
- `.env` outside permitted workspace;
- `/proc/*/environ` style secret leakage.

Do not rely only on string blacklists.

## Shell safety

Shell is optional and disabled by default.

If enabled:
- prefer executable + argv, not `sh -c`;
- executable allowlist/denylist policy;
- scoped cwd;
- scrub environment;
- timeout;
- max output;
- no implicit sudo;
- no shell interpolation in recipes;
- risk class `code_execution`.

## Prompt injection containment

A web page or document is data, not authority.

The broker cannot solve all prompt injection, but it can reduce blast radius:
- browser adapter cannot automatically gain filesystem capability;
- UI read capability does not imply shell;
- cross-app actions can require policy;
- sensitive capability use can require confirmation;
- commands record provenance.

## Plugin sandboxing

### Landlock

Use Landlock when available to add kernel-enforced filesystem restrictions to plugin processes. Detect ABI/support at runtime.

### Bubblewrap

Optional stronger process/filesystem/network isolation when installed/available.

### seccomp/no_new_privs

Use where practical for helpers/plugins, with a maintainable profile. Do not ship an over-broad brittle seccomp policy without tests.

### environment

Plugins receive a minimal environment. Do not pass:
- SSH agent socket;
- cloud tokens;
- arbitrary API keys;
- browser secrets;
unless a plugin permission explicitly needs them.

## Wayland consent

Do not try to bypass XDG portal user consent.

Represent permission state:

```text
not_requested
pending
granted
denied
expired
```

Persistent restore tokens must be:
- stored with restrictive permissions;
- treated as sensitive capability material;
- revocable;
- documented.

## Low-level input hazards

Before input fallback:
- verify target window/focus where backend permits;
- record expected window;
- fail if focus changed and action is sensitive;
- prefer semantic invoke over coordinates.

Pointer coordinates must carry coordinate-space metadata.

## Screen capture

Screenshots can contain secrets.

Default:
- no capture unless explicitly requested or fallback allowed;
- temp artifacts `0600`;
- expiry;
- no audit retention of image bytes;
- warning when capture spans entire monitor.

## Clipboard

Treat clipboard contents as potentially sensitive.

- separate `clipboard.read` and `clipboard.write`;
- do not log contents;
- allow policy to deny reads while allowing writes.

## Audit log security

Store metadata, not sensitive payloads.

Redaction system:
- secret-typed args automatically redacted;
- command-specific redactors;
- configurable retention;
- file permissions `0600`.

## Local IPC

- Unix socket under `$XDG_RUNTIME_DIR`;
- mode `0600`;
- validate peer UID;
- no public TCP listener by default;
- protocol version negotiation;
- request limits;
- malformed frame fuzz testing.

## Supply-chain security

Repository must include:
- lockfile;
- `cargo audit`;
- `cargo deny`;
- dependency license policy;
- GitHub dependency review if available;
- reproducible-ish release instructions;
- checksums for release assets;
- signed release option/Sigstore if practical;
- no install script that downloads unsigned opaque binaries.

## Security documentation

Ship:
- `SECURITY.md`;
- threat model;
- permissions reference;
- vulnerability disclosure process;
- secure deployment examples;
- “what this does not protect against”.

## Important limitation

If the user explicitly grants an unrestricted shell running as their own user, the shell can usually access whatever that user can access. The project must state clearly that policy-scoped commands provide stronger control than an unrestricted shell.
