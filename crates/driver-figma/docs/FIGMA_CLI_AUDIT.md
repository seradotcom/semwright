# figma-cli audit

Inspected upstream: `silships/figma-cli` commit `2b1c22aaf8ff0ceeb7c08fcbaf553d6c1f963b5b`.
Reviewed README.md, REFERENCE.md, SECURITY.md, LICENSE, package files, plugin/, src/ and tests/. The isolated engineering workspace retained a frozen audit checkout during development; the final distributable records the exact commit instead of vendoring that upstream source tree.

## Useful prior art

The project has unusually broad command coverage: variables, components/variants/instances, design-system extraction/import, snapshots, deterministic validation, accessibility, rich text, Motion and a Safe Mode plugin. Its design-system reuse handles, round-trip mindset, snapshot contracts, variable alias/mode coverage and focused command semantics informed this pack.

Its tests cover render parity, snapshots, variable import chunking, instance/variant plans, typography/rich text and Motion core. Those are good coverage ideas and were treated as reference behavior rather than protocol authority.

## Security divergence

Yolo mode patches one string in Figma Desktop's app.asar to expose remote debugging. Browser/Yolo use unauthenticated CDP. Its daemon executes arbitrary JavaScript. Even its Safe Mode plugin forwards `eval` / `eval-batch` and its manifest currently uses wildcard network access.

Semwright deliberately does none of those things. The production architecture is official Plugin API only, with an allowlisted typed operation dispatcher, loopback-only WebSocket, per-session pairing, generation/revision checks, bounded payloads and no generic JavaScript execution.

## License / reuse

Upstream LICENSE is MIT, copyright 2026 Sil Bormüller. No source code was copied into the implementation. The upstream audit snapshot is included only as review evidence and retains its original LICENSE. Therefore there is no modified upstream source requiring attribution inside production code.
