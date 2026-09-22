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

## Protocol v1

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
  "network": false,
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
resource declarations are rejected. The executable must be an owned/root regular ELF file,
must not be writable by group/others, and must match the manifest SHA-256.

## Sandbox and permissions

The v1 host refuses unsandboxed execution. It stages the exact verified bytes and launches
them through bubblewrap plus Semwright's Landlock helper. Environment variables are cleared,
network is isolated unless both manifest and owner configuration allow it, and named
filesystem mounts can only refer to existing policy grants.

The manifest defines what the driver needs; it never creates a policy grant. Capability calls
still pass through the broker's normal risk, confirmation, cancellation and audit path.

Current host v1 intentionally rejects drivers that advertise dynamic-capability changes,
provider events or cooperative cancellation. The Provider Runtime supports those concepts,
but the out-of-process driver transport has not yet negotiated them. Failing closed here is
preferable to advertising semantics the host cannot enforce.

## Developer workflow

```sh
computerctl driver scaffold myapp ./semwright-myapp \
  --sdk-path /absolute/path/to/semwright/crates/driver-sdk

# Build the generated project, pin its absolute path + SHA-256 in the manifest:
computerctl --json driver validate ./driver.json
computerctl --json driver inspect ./driver.json
computerctl --json driver conformance ./driver.json
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

Driver SDK v1 is the foundation for deeper first-party and community integrations such as
Blender, KiCad, LibreOffice, Krita/GIMP and OBS. Those applications are not automatically
certified merely because the protocol exists. Each driver still needs application-specific
implementation and real conformance/integration evidence.
