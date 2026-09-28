# Testing and conformance

Minimum evidence for a serious driver:

- descriptor/schema unit tests;
- protocol/handshake and lifecycle tests;
- negative inputs and stale-ref cases;
- policy/risk metadata checks;
- deterministic fixtures for semantic operations;
- property/fuzz tests for parsers, refs, paths, or envelopes when they accept attacker-controlled structure;
- sandboxed Driver Host conformance;
- live application evidence only when the application/runtime is actually available.

Use `semwright driver validate` before conformance. Conformance launches the pinned driver through the production sandbox path; it still does not prove every application capability works live.

For distribution, inspect the package/index metadata and digests. Do not describe integrity pinning as publisher trust or certification.
