# Driver SDK scorecard for Figma

| Area | Status | Evidence / limitation |
|---|---|---|
| Static capability catalog | SUPPORTED | 91 operation-specific descriptors; fake E2E attestation |
| Descriptor pinning | SUPPORTED | SDK digest contract exercised by production Driver Protocol E2E |
| Persistent child process | SUPPORTED | driver owns long-lived loopback bridge |
| Health | SUPPORTED | doctor/health expose bounded non-secret state |
| Sandbox | SUPPORTED_WITH_LIMITATION | Linux host is fail-closed; local conformance blocked by unavailable bwrap user namespace |
| Network | SUPPORTED_WITH_LIMITATION | manifest is boolean; Figma needs loopback-only authority |
| Filesystem grants | SUPPORTED | no broad HOME access required by core bridge |
| Pairing secret | SUPPORTED | ephemeral 256-bit driver-generated secret; no persisted host secret required |
| REST/OAuth secrets | GAP | future optional cloud transport would need generic secret references |
| Child events | GAP | bridge consumes events internally; Driver Protocol v1 cannot publish them |
| Child cancellation | GAP | v1 does not negotiate cooperative cancel |
| Dynamic capabilities | GAP | editor/Motion/FigJam availability is checked per operation |
| Core jobs | SUPPORTED_WITH_LIMITATION | no child progress/artifact lifecycle |
| Binary artifacts | GAP | large Figma exports need generic artifact/stream handoff |
| Figma refs | SUPPORTED_WITH_LIMITATION | session generation + document revision/node identity; real collaboration acceptance pending |
| Long-lived CPU accounting | GAP | cumulative process CPU budget is awkward for a persistent bridge |
