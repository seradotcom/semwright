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
semwright plugin install ./textstats.json
semwright plugin doctor textstats
semwright commands describe plugin.textstats.count
semwright execute plugin.textstats.count --args-json '{"text":"Hello local tools"}'
semwright plugin remove textstats
semwright plugin scaffold ./my-plugin --sdk-path /absolute/path/to/semwright/crates/plugin-sdk
```

Stdio uses **Plugin Protocol v2**. The owner manifest and child mutually bind the plugin
name, plugin version and SHA-256 digest of the complete ordered command descriptors before
execution. A v1 child, a different plugin version, or a binary whose embedded descriptors
do not match the reviewed manifest fails the handshake. Execution then uses request IDs and
structured results. This attestation proves descriptor identity; it does not make plugin
metadata trusted or grant policy authority.

The host stages and hashes an owned ELF file. A scrubbed child runs through bubblewrap
and `semwright-sandbox`, which adds a required Landlock ruleset before executing the
staged plugin. Named policy roots become explicit `/workspace/<name>` mounts. No manifest
can mount the broker's private runtime/state/config. Writable mounts prevent a plugin
from claiming read-only command risk. Network needs both owner and manifest consent.
Output is bounded, timeouts kill the child, resource limits are applied and inherited
secrets are not passed through.

Missing bubblewrap/user namespace/Landlock enforcement causes `SandboxDenied`; there is
no direct-exec fallback. The hosted adversarial sandbox gate executes hostile plugin and DriverProvider fixtures and
checks read-only/write mount boundaries, host-file invisibility, host PID isolation, loopback
network isolation, environment scrubbing, driver resource limits, descendant cleanup and plugin
manifest/child version+descriptor attestation. These tests exercise the configured Linux boundary;
they are not a formal proof against kernel, Bubblewrap, Landlock or native-code vulnerabilities.
See [SECURITY.md](../SECURITY.md) and [manual testing](manual-testing.md).
