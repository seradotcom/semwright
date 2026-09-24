# Application Driver SDK

Semwright drivers are persistent application integrations that enter the broker through the
same Provider Runtime as native Linux backends and federated MCP servers. A driver does not
receive caller authority, bypass policy, or become an MCP tool surface by itself.

```text
agent -> CLI/MCP -> broker -> policy/audit -> DriverProvider -> sandboxed driver -> application
```

Use a driver when an application exposes a richer API than generic AT-SPI/UI automation and
the integration needs a long-lived connection or application state. Plugins remain appropriate
for narrow stateless commands: the current plugin host starts one sandboxed process per
invocation, while a driver process persists for its provider lifetime.

## Protocol versions

The owner supplies a strict manifest. Semwright assigns the provider identity; the child cannot
claim core authority or choose another namespace. The initial stdio protocol performs:

1. owner identity/version + pinned executable digest hello;
2. driver identity/version acknowledgement;
3. capability enumeration plus catalog digest;
4. health probe;
5. descriptor-pinned execute requests;
6. explicit shutdown acknowledgement.

Capabilities must live under `driver.<id>.*`, require `driver:<id>`, and route through that same
provider ID. External metadata is treated as untrusted data by the Provider Runtime.

Protocol v1 provides the baseline request/response lifecycle. Protocol v2 additionally
negotiates interfaces for cooperative cancellation, child events, progress, artifacts,
health and dynamic capability changes. Each interface remains fail-closed unless both the
child and owner manifest negotiate the same value.

## Manifest

A v1 JSON manifest contains the driver identity and execution constraints:

```json
{
  "manifest_version": 1,
  "protocol": 1,
  "id": "example",
  "version": "1.0.0",
  "publisher": "example-org",
  "executable": "/absolute/path/to/semwright-example-driver",
  "sha256": "<64 lowercase hex characters>",
  "application": {
    "desktop_id": "org.example.Application",
    "process_names": [],
    "supported_versions": []
  },
  "transport": "stdio_v1",
  "mounts": [],
  "system_config": [],
  "network": false,
  "resources": {
    "open_files": 128,
    "processes": 32,
    "cpu_seconds": 20,
    "address_space_bytes": 536870912,
    "file_size_bytes": 16777216
  },
  "request_timeout_ms": 30000,
  "interfaces": {
    "dynamic_capabilities": false,
    "cooperative_cancellation": false,
    "events": false,
    "health": true
  }
}
```

Unknown fields, noncanonical IDs, namespace escape, unpinned executables and over-broad
resource declarations are rejected. Executable trust and process isolation are platform-host
responsibilities; they are not part of Driver Protocol authority.

## Sandbox and permissions

The v1 host refuses an unsupported unsandboxed execution path. On Linux it verifies an owned/root
regular ELF, stages the exact digest-pinned bytes and launches them through bubblewrap plus
Semwright's Landlock helper. The macOS host foundation can validate executable identity/formats,
but arbitrary driver/plugin launch remains fail-closed until a supported isolation model provides
the required guarantees. Environment variables are cleared,
network is isolated unless both manifest and owner configuration allow it, and named
filesystem mounts can only refer to existing policy grants.

Ordinary `mounts` are exposed below `/workspace/<grant>`. A writable mount requires a matching
owner grant with write authority. `system_config` is narrower: it may expose only a direct
child of `/etc`, always read-only, and only when the owner explicitly granted that canonical
host directory. This supports packaged applications whose runtime data is split between
`/usr` and distribution configuration such as `/etc/libreoffice` without granting arbitrary
host configuration access.

Each driver also declares bounded resource limits. The defaults preserve the original sandbox
limits (128 file descriptors, 32 processes, 20 CPU seconds, 512 MiB virtual address space and
16 MiB output files); the manifest may request larger values only inside hard SDK/helper
ceilings. Scratch/cache/config state is redirected into the sandbox-private `/tmp`, not the
user's real home directory.

The manifest defines what the driver needs; it never creates a policy grant. Capability calls
still pass through the broker's normal risk, confirmation, cancellation and audit path.

## Cross-driver artifact handoff

Drivers do not bind directly to each other. Capabilities may advertise semantic artifact ports
using tags such as `artifact-out:model/3d` and `artifact-in:model/3d`. These tags describe
compatibility only; they grant no filesystem access and do not move bytes.

`artifact.handoff` is the broker-owned transfer primitive. It copies a bounded binary file from
one explicitly readable filesystem grant to one explicitly writable grant, using relative paths
only. The source may be pinned with `expected_sha256`; a mismatch fails before the destination is
written. The destination uses the platform scoped-filesystem atomic-write boundary. The current
handoff ceiling is 64 MiB; larger media requires a future streaming artifact transport rather
than weakening the bounded in-memory contract.

This deliberately keeps applications independent. For example, Blender may advertise
`artifact-out:model/3d`; Godot may advertise `artifact-in:model/3d`; the agent can discover
both with the normal capability-catalog tag filter, handoff a `.blend` file from the Blender
workspace into the Godot project grant, then request the normal Godot asset rescan. Neither
driver needs to know the other exists. The same contract can connect Godot movie capture to
MLT (`video/clip`) or future audio/design providers.

Artifact ports are intentionally semantic rather than pairwise bindings. A typical planner flow is
`capabilities.search(tags=["artifact-out:model/3d"])` followed by
`capabilities.search(tags=["artifact-in:model/3d"])`, then `artifact.handoff` when the concrete
artifact is file-backed and both filesystem grants are authorized. Matching semantic tags do not
prove that every native file format is accepted; the producer result/media type and consumer
operation still require normal compatibility checks.

