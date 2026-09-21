# Test evidence boundaries

- `python/`: executed Python host/dispatcher and data-contract tests, with mocked bpy.
- `python/cdp_live.py`: separately invoked real sandboxed Chromium, Python CDP client.
- `js/`: executed shared desktop-bridge validation contract, not a GNOME/KWin runtime.
- `native/`: compiled/executed C harness probing Linux openat2 semantics.
- `golden/`: canonical broker hello fixtures; Rust comparison test authored, not run.
- Rust crate tests and `crates/core/tests/`: unit/property/fake integration source, not run.
- `fuzz/` at repository root: five parser target sources and seed corpora, not run.
- `crates/core/benches/core.rs`: benchmark source, not run; no performance numbers supplied.

Consult [VERIFY.md](../VERIFY.md) for exact commands, logs and counts. Presence in this
directory is not a passed test. All live tests must use owned/disposable fixtures only.
