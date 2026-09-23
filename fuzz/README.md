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
```

The video-domain targets exercise backend-neutral model decoding/round-trip invariants and
bounded semantic edit sequences. They do not invoke MLT, a GUI, media decoders or native
render processes; backend-specific fuzzing remains the responsibility of each video driver.

Run from the repository root. Keep failures as minimal regression fixtures; do not
label these commands PASS until their actual exit statuses have been recorded.
The short budgets are intentional. No unbounded background fuzzing is started.
