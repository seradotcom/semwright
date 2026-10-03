# Staging, publication and post-v1 certification

Current policy decision: **2026-10-03, Option B**, authorized by the maintainer for the
v1 staging closeout. This supersedes the physical-matrix admission requirement in older
release ledgers; it does not rewrite their observations or turn an unexecuted test into PASS.

## Three different decisions

| Decision | Requirements | What it authorizes |
| --- | --- | --- |
| Engineering staging | Nine engineering gates, exact gate/schema validation, pinned toolchain and parseable lockfile | Building and testing candidate packages; not publication |
| Public release | Staging, genuine independent security review, explicit maintainer approval and final exact-SHA six-platform installation validation | A separately authorized upload to an existing tagged GitHub Release |
| Physical/interactive certification | Actual execution and review in each required environment | Only the support claims exercised in that environment |

R16 is **CLOSED** under its recorded separate revalidation. It is not an external audit or a
substitute for the broader independent `security_review` gate. That gate is currently **false**;
public release remains **BLOCKED_PENDING_SECURITY_REVIEW**.

The remaining R06 physical Hyprland/mixed-display and R18 unlocked-Windows interactive cases are
**OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT**. They do **not** block preparing or publishing
the initial v1 once the actual public release requirements are met. Their procedures, unexecuted
states and withheld claims remain in [POST_V1_BACKLOG.md](../POST_V1_BACKLOG.md).
The unsupported Windows external-MCP mount-virtualization boundary remains fail-closed; it is not
waived by this policy. A software failure found during future physical testing reopens engineering work.

## Executable contract

`release-readiness.json` schema 2 keeps the engineering/security booleans in `gates` and the two
explicit deferred records in `post_v1_certification`. The required names are independently fixed
in the validator, not derived from a submitted JSON document. Missing, unknown, duplicate,
nonboolean, inconsistent or malformed values fail closed. A deferred record is never a boolean PASS.

```sh
python3 scripts/release/assert-ready.py --mode staging
python3 scripts/release/assert-ready.py --mode publish
```

Staging prints **NOT PUBLIC RELEASE AUTHORIZATION**. The second command intentionally fails on
the current checkout. Publication also requires an exact candidate SHA, a separately supplied
review record, a complete distribution manifest and explicit authorization. Neither a status
string nor a boolean in the repository authenticates a reviewer.

The public review record is described in [security-review.md](security-review.md). A maintainer
must verify the report, reviewer independence, scope, findings/remediation and artifact provenance.
The JSON validator only checks required fields and SHA binding; generating a syntactically valid
record is not performing or approving a security review.

## Candidate workflow

Dispatch `.github/workflows/v1-distribution.yml` on the intended branch. Record the workflow's
`headSha`, the checked-out source SHA and run ID. A pull-request run normally validates GitHub's
merge candidate, which must not be relabeled as the branch head. Final main is checked separately.

Every native job builds the five command binaries and checks architecture, deterministic package
assembly, every internal checksum, installation of the **extracted** bundle into a disposable
user location, installed execution, removal and cleanup. Tests also preserve changed/unowned files
and reject an existing destination. Linux additionally installs/removes the actual `.deb` through
the package manager on disposable GitHub-hosted runners. Fake-doctor checks are synthetic broker
smokes, not physical desktop acceptance.

The global manifest requires six unique exact-SHA native certificates and verifies the actual
hashes of all eight packages against those certificates. It includes `release_admission=false`,
`verified_platforms` and `install_uninstall_verified`. Reproducibility here means repeated package
assembly from the same input binaries; it does not claim bit-identical independent Rust builds.

Artifacts remain in Actions with 30-day retention. **This repository is public: Actions artifacts
are not confidential/private storage.** No credentials or private project data may enter a bundle.
This mission creates no tag, GitHub Release or release assets, and changes no repository visibility.

## Later publication, not part of this closeout

Finish the user's demo/polish work, select a final source SHA and complete all required engineering
and independent review gates for that SHA. Record the review decision without claiming that older
CI or an earlier review covers later code. The containing SHA of an evidence document is not
necessarily the SHA it certifies.

Only the maintainer can authorize the final tag/release operation. In `release.yml`, tag pushes
can build candidates but **never** invoke the publication job. Publication requires an explicit
`workflow_dispatch` on a `v*` tag with `publish=true`, the matching `candidate_sha`, and a successful
same-repository `review_run_id` carrying an `independent-security-review` artifact with the genuine
`security-review.json` handoff. The workflow rechecks the review binding, native certificates and
package hashes before upload, and requires that the GitHub Release already exists.

Review handoff/provenance must be independently checked by the maintainer; an arbitrary successful
Actions run is not a trusted reviewer. The workflow does not create that report, tag or release and
cannot turn pending security review into approval. The public guard's default mode is publication,
so direct invocation without the required records also fails closed.

Historical sources: [engineering closeout](../V1_ENGINEERING_CLOSEOUT.md),
[verification ledger](../VERIFY.md), [historical blocker ledger](../RELEASE_BLOCKERS.md),
[R16 evidence](../verification/r16-closeout/README.md).

## Historical R16 backup preservation

The R16 documentary workflow checks the immutable PR #208 backup with
`package_r16_closeout.py --historical-check`: all three delivery-file hashes and the embedded
per-file manifest must match the recorded delivery. The older `--check` mode still tests exact
reassembly from its matching historical tree; running it after current README/policy changes
would correctly report a different input tree. This policy update does not regenerate or edit
any R16 delivery/evidence files.
