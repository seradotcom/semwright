# Semwright Blender deep driver

This first-party driver runs a private Blender process inside the Semwright DriverProvider sandbox.
It is separate from the interactive `blender.*` add-on backend: the existing add-on remains useful
for an already-open user session, while this driver provides a disposable, reproducible, policy-
scoped Blender runtime for agents and CI.

## Control surface

The driver reuses the existing curated Blender operations under the `driver.blender.*` namespace:

- scene inspection;
- object list/get/create/delete/transform;
- collection list/create/link;
- material list/create/assign;
- render settings and PNG render;
- scoped `.blend` open/save.

It also exposes bounded read-only introspection:

- `driver.blender.introspect.summary`
- `driver.blender.introspect.operators`
- `driver.blender.introspect.operator.describe`
- `driver.blender.introspect.types`
- `driver.blender.introspect.addons`

RNA and operator metadata are data for discovery. **There is no generic operator invoke, Python
`exec`/`eval`, shell command, or user-site import surface.**

## Isolation

The DriverProvider pins the Rust driver ELF by SHA-256 and launches it through Bubblewrap plus
Landlock. The driver starts Blender with `--background --factory-startup --disable-autoexec`,
a private temporary runtime and a single owner-granted workspace. Network access is disabled.
The first accepted live target is Blender 4.5.14 LTS.

The driver requires read-only access to `/etc/fonts` and a writable `workspace` grant. Blender
itself remains a large native application; the sandbox reduces authority but is not a claim that
Blender or third-party add-ons are memory-safe.

## Verification

The native GitHub Actions job downloads Blender 4.5.14 LTS from blender.org, verifies the published
SHA-256, and runs:

1. a real DriverProvider integration inside the production sandbox;
2. RNA/operator/add-on introspection;
3. object/material mutation;
4. a 64x64 CPU Cycles render;
5. a real `.blend` save;
6. a full CLI -> daemon -> broker -> DriverProvider -> Blender smoke path.

Mocked add-on tests remain useful regression coverage but are not used as evidence for this live
driver.
