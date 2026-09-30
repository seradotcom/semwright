# Research baseline — role C

Inspection date: 2026-09-28. Main frozen at `b736d41b61c4a4146c9e75c16796e251b025e69f`. A source inspected at `7ed5b848e4d2e7af235d6166e6f93e0cf0bac90d`; isolated C0 consumed at `26602e4b25929be869d69ef28fef4dd9713180d7`. Source inspection is not native test evidence.

| Source | Observation and decision |
|---|---|
| https://www.w3.org/TR/prov-dm/ (Recommendation 2013-04-30) | Entities, activities and derivation inform separate logical revisions and receipts. No RDF/PROV conformance claim or graph database requirement. |
| https://bazel.build/basics/hermeticity | Determining inputs include tools/environment and undeclared dependencies break reuse reasoning. Record unknown frontier; no Bazel dependency or hermetic cross-app claim. |
| https://docs.github.com/en/actions/reference/workflows-and-actions/events-that-trigger-workflows | Use branch push for the new diagnostic; do not assume dispatch exists on default. Record source SHA separately from workflow/event identity. |
| https://cli.github.com/manual/gh_run_rerun | A rerun preserves source identity. New fixes require new source commits/runs, not claiming an old rerun tested a new push. |
| `crates/semantic-composition/src/{model,canonical,vault}.rs` at C0 | Reuse A owner/base/resource/evidence and bounded duplicate-rejecting semwright-json-v1; do not fork the kernel or relabel it RFC 8785. |
| `crates/platform-api/src/filesystem.rs`, `crates/platform-common/src/artifact.rs` at baseline | Existing bounded root-relative transfer already owns artifact movement. A digest is content evidence, not logical identity or authority. |
| `crates/platform-services/src/{lib,unix}.rs` at baseline | Reuse trusted private-directory and OS-selected scoped filesystem; no lexical path-prefix confinement claim. |
| `crates/workflow/src/store.rs`, `crates/core/src/{lib,workflows}.rs` at baseline | Workflow distillation and Broker re-entry already exist. Graph persistence is separate; reconstruction proposes typed existing capabilities. |
| `docs/composition/AUDIO_AV_CONTRACT.md` at inspected A SHA | C1 adds media receipts/fixed AV/publication. Its native integration must be consumed explicitly; C does not create a competing publication pointer. |
| rusqlite 0.40.2 / bundled SQLite, remotely resolved in the C lock lane | Chosen for bounded transactional local state and backup API. C still keeps a canonical event journal and rebuildable indexes; SQLite is not used to claim cross-app ACID. |
| `crates/skills` + Agent Skills package shape | Continuity guidance stays agent-side; requirements/examples use the real Skills/Registry tooling and never grant Broker authority. |

Runtime/library pins and native method versions will be recorded per integrated lane. Documentation from latest/current is not evidence that a pinned native runtime implements an API. Product-state claims require executed source-SHA-bound reports, not these sources.
