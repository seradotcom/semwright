# Post-merge reconciliation

Observed after PR #156 merge on 2026-09-28.

- `origin/main`: `6dc9da507a2fc239a766a6a81a7607fbcc79618d`
- PR #156: merged; audit head `0b8356f6bb7ce3366964150c4378b9ba5b14bdbb`
- All audit remediation commits are ancestors of `main`.
- PR #155 and PR #167 landed before the audit merge and are part of the current main tree.
- PR #154 remains owner-managed Blender runtime work.
- Newer Windows/composition/dependency PRs are not automatically part of this audit's scope.

At the first exact-main CI observation, Maintainer secret precheck and Native X11 EWMH live were successful.
Other required workflows were queued/in progress, so this document does not call the merge SHA globally green
and does not declare a candidate.

This reconciliation changes documentation/machine-readable state only. It does not modify Windows, Blender/Godot,
composition/media or dependency-owner source.
