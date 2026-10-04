# Semwright Blender deep driver

This first-party driver runs a private Blender process inside the Semwright DriverProvider sandbox.
It is separate from the interactive `blender.*` add-on backend: the existing add-on remains useful
for an already-open user session, while this driver provides a disposable, reproducible,
policy-scoped Blender runtime for agents and CI.

## Control surface

The driver reuses the existing curated Blender operations under the `driver.blender.*` namespace:

- scene inspection;
- object list/get/create/delete/transform;
- collection list/create/link;
- material list/create/assign;
- typed, collection-scoped GLB export;
- render settings and PNG render;
- scoped `.blend` open/save;
- the broader bounded semantic RNA authoring surface documented by the capability catalog.

It also exposes bounded read-only introspection:

- `driver.blender.introspect.summary`
- `driver.blender.introspect.operators`
- `driver.blender.introspect.operator.describe`
- `driver.blender.introspect.types`
- `driver.blender.introspect.addons`

RNA and operator metadata are data for discovery. **There is no generic operator invoke, Python
`exec`/`eval`, shell command, or user-site import surface.**

## Typed GLB handoff

`driver.blender.export.glb` is the supported game-asset handoff primitive. It exports exactly one
named collection to one new self-contained glTF 2.0 binary artifact below the owner-granted
workspace.

Example input:

```json
{
  "collection": "Fish_Player_Juvenile",
  "path": "exports/fish-player-juvenile.glb",
  "animations": true
}
```

The command:

- accepts only a named collection;
- requires 1–2048 collection objects;
- accepts meshes, armatures and empties;
- rejects parents or armature dependencies outside the collection;
- rejects non-object-mode export;
- never overwrites an existing destination;
- publishes from a private temporary file with a no-clobber hard-link step;
- emits GLB 2.0 only, with cameras/lights/Draco disabled;
- restores the previous Blender selection and active object;
- validates the GLB header and bounded artifact size;
- returns the relative path, SHA-256, byte size and exported object count.

The exported capability is tagged `artifact-out:model/3d`, so downstream orchestration can treat
the result as a typed 3D-model handoff without granting generic Blender operator execution.

## Blender runtime authority

The deep driver no longer discovers Blender from `/usr/bin`, `/usr/local/bin` or ambient
`PATH`. The owner must explicitly grant the supported Blender runtime:

1. a read-only `blender-runtime` mount containing the portable Blender 4.5.14 LTS directory; and
2. a `blender-executable` tool grant for that runtime's exact `blender` executable, pinned by
   SHA-256 and materialized by Driver Host as `/plugin/tools/blender`.

The driver verifies that the portable runtime contains the expected `lib` and `4.5/*` resource
directories, configures Blender's system-resource environment to those read-only paths, and refuses
to start unless the binary reports exactly `Blender 4.5.14 LTS`.

See `driver.manifest.example.json` for the manifest side. The daemon policy must map both logical
roots to the same owner-approved portable installation, for example:

```toml
[[policy.filesystem]]
name = "blender-runtime"
path = "/home/user/.local/opt/blender-4.5.14"
read = true
write = false

[[policy.filesystem]]
name = "blender-executable"
path = "/home/user/.local/opt/blender-4.5.14/blender"
read = true
write = false
```

The executable file may live inside the runtime directory. Driver Host treats the file-backed tool
authority separately from generic directory-backed filesystem/artifact providers.

## Isolation

The DriverProvider pins the Rust driver ELF by SHA-256 and launches it through Bubblewrap plus
Landlock. The Blender executable is separately digest-attested as an owner-pinned sealed tool.
The driver starts Blender with `--background --factory-startup --disable-autoexec`, a private
temporary user/runtime directory and a single writable owner-granted workspace. Network access is
disabled. The accepted live target is Blender 4.5.14 LTS.

The driver requires read-only access to `/etc/fonts`, the read-only portable Blender runtime and a
writable `workspace` grant. Blender itself remains a large native application; the sandbox reduces
authority but is not a claim that Blender or third-party add-ons are memory-safe.

## Verification

The native GitHub Actions job downloads Blender 4.5.14 LTS from blender.org, verifies the published
archive SHA-256, and then exercises the same portable-runtime/sealed-tool architecture expected in
production. The job is expected to run:

1. the pinned Blender RNA coverage inventory;
2. the real typed GLB adapter contract, including path/overwrite/dependency denials;
3. a scoped Bubblewrap preflight with Blender mounted only as `/workspace/blender-runtime` plus
   `/plugin/tools/blender`;
4. a DriverProvider integration inside the production sandbox;
5. RNA/operator/add-on and semantic authoring coverage;
6. object/material mutation;
7. a real collection-scoped GLB export with SHA-256 verification and a no-overwrite probe;
8. a 64×64 CPU Cycles render;
9. a real `.blend` save;
10. a full CLI → daemon → broker → DriverProvider → Blender smoke path including GLB export;
11. the existing interactive add-on path under Xvfb as a separate compatibility surface.

Mocked add-on tests remain useful regression coverage but are not used as evidence for this live
driver.

## Cycles GPU sessions

`driver.blender.render.settings` accepts the finite process-local compute selector
documented in `../../adapters/blender/README.md`. For a production GPU session,
request `{"engine":"CYCLES","device":"GPU","backend":"CUDA"}` explicitly and
verify its returned device identities. `AUTO` permits an explicit CPU fallback;
it is unsuitable when GPU usage must be guaranteed.

GPU authority is disabled by default and belongs to the Host-owned Blender
session-runner tool. The owner must enable `driver_nvidia_gpu` in the daemon,
`nvidia_gpu` in the driver manifest, and `nvidia_gpu` on the
`blender-session-runner` tool contract. The Rust driver itself receives no GPU
device nodes. Network, filesystem grants, sealed executables and resource limits
remain independently enforced.

The runner starts every Blender session from factory settings. Compute selection
must therefore be repeated after each native session reopen. No user preferences
file is read or written, and persistent semantic authoring does not gain a general
preferences interface. GPU and CPU may produce numerical pixel differences;
benchmark a representative shot with unchanged cameras, lighting, samples,
denoising and bounce settings before adopting a new execution revision. A typed
device-selection response proves enumeration and enabled settings; an actual
native render and its receipt are still required to prove GPU execution.

The Linux compute grant currently selects `/dev/nvidia0`, `nvidiactl`, and
`nvidia-uvm`, plus `nvidia-uvm-tools` when present. Host verification requires
root-owned character devices and refuses symlinks. Other GPU vendors, additional
GPU indices, and non-Linux platforms are unsupported by this grant.

For CUDA workloads that exceed the ordinary 4 GiB address-space limit, the owner
may add a `resources` object to the `blender-session-runner` tool contract. It
must match every enclosing driver resource except `address_space_bytes`, which
may be at most `8589934592` (8 GiB). The driver keeps its original limits; CPU
tools, network access, process counts and other limits receive no expansion.
The GPU override requires `operation_cpu_seconds = 0`.

The runner sets process-local `MALLOC_CONF=narenas:4,retain:false` for Blender's
allocator and confines GPU caches to its private session. It passes an immutable
Host policy to the fixed native helper, which applies Landlock before Blender
starts. NVIDIA thread naming needs write/truncate access under this native
process's own `/proc/<pid>/task` subtree; this also covers other own-thread
metadata subject to kernel permissions. It grants no other PID tree or proc
driver-state writes.

Native transport failures retain at most 8 KiB of log tail in a private local
`.semwright-native-failures` directory before deleting session scratch. Public
errors expose only the validated failure phase, exit status, byte count and
digest; they remain uncertain and carry no raw log text or local path.
