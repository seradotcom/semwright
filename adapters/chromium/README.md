# Chromium semantic adapter

Semwright launches and owns a disposable Chromium-family process with a private profile and loopback CDP endpoint. It never attaches to the user's normal browser and does not accept arbitrary remote-debugger URLs. Running as root or disabling Chromium's sandbox is not supported.

```toml
[policy]
profile = "observe"
allow = ["browser.observe", "browser.modify"]

[[policy.filesystem]]
name = "uploads"
path = "/absolute/owner/path"
read = true
write = false

[browser]
executable = "/usr/bin/chromium"
allowed_origins = ["http://127.0.0.1:8000"]
allow_downloads = false
```

`allowed_origins` is exact owner configuration. The default grants no network origin; `about:blank` remains available. The allowlist controls Semwright's top-level navigation and semantic authority for frames. It is not a general network firewall: normal page JavaScript and subresource loading still follow Chromium's own browser/network rules.

## Semantic model

The provider combines official CDP Accessibility, DOM, Page/Target, Input and Browser metadata rather than exposing raw CDP to the agent. `Runtime.evaluate` and arbitrary JavaScript are deliberately absent.

Prefer these high-level surfaces:

- `browser.semantic.snapshot/query/inspect/wait` for role/name/value/state discovery;
- `browser.frame.list` for generation-bound frame/OOPIF refs;
- `browser.page.*` for lifecycle/history and semantic viewport scrolling;
- `browser.element.*` for focus, scroll-to-element, hover, click, fill, bounded keys, check/radio/switch, native select, drag and grant-scoped upload;
- `browser.tab.*` for owned tabs and popup discovery;
- screenshot/download/diagnostic operations for bounded artifacts and redacted metadata.

Legacy `browser.dom.*` operations remain for compatibility. CSS query is not required for ordinary semantic operation.

## Identity and stale references

Opaque refs are tied to the owned browser instance and the relevant CDP document/session generation. OOPIF frames use their attached CDP session when Chromium site isolation creates one. Navigation and observed document mutation retire affected refs rather than silently retargeting a replacement node.

Clicks, hovers and drags derive coordinates internally from a verified DOM box model and confirm the painted target subtree with bounded DOMSnapshot layout/paint-order evidence; the agent never supplies pixel coordinates. `browser.element.drag_to` requires two independent broker-resolved refs in the same frame context. `browser.page.scroll` uses visual viewport metrics plus a semantic direction/amount, which keeps lazy/infinite-page discovery above raw wheel coordinates.

## File upload

`browser.element.upload` is intentionally not an arbitrary path API. The descriptor requires `filesystem.read:root`; the policy must name a readable owner grant. Relative paths are validated and read through Semwright's confined `Root` API with file/count/byte budgets. Data is copied to private `0600` staging inside the disposable browser profile, and only those staged paths are passed to Chromium. Host source paths are never returned to the agent and staged data dies with the browser instance.

## Security boundary

The adapter does not expose:

- arbitrary JavaScript or raw CDP;
- the user's Chrome profile, extensions or credentials;
- unrestricted cookies/storage/headers/request/response bodies;
- developer debugger/profiler/tracing surfaces;
- cross-frame drag/drop or browser-sandbox/origin bypasses.

Downloads are denied by default. When explicitly enabled they remain bounded by configured per-file, total-byte and count quotas; excess downloads are cancelled and removed from owned storage. Diagnostics expose bounded metadata only, never console text, headers, request bodies or download filenames.

## Evidence

The Rust live suite uses disposable Chromium plus loopback HTTP fixtures. It covers Accessibility semantics, open shadow DOM, popup/new-tab discovery, forms/contenteditable, check/select/key/hover, semantic waits, page lifecycle/scroll, dialogs, downloads, same-frame drag, grant-scoped upload, frame navigation and a separate cross-origin fixture for OOPIF semantics. Browser/profile/artifact cleanup is verified after shutdown/crash paths.

See `SEMANTIC_COMPLETENESS.md` for the bounded completeness definition, `CDP_COVERAGE.json` for the machine-readable classification, and `VERIFY.md` for exact-head CI evidence.
