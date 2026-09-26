# Driver SDK gaps proven by Motion Canvas

This document records only gaps exercised by the implementation; it is not a proposal for speculative protocol features.

## 1. Multi-tool runtime distribution

Driver Registry packages pin a driver executable/manifest but do not currently distribute and attest a complete auxiliary runtime such as Node + Firefox + helper files. Motion Canvas therefore requires an explicit owner `runtime` filesystem grant with the Driver-only `execute: true` opt-in. Other read-only grants remain non-executable. The driver verifies SHA-256 pins for Node, the helper and the selected browser executable before rendering.

A generic future tool-dependency/package primitive could remove this manual runtime preparation without widening filesystem access.

## 2. Protocol-v3 adoption for long-running child jobs

The current Driver SDK exposes Protocol v3 progress, artifacts and request cancellation. Motion Canvas negotiates Driver Protocol v3 for cooperative cancellation, progress and artifact reporting. The asynchronous `render.start/status/cancel/result` API remains available, while `render.execute` maps the same renderer onto one protocol-owned request lifecycle. Native refs remain disabled because Motion Canvas refs are managed semantic refs rather than broker-native application references.

The protocol-v3 path reuses the existing bounded job registry; it does not introduce a second render authority or duplicate renderer implementation.

## 3. Browser sandbox composition

The renderer needs a real browser process plus writable temporary/profile state while the driver remains inside Bubblewrap + Landlock with `network=false`. The final Firefox route uses the existing generic Driver-only executable-mount opt-in: the runtime is read-only, execution is explicit, and the helper can launch only the SHA-256-pinned browser path supplied by Rust. Firefox's nested content sandbox is disabled only after the outer Driver Host marker is verified; the outer sandbox remains authoritative. The helper also disables Firefox's Linux fork-server preference because that broker failed to create tab subprocesses inside the already-isolated namespace; this is a fixed compatibility choice, not an agent-controlled escape hatch.

The current driver package model still does not express the complete auxiliary Node/browser bundle or profile storage as a first-class distribution primitive. A future generic tool-dependency package could remove the owner-prepared runtime mount without broadening filesystem access.

## Browser-version compatibility evidence

Playwright 1.63.0 / Firefox 155.0 repeatedly failed inside the otherwise-conformant Driver Host after Juggler startup with a tab-subprocess `SIGSEGV`. Supplying a private `/dev/shm` did not remove the crash and AppArmor logs showed no relevant denial. The runtime therefore pins Playwright 1.61.1 / Firefox 151.0, the last-good pair documented by a current upstream Firefox SIGSEGV regression report, while preserving the same digest pinning, filesystem, network and process policy. This is an upstream compatibility pin, not a new Driver SDK authority gap.

## Resolved experiment: resource ceilings

Earlier Chromium experiments drove temporary larger resource requests without changing the reproducible Chrome-for-Testing `SIGTRAP` startup failure. The final Firefox path returns to the existing 4 GiB SDK hard ceiling. After pinning the known-good Firefox 151 generation, real Driver Host CI isolated one remaining resource constraint: at 128 tasks WebRender failed to create a thread with `EAGAIN` (`Resource temporarily unavailable`). Motion Canvas therefore opts into the already-supported 256-task maximum. Linux `RLIMIT_NPROC` counts threads as well as processes; no generic resource-limit ceiling is increased by this driver.

## Resolved during final integration: Windows platform services

The frozen implementation baseline originally lacked Windows `semwright-platform-services`. Final integration with current `main` brought the Windows platform host and secure driver-host plumbing into this branch, so that item is no longer an SDK gap. Motion Canvas now checks the complete domain/Driver Protocol adapter on Windows CI. Live browser rendering is still Linux-only evidence and is not promoted to a Windows support claim.

## Not a gap: network authority

The final renderer opens no Vite server and no browser-control listener. Static built files are delivered through Playwright request interception at a synthetic origin; external page requests are aborted. Driver Host therefore remains `network=false` with no loopback exception or Internet grant.
