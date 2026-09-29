# Audio CI triage log

This is diagnostic history, not evidence for later SHAs.

- Run 36501975020 at b4ede769: portable clippy rejected a manual modulo check in signal-analysis test code. Faust native accepted generated source but the fixed helper rejected the decorated libfaust 2.37 version string. Fixed by is_multiple_of and by extracting a strict numeric X.Y.Z while retaining the Rust allowlist.
- Run 36506038165 at 4ed9a71: GitHub created zero jobs because embedded newlines made audio-diagnostics.yml invalid. Fixed with single-line GITHUB_ENV writes and local YAML parse.
- Run 36522362551 at c9b1935 localized three independent issues: portable authoring omitted `StemSendSet` from its replay allowlist; analysis conformance accidentally requested Broker dry-run instead of executing the read-only meter; Faust's sealed helper probe succeeded but `synth.validate` failed inside Driver Host. The first two are fixed in 179c958.
- Run 36525077066 at 179c958 proved `analysis-native` green and direct Faust translation/helper diagnostics green. Faust Host conformance reached and passed the sealed `runtime.probe`, then failed specifically at `synth.validate`. The next candidate replaces the fixture/runtime's flat standard-library staging with an exact bounded recursive relative `.lib` inventory and prevalidates that materialized closure before Driver Host.

- Run 36530198461 at 6e073f7: analysis-native remained green. Portable reached clippy and failed only on a complex tuple type in the expanded Ardour capability table; d1d35df replaced it with a named DeepMutationSpec. Faust failed in a diagnostic control that incorrectly mounted the helper as /plugin/bin instead of reproducing Driver Host driver-plus-sealed-tool topology; 17a1083 removed that invalid control. Ardour direct new_empty_session succeeded outside the sandbox but Broker deep.create still failed; d1d35df added a raw DriverProvider diagnostic before Broker redaction and expanded fixed-Lua sends/groups/allowlisted-plugin coverage.

Rerunning those exact SHAs cannot prove later code changes.
