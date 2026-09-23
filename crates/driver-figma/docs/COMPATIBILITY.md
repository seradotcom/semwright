# Compatibility

| Surface | Status |
|---|---|
| Semwright Driver Protocol | v2; child events negotiated and exercised |
| Figma Design | Type-checked plugin subset; real Figma acceptance pending |
| FigJam | Type-checked curated subset; real Figma acceptance pending |
| Motion | Beta; type-checked/fake-runtime tested, real Figma pending |
| Prototyping | Current reactions API subset |
| Slides/Buzz | Recognized; unsupported operations fail explicitly |
| Linux Driver Host | Real conformance test integrated; hosted native CI is authoritative where local user namespaces are unavailable |
| macOS/Windows Driver Host | Depends on Semwright host sandbox support; no live Figma claim from Rust-only portability |
| Real Figma | Not tested; no disposable authorized session used |

The Figma bridge emits allowlisted selection/page/document-change events through Driver Protocol v2. Cooperative cancellation, progress, artifacts and dynamic capabilities remain deliberately unnegotiated until the Figma surface can satisfy those contracts.
