# Driver SDK gaps proven by Motion Canvas

This document records only gaps exercised by the implementation; it is not a proposal for speculative Driver Protocol v2.

## 1. Multi-tool runtime distribution

Driver Registry packages pin a driver executable/manifest but do not currently distribute and attest a complete auxiliary runtime such as Node + Chromium + helper files. Motion Canvas therefore requires an explicit owner `runtime` filesystem grant. The driver itself verifies SHA-256 pins for every executable/helper entry before rendering.

A generic future tool-dependency/package primitive could remove this manual runtime preparation without widening filesystem access.

## 2. Long-running child jobs

Driver Protocol v1 request/response does not negotiate driver child-job progress, events or cooperative cancellation. Motion Canvas exposes `render.start/status/cancel/result` as a driver-local compatibility surface. It reports observed states rather than fabricated percentages.

A later negotiated jobs/events interface could unify this with broker jobs without changing the semantic render model.

## 3. Browser sandbox composition

Ubuntu 24.04 GitHub runners reject Chromium's nested user-namespace sandbox while the driver is already inside the required Bubblewrap namespace. This implementation makes that composition explicit: Chromium sandboxing may be disabled only after the helper verifies the Driver Host sandbox marker; the outer Bubblewrap + Landlock and `network=false` remain mandatory.

A platform service capable of declaring/attesting nested browser sandbox requirements could make this dependency more portable.

## 4. Resource budgets

Motion Canvas + Vite + Chromium needs materially more address space/process budget than small stdio drivers. Protocol v1 permits up to the current 4 GiB ceiling; CI records Node compatibility with that ceiling. The branch does not raise generic limits without evidence.

## Not a gap: loopback

The render harness intentionally avoids a Vite HTTP listener. Static built files are delivered through Playwright request interception, so no loopback-network exception or full network grant is required.
