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
