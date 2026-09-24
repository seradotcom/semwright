# Driver SDK gaps proven by Motion Canvas

This document records only gaps exercised by the implementation; it is not a proposal for speculative Driver Protocol v2.

## 1. Multi-tool runtime distribution

Driver Registry packages pin a driver executable/manifest but do not currently distribute and attest a complete auxiliary runtime such as Node + Firefox + helper files. Motion Canvas therefore requires an explicit owner `runtime` filesystem grant. The driver itself verifies SHA-256 pins for every executable/helper entry before rendering.

A generic future tool-dependency/package primitive could remove this manual runtime preparation without widening filesystem access.

## 2. Long-running child jobs

Driver Protocol v1 request/response does not negotiate driver child-job progress, events or cooperative cancellation. Motion Canvas exposes `render.start/status/cancel/result` as a driver-local compatibility surface. It reports observed states rather than fabricated percentages.

A later negotiated jobs/events interface could unify this with broker jobs without changing the semantic render model.

## 3. Browser sandbox composition

The supported renderer needs a real browser process plus writable temporary/profile state while the driver itself remains inside Bubblewrap + Landlock with `network=false`. Firefox's nested Linux content sandbox cannot create its tab-process boundary inside the current outer namespace, so the pinned helper disables that inner layer only after verifying the Driver Host marker. The current generic runtime package model does not express browser profile storage separately, so this driver pins `TMPDIR`/XDG state to its job-specific owner-granted output root and keeps the Playwright-installed Firefox binary in the read-only runtime grant.

A future platform/tool dependency primitive could make browser runtime/profile requirements explicit without granting broader filesystem or network access.

## 4. Resource budgets

Motion Canvas + Vite + Firefox needs materially more address space/process budget than small stdio drivers. Protocol v1 permits up to the current 4 GiB ceiling; CI records Node compatibility with that ceiling. The branch does not raise generic limits without evidence.

## Not a gap: loopback

The render harness intentionally avoids a Vite HTTP listener. Static built files are delivered through Playwright request interception, so no loopback-network exception or full network grant is required.
