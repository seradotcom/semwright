# Semwright

Typed, policy-gated commands for desktop applications and agent workflows.

Semwright connects a CLI or MCP client to semantic desktop interfaces and application
APIs. An agent can discover a named control, author a scene, or pass a verified artifact
between applications without treating every operation as a guessed mouse click.
The broker checks schemas, permissions, references and provenance before dispatch.

> **Development software, 0.9.0-dev.1. R16 is CLOSED after separate revalidation.**
> R06/R18 physical/interactive certification remains OPEN and deferred to the required environments.
> Start with the isolated fake-desktop example, not a credential-rich desktop.
> Native CI, package certification, interactive desktop evidence and release approval are different things.
> See [verification](VERIFY.md), [release blockers](RELEASE_BLOCKERS.md) and [security](SECURITY.md).

[Start here](docs/installation.md) · [Architecture](docs/architecture.md) ·
[Platform support](docs/platforms.md) · [Application drivers](docs/drivers.md) ·
[Contribute](CONTRIBUTING.md)

## Try a semantic operation

The repository includes a fake desktop and an export recipe. The recipe discovers one
exact Export button, asserts that the result is unique, and invokes the returned reference.
It runs through the real daemon, CLI, recipe runner and policy path, but its application
is a fixture. It does not export a real Blender or Godot project.

On a disposable Ubuntu 24.04 environment, install the
[prerequisites](docs/installation.md#prerequisites) and use the pinned source instructions
in the installation guide. From that checkout:

```sh
cargo build --locked -p semwright-daemon -p semwright-cli --bins
BIN_DIR=target/debug ./scripts/dev/fake-smoke.sh
```

The smoke starts only `--fake`, uses its own private temporary runtime and socket,
performs discovery and the recipe, prints metadata audit, stops its daemon and removes
its temporary directory. A successful recipe reports the `changed` result. This is a
functional smoke test, not live-desktop certification or a security verdict.

`Cargo.lock` is already committed. Do not run `scripts/dev/bootstrap.sh` for an ordinary
checkout: it is a dependency-initialization tool that deliberately refuses an existing
lockfile. Keep the lock and use `--locked`.

## How requests flow

```text
CLI / MCP / inspector / recipes
                |
                v
       Broker: schema, policy, approval, refs, audit
                |
       Capability Registry / Provider Runtime
                |
       platform hosts / Driver Host / federated MCP
       Linux, macOS, Windows / pinned tools and runtimes
                |
            applications
```

Discovery describes support; it does not grant permission. A Skill is procedural knowledge,
not an executable authority source. A recipe re-enters the broker for each step. Choosing
another frontend does not create a separate policy path.

Composition adds typed plans and bounded attempts. Project Graph records persistent identity,
dependencies and drift; it does not turn a stored object identity into permission to mutate
it. Effects evaluates observations within their declared scope. Missing or incomplete
observations must not be described as a verified global postcondition.

See [architecture](docs/architecture.md), [Composition](docs/composition/INTEGRATION.md),
[Project Graph](docs/project-graph/INTEGRATION.md) and [Effects](docs/effects/INTEGRATION.md).

## Applications and platforms

The source includes desktop backends and application providers for Blender, Chromium,
LibreOffice, OBS, MLT, Figma, Godot, Motion Canvas, KiCad and audio workflows.
Their supported operations, runtime requirements and evidence differ. Installing a driver
or adding an MCP definition neither installs its application nor authorizes its commands.

| Scope | What to check before use |
| --- | --- |
| Linux | Native backend, kernel/sandbox prerequisites, application versions and exact-SHA evidence. Physical desktop gaps remain under R06. |
| macOS | Native host and executable verification exist; interactive TCC acceptance is separate. Arbitrary Driver/Plugin Host execution remains fail-closed. |
| Windows | UIA and native host tests exist on x64/ARM64. An unlocked-desktop certification bundle and residual R18 cases are still required. |
| Application workflows | Read the driver's guide and integrated evidence record. A headless native test does not certify every operation or the whole interactive application. |

[Platform matrix](docs/platforms.md) · [Compatibility](docs/compatibility.md) ·
[Host-managed runtimes](docs/runtime-tools.md) ·
[Integrated engineering evidence](docs/semantic-creation/INTEGRATION.md)

## Connect an MCP client

After building/installing the MCP frontend and starting the broker separately in the same
user session, configure your client with the absolute path to `semwright-mcp`:

```json
{
  "mcpServers": {
    "semwright": {"command": "/home/YOUR_USER/.local/bin/semwright-mcp"}
  }
}
```

The configuration does not start the daemon, approve a mutation, or grant portal consent.
The frontend provides discovery and a gateway to broker commands rather than exporting
every internal capability as a separate static tool. See [MCP](docs/mcp.md) and
[governed MCP federation](docs/mcp-federation.md).

## Security before automation

Observe is the default. Input, clipboard contents, screenshots, application launching,
plugins and application-native mutation need explicit grants. Sensitive operations also
need a separate operator decision; an agent cannot confirm its own request.

A same-UID hostile process is outside the local IPC boundary. A separately granted
unrestricted shell bypasses this mediated surface. Sandboxing a child does not sandbox
an existing application. Browser origin restrictions are not a network firewall; package
hashes are not publisher signatures; bounded readback is not global noninterference.
There is no claim of universal prompt-injection immunity.

Read [SECURITY.md](SECURITY.md), [permissions](docs/permissions.md) and the
[threat model](docs/security.md). Report vulnerabilities through the repository's enabled
private reporting channel, not a public issue with sensitive details.

## Explore and contribute

[Installation and removal](docs/installation.md), [troubleshooting](docs/troubleshooting.md)
and [development](docs/development.md) cover setup, failures and exact-commit checks.
[Agent Skills](docs/skills.md), [recipes](docs/recipes.md), [events/jobs](docs/events-jobs.md)
and [Workflow Distillation](docs/workflow-distillation.md) describe higher-level use without
introducing another authorization system.

Send focused changes through [pull requests](https://github.com/seradotcom/semwright/pulls)
and follow [CONTRIBUTING.md](CONTRIBUTING.md). See [support](SUPPORT.md) for public questions.
Historical records under `verification/` remain evidence for their recorded sources,
not approval of later code.

## License and status

Original core source is **MIT OR Apache-2.0**. The isolated `integrations/kicad-driver`
subtree is **GPL-3.0-or-later** with its own notices. This documentation does not change
those licenses or promise a published release, production support or benchmark superiority.
See [governance](GOVERNANCE.md), [changelog](CHANGELOG.md) and
[original requirements](docs/requirements/START_HERE.md).
