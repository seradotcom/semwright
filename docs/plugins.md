# Plugin SDK and process host

A plugin is an explicitly installed native executable speaking a versioned bounded stdio
protocol. It is not a shared library loaded into the broker. Commands must be under
`plugin.<name>.*`, require `plugin:<name>`, and use backend `plugin`. The manifest includes
protocol/name/version, absolute executable, SHA-256, command descriptors, named mounts,
network boolean and timeout. The typed `Manifest` is authoritative; its companion JSON
Schema is a discovery/authoring aid.

The sample is a Unicode text-statistics plugin, with no filesystem mounts or network.
After compiling the workspace:

```sh
python scripts/plugin-manifest.py target/debug/semwright-example-plugin ./textstats.json
# The script creates mode 0600 and prints the actual executable digest.
```

The operator reviews that file and configures `plugin.manage` and `plugin:textstats` as
needed. An IPC install requires the foreground operator console. Alternatively the owner
can load an exact `0600` manifest path through the top-level `plugins` configuration array
at startup. Runtime installs last until broker shutdown. There is no automatic plugin
marketplace, dependency installer, agent-approved trust grant or durable install wizard.

```sh
computerctl plugin install ./textstats.json
computerctl plugin doctor textstats
computerctl commands describe plugin.textstats.count
computerctl execute plugin.textstats.count --args-json '{"text":"Hello local tools"}'
computerctl plugin remove textstats
computerctl plugin scaffold ./my-plugin --sdk-path /absolute/path/to/semwright/crates/plugin-sdk
```

Stdio begins with a protocol/name hello; execution has request IDs and structured results.
The current runtime handshake checks protocol and identity. The broker validates schemas
from the manifest, but the child does **not** independently attest the complete schema/
version digest; that is a recorded release gap, not full conformance.

The host stages and hashes an owned ELF file. A scrubbed child runs through bubblewrap
and `semwright-sandbox`, which adds a required Landlock ruleset before executing the
staged plugin. Named policy roots become explicit `/workspace/<name>` mounts. No manifest
can mount the broker's private runtime/state/config. Writable mounts prevent a plugin
from claiming read-only command risk. Network needs both owner and manifest consent.
Output is bounded, timeouts kill the child, resource limits are applied and inherited
secrets are not passed through.

Missing bubblewrap/user namespace/Landlock enforcement causes `SandboxDenied`; there is
no direct-exec fallback. Sandbox execution, crash containment and malicious-plugin negative
tests remain unexecuted. Do not treat the mere presence of hardening flags as a verified
confinement guarantee. See [SECURITY.md](../SECURITY.md) and [manual testing](manual-testing.md).
