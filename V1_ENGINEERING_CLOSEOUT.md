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
| RELEASE_READINESS | **BLOCKED_DEVELOPMENT_SOURCE**; see `release-readiness.json` |

R16 closed only after a separate reviewer session inspected the R-authored federation pagination fix,
confirmed that the bounds are applied during pagination, and checked two hosted sandboxed federation
runs with 7/7 passing tests. The evidence is
`verification/r16-closeout/evidence/INDEPENDENT_R16_REVALIDATION_2026-10-03.json`.

R06 and R18 remain open because the remaining rows require physical or interactive environments that
are not available in this mission. They are not marked PASS, CLOSED, simulated, or inferred from
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

`release-readiness.json` is intentionally unchanged. Its broader certification gates still fail closed
while required live/security gates are false. `V1_ENGINEERING_CLOSEOUT = COMPLETE` records that the
known software work in the declared v1 engineering scope is closed; it is not release authorization
and it does not convert R06/R18 into PASS or CLOSED.

The hosted multiplatform distribution workflow remains a separate reproducibility/package-integrity
check. Any packaging defect exposed there is a software defect and must be fixed rather than waived as
an R06/R18 environment limitation. Passing hosted package jobs still does not certify physical
Hyprland, unlocked Windows interaction, TCC, signing/notarization, or the broader release gates.

Machine-readable companion: `verification/v1-engineering-closeout.json`.
