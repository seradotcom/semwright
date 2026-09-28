# Owner handoff and independent audit lane

The audit's prior green head remains `495df3283f6e591c2db1a2448a7192e41335c580`.
New commits need their own evidence; they do not erase that historical result.
The integration observation for the recorded audit is main `be375a12e8afa4d779f9dc0de501b0d4a262a682`.

| Owner | Required action | Completion evidence |
|---|---|---|
| Windows | Resolve native ARM64 UIA StaleReference after bounded physical-point probing; retain assertion and stale-ref enforcement | Exact fixing SHA; x64 and ARM64 native fixture passes; relevant main checks |
| Windows | Reject zero-selected, ignored-only, malformed or incomplete test output for each interactive harness row and each authority subcommand | Positive expected-test counts and negative harness regressions |
| Windows | Replace generic self-hosted Windows targeting with a dedicated disposable interactive runner and owner approval | Workflow/harness source plus actual authorized evidence; no hosted-to-interactive relabeling |
| Blender/Godot | Complete owner-pinned runtime, sandbox/Driver Host, broker and real Blender acceptance; the 143/143 provenance source defect is already fixed | PR154 exact head and eventual main SHA, nonzero executed tests and no unsandboxed fallback |
| Blender/Godot | Dispose of the nine historical application heads listed in branch-dispositions-v2.json | Missing release-critical delta fixed or source/test-based supersession; retain unrelated untracked evidence |
| Composition/media | Keep new shared kernel, media-time, motion/audio and cumulative-plan features outside this audit freeze | Own C0 contract, own integration tests; no inherited audit certification |

Windows instructions: PR167 comment 5876090544.
Blender/Godot instructions: PR154 comment 5876090829.
Composition/media instructions: PR168 comment 5876417178.

The composition owner should preserve the existing-runtime codec correction from PR156:
main-thread plugin code now uses native Figma base64 and bounded UTF-8 rather than test-only
browser globals. This changes no capabilities or permissions and does not adopt the large
uncommitted typography/controller draft.

Routine dependency version bumps and TIDELING demo expansion are deliberately outside the
freeze unless a documented mandatory security fix is isolated. No other agent's files,
worktrees, index or branch were modified, no worktree was deleted, and no generic self-hosted
interactive workflow was dispatched by the audit. Global readiness remains explicit and separate.
