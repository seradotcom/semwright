# Browser semantic completeness

Baseline: `e543a6bed1497840efdb06ce0f9b96748e9529a1`.
Provider: existing first-party `chromium` backend over an owned, isolated Chromium instance and CDP.

## Status

**Implementation boundary: COMPLETE.**

This means the provider covers the ordinary user-facing browser semantics defined below without requiring agent-provided JavaScript, raw CDP, blind coordinates, a user Chrome profile, or CSS selectors for every action. Release acceptance remains evidence-driven: the exact delivery HEAD must pass source contracts, Rust quality gates, broker contracts and the real Chromium acceptance suite in GitHub Actions before this status is treated as certified.

## Semantic authority

The browser model composes several official Chromium surfaces instead of pretending one tree is sufficient:

1. **Accessibility** — role, accessible name, value, state and semantic relationships.
2. **DOM** — stable backend identity, bounded structure, shadow roots, focus and exact action grounding.
3. **Page/Target** — owned tabs, history, page lifecycle, frame hierarchy, popups and OOPIF sessions.
4. **Input** — bounded keyboard, pointer, hover, page-scroll and same-frame drag after semantic grounding.
5. **Browser/Network metadata** — bounded downloads and redacted diagnostics without bodies, headers or credentials.
6. **Filesystem grants** — owner-authorized upload sources copied into private browser-instance staging before Chromium sees them.

Playwright may be useful as internal test/render machinery elsewhere in Semwright. It is deliberately **not** the agent-facing authority for this provider.

## User-facing semantic surface

The first-party surface now covers:

- isolated browser launch/status and owned tab list/open/close/focus;
- navigation, reload, back, forward and stop;
- bounded page scrolling by semantic direction/amount for lazy or infinite content discovery;
- frame discovery and generation-bound frame refs, including cross-origin/OOPIF sessions when the frame origin is owner-allowed;
- accessibility snapshot, semantic query, semantic inspect and bounded semantic wait;
- bounded DOM snapshot/query for structural evidence and compatibility;
- open shadow-root semantics exposed by Chromium;
- focus, scroll-to-element, hover, click and text/contenteditable fill;
- bounded semantic key presses with an allowlisted key/modifier vocabulary;
- verified checkbox/radio/switch state changes;
- native single-select selection with post-action semantic verification;
- same-frame drag between two independently broker-validated semantic refs;
- JavaScript-dialog status and explicit accept/dismiss response;
- popup/new-tab discovery through owned tab semantics;
- grant-scoped file upload to exact `INPUT type=file` targets, with relative-path validation, byte/file budgets, private staging and no host-source path disclosure;
- screenshots, download status/quota handling and redacted diagnostics.

Legacy `browser.dom.*` operations remain for compatibility. New agents should prefer `browser.semantic.*`, `browser.element.*`, `browser.page.*` and `browser.frame.*` where possible.

## Reference model

Semantic mutation requires exact opaque refs. Tab, frame and DOM refs are bound to the owned browser instance and to the relevant document/CDP-session generation. Site-isolated subframes use their own attached CDP session when Chromium promotes them to OOPIF targets. Navigation or observed document mutation retires stale refs rather than silently retargeting replacement nodes.

Operations requiring two elements, currently `browser.element.drag_to`, resolve and validate both public refs in the broker before the backend receives `_target`/`_target2`. Cross-session or cross-backend secondary refs fail closed.

## Layer discipline

A lower layer does not silently replace a higher one:

- semantic query may return multiple candidates;
- mutating operations require one exact ref (or two exact refs for drag);
- pointer coordinates are calculated internally from verified DOM box models and never supplied by the agent;
- click/hover/drag use hit-testing before input dispatch;
- check/select/upload verify type/state contracts instead of assuming input succeeded;
- page scrolling uses viewport metrics plus a fixed semantic amount rather than arbitrary coordinates;
- vision remains outside this adapter and is only an upper-layer fallback when browser semantics are genuinely absent.

## Explicitly outside the completeness boundary

These surfaces are classified, not forgotten:

- `Runtime.evaluate`, arbitrary JavaScript/functions and arbitrary raw CDP — **unsupported by design**;
- unrestricted cookies, storage, headers, request bodies and response bodies — **unsupported by design** because they can contain credentials/secrets and are not necessary for ordinary page operation;
- DevTools debugger/profiler/tracing — **unsupported by design** as developer diagnostics rather than user-facing browser semantics;
- extensions, the user's normal Chrome profile and credential stores — **unsupported by design**;
- cross-frame drag/drop — **unsupported by design** until Chromium provides a trustworthy same-coordinate/context contract; callers should compose semantic actions within each frame instead;
- closed/opaque shadow DOM not exposed through Chromium semantics — outside the provider's observable authority;
- bypassing browser origin policy or sandboxing — never permitted.

`DOMSnapshot.captureSnapshot` is used as bounded layout/paint-order evidence for fail-closed pointer hit-testing. AX + DOM identity remain the semantic authority for object identity and intent; DOMSnapshot answers only whether the painted element at the derived action point belongs to the exact target subtree.

## Security invariants

- no agent-exposed JavaScript evaluation;
- no attach to arbitrary remote debugger endpoints;
- no use of the user's normal browser profile;
- exact owner origin allowlist for top-level/frame semantic authority;
- generation-bound stale-reference handling;
- opaque public refs only;
- sensitive input fields are redacted from confirmation/audit summaries;
- uploads require `filesystem.read:<root>` and use descriptor-relative reads plus private staging;
- bounded files, bytes, nodes, query results, diagnostics and artifacts;
- browser sandbox remains enabled and launch authority remains explicit.

See `CDP_COVERAGE.json` for the machine-readable classification and `VERIFY.md` for exact-head acceptance evidence.
