# Reading project knowledge

Keep existence (present/missing/unknown), freshness (current/stale/unknown), divergence (clean/diverged/unknown), verification and evidence coverage separate. Human labels are only a presentation of those dimensions.

CURRENT is relative to exact inputs, runtime, parameters, methods, base and observation scope. A raw complete byte read is not exhaustive native dependency extraction. STALE means a known determinant changed; the output can retain identical bytes. DIVERGED means an observed output differs from the receipt's expected realization. UNKNOWN includes provider offline, insufficient permission, ambiguity, unsupported durable identity, watcher gaps and incomplete dependency coverage. MISSING requires a conclusive authorized not-found observation.

A restart does not restore live refs or a current observation epoch. A copy starts as a different logical asset unless an explicit audited relationship says otherwise. Renaming a bound logical asset preserves history; replacing the file at its path does not prove continuity. A rebind does not validate older generation-dependent receipts just because the replacement bytes match.

Reuse an output only with current/present/clean state, sufficient required verification and complete determining dependencies. Fonts, textures, import settings, runtime/toolchain, parameters, descriptors, Recipes and contracts can invalidate outputs. Do not assert that audio is unaffected when its dependency extractor is incomplete.


## Common continuity cases

- **Manual edit:** if a native output no longer matches its admitted production receipt, report DIVERGED (and any downstream stale/unknown consequences). Preserve the human edit; do not overwrite it merely because a rebuild proposal exists. Reconcile first and require an explicit authorized plan for replacement.
- **Provider offline or permission denied:** existence remains UNKNOWN. Do not convert inability to observe into MISSING and do not reuse a cached output as if the dependency had been checked.
- **Ambiguous identity or path replacement:** keep continuity UNKNOWN until a trusted resolver proves the same native instance or the owner performs an explicit rebind. Matching bytes alone do not restore old generation-dependent provenance.
