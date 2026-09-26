# Browser semantic completeness

Baseline: `e543a6bed1497840efdb06ce0f9b96748e9529a1`.
Provider: existing first-party `chromium` backend over an owned, isolated CDP browser.

## Definition

Browser semantic completeness means Semwright can observe, identify and perform the
ordinary user-facing interactions of a modern web page without requiring an agent to
supply JavaScript, raw CDP, blind coordinates or a CSS selector for every action.

The semantic authority is a bounded composition of:

- Accessibility tree: role, accessible name/value/state and semantic relationships;
- DOM: stable backend identity, document structure, focus and exact action grounding;
- Page: tabs, frames, history, lifecycle and dialogs;
- Input: bounded keyboard/pointer actions against already-resolved semantic targets;
- Browser/Network metadata: downloads and diagnostics without exposing bodies/secrets.

Playwright may remain useful as internal test/render machinery elsewhere in Semwright,
but it is not the agent-facing semantic authority for this provider.

## Completeness boundary

In scope:

- owned tab/page lifecycle;
- frame hierarchy, including frame-scoped semantics where CDP can prove identity;
- accessibility semantics for interactive/content nodes;
- DOM/shadow-root inspection needed to ground those semantics;
- focus, scroll, click, text entry, checked/toggled state, selection and bounded keys;
- bounded dialogs, popups and download lifecycle;
- deterministic stale-reference invalidation across navigation/document mutation;
- safe screenshots and bounded diagnostic/network metadata.

Explicitly not part of semantic completeness:

- `Runtime.evaluate`, arbitrary JavaScript/functions or arbitrary raw CDP;
- DevTools debugging/profiling/tracing as an agent feature;
- browser extensions, user Chrome profiles or credential stores;
- unrestricted cookies/storage/headers/request/response bodies;
- arbitrary local file upload without a broker-owned file-grant contract;
- bypassing origin policy or browser sandboxing.

## Semantic layers

1. **Page** — isolated browser, tabs, navigation/history, lifecycle and frames.
2. **Semantic tree** — AX role/name/value/state, with exact DOM refs when backed by DOM.
3. **DOM structure** — bounded document/shadow/frame structure for evidence and fallback.
4. **Semantic actions** — typed operations against refs, never agent-provided coordinates.
5. **Input fallback** — CDP Input only after target identity, focus and hit-test checks.
6. **Vision** — outside this adapter; used only above Semwright when semantics are absent.

A lower layer does not silently replace a higher one. Semantic query returns candidates;
mutating operations require one exact, generation-bound target. Navigation and page-visible
mutations retire old document refs conservatively before dispatch.

## Current tranche

The first completeness tranche adds frame refs, frame listing, Accessibility enablement,
semantic snapshot/query/inspect, page reload/back/forward/stop and semantic focus/scroll/
click/fill aliases. Existing DOM operations remain supported for compatibility.

The second tranche adds verified checked-state mutation, native single-select choice,
bounded semantic key presses, hover, semantic waits and JavaScript-dialog status/response.
The remaining completeness work is cross-origin/OOPIF acceptance plus an explicit decision
on drag/drop and broker-granted file upload. DOMSnapshot remains an optional evidence layer,
not a replacement for AX + DOM identity. This document must not claim COMPLETE until those
remaining boundaries are classified and tested.
