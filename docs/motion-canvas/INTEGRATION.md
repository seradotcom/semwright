# Integration

Frozen implementation baseline: `a14abd8328e092a8227584750e47c38a77449ffa`.
That baseline remains the historical snapshot recorded in `SEMWRIGHT_SNAPSHOT.md`; it is not rewritten after implementation.

During PR integration the branch incorporated already-merged shared work without changing the frozen implementation baseline. The final reconciliation before closeout is merge commit `f25bd3db67c488cf76419927e2aa5977df1abaca` with `origin/main` parent `f2f3ec470f95c2010df89a61d4835afe5c4926a1`. The only merge conflict was additive in `fuzz/Cargo.toml`: the resolution retains all six Motion Canvas fuzz targets and also keeps main's `godot_substrate_schema` target. This is an integration merge, not a moving-baseline change.

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

Chromium compatibility experiments temporarily raised the virtual-address-space and task ceilings, but later CI proved those increases did not affect the pinned Chrome-for-Testing `SIGTRAP` failure. The final Firefox route returns the address-space limit to the original 4 GiB hard ceiling and uses the existing 256-task SDK maximum after Firefox 151 CI measured `EAGAIN` while creating a graphics thread at 128 tasks. The generic change that remains is narrower: explicit executable authority for read-only Driver mounts, required so an owner-approved Node/browser runtime can execute without making ordinary project/media/config mounts executable.

Launch-film semantic source, storyboard, visual system, recipe and asset manifest
are source-controlled. Generated browser profiles, node_modules, PNG sequences,
intermediates and final binary media remain outside Git and are produced on
ephemeral GitHub Actions runners.

The integration is maintained in PR #49. This mission may push and iterate that
branch but does not merge the PR.
