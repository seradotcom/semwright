<h1 align="center"><img src="./docs/assets/semwright-mark.svg" alt="" width="44" height="44" />&nbsp;Semwright</h1>

<p align="center">
  <strong>Use real software from AI agents.</strong>
</p>

<p align="center">
  An open runtime that connects AI agents to desktop and professional applications through<br />
  structured operations, native APIs, and governed system interfaces.
</p>

<p align="center">
  <a href="./LICENSE-MIT"><img alt="License: MIT OR Apache-2.0" src="https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-234ea2"></a>
  <a href="./Cargo.toml"><img alt="Rust" src="https://img.shields.io/badge/Rust-runtime-000000?logo=rust&logoColor=white"></a>
  <a href="./docs/installation.md"><img alt="Native bundles: Linux, macOS, Windows" src="https://img.shields.io/badge/native%20bundles-Linux%20%7C%20macOS%20%7C%20Windows-234ea2"></a>
  <a href="./docs/mcp.md"><img alt="MCP frontend" src="https://img.shields.io/badge/MCP-frontend-6f42c1"></a>
  <a href="./docs/installation.md"><img alt="Status: pre-v1" src="https://img.shields.io/badge/status-pre--v1-e67e22"></a>
  <a href="./VERIFY.md"><img alt="Verification documented" src="https://img.shields.io/badge/verification-documented-2ea44f"></a>
</p>

<p align="center">
  <a href="./docs/installation.md"><strong>Installation</strong></a> ·
  <a href="https://semwright.com/docs/"><strong>Documentation</strong></a> ·
  <a href="./docs/drivers.md"><strong>Drivers</strong></a> ·
  <a href="./VERIFY.md"><strong>Verification</strong></a>
</p>

