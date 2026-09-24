# Driver SDK gaps proven by Motion Canvas

This document records only gaps exercised by the implementation; it is not a proposal for speculative Driver Protocol v2.

## 1. Multi-tool runtime distribution

Driver Registry packages pin a driver executable/manifest but do not currently distribute and attest a complete auxiliary runtime such as Node + Chromium headless shell + helper files. Motion Canvas therefore requires an explicit owner `runtime` filesystem grant. The driver itself verifies SHA-256 pins for every executable/helper entry before rendering.

A generic future tool-dependency/package primitive could remove this manual runtime preparation without widening filesystem access.

## 2. Long-running child jobs

Driver Protocol v1 request/response does not negotiate driver child-job progress, events or cooperative cancellation. Motion Canvas exposes `render.start/status/cancel/result` as a driver-local compatibility surface. It reports observed states rather than fabricated percentages.

A later negotiated jobs/events interface could unify this with broker jobs without changing the semantic render model.

## 3. Browser sandbox composition

The supported renderer needs a real browser process plus writable temporary/profile state while the driver itself remains inside Bubblewrap + Landlock with `network=false`. Chromium's normal nested sandbox/process topology cannot compose with the current outer namespace, so the pinned helper uses `chromiumSandbox:false` and a fixed single-process/no-zygote launch only after verifying the Driver Host marker. The current generic runtime package model does not express this browser composition or profile storage separately, so this driver pins `TMPDIR`/XDG state to its job-specific owner-granted output root and keeps the Playwright-installed Chromium headless shell binary in the read-only runtime grant.

A future platform/tool dependency primitive could make browser runtime/profile requirements explicit without granting broader filesystem or network access.

## 4. Resource budgets

Motion Canvas + Vite + Chromium headless shell needs materially more address space/process budget than small stdio drivers. Protocol v1 permits up to the current 4 GiB ceiling; CI records Node compatibility with that ceiling. The branch does not raise generic limits without evidence.

## 5. Windows platform-service composition

The frozen baseline has no Windows implementation of `semwright-platform-services`, while Driver Protocol depends on that crate. A Windows compile of the complete protocol adapter therefore fails before Motion Canvas-specific code. This branch keeps the managed model OS-neutral and verifies the complete adapter on macOS, but does not redesign generic platform services or claim Windows support.

## Not a gap: loopback

The render harness intentionally avoids a Vite HTTP listener. Static built files are delivered through Playwright request interception, so no loopback-network exception or full network grant is required.
