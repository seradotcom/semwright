# Audio CI triage log

This is diagnostic history, not evidence for later SHAs.

- Run 36501975020 at b4ede769: portable clippy rejected a manual modulo check in signal-analysis test code. Faust native accepted generated source but the fixed helper rejected the decorated libfaust 2.37 version string. Fixed by is_multiple_of and by extracting a strict numeric X.Y.Z while retaining the Rust allowlist.
- Run 36506038165 at 4ed9a71: GitHub created zero jobs because embedded newlines made audio-diagnostics.yml invalid. Fixed with single-line GITHUB_ENV writes and local YAML parse.

Rerunning those exact SHAs cannot prove later code changes.
