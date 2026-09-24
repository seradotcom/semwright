# Driver SDK scorecard for Figma

| Area | Status | Evidence / limitation |
|---|---|---|
| Static capability catalog | SUPPORTED | 383 implementation-backed descriptors; 324 Plugin API operations + 56 cloud operations + 3 local driver/session operations |
| Descriptor pinning | SUPPORTED | SDK digest contract exercised by production Driver Protocol E2E |
| Protocol v2 | SUPPORTED | interfaces negotiation and v2 E2E exercised |
| Persistent child process | SUPPORTED | driver owns long-lived authenticated loopback bridge |
| Health | SUPPORTED | doctor/health expose bounded non-secret state |
| Sandbox | SUPPORTED_WITH_LIMITATION | fail-closed host path; local user-namespace setup can block bubblewrap, hosted CI is authoritative |
| Network | SUPPORTED_WITH_LIMITATION | manifest grant is boolean; Figma only needs loopback authority |
| Filesystem grants | SUPPORTED | no broad HOME access required by the core bridge |
| Pairing secret | SUPPORTED | ephemeral 256-bit driver-generated secret; no persisted host secret |
| REST/OAuth secrets | SUPPORTED_WITH_LIMITATION | cloud transport uses a same-UID protected credential socket; a generic Semwright SecretRef helper remains a platform gap |
| Child events | SUPPORTED | protocol v2 transports bounded `figma.*` selection/page/document events |
| Cooperative cancellation | SUPPORTED_WITH_LIMITATION | SDK supports it; Figma does not negotiate it until operations are safely cancellable |
| Dynamic capabilities | SUPPORTED_WITH_LIMITATION | SDK supports notifications; Figma intentionally uses a stable catalog plus execution-time availability |
| Child progress | SUPPORTED_WITH_LIMITATION | SDK supports it; current bounded Figma calls do not negotiate progress |
| Binary artifacts | SUPPORTED_WITH_LIMITATION | static/animated exports use bounded driver-local artifact tokens and chunk reads; broker-native artifact promotion is not yet negotiated |
| Figma refs | SUPPORTED_WITH_LIMITATION | session generation + document revision/node identity; real collaboration acceptance pending |
| Long-lived CPU accounting | GAP | cumulative process CPU budget is awkward for a persistent event-driven bridge |
