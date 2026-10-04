# Browser semantic completeness verification

Baseline: `e543a6bed1497840efdb06ce0f9b96748e9529a1`.
PR: #121.

## Acceptance rule

The implementation may describe its bounded semantic surface as complete, but the delivery is not certified until the exact final PR HEAD has green evidence for:

- source-contract/schema synchronization;
- Rust x86_64 + ARM64 quality/MSRV/static-lint gates;
- broker reference contracts, including dual-ref validation;
- real Chromium live acceptance on an owned disposable browser/profile;
- OOPIF/cross-origin frame semantics;
- shadow DOM, popup/new-tab, forms, dialogs, page lifecycle, semantic scroll, drag and grant-scoped upload;
- dependency/coverage/fuzz gates;
- Windows/macOS/Linux regression and supply-chain/package gates required by the repository.

Heavy compilation, fuzzing and real-browser work belongs in GitHub Actions. Local development on the constrained device is limited to formatting, JSON/schema/source-contract checks and other lightweight validation.

## Real-browser acceptance shape

`crates/adapters/tests/chromium_live.rs` exercises the Rust Chromium backend against loopback HTTP fixtures, including a second loopback origin for cross-origin/OOPIF acceptance. The suite uses no agent-exposed `Runtime.evaluate` and verifies semantic refs/actions through the same backend used by Semwright.

Upload acceptance uses an owner-readable `FilesystemGrant`, rejects unknown roots and traversal, stages private copies under the disposable browser profile, and verifies cleanup on shutdown. Drag acceptance uses two independently broker-resolved refs. Popups are discovered through tab semantics rather than a hidden Playwright surface.

## Final exact-head evidence

Pending the final CI run for the closeout HEAD. Record run/job IDs and any retained lightweight evidence here after all required checks are terminal.
