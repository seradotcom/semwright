# Contributing to Semwright

Semwright is development software. Contributions are welcome through focused pull requests;
no contribution implies release approval. Read [architecture](docs/architecture.md),
[development](docs/development.md), [security](SECURITY.md) and [support](SUPPORT.md).

## Propose and reproduce

Use the public [issue tracker](https://github.com/seradotcom/semwright/issues) for non-sensitive
questions/defects. Include exact source and platform/application versions, expected and actual
results, and a small synthetic fixture. Use the enabled private vulnerability-reporting
channel for security findings. Do not paste credentials, private documents or live profiles.

## Keep changes reviewable

Use your own branch/worktree and preserve other contributors' work. Separate functional fixes
from documentary/evidence-only changes. Do not edit historical FAIL into PASS: append the
fix SHA, new run/job and disposition. Generated command schemas/documentation must remain
consistent with their generating contracts. Never loosen an assertion or required security
boundary just to obtain a green job.

Use the affected-area CI lanes during iteration. Heavy Rust/native-runtime work belongs in
compatible disposable CI, not on a constrained shared device. The full final gates remain
separate. Record source SHA, suite SHA where different, test selection/counts, runtime and
artifact digests. Skips, filtered tests, fixtures and native application runs are not
interchangeable. See [verification](VERIFY.md) and [release blockers](RELEASE_BLOCKERS.md).

## Public documentation and internal coordination

Tracked documentation should be useful to users, contributors or reviewers of the product. Keep
temporary orchestration notes, master prompts, branch choreography, scratch plans and private
handoffs outside the repository (or under an ignored local-only directory). When an internal
investigation produces lasting value, rewrite it as an ADR, design note, test plan or evidence
record before committing it. Preserve technical provenance such as source SHAs, run IDs and failure
receipts; do not publish conversational scaffolding merely because it helped produce them.

## Security-sensitive changes

Describe the authority/configuration impact and rollback/compatibility implications. A new
capability or Skill does not confer permissions; preserve owner grants, approval, stale-ref
checks, output validation and fail-closed unsupported-platform behavior. Security-affecting
changes need a reviewer other than their author. Do not claim an external audit based on
AI-assisted review or CI alone.

## Licenses and history

Keep existing copyright, license and third-party notices. The core is MIT OR Apache-2.0;
`integrations/kicad-driver` is GPL-3.0-or-later with separate notices. Do not silently change
license scope or vendor a new dependency without its provenance. Preserve useful ADRs,
receipts, hashes and failure history. Merging, closing other contributors' PRs and publishing
releases remain maintainer decisions, not side effects of documentation cleanup.