> **Pre-release:** native candidate bundles are for evaluation; no public v1 has been published.
> Physical/interactive residuals are explicit post-v1 certification work. Independent security
> review remains required before public release. [Status and evidence](#status-and-verification).

```text
AI agent  →  Semwright  →  Blender · Godot · Browser · LibreOffice · Figma · KiCad · …
```

## What can I do with it?

- **Connect compatible agents to real desktop and professional software through one runtime.**
- **Prefer structured application operations and native APIs over pixels and clicks when available.**
- **Move verified file-backed artifacts between integrations without bypassing broker policy.**
- **Keep mutations policy-gated, auditable, and bounded by explicit authority.**
- **Extend applications with the Driver SDK, or add narrow external commands with the Plugin SDK.**

A Semwright request can discover an application's structured capabilities, perform an authorized
operation, move an artifact between tools, and verify the result through the same broker.

For example, a cross-application workflow can look like this:

```text
Agent
  |
  |  "Change this asset and update the project."
  v
Semwright
  |
  +--> Blender: inspect / author / export
  |
  +--> artifact.handoff: verify + transfer
  |
  +--> Godot: import / rescan / update
  |
  `--> readback + Effects: verify the bounded outcome
```

The exact operations and evidence depend on each integration. Semwright does not pretend that one
headless test certifies an entire interactive application.

## Try Semwright

### Native bundle — no Rust build required

Use the matching candidate from an exact-SHA **V1 multiplatform distribution** Actions run.
There is no public v1 download in this closeout. Verify the external checksums, extract the archive,
and run its included helper:

| Platform | Bundle | Install from the extracted directory |
| --- | --- | --- |
| Linux x86_64 / aarch64 | `.tar.gz` or `.deb` | `./install.sh` (or the system package manager for `.deb`) |
| Windows x86_64 / ARM64 | `.zip` | `.\Install-Semwright.ps1` |
| macOS Apple Silicon / Intel | `.tar.gz` | `./install.sh` |

Every portable bundle includes **all five runtime commands**, required packaged native companions,
checksums and reversible user-local helpers. After the installer, run **`semwright setup`**: it creates
a private observe-only configuration plus a ready-to-copy MCP client snippet without granting
desktop authority, starting a service, or changing a third-party client. Windows/macOS bundles are
unsigned; no security-control bypass is provided.
[Exact commands, requirements and removal →](docs/installation.md#install-a-candidate-bundle)

### Build and try the synthetic desktop

The safest first run uses the repository's synthetic desktop. It exercises the real daemon, CLI,
recipe runner and policy path without connecting to your real desktop or credentials.

On Ubuntu 24.04 x86_64, install the
[development prerequisites](docs/installation.md#prerequisites), then:

```sh
git clone https://github.com/seradotcom/semwright.git
cd semwright

cargo build --locked -p semwright-daemon -p semwright-cli --bins
BIN_DIR=target/debug ./scripts/dev/fake-smoke.sh
```

The smoke will:

1. start an isolated fake Semwright daemon;
2. discover one exact `Export` control;
3. run a typed recipe through normal policy and dispatch;
4. report the observed `changed` result and audit metadata;
5. stop the daemon and remove the temporary runtime.

This is a functional first-use path, **not** live-desktop certification or a security verdict.

`Cargo.lock` is committed. Keep it and use `--locked`; do not run `scripts/dev/bootstrap.sh` on an
ordinary checkout. For the full build, per-user installation, portable package layout and uninstall
flow, use the [installation guide](docs/installation.md).

### After installation

The safe onboarding command is:

```sh
semwright setup
```

It is local and idempotent: existing configuration is preserved, a missing configuration is created
with the **observe-only** profile, and a ready-to-copy `mcp-client.json` snippet points at the exact
installed `semwright-mcp` binary. The native distribution gate exercises this setup path after
installing each platform bundle. It does **not** grant desktop/application authority, start a service,
modify a third-party MCP client, or bypass OS consent.

`semwright setup` prints the exact installed paths and next commands for the current platform. Start
the broker as your normal user in the graphical login session, then verify it from another terminal:

```sh
semwright --json doctor
semwright ui snapshot --max-nodes 100
semwright-inspect
```

Start with observe-only policy and a disposable environment. Sensitive mutations require explicit
grants and, where configured, a separate operator approval path.

## Connect your agent

Semwright exposes a deliberately small MCP frontend that routes back through the same broker,
policy, references and audit path as the CLI.

After installing `semwright-mcp` and starting the broker, a compatible MCP client can use an
absolute executable path such as:

```json
{
  "mcpServers": {
    "semwright": {
      "command": "/home/YOUR_USER/.local/share/semwright/bin/semwright-mcp"
    }
  }
}
```

The MCP process does not grant desktop authority, approve mutations or start the broker for you.
See [MCP](docs/mcp.md) for socket/session configuration and
[governed MCP federation](docs/mcp-federation.md) for connecting external MCP providers.

## Applications

Semwright has application-specific integrations in addition to generic desktop/platform backends.
The table below is intentionally compact; it describes the integration path, **not a blanket support
certificate**.

| Application / domain | Semwright path | Evidence boundary today |
| --- | --- | --- |
| **Blender** | First-party driver and semantic authoring/export | Driver/native authoring evidence exists; live/version coverage remains scoped |
| **Godot** | Driver + EditorPlugin + pinned runner | Production driver is exercised through Driver Host and pinned Godot CI; broader editor interaction remains scoped |
| **Chromium** | Private-profile CDP adapter | Real hosted browser integration exists on the Linux development line |
| **Figma** | Official Plugin API through authenticated loopback driver | Typed/fake-host/sandboxed CI exists; real Figma acceptance is separate |
| **LibreOffice** | First-party driver | Repository integration exists; per-application live coverage varies |
| **OBS Studio** | `obs-websocket` driver | Fake-server, sandbox and disposable read-only OBS paths are exercised |
| **MLT video** | Offline timeline/render driver | Semantic/render tests exist; arbitrary Kdenlive/Shotcut round trips are not implied |
| **KiCad** | Curated driver integration | Deterministic IPC/conformance exists; fake IPC is not a real KiCad interoperability certificate |
| **Motion Canvas** | Typed project/render driver | Deterministic model generation and bounded render jobs |
| **Audio** | Faust + Ardour drivers | Curated synthesis, analysis and managed-session paths with explicit coverage gaps |

Full details live in the [platform matrix](docs/platforms.md),
[compatibility matrix](docs/compatibility.md), [Driver SDK guide](docs/drivers.md) and each
integration's own README.

## Why Semwright?

### Native and structured operations first

When an application exposes a richer semantic interface, Semwright can use it instead of reducing
every task to screen coordinates. Generic accessibility/input paths remain separate capabilities
with their own preconditions and evidence.

### One runtime across agents

CLI, MCP, recipes and higher-level workflows enter the same broker. Changing the frontend does not
silently create another authorization system.

### Persistent project context

Semwright can retain typed project identity, dependencies and drift information so an agent can
reason about what changed and what became stale rather than rediscovering every project from zero.

### Verified outcomes

A successful process exit is not automatically a successful task. Semwright's Effects/evidence
model separates requested, expected, observed and unobservable outcomes within an explicit scope.

### Local, inspectable authority

Discovery is not permission. Providers, drivers and federated tools receive bounded identities;
mutations re-enter policy; sensitive actions can require a separate operator decision; execution is
audited.

## How it works

```text
                 compatible agent / MCP / CLI
                           |
                           v
                +-----------------------+
                |       Semwright       |
                | discovery + schemas   |
                | policy + approvals    |
                | refs + jobs + audit   |
                +-----------+-----------+
                            |
                    Provider Runtime
                            |
          +-----------------+------------------+
          |                 |                  |
          v                 v                  v
   native/platform      app drivers       federated MCP
      backends          + plugins          providers
          |                 |                  |
          +-----------------+------------------+
                            |
                            v
                    real applications
```

Semwright does **not** replace MCP or an agent SDK. MCP is one way to reach the runtime and one kind
of provider Semwright can govern. The execution layer is responsible for capability discovery,
policy, application identity, bounded jobs, references, artifact handoff and audit.

See [architecture](docs/architecture.md) for the full model.

## Reuse work and keep projects coherent

### Recipes — reuse a successful procedure

A typed Recipe captures a bounded multi-step procedure. Every step still re-enters normal broker
policy and reference validation.

[Learn about Recipes →](docs/recipes.md)

### Project Graph — know what became stale

Project Graph records persistent project identity, dependencies, derivations and drift. It can tell
higher-level workflows which outputs depend on which sources without turning stored identity into
permission.

[Learn about Project Graph →](docs/project-graph/INTEGRATION.md)

### Effects — verify what actually happened

Effects evaluates observations inside a declared scope. Missing readback stays unknown instead of
being promoted to a global success claim.

[Learn about Effects →](docs/effects/INTEGRATION.md)

## Build an integration

Choose the surface by what you are trying to connect:

| I want to… | Use |
| --- | --- |
| **Connect an existing application with a rich API or long-lived state** | [Application Driver SDK](docs/drivers.md) |
| **Add a narrow, stateless external command** | [Plugin SDK](docs/plugins.md) |
| **Call Semwright from an agent or tool** | [CLI](docs/commands.md) or [MCP frontend](docs/mcp.md) |
| **Bring an existing MCP server under the same broker** | [Governed MCP federation](docs/mcp-federation.md) |
| **Move a verified file-backed artifact between integrations** | `artifact.handoff` in the [Driver SDK](docs/drivers.md#cross-driver-artifact-handoff) |

Drivers and plugins do not get ambient authority by existing. Their manifests, executable identity,
resource limits and requested filesystem/network surfaces are validated before use, and each
capability still enters broker policy.

## Security

Semwright is designed so that **discovery does not imply permission** and an agent cannot approve
its own sensitive request.

Observe is the default. Input, clipboard contents, screenshots, application launching, plugins and
application-native mutation require explicit authority. A separately granted unrestricted shell can
bypass this mediated surface; sandboxing a child does not sandbox an already-running application;
there is no claim of universal prompt-injection immunity.

Read [SECURITY.md](SECURITY.md), [permissions](docs/permissions.md) and the
[threat model](docs/security.md). Report sensitive vulnerabilities through the repository's enabled
private vulnerability-reporting channel, not a public issue.

## Status and verification

Engineering completion and release certification are deliberately separate states.

| Gate | Current state |
| --- | --- |
| **R16** | **CLOSED** after separate revalidation |
| **R06** | **OPEN — `DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT`** |
| **R18** | **OPEN — `DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT`** |
| **V1_ENGINEERING_CLOSEOUT** | **COMPLETE** |
| **STAGING** | Engineering metadata admitted; native bundles require exact-SHA workflow evidence |
| **PUBLIC RELEASE** | **NOT PUBLISHED**; **BLOCKED_PENDING_SECURITY_REVIEW** |

R06 still requires the declared physical Hyprland/mixed-display cases. R18 still requires the
declared unlocked Windows interactive, UIPI/UAC, real-app UIA, mixed-DPI and lifecycle cases.
Those gaps are not simulated and are not converted into PASS by hosted CI. They are no longer
prerequisites for the initial-v1 publication decision. Independent security review, explicit
maintainer authorization and final exact-SHA validation remain mandatory under
[the release policy](docs/release-policy.md).

Start with:

- [V1 engineering closeout](V1_ENGINEERING_CLOSEOUT.md)
- [Post-v1 environment-dependent procedures](POST_V1_BACKLOG.md)
- [Verification ledger](VERIFY.md)
- [Release blockers](RELEASE_BLOCKERS.md)
- [Compatibility and evidence levels](docs/compatibility.md)
- [Security](SECURITY.md)

Historical records under `verification/` remain evidence for their recorded source and environment;
they are not approval of later code.

## Documentation and contributing

- [Installation and removal](docs/installation.md)
- [Troubleshooting](docs/troubleshooting.md)
- [Architecture](docs/architecture.md)
- [Platform support](docs/platforms.md)
- [Application Driver SDK](docs/drivers.md)
- [Agent Skills](docs/skills.md)
- [Events and jobs](docs/events-jobs.md)
- [Workflow Distillation](docs/workflow-distillation.md)
- [Development](docs/development.md)
- [Contributing](CONTRIBUTING.md)
- [Support](SUPPORT.md)

Focused pull requests are welcome. Keep technical and verification claims bound to the exact source,
environment and scope that produced the evidence.

## License

Original core source is **MIT OR Apache-2.0**.

The isolated `integrations/kicad-driver` subtree is **GPL-3.0-or-later** with its own notices.

See [governance](GOVERNANCE.md), [changelog](CHANGELOG.md) and
[original requirements](docs/requirements/START_HERE.md).
