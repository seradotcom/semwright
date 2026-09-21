# Testing, Release and Quality Strategy

## Quality philosophy

The repository should be releasable even if the coding environment cannot attach to a real graphical Linux desktop.

That means maximizing deterministic tests and clearly isolating the final manual desktop validation.

## Test pyramid

### Unit tests

Required for:
- selectors;
- refs and stale detection;
- policy;
- capability resolution;
- recipe parsing/evaluation;
- path scoping;
- redaction;
- command schema validation;
- environment parsing;
- error mapping.

### Property tests

Use `proptest` or equivalent for:
- selector invariants;
- path scoping;
- protocol encode/decode;
- recipe variable resolution;
- reference generation;
- policy monotonicity where applicable.

### Golden tests

Stable fixtures for:
- normalized AT-SPI tree snapshots;
- compact LLM text snapshots;
- JSON command schemas;
- MCP tool schemas;
- doctor output;
- audit redaction.

### Fake backend integration tests

A deterministic fake desktop should simulate:
- apps/windows;
- accessibility tree;
- focus changes;
- disappearing nodes;
- duplicate labels;
- backend failure;
- stale refs;
- consent requirements.

Run full recipes against it.

### D-Bus test session

Where possible use a private/session bus for backend protocol tests.

### X11 headless

Use Xvfb plus a small accessible test app where viable.

### Wayland headless

Attempt a nested/headless compositor test harness where feasible (for example Weston headless or another practical CI-compatible compositor).

If portal backend behavior cannot be meaningfully tested in CI, mock the portal contract and clearly label live tests.

### Desktop bridge tests

GNOME extension/KWin bridge:
- lint;
- protocol schema;
- mock transport;
- install/uninstall validation where possible.

### Adapter tests

Blender:
- unit/protocol tests without Blender;
- if Blender can be installed headlessly in CI, run a subset using background mode;
- do not mark GUI actions verified if only background API was tested.

Chromium:
- launch isolated headless Chromium if available;
- run CDP tests;
- test refusal to attach to unsafe profile configuration by default.

## Fuzzing

Targets:
- broker protocol parser;
- plugin handshake;
- selector parser;
- recipe parser;
- path policy normalization.

Keep seed corpora in repo.

Run short fuzz smoke in CI and document longer local fuzz commands.

## Static quality gates

At minimum:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
cargo audit
cargo deny check
```

Also:
- no undocumented unsafe blocks;
- minimize `unsafe`;
- `cargo machete`/unused dependency check if appropriate;
- shellcheck for shell scripts;
- actionlint for GitHub Actions;
- JS lint for GNOME/KWin code;
- Python lint/format for Blender add-on.

## Coverage

Coverage target:
- core pure crates: >= 90% line coverage where practical;
- workspace aggregate target should be documented, not gamed by excluding hard code;
- OS adapters may have lower automated coverage but require contract tests.

Coverage does not replace behavioral tests.

## CI matrix

At least:
- stable Rust;
- MSRV if declared;
- x86_64 Linux;
- aarch64 compile/cross-build where practical;
- Ubuntu/Debian-ish;
- Fedora-ish compatibility job if manageable;
- feature combinations;
- release build.

Do not require every DE in hosted CI if technically unrealistic. Keep a manual matrix.

## Release artifacts

Target artifacts:
- x86_64 GNU/Linux tarball;
- aarch64 GNU/Linux tarball;
- checksums;
- `.deb` if packaging is reliable;
- `.rpm` if reliable;
- Nix flake/package;
- `cargo install` support where system dependencies permit.

Optional later:
- AUR recipe;
- Homebrew/Linuxbrew tap.

## Install experience

A polished project needs:

```bash
computerctl doctor
```

immediately after install.

Installer must:
- detect missing AT-SPI/portal requirements;
- never silently `sudo`;
- explain optional helper setup;
- verify downloaded release checksums.

## User daemon

Ship:
- systemd user unit;
- `computerctl service install/start/stop/status`;
- graceful shutdown;
- socket cleanup;
- restart policy that does not loop on bad config.

## Release automation

GitHub Actions should:
1. run all gates;
2. build artifacts;
3. generate checksums;
4. produce SBOM if practical;
5. publish GitHub Release on version tag;
6. attach changelog excerpt;
7. optionally sign artifacts/provenance.

## Documentation tests

Examples in docs should be checked where practical.

`computerctl --help` examples must stay synchronized.

## Manual acceptance matrix

Create `docs/manual-testing.md` with exact scripts for:

### GNOME/Wayland
- doctor;
- window list/focus;
- AT-SPI snapshot;
- invoke button;
- type;
- portal consent;
- screenshot;
- GNOME bridge if installed.

### KDE/Wayland
Equivalent, including KWin bridge.

### Sway/Hyprland
Window adapter + AT-SPI + input path.

### X11
Semantic tree + EWMH + input.

Record:
- distro/version;
- desktop/compositor version;
- session type;
- pass/fail;
- notes;
- date.

## Definition of verified

Use exact language:
- `unit-tested`
- `CI integration-tested`
- `headless-tested`
- `manually verified on <env>`
- `implemented, live verification pending`

Never say “supports GNOME/KDE” solely because code compiles.

## Performance benchmark suite

Benchmarks:
- selector exact lookup;
- selector ranked lookup;
- snapshot compaction;
- registry discovery;
- protocol serialization;
- policy decision;
- fake end-to-end invoke.

Optional live benchmark tool records actual backend latency.

## Regression fixtures

Every discovered real-world bug should gain:
- minimal fixture;
- regression test;
- changelog entry if user-visible.

## Final ZIP criteria

The new chat must deliver a ZIP that includes:
- source;
- lockfile;
- tests;
- docs;
- CI;
- packaging;
- license;
- no build cache/target directory;
- no secrets;
- no fake screenshots;
- `VERIFY.md` with exact commands run and outputs summarized.

The ZIP should be created only after the final verification pass.
