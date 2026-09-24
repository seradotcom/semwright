# Integration

Frozen implementation baseline: `a14abd8328e092a8227584750e47c38a77449ffa`. Implementation did not chase moving `main`; the branch performed one explicit final integration with current `main` before closeout, as permitted by the mission.

The frozen implementation baseline used Driver SDK/Host protocol v1 and adds no broker-specific authority path. During final integration, `main` was merged into this branch at `9ecde7c`; the integrated SDK now supports protocol versions 1–2 while retaining manifest version 1. Motion Canvas intentionally continues negotiating protocol v1 in this PR: its driver-local render job surface remains truthful and stable, and no v2 progress/event claims are made without dedicated adoption tests. Existing driver package/index machinery remains the distribution mechanism; filesystem and policy grants remain owner-controlled.

Launch-film semantic source, storyboard, visual system, recipe and asset manifest are source-controlled. Generated browser profiles, node_modules, PNG sequences, intermediates and final binary media remain outside Git and are produced on ephemeral GitHub Actions runners.

The integration is maintained in PR #49. This mission may push and iterate the branch but does not merge the PR.
