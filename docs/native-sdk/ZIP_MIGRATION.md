# Migration ledger

Original inputs are preserved byte-for-byte outside the public checkout.
SOURCE_LOCK.json records member-level disposition and actual port status.
The following are migration decisions, not completed-test claims.

| Area | Disposition | Destination / reason |
|---|---|---|
| Model / NativeApp / journal | REFACTORED | Optional file-backed module; not the generic root contract |
| Scene, Table, Counter | PORTED | Explicit file-backed examples, with original regression assertions |
| Recovery tests | PORTED | Preserve uncertainty and request binding; add retention tests separately |
| Protected file verifier | REFACTORED | Optional Linux-only reader using canonical Effects |
| Composition report handling | PORTED | Keep canonical report validation; never grant authority from JSON |
| Package executable | REPLACED | Current driver-registry API; no new archive format |
| Historical Cargo.lock / build records | HISTORICAL_ONLY | Workspace lock and current exact-SHA runs replace build authority |
| Historical catalogs / schema snapshots | RETAINED_AS_REFERENCE | Regenerate and compare, never assume current descriptors |
| Packaging / clean-room tooling | REFACTORED | Workspace package paths, private packaging and clean consumers |
| Validation ZIP | HISTORICAL_ONLY | Retain all logs; no current PASS inheritance |
| Generic app-owned interfaces | NEW | Small optional traits, canonical operation metadata, opaque revisions |
| TypeScript own-storage example | NEW | Application transaction/CAS and public SDK bridge |

All derived source remains BLOCKED_FOR_PUBLIC_DISTRIBUTION until an explicit
owner decision identifies the rights holder and permitted license. Local/private
implementation is permitted; no silent workspace-license inheritance is used.
