# Verification evidence

This directory contains durable, exact-SHA technical evidence used by Semwright verification and
release-review tooling. It is not primary product documentation and a historical PASS never certifies
a later commit.

Public repository evidence follows these rules:

1. Evidence is exact-SHA scoped.
2. Executed, skipped, blocked and not-run states stay distinct.
3. Only durable technical evidence belongs here. Temporary development records are intentionally excluded.
4. Current release policy wins over historical observations. See docs/release-policy.md, VERIFY.md
   and RELEASE_BLOCKERS.md.

R16 retained repository-review evidence is in verification/r16-closeout.
