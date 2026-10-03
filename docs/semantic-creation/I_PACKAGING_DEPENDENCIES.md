# Integrated source backup dependencies, version 2

The historical Blender/Godot/Effect Conformance source packagers pinned the original C0/P0 trees. The
integrated A implementation adds the explicit reconciliation contract; C includes
its completed rebuild/provenance work and F includes its final production adapter
contracts. Those old tree pins correctly rejected the integrated candidate.

Version 2 uses immutable reconstruction base
`6e1261d645699e99fe94ad902b5fb26956f92102`, which contains the integrated Host and
all consumed owner sources. This base passed source contracts and strict lints in
run `37011740338`, and real Broker/Host AV plus C14 in `37011740065`. These scopes
are evidence for the reviewed dependency update, not complete owner acceptance.

Expected A implementation is pinned to that base. Expected C implementation is
`77b34d8abad50f242c4c8494e280fe82d5cbcf55` and expected F implementation is
`eadd5caf9b3f47f24158de530b87ad07e597f25e`; their component trees match the base.
SDK and Host component trees are additionally pinned to the base. Historical
logical C0/P0 contract references remain separately recorded. Future component
changes fail the tree comparisons until the pins are explicitly reviewed again.

Every source backup retains bounded immutable-object collection, deterministic
ZIP bytes, checksum verification and reconstruction with `git apply --check`,
application and byte-for-byte source comparisons. These checks run in Actions.
Previous artifacts and evidence remain intact. Packaging grants no release,
security, production readiness or R16 authority.
