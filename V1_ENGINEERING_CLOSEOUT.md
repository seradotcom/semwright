# V1 engineering closeout

Date: 2026-10-03

This document records engineering completion separately from full platform/release certification.

## Final engineering state

| Gate | State |
| --- | --- |
| R16 | **CLOSED** |
| R06 | **OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT** |
| R18 | **OPEN — DEFERRED_TO_POST_V1_ENVIRONMENT_DEPENDENT** |
| V1_ENGINEERING_CLOSEOUT | **COMPLETE** |
| RELEASE_READINESS | **BLOCKED_PENDING_SECURITY_REVIEW**; see `release-readiness.json` |

R16 closed only after a separate revalidation pass inspected the bounded federation pagination fix,
confirmed that the bounds are applied during pagination, and checked two hosted sandboxed federation
runs with 7/7 passing tests. The evidence is
`verification/r16-closeout/evidence/SEPARATE_REVALIDATION_2026-10-03.json`.

R06 and R18 remain open because the remaining rows require physical or interactive environments that
are not available in hosted CI. They are not marked PASS, CLOSED, simulated, or inferred from
hosted CI. Exact post-v1 procedures are in `POST_V1_BACKLOG.md`.

## Why R06/R18 do not prevent engineering closeout

The residual R06/R18 rows are evidence gaps for physical/interactively exercised support claims, not
known product defects being hidden as hardware limitations. Existing implementation and hosted/native
evidence remain scoped to what they actually executed. Any future physical run that exposes a software
defect reopens engineering work; it must not be converted into a certification PASS.

Windows external MCP filesystem mounts are a separate explicit non-claim:
`BLOCKED_PORTABLE_PATH_VIRTUALIZATION`. That unsupported surface remains fail-closed and is not
reclassified as an R18 environment-dependent PASS.

## Release-readiness remains separate

The later maintainer-authorized [staging policy](docs/release-policy.md) separates engineering
package admission from publication. R06/R18 residuals are post-v1 certification, not initial-v1
release prerequisites; their OPEN/deferred evidence is unchanged. The independent security review
remains pending and mandatory before publication, together with explicit maintainer authorization
and final exact-SHA validation. Engineering completion is not release authorization.

The hosted multiplatform distribution workflow remains a separate reproducibility/package-integrity
check. Any packaging defect exposed there is a software defect and must be fixed rather than waived as
an R06/R18 environment limitation. Passing hosted package jobs still does not certify physical
Hyprland, unlocked Windows interaction, TCC, signing/notarization, or the broader release gates.

Historical distribution revalidation is recorded in
`verification/v1-engineering-closeout-revalidation.json`: PR head `f6d7b4d13834a633e54e61fb2038eb8b5989735f`
was tested as GitHub PR merge candidate `90acc6f8c1baaac0e9ed273c758084f7bc540ae3` in run `37109958115`;
all six native platform jobs and the global manifest passed, yielding eight packages. The manifest
retains `release_admission=false`; this is package/integration evidence, not release authorization.

After PR #207 merged, the exact main SHA `9954c1f95f68305f32f153fe5ab302441845b7ed` was
revalidated again. R16 run `37146328471` passed both `documentary` and `positive-smoke`; V1
distribution run `37146331051` passed all six native platform package jobs plus the distribution
manifest. This post-merge evidence strengthens the exact-main record but does not close R06/R18,
change `release-readiness.json`, or authorize release publication.

Historical machine-readable companion (records its original policy/source, not the subsequent staging-policy change): `verification/v1-engineering-closeout.json`.
