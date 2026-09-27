# Semwright Blender deep driver

This first-party driver runs a private Blender process inside the Semwright DriverProvider sandbox.
It is separate from the interactive `blender.*` add-on backend: the existing add-on remains useful
for an already-open user session, while this driver provides a disposable, reproducible, policy-
scoped Blender runtime for agents and CI.

## Control surface

The driver keeps the original curated scene/object/material/render/file operations for ergonomic
common tasks, and adds a version-pinned semantic RNA substrate for deep authoring:

- persistent datablock refs with generation-based stale-reference protection;
- root/concrete RNA type discovery and property classification;
- bounded property get/set/reset, query pushdown and custom properties;
- pointer/collection traversal plus safe relation set/link/unlink;
- datablock lifecycle and scoped file-backed asset loading;
- modifiers, constraints, keyframes and node graphs;
- mesh topology, attributes, UVs, shape keys and vertex groups;
- curve splines and armature bone lifecycle/parenting.

It also retains bounded read-only introspection:

- `driver.blender.introspect.summary`
- `driver.blender.introspect.operators`
- `driver.blender.introspect.operator.describe`
- `driver.blender.introspect.types`
- `driver.blender.introspect.addons`

RNA metadata drives typed semantic discovery; operator metadata remains discovery-only. **There is no generic operator invoke, Python `exec`/`eval`, shell command, animation-driver expression surface, or user-site import surface.** See `../../docs/blender/SEMANTIC_COMPLETENESS.md` for the exact managed boundary.

## Isolation

The DriverProvider pins the Rust driver ELF by SHA-256 and launches it through Bubblewrap plus
Landlock. The driver starts Blender with `--background --factory-startup --disable-autoexec`,
a private temporary runtime and a single owner-granted workspace. Network access is disabled.
The first accepted live target is Blender 4.5.14 LTS.

The driver requires read-only access to `/etc/fonts` and a writable `workspace` grant. Blender
itself remains a large native application; the sandbox reduces authority but is not a claim that
Blender or third-party add-ons are memory-safe.

## Verification

The native and dedicated Blender semantic GitHub Actions jobs download Blender 4.5.14 LTS from
blender.org, verify the pinned SHA-256, generate the reachable RNA coverage matrix, run adversarial
ref/security tests, and exercise the semantic driver inside the production sandbox. Acceptance
includes deep RNA traversal/mutation, stale refs, topology, rigging, node graphs, asset loading,
a 64x64 CPU Cycles render, a real `.blend` save and the broker/CLI path.

Mocked add-on tests remain useful regression coverage but are not used as evidence for this live
driver.
