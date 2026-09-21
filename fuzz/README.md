# Bounded parser fuzzing

These targets are source-only in this handoff. They do not exercise a real desktop.
The protocol target fuzzes JSON messages; Rust protocol unit tests exercise length
framing. Full transport-state fuzzing is a separate release task, not claimed here.

After the workspace compiles and a nightly toolchain plus cargo-fuzz is installed:

```sh
cargo +nightly fuzz run protocol -- -max_total_time=15 -rss_limit_mb=512 -max_len=1048576
cargo +nightly fuzz run plugin -- -max_total_time=15 -rss_limit_mb=512 -max_len=65536
cargo +nightly fuzz run selector -- -max_total_time=15 -rss_limit_mb=512 -max_len=65536
cargo +nightly fuzz run recipe -- -max_total_time=15 -rss_limit_mb=512 -max_len=262144
cargo +nightly fuzz run path -- -max_total_time=15 -rss_limit_mb=512 -max_len=4096
```

Run from the repository root. Keep failures as minimal regression fixtures; do not
label these commands PASS until their actual exit statuses have been recorded.
The short budgets are intentional. No unbounded background fuzzing is started.
