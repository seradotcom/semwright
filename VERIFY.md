# Verification — Semwright 0.9.0-dev.1

**Verdict: development source delivered; full build/release acceptance is NOT met.**
This report separates execution evidence from authored tests and unavailable tools.
The Rust workspace was not compiled. No Cargo.lock, Rust binary, Rust test pass,
coverage result, benchmark measurement, live compositor certificate or plugin sandbox
certificate is supplied. Consult [release blockers](RELEASE_BLOCKERS.md).

## Authoring environment

Debian GNU/Linux 13.3 (trixie), x86_64, Linux 6.18.44. Python 3.13.5, Node 22.16.0,
GCC 14.2.0 and Chromium 144.0.7559.96 were available. Xvfb was present but the Rust X11
backend was not compiled/exercised. Rust/Cargo/rustup and crate caches were unavailable.
Terminal network/DNS/toolchain download attempts did not obtain a compiler. No user's
remote computer was modified to work around this restriction.

No real Blender, GNOME, Plasma/KWin, Sway, Hyprland, Wayland compositor/portal session,
or executed Rust plugin sandbox was available. Python/JS and kernel checks ran in the
build sandbox; the live browser test used a disposable profile under an unprivileged UID
without disabling Chromium's sandbox.

## Actually executed checks

| Check | Final recorded result | What it proves—and does not prove |
|---|---|---|
| Python unittest discovery | **71 tests passed** | Actual Python host/validation/dispatch/data tests with mocked bpy. Not Blender or Rust execution |
| Node shared bridge contracts | **20 tests passed** | Actual shared JS command validation/exact-window contract. Not GNOME/KWin runtime support |
| GCC native openat2 harness | **20 checks passed**, compiled with `-Wall -Wextra -Werror` | Kernel/FD semantics on this sandbox; includes setup checks. Not Rust filesystem implementation execution |
| Live Chromium CDP probe | **8 tests passed** | Python client against real sandboxed Chromium. Not Rust adapter/broker |
| Source/data validation | Available check script run; exact totals in final log | JSON/TOML/YAML parsing, Python AST, JS syntax, shell syntax, schema checks, generated-doc sync, local-link checks. Not Rust parsing or full lint |

Commands and first successful component logs are preserved under `verification/`:

```sh
python -m unittest discover -s tests/python -v
node --test tests/js/bridge.test.mjs
gcc -Wall -Wextra -Werror -O2 tests/native/openat2.c -o <temporary executable>
<temporary executable> <private temporary fixture directory>
python tests/python/cdp_live.py
python scripts/verify-source.py
```

The final consolidated rerun is recorded in
[verification/local-latest/summary.json](verification/local-latest/summary.json), with
one log per actually runnable command. Its overall status remains **INCOMPLETE_OR_FAILED**
when required Rust/quality tools are absent. Each result distinguishes PASS/FAIL/BLOCKED.
A bounded run cannot be interpreted as a background service or future verification promise.

The 71 Python count includes a deterministic randomized framing loop and a schema sweep;
it does not count each random iteration/schema as a separate unit test. The 20 Node count
similarly includes a seeded coordinate loop. Do not add those iterations to inflate test
counts. Native checks include setup assertions, so they are called checks rather than
20 complete integration scenarios. Rust source contains unit/property/golden/fake-integration
tests, but the number of executed Rust tests is **zero**.

## Final-pass regression found and corrected

`verification/consolidated-initial/` preserves the first consolidated pass. Its source
check found a report link whose target had not yet been written, and its Python suite
reported 1 failure plus 15 errors because a new contract generator emitted a list of
Blender command descriptors instead of the command-name-to-input-schema map consumed
by the add-on. The generator was corrected; the existing schema-equality and dispatcher
tests exercise that regression. The runner now writes an explicitly RUNNING report
before checking documentation links. Only the subsequent final rerun is used for the
PASS counts above; the failing intermediate logs are retained.

## Chromium evidence boundaries and exploratory failures

The successful live probe verifies DOM query/labels, native focus+insertText, hit-test+click,
PNG screenshot structure, target metadata, accepting a download-deny configuration API,
and navigation invalidation events. It does not test an actual download being blocked.
The fixture is inserted into `about:blank` by a **test-only** Page.setDocumentContent call.
That is not an exposed Semwright command. Real origin/HTTP restrictions are not verified
by injecting a local document.

`chromium-cdp-initial-attempt.log` records a timed-out attempt using an HTTP fixture; it did
not complete and is not a success. `chromium-cdp-detached-node-discovery.log` records an
intermediate 7-pass/1-fail run: the failing test incorrectly assumed a backend node ID
would become undescribable after navigation. Chromium can retain detached nodes. The test
was corrected to check current-document membership and invalidation events, and the Rust
source retains generation checks. `chromium-startup.log` is exploratory startup output,
not a separate acceptance result. These files are retained rather than rewriting history.

The failed attempt's owned browser processes were explicitly stopped and its disposable
profile removed. The final harness closes its own sessions/browser and removes its own
profile. No normal user browser was attached, no credentials were supplied and no
screenshot was preserved as a fabricated demo.

## Blocked or unexecuted quality gates

| Gate | Status/reason |
|---|---|
| `cargo fmt --all -- --check` | BLOCKED: cargo/rustfmt unavailable; source is not certified formatted |
| `cargo check --locked --workspace --all-targets` | BLOCKED: no Cargo/toolchain/lockfile |
| `cargo clippy -- ... -D warnings` | BLOCKED; warnings/type/API compatibility are not certified |
| Workspace/all-target tests and doctests | BLOCKED; no Rust tests executed |
| `cargo doc` | BLOCKED; Rust API docs not built |
| Release build | BLOCKED; no native binary artifact |
| cargo-audit / cargo-deny | BLOCKED; dependencies/licenses not resolved/audited |
| Rust coverage and fuzzing | NOT RUN; source/seed/budget definitions only |
| Benchmarks | NOT RUN; benchmark source only, no performance numbers |
| Ruff, shellcheck, actionlint | BLOCKED where executables absent; syntax checks are not substituted for linters |
| GNOME/KWin/GTK/Qt fixtures | Source/syntax checks only; no real shell/toolkit run |
| Live AT-SPI/portal/Sway/Hyprland/X11 | NOT RUN; not labeled “compiled only” |
| Live Blender | NOT RUN; mocked bpy is not the application |
| Rust Chromium adapter | NOT RUN; independent Python CDP probe is a separate layer |
| Bubblewrap/Landlock Rust plugin host | NOT RUN; no sandbox security guarantee from flag presence |
| CI / aarch64 / Debian / Nix / installation | Definitions only; not built/deployed/evaluated |

The source is not assumed correct merely because it is complete enough to attempt a build.
The first compiler/linter run may find additional defects. `release-readiness.json` stays
false until real evidence closes each gate, and the release script refuses this snapshot.

## Completion and delivery audit

[ACCEPTANCE.md](ACCEPTANCE.md) resolves all 123 original checklist entries with explicit
status/evidence. [Manual testing](docs/manual-testing.md) lists isolated live procedures.
[DEVELOPMENT_HANDOFF.md](DEVELOPMENT_HANDOFF.md) identifies the first real build/fix path
without asking another engineer to redesign the system.

The delivered source ZIP contains one top-level `semwright/` directory, with source,
schemas, recipes, fixtures, docs, verification logs and packaging definitions. It excludes
build targets, caches, credentials, font/browser/toolchain binaries and native test outputs.
The archive CRC/path listing and SHA-256 are checked during packaging and reported in an
external archive-check file and checksum file. Hashing a source ZIP is not binary-release
validation, an SBOM or a publisher signature.
