# Chromium adapter

The Rust backend launches its own Chromium-family process with a private profile and
loopback CDP endpoint. It does not attach to the user's normal browser or accept arbitrary
remote debugger URLs. Running the browser as root or disabling its sandbox is not a
supported mode. Browser launching itself is a code-execution risk requiring explicit
capability and operator approval.

```toml
[policy]
profile = "observe"
allow = ["browser.observe", "browser.modify"]

[browser]
executable = "/usr/bin/chromium"
allowed_origins = ["http://127.0.0.1:8000"]
allow_downloads = false
```

The exact allowed origins are owner configuration. The default is no network origin;
`about:blank` remains available. This controls **top-level targeting/navigation**, not every
subresource or redirect request. Normal JavaScript inside web pages still runs. This is
not an offline browser or a network sandbox.

Commands include browser status/launch, tabs, navigation, DOM snapshot/query/click/fill,
screenshot and bounded diagnostic/download metadata. There is no `Runtime.evaluate` tool
or arbitrary JS function exposed to an agent. Input uses DOM identity/focus and the CDP
Input domain. Clicks require a hit-test consistent with the selected node; ambiguous CSS
queries return candidates. Target/session/DOM generations invalidate refs when relevant
navigation/tree events arrive.

A CDP backendNodeId can remain describable after navigation even though the node is no
longer in the active document. Identity alone is not liveness. This was observed during
the separate Python probe and is why the Rust adapter retains generation checks before
using a DOM reference. Cross-frame, event-order and reload races still need a Rust adapter
integration suite.

Screenshots are private temporary PNG artifacts rather than base64 blobs in audit. Download
handling is denied by default. Enabling it lacks a complete byte/quota policy and therefore
requires further hardening. Normal shutdown closes the owned browser/profile, but all
crash/error cleanup paths are not complete; do not use a credential-bearing profile.

## Executed evidence

`python tests/python/cdp_live.py` ran eight tests against sandboxed Chromium
144.0.7559.96 under an unprivileged account and a disposable profile. The fixture was
inserted into `about:blank` using the test harness's Page.setDocumentContent, not loaded
through a real HTTP origin. The harness covers DOM query, focus/type, hit-test/click,
screenshot signature, metadata, download denial API and navigation invalidation events.

That test-only fixture injection is **not an agent-exposed command**. The test uses a
Python CDP client: it does not compile, start or validate the Rust adapter or broker. The
first HTTP-fixture attempt did not complete; an intermediate test incorrectly assumed a
detached backend node must stop being describable. Both exploratory logs are retained and
classified in VERIFY.md. Test the Rust route and origin enforcement separately.
