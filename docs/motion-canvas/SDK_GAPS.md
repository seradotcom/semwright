# Driver SDK gaps proven by Motion Canvas

This document records only gaps exercised by the implementation; it is not a proposal for speculative Driver Protocol v2.

## 1. Multi-tool runtime distribution

Driver Registry packages pin a driver executable/manifest but do not currently distribute and attest a complete auxiliary runtime such as Node + Chromium + helper files. Motion Canvas therefore requires an explicit owner `runtime` filesystem grant with the Driver-only `execute: true` opt-in. Other read-only grants remain non-executable. The driver itself verifies SHA-256 pins for every executable/helper entry before rendering.

A generic future tool-dependency/package primitive could remove this manual runtime preparation without widening filesystem access.

## 2. Protocol-v2 adoption for long-running child jobs

The final integration target now includes Driver Protocol v2 support for progress, artifacts and request cancellation. Motion Canvas deliberately remains on protocol-v1 compatibility in this PR because its tested render lifecycle is already exposed as `render.start/status/cancel/result`; it does not advertise v2 interfaces it has not wired end-to-end. This is now a driver adoption gap, not a generic SDK absence.

A later Motion Canvas pass can map its existing job registry onto protocol-v2 progress/cancellation without changing the semantic render model.

## 3. Browser sandbox composition

The supported renderer needs a real browser process plus writable temporary/profile state while the driver itself remains inside Bubblewrap + Landlock with `network=false`. Chromium's nested sandbox cannot compose with the outer namespace, and Playwright `launch()` repeatedly crashed the pinned browser through its remote-debugging-pipe path. The driver therefore starts the attested full Chromium executable directly and attaches Playwright via an ephemeral `127.0.0.1` CDP endpoint. This does not require a network grant: Bubblewrap's unshared network namespace exposes loopback only. The current generic runtime package model still does not express the auxiliary Node/browser bundle or profile storage separately, so the driver pins `TMPDIR`/XDG state to its job-specific output root and keeps Chromium in the read-only runtime grant.

A future platform/tool dependency primitive could make browser runtime/profile requirements explicit without granting broader filesystem or network access.

## 4. Resource budgets

The browser process tree also exercises the Driver Host task ceiling: Linux `RLIMIT_NPROC` counts both processes and threads. Motion Canvas therefore opts into the existing SDK maximum of 256 tasks rather than the smaller 128-task request used by earlier render probes. This does not raise the generic SDK maximum; it is a driver-specific bounded request and CI must still prove that the exact browser starts under it.

Motion Canvas + Vite + modern Chromium needs materially more virtual address space than small stdio drivers. Real Driver Host CI showed Chromium failing under the former 4 GiB `RLIMIT_AS` ceiling after Node/Vite had already succeeded. The branch therefore makes one minimal generic adjustment: keep the 512 MiB default, raise only the validated hard maximum to 16 GiB, and have Motion Canvas opt into 16 GiB explicitly. The Linux helper enforces the same maximum. CI records paired direct Chromium probes at 4 GiB and 16 GiB and requires the 16 GiB probe to launch. This changes virtual address-space reservation, not an ambient RAM grant.

## Resolved during final integration: Windows platform services

The frozen implementation baseline originally lacked Windows `semwright-platform-services`. Final integration with current `main` brought the Windows platform host and secure driver-host plumbing into this branch, so that item is no longer an SDK gap. Motion Canvas now checks the complete domain/Driver Protocol adapter on Windows CI. Live browser rendering is still Linux-only evidence and is not promoted to a Windows support claim.

## Not a gap: isolated loopback

The renderer requires an ephemeral CDP listener on `127.0.0.1`, but Driver Host `network=false` already creates a private Bubblewrap network namespace containing only loopback. The listener is therefore reachable only by the helper/Chromium processes inside that sandbox and does not require `--share-net`, Internet access or a new policy grant. Static project files continue to use Playwright request interception; no Vite HTTP server is exposed.
