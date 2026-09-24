# Integration

Frozen implementation baseline: `a14abd8328e092a8227584750e47c38a77449ffa`.
That baseline remains the historical snapshot recorded in `SEMWRIGHT_SNAPSHOT.md`; it is not rewritten after implementation.

During final PR integration, the branch received merge commit `9ecde7c` with
`origin/main` parent `3a048cdde7f531811b7ffb7e126ad24346d6cd3a`. This is a final integration
merge, not a moving-baseline change. It brought newer shared Driver SDK/Host,
video-domain and other already-integrated repository work into the PR.

The integrated Driver SDK keeps manifest version 1 and accepts Driver Protocol
versions 1 through 2. The Motion Canvas manifest deliberately requests protocol 1
for this PR. Its `render.start/status/cancel/result` surface remains driver-local,
and the driver does not advertise v2 child events, progress, artifacts or
request cancellation that it does not yet emit truthfully. A later migration can
adopt those generic interfaces without changing the managed project format.

The driver adds no broker-specific authority path. Existing driver package/index
machinery remains the distribution mechanism; filesystem and policy grants remain
owner-controlled. The runtime/browser is an explicit, read-only, digest-pinned
owner grant rather than an implicit host dependency.

Motion Canvas also proved one narrow generic resource constraint during final integration: Chromium headless shell could not start under the previous 4 GiB virtual-address-space hard maximum. The dedicated `fix: allow browser-compatible driver address space` commit keeps the 512 MiB default, raises only the SDK/Linux-helper maximum to 16 GiB, and makes this driver opt in explicitly. CI measures the browser under both ceilings.

Launch-film semantic source, storyboard, visual system, recipe and asset manifest
are source-controlled. Generated browser profiles, node_modules, PNG sequences,
intermediates and final binary media remain outside Git and are produced on
ephemeral GitHub Actions runners.

The integration is maintained in PR #49. This mission may push and iterate that
branch but does not merge the PR.
