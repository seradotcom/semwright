# Owner handoff after audit merge

The maintainer audit is merged in `main` at `6dc9da507a2fc239a766a6a81a7607fbcc79618d`.
The audit lane is finished; remaining actions belong to their named owners.

| Owner | Required action | Completion evidence |
|---|---|---|
| Windows | Resolve native ARM64 UIA StaleReference while retaining fail-closed stale-ref/focus semantics | Exact fixing SHA; x64 + ARM64 native fixture passes; relevant main checks |
| Windows | Reject zero-selected, ignored-only, malformed or incomplete interactive test output | Positive expected-test counts plus negative harness regressions |
| Windows | Use a dedicated disposable interactive runner with explicit owner authorization | Workflow/harness source and actual authorized evidence |
| Blender/Godot | Complete owner-pinned runtime, sandbox/Driver Host, broker and real Blender acceptance | Exact owner SHA and eventual main SHA; nonzero executed tests; no unsandboxed fallback |
| Blender/Godot | Dispose of the nine historical application heads in `branch-dispositions-v2.json` | Source/test-based supersession or integration; preserve unrelated evidence |
| Composition/media | Own its new kernel/media/motion scope independently | Own contracts and integration evidence; no inherited audit certification |

Historical coordination comments remain useful references: Windows `5876090544`, Blender/Godot `5876090829`,
composition/media `5876417178`.

PR #155 and PR #167 are now merged. PR #167 closed documentation drift only; it did not itself satisfy
the outstanding native/interactive Windows evidence. TIDELING is now part of `main`, so a future candidate
includes it, while application-specific validation remains with the owning lane.

The Figma codec correction from PR #156 is merged: plugin runtime code uses Figma-native base64 operations
and bounded UTF-8 byte handling rather than test-only browser globals. It changes no capability or permission surface.

No other agent's worktree is to be edited or deleted by the audit lane. R16 remains a separate independent review
after an exact main candidate is admitted.
