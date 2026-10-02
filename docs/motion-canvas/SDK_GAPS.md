# Driver SDK gaps proven by Motion Canvas

This document records only gaps exercised by the implementation; it is not a proposal for speculative protocol features.

## 1. Runtime bundle distribution remains the open packaging gap

Protocol v7 now removes driver-owned executable discovery: Node is an owner-pinned `Manifest.tools` entry, the render helper is embedded in the Rust driver and delivered over bounded stdin, and project/output/runtime/fontconfig paths are typed Host-resolved arguments. The browser and its Playwright resources still live in one explicit owner-granted read-only executable `runtime` bundle. Semwright does not yet snapshot or attest every file in that bundle, so this document does not claim full bundle immutability or per-file browser attestation.

A generic runtime-bundle packaging/attestation primitive would close that remaining distribution gap for Motion Canvas, Blender, LibreOffice and similar applications without returning executable-path discovery to individual drivers.

## 2. Protocol-v7 Host-owned render jobs

Motion Canvas now negotiates Driver Protocol v7 for Host-mediated tools, typed logical path arguments and detached session-bound runtime-tool jobs. `render.start/status/cancel/result` map to that Host job lifecycle; `render.execute` uses the same job path while reporting progress/artifacts and performing narrow cleanup cancellation if its parent request is cancelled. The driver no longer spawns or kills Node/browser process trees itself. Native refs remain disabled because Motion Canvas refs are managed semantic refs rather than broker-native application references.

## 3. Browser sandbox composition

The renderer needs a real browser process plus writable temporary/profile state while the runtime-tool child remains inside Bubblewrap + Landlock with `network=false`. The runtime bundle is read-only with explicit execute authority; the embedded helper resolves Playwright's Firefox executable and rejects it unless its canonical path remains inside that bundle. Firefox writable profile/cache state is redirected to the delegated output root. Firefox's nested content sandbox is disabled only after the outer Driver Host marker is verified; the outer sandbox remains authoritative. The helper also disables Firefox's Linux fork-server preference because that broker failed to create tab subprocesses inside the already-isolated namespace; this is a fixed compatibility choice, not an agent-controlled escape hatch.

The current package model still does not attest the complete Node/browser resource bundle as one immutable artifact. That remains a generic packaging gap rather than a Motion-specific resolver.

## Browser-version compatibility evidence

Playwright 1.63.0 / Firefox 155.0 repeatedly failed inside the otherwise-conformant Driver Host after Juggler startup with a tab-subprocess `SIGSEGV`. Supplying a private `/dev/shm` did not remove the crash and AppArmor logs showed no relevant denial. The runtime therefore pins Playwright 1.61.1 / Firefox 151.0, the last-good pair documented by a current upstream Firefox SIGSEGV regression report, while preserving the same filesystem, network and process policy. The CI evidence records the concrete browser digest, but the complete browser bundle is still an owner-granted runtime root rather than a Host-attested immutable bundle. This is an upstream compatibility pin, not a new Driver SDK authority gap.

## Resolved experiment: resource ceilings

Earlier Chromium experiments drove temporary larger resource requests without changing the reproducible Chrome-for-Testing `SIGTRAP` startup failure. The final Firefox path returns to the existing 4 GiB SDK hard ceiling. After pinning the known-good Firefox 151 generation, real Driver Host CI isolated one remaining resource constraint: at 128 tasks WebRender failed to create a thread with `EAGAIN` (`Resource temporarily unavailable`). Motion Canvas therefore opts into the already-supported 256-task maximum. Linux `RLIMIT_NPROC` counts threads as well as processes; no generic resource-limit ceiling is increased by this driver.

## Resolved during final integration: Windows platform services

The frozen implementation baseline originally lacked Windows `semwright-platform-services`. Final integration with current `main` brought the Windows platform host and secure driver-host plumbing into this branch, so that item is no longer an SDK gap. Motion Canvas now checks the complete domain/Driver Protocol adapter on Windows CI. Live browser rendering is still Linux-only evidence and is not promoted to a Windows support claim.

## Not a gap: network authority

The final renderer opens no Vite server and no browser-control listener. Static built files are delivered through Playwright request interception at a synthetic origin; external page requests are aborted. Driver Host therefore remains `network=false` with no loopback exception or Internet grant.