Protocol-v2 `JobArtifact` references may also represent provider-owned tokenized artifacts. Those
references are metadata today, not broker-readable file handles. Figma, for example, keeps exports
behind authenticated driver-local tokens and bounded chunk reads. This v1 handoff does not pretend
those tokens are file-backed; a future generic streaming/materialization contract can bridge them
without changing the file-backed handoff security boundary.

Current semantic ports include:

| Provider operation | Artifact port |
|---|---|
| Blender file save | `artifact-out:model/3d` |
| Blender render | `artifact-out:image/raster` |
| Figma node export | raster/vector image, PDF and video outputs |
| Figma design-system/source export | `artifact-out:text/source` |
| LibreOffice PDF export | `artifact-out:document/pdf` |
| Godot asset rescan | 3D model, raster/vector image and audio inputs |
| Godot movie capture | `artifact-out:video/clip` |
| MLT asset import | video, audio and raster-image inputs |
| MLT render result | `artifact-out:video/clip` |

Protocol v1 intentionally rejects dynamic-capability changes, child events, progress,
artifacts and cooperative cancellation. Protocol v2 transports those interfaces explicitly,
including bounded event/progress frames and cancellation acknowledgements. Drivers that do not
negotiate an interface remain fail-closed rather than advertising semantics the host cannot enforce.

## Developer workflow

```sh
semwright driver scaffold myapp ./semwright-myapp \
  --sdk-path /absolute/path/to/semwright/crates/driver-sdk

# Build the generated project, pin its absolute path + SHA-256 in the manifest:
semwright --json driver validate ./driver.json
semwright --json driver inspect ./driver.json
semwright --json driver conformance ./driver.json
```

Scaffolding does not install the driver or grant it authority.

The conformance command launches the real pinned binary in the production sandbox, verifies the
handshake/catalog/health contract, executes a safe read-only capability when the catalog exposes
one compatible with the harness, then requires a clean shutdown.

The repository also ships a deterministic fixture and:

```sh
BIN_DIR=target/debug scripts/dev/driver-conformance.sh
```

That path is exercised locally and by the Native application integration workflow.

## Included application drivers

The workspace includes several larger integration surfaces in addition to the existing examples:

- `crates/driver-mlt-video` provides 68 bounded offline timeline, metadata, render-plan and
  job capabilities. Deep mutation is limited to the driver's normalized MLT form; arbitrary
  Kdenlive and Shotcut projects remain conservative/read-mostly inputs. Its fixtures and
  semantic tests do not by themselves certify a real editor round trip.
- `integrations/kicad-driver` is a separately licensed GPL-3.0-or-later integration. It links
  a Rust SDK client to a bounded Go/C ABI IPC core and supports the curated KiCad 9/10 PCB
  surface described in its own compatibility documentation. The deterministic fake IPC gate
  is not evidence of interoperability with a real KiCad process.
- `crates/driver-obs` provides a curated obs-websocket 5.x integration with 65 capabilities,
  connection generations, bounded reconnects, driver-local refs, event backpressure and output
  lifecycle tracking. Its dedicated CI exercises the production Rust client against an
  independent fake server, the real Semwright Driver Host sandbox, bounded fuzz targets and a
  disposable read-only OBS Studio instance. Driver Protocol v1 still does not transport child
  events, cooperative cancellation or dynamic capability changes into the broker.
- `crates/driver-figma` provides 91 typed capabilities through an authenticated loopback bridge
  to a Semwright development plugin using the official Figma Plugin API. It covers design nodes,
  Auto Layout, typography, components/variants/instances, variables/design systems, prototypes,
  Figma Motion Beta and FigJam. Automated CI uses an independent fake Figma host and the real
  Driver Host sandbox; real Figma acceptance remains explicitly separate and disposable-file only.
- `crates/driver-godot` provides 53 typed capabilities through an authenticated loopback bridge
  to a Godot EditorPlugin plus a digest-pinned Godot runner. It covers scenes/nodes/resources,
  managed scripts, signals, project InputMap persistence, animation tracks/keyframes, shaders,
  UI/physics settings, headless validation/runtime and artifact exports. Dedicated CI executes
  the production driver against both the real Driver Host sandbox and pinned Godot 4.7.2.

These integrations use the normal owner-assigned DriverProvider identity, digest pinning,
policy grants, bubblewrap/Landlock sandbox and descriptor-pinned execution where applicable.
They add no ambient authority or unsandboxed fallback. Consult each directory's README and
security notes before enabling it.

## Distribution

The App Driver SDK also has a non-executing local distribution layer for versioned packages and
static indexes. See [Driver distribution](driver-distribution.md) for package/index formats,
compatibility resolution and install/update/remove security semantics. Distribution never creates
policy grants and does not replace explicit `driver conformance`.

## Loading drivers in the daemon

Owner configuration may list protected manifest files:

```toml
drivers = [
  "/home/user/.config/semwright/drivers/example.json"
]
driver_network = false

[policy]
profile = "observe"
allow = ["driver:example"]
```

The manifest file is read through the same owner-only configuration boundary used by other
trusted local definitions. On startup the daemon launches the driver host, validates its
capability catalog, and mounts it with `Broker::mount_provider`.

## Scope

Driver SDK v1 is the foundation for deeper first-party and community integrations. The presence
of a driver in the workspace does not by itself certify every application version or runtime
path: each integration still needs application-specific conformance and real execution evidence.
Further drivers such as Krita/GIMP can reuse the same provider, policy and sandbox contracts
without adding application-specific branches to the broker.
