# Repository Blueprint

The final coding agent may adjust crate boundaries if compilation or dependency design makes a different split clearly better, but it must preserve separation of concerns.

## Proposed repository tree

```text
<repo>/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── rustfmt.toml
├── clippy.toml
├── deny.toml
├── LICENSE-APACHE
├── LICENSE-MIT
├── README.md
├── CHANGELOG.md
├── CONTRIBUTING.md
├── CODE_OF_CONDUCT.md
├── SECURITY.md
├── GOVERNANCE.md
├── AGENTS.md
│
├── crates/
│   ├── types/                 # stable domain types and errors
│   ├── protocol/              # broker IPC protocol
│   ├── registry/              # command registry + schema metadata
│   ├── policy/                # permission/risk engine
│   ├── audit/                 # redacted structured audit
│   ├── broker/                # sessions, planner, references, execution
│   ├── backend-api/           # backend traits
│   ├── backend-atspi/         # semantic UI
│   ├── backend-portal/        # RemoteDesktop/ScreenCast/Clipboard
│   ├── backend-x11/           # EWMH/X11/XTEST fallback
│   ├── backend-gnome/         # bridge client + capability probing
│   ├── backend-kde/           # KWin bridge client
│   ├── backend-wlroots/       # common wlroots abstractions if useful
│   ├── backend-sway/
│   ├── backend-hyprland/
│   ├── system/                # safe system-level commands
│   ├── recipes/               # validated recipe engine
│   ├── plugin-sdk/            # public plugin types/client helpers
│   ├── plugin-host/           # launch/sandbox/handshake
│   ├── mcp/                   # MCP mapping only
│   ├── cli/                   # CLI binary
│   ├── daemon/                # user broker binary
│   └── tui/                   # inspector TUI
│
├── adapters/
│   ├── blender/
│   │   ├── rust-bridge/
│   │   ├── addon/
│   │   ├── README.md
│   │   └── tests/
│   ├── chromium/
│   │   ├── ...
│   │   └── README.md
│   └── example-plugin/
│
├── desktop/
│   ├── gnome-extension/
│   └── kwin-script/
│
├── recipes/
│   └── examples/
│
├── schemas/
│   ├── broker-protocol/
│   ├── plugin-manifest/
│   └── recipe/
│
├── tests/
│   ├── conformance/
│   ├── integration/
│   ├── fake-desktop/
│   └── fixtures/
│
├── fuzz/
│   ├── protocol/
│   ├── selectors/
│   └── recipes/
│
├── packaging/
│   ├── systemd-user/
│   ├── deb/
│   ├── rpm/
│   ├── nix/
│   └── install/
│
├── docs/
│   ├── architecture.md
│   ├── commands.md
│   ├── mcp.md
│   ├── plugins.md
│   ├── recipes.md
│   ├── security.md
│   ├── permissions.md
│   ├── wayland.md
│   ├── compatibility.md
│   ├── troubleshooting.md
│   ├── manual-testing.md
│   └── development.md
│
├── scripts/
│   ├── ci/
│   ├── release/
│   └── dev/
│
└── .github/
    ├── workflows/
    ├── ISSUE_TEMPLATE/
    ├── PULL_REQUEST_TEMPLATE.md
    └── dependabot.yml
```

## Crate responsibilities

### `types`

No OS side effects. Stable:
- command names;
- refs;
- errors;
- risk types;
- capability types;
- normalized UI nodes;
- result envelope.

High unit/property coverage.

### `protocol`

IPC frames, versioning, request/response/events.

Fuzz aggressively.

### `registry`

Schemas and command descriptors.

Supports discovery without loading all implementation code into model context.

### `policy`

Pure decision logic where possible. Golden tests.

### `broker`

Orchestration only:
- session;
- refs;
- policy enforcement;
- planner;
- dispatch;
- cancellation;
- audit hooks.

### `backend-api`

Traits like:

```rust
trait UiBackend { ... }
trait WindowBackend { ... }
trait InputBackend { ... }
trait ScreenBackend { ... }
```

Capabilities are explicit.

### backend crates

OS integration only. No MCP concepts.

### `recipes`

Pure validated execution graph plus broker calls.

### `plugin-sdk`

Public and versioned; must be pleasant enough for third-party developers.

### `mcp`

Thin adapter:
MCP request -> broker command -> MCP response.

### `cli`

Thin adapter:
args -> broker command -> format.

### `daemon`

Wires components, config, Unix socket and systemd lifecycle.

## Dependency direction

Avoid cycles.

Conceptually:

```text
types
 ↑
protocol registry policy backend-api recipes plugin-sdk
 ↑
broker
 ↑
daemon
 ↑
cli/mcp/tui clients connect over IPC
```

Do not let backends depend on MCP.

## Config

Follow XDG paths:

- config: `$XDG_CONFIG_HOME/<project>/`
- state: `$XDG_STATE_HOME/<project>/`
- cache: `$XDG_CACHE_HOME/<project>/`
- runtime: `$XDG_RUNTIME_DIR/<project>/`

Provide `computerctl config paths`.

## Public docs

The actual repository should be documented in **English** for global adoption.

README should have:
1. one-sentence value proposition;
2. demo command block;
3. semantic-vs-visual diagram;
4. install;
5. 5-minute quickstart;
6. supported desktop matrix;
7. MCP setup;
8. security model summary;
9. plugin link;
10. roadmap/support status.

Do not overmarket unverified support.

## Branding quality

Before final release:
- choose unique name;
- simple logo/icon optional but not a blocker;
- consistent command examples;
- asciinema/demo GIF may be documented as future capture if unavailable in build environment;
- screenshots should not be fabricated.

## License

Preferred: dual MIT OR Apache-2.0 for the original project code, subject to dependency/license review.

Do not copy code from reference projects without preserving/complying with their licenses.
