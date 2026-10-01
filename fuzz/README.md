# Bounded parser fuzzing

These are bounded fuzz harnesses for pure parsers, semantic models and protocol logic.
They do not exercise a real desktop. The security workflow runs the core protocol/path/plugin/
recipe targets plus the backend-neutral video-domain targets with a fixed nightly toolchain and
bounded budgets. Integration-specific workflows may run additional driver fuzzers.

The protocol target fuzzes JSON messages; Rust protocol unit tests exercise length framing.
Full transport-state fuzzing is a separate concern.

With the pinned nightly toolchain plus cargo-fuzz installed:

```sh
cargo +nightly fuzz run protocol -- -max_total_time=15 -rss_limit_mb=512 -max_len=1048576
cargo +nightly fuzz run plugin -- -max_total_time=15 -rss_limit_mb=512 -max_len=65536
cargo +nightly fuzz run selector -- -max_total_time=15 -rss_limit_mb=512 -max_len=65536
cargo +nightly fuzz run recipe -- -max_total_time=15 -rss_limit_mb=512 -max_len=262144
cargo +nightly fuzz run path -- -max_total_time=15 -rss_limit_mb=512 -max_len=4096
cargo +nightly fuzz run video_domain_model -- -max_total_time=15 -rss_limit_mb=512 -max_len=8192
cargo +nightly fuzz run video_domain_edit -- -max_total_time=15 -rss_limit_mb=512 -max_len=8192
cargo +nightly fuzz run video_domain_contract -- -max_total_time=15 -rss_limit_mb=512 -max_len=8192
cargo +nightly fuzz run video_domain_render -- -max_total_time=15 -rss_limit_mb=512 -max_len=8192
```

The video-domain targets exercise backend-neutral model decoding/round-trip invariants,
bounded semantic edit sequences, and render-intent/preset validation. They do not invoke MLT,
a GUI, media decoders or native
render processes; backend-specific fuzzing remains the responsibility of each video driver.

Run from the repository root. Keep failures as minimal regression fixtures; do not
label these commands PASS until their actual exit statuses have been recorded.
The short budgets are intentional. No unbounded background fuzzing is started.

## Agent Skills targets

- `skill_frontmatter`: bounded SKILL.md/frontmatter parser, including YAML/UTF-8 edge cases.
- `skill_requirements`: v1 requirements JSON + schema/typed validation.
- `skill_archive_path`: bundle/resource relative-path sanitizer.

The hosted bounded-fuzz job runs these with the existing target set. Semwright Skill fuzzing never executes `scripts/`.

Audio semantic targets:
- audio_domain_model: strict JSON/model validation and semantic digest stability.
- audio_domain_edit: successful arbitrary semantic edits preserve invariants and source immutability.
- audio_wav: hostile bounded RIFF/WAVE envelopes, seeks, decode and analysis stay fail-closed.
