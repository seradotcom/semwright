# Driver SDK gaps proven by Motion Canvas

This document records only gaps exercised by the implementation; it is not a proposal for speculative Driver Protocol v2.

## 1. Multi-tool runtime distribution

Driver Registry packages pin a driver executable/manifest but do not currently distribute and attest a complete auxiliary runtime such as Node + Chromium headless shell + helper files. Motion Canvas therefore requires an explicit owner `runtime` filesystem grant. The driver itself verifies SHA-256 pins for every executable/helper entry before rendering.

A generic future tool-dependency/package primitive could remove this manual runtime preparation without widening filesystem access.

## 2. Protocol-v2 adoption for long-running child jobs

The final integration target now includes Driver Protocol v2 support for progress, artifacts and request cancellation. Motion Canvas deliberately remains on protocol-v1 compatibility in this PR because its tested render lifecycle is already exposed as `render.start/status/cancel/result`; it does not advertise v2 interfaces it has not wired end-to-end. This is now a driver adoption gap, not a generic SDK absence.

A later Motion Canvas pass can map its existing job registry onto protocol-v2 progress/cancellation without changing the semantic render model.

## 3. Browser sandbox composition

The supported renderer needs a real browser process plus writable temporary/profile state while the driver itself remains inside Bubblewrap + Landlock with `network=false`. Chromium's nested sandbox cannot compose with the current outer namespace, so the pinned helper uses `chromiumSandbox:false` and `--no-zygote` only after verifying the Driver Host marker. Renderer child processes remain confined by the outer Bubblewrap + Landlock namespace and the existing process limit. CI proved that forcing `--single-process` makes the pinned headless shell abort with `SIGTRAP`, so that unsupported workaround is explicitly avoided. The current generic runtime package model does not express this browser composition or profile storage separately, so this driver pins `TMPDIR`/XDG state to its job-specific owner-granted output root and keeps the Playwright-installed Chromium headless shell binary in the read-only runtime grant.

A future platform/tool dependency primitive could make browser runtime/profile requirements explicit without granting broader filesystem or network access.

## 4. Resource budgets

Motion Canvas + Vite + Chromium headless shell needs materially more virtual address space than small stdio drivers. Real Driver Host CI showed Chromium failing under the former 4 GiB `RLIMIT_AS` ceiling after Node/Vite had already succeeded. The branch therefore makes one minimal generic adjustment: keep the 512 MiB default, raise only the validated hard maximum to 16 GiB, and have Motion Canvas opt into 16 GiB explicitly. The Linux helper enforces the same maximum. CI records paired direct Chromium probes at 4 GiB and 16 GiB and requires the 16 GiB probe to launch. This changes virtual address-space reservation, not an ambient RAM grant.

## 5. Windows platform-service composition

The frozen baseline has no Windows implementation of `semwright-platform-services`, while Driver Protocol depends on that crate. A Windows compile of the complete protocol adapter therefore fails before Motion Canvas-specific code. This branch keeps the managed model OS-neutral and verifies the complete adapter on macOS, but does not redesign generic platform services or claim Windows support.

## Not a gap: loopback

The render harness intentionally avoids a Vite HTTP listener. Static built files are delivered through Playwright request interception, so no loopback-network exception or full network grant is required.
