# Branch disposition closeout

Observed integration main: `be375a12e8afa4d779f9dc0de501b0d4a262a682`. Original inventory retained unchanged.

This is a release-scope disposition, not whole-tree equality and not cleanup permission.
The original 63 heads now have explicit dispositions. Nine application heads still require the named Blender/Godot owner; they are not relabeled as superseded.

| SHA | Family | Classification | Disposition |
|---|---|---|---|
| `e6798c63ceb1` | motion | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `d6ae8856ed9f` | motion | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `776b0765a078` | obs | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `eb9e0a801d91` | atspi | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `3f087430aa00` | windows-network | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `8dbe45ef881b` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `b28a2bac5cf4` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `117afd2b9fdd` | prior-exact-comparison | C | CHANGED_PATHS_IDENTICAL_TO_MAIN |
| `b447cfadc6e1` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `0b6747384490` | applications | F | DELEGATED_APPLICATION_OWNER_REVIEW |
| `0d551e9a1b5b` | docs | C | SUPERSEDED_DOCUMENTATION |
| `6a4883dae2a0` | eis | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `bd6f9f4ce626` | applications | F | DELEGATED_APPLICATION_OWNER_REVIEW |
| `0a41b6fb7b03` | applications | F | DELEGATED_APPLICATION_OWNER_REVIEW |
| `6c067e90615f` | macos | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `9d2d0c1bde36` | docs | C | SUPERSEDED_DOCUMENTATION |
| `e682f52e4727` | atspi | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `d46b241b97be` | applications | F | DELEGATED_APPLICATION_OWNER_REVIEW |
| `d1d7fda2c7db` | browser | C | EXACT_DOMAIN_SCHEMA_AND_SOURCE_SUPERSESSION |
| `8e0928605407` | browser | C | EXACT_DOMAIN_SCHEMA_AND_SOURCE_SUPERSESSION |
| `5b6946251e71` | browser | C | EXACT_DOMAIN_SCHEMA_AND_SOURCE_SUPERSESSION |
| `8772956a8157` | continuity | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `4a9641a81596` | figma-authoring | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `db46fd22b574` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `10b88d81b24b` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `0c21c2a9bb99` | skills | C | SUPERSEDED_DOCUMENTATION |
| `ca23b3158380` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `95dd7f9a92d5` | applications | F | DELEGATED_APPLICATION_OWNER_REVIEW |
| `9e8b5813c2d3` | applications | F | DELEGATED_APPLICATION_OWNER_REVIEW |
| `ebbeb7b2dbc1` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `e23c16752432` | applications | F | DELEGATED_APPLICATION_OWNER_REVIEW |
| `f56e57e08d05` | applications | F | DELEGATED_APPLICATION_OWNER_REVIEW |
| `3a7bd313aa85` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `3046a9a3e8ca` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `7986a5f84109` | macos | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `f90ca8cab72e` | macos | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `c4687cb493d1` | eis | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `080f6444f419` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `8c2dddb2fcc6` | semantic-ui | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `4bcac7927029` | atspi | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `98246dbc66a2` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `964b14db51c8` | semantic-ui | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `5edf45e5d0d6` | semantic-ui | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `d53913c06083` | semantic-ui | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `af1a408c41ae` | semantic-ui | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `fcab62931db1` | semantic-ui | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `fa3144948651` | demo | E | DEFERRED_POST_RELEASE |
| `346df02e8c9f` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `5808f5e7cd78` | video | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `c7ca999dea53` | windows-harness | A | INTEGRATED_WITH_OWNER_FOLLOWUPS |
| `99287e7a72d7` | semantic-ui | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `130442f33eec` | atspi | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `bdbaafa10771` | blender-pagination | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `a5b49cc87848` | applications | F | DELEGATED_APPLICATION_OWNER_REVIEW |
| `ff4ccb22d943` | figma-continuity | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `1c85875cdfcc` | motion | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `4190f4016e58` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `1eccc765fd2b` | prior-exact-comparison | C | PATCH_EQUIVALENT_TO_MAIN |
| `32e186ebffec` | hyprland-experiment | E | RETIRED_TEST_STRATEGY_KEEP_EVIDENCE |
| `8ec23daf106d` | macos | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `2424c389eaaf` | docs | C | SUPERSEDED_DOCUMENTATION |
| `d1ef157d46bf` | motion | C | SUPERSEDED_RELEASE_BEHAVIOR |
| `d91f9498b404` | hyprland-experiment | E | RETIRED_TEST_STRATEGY_KEEP_EVIDENCE |

## Security-relevant dirty worktrees

| Preserved work | Disposition and concrete comparison |
|---|---|
| file-grant-driver-host | Centralized daemon directory-grant filtering and regression replace constructor-level filtering; driver secret/tool file grants stay separate. |
| sealed-tool-budget | Main has the distinct 64 MiB provider / 256 MiB sealed-tool verifier and boundary tests. |
| mcp-sandbox-v1 | Main implements mandatory platform sandbox, bounded resource grants, data-only MCP mounts, explicit network gates and hostile fixtures. |
| workflow-distillation-v1 | Compiler/persistence checks are in current code. The empty/truncated store draft is not a missing fix. PRE-010 separately corrects unfinished recording retention. |
| release-packaging | Full diff shows the draft has truncated Python and older permissive packaging; current source adds normalized modes, strict tar/deb checks, reproducibility and tamper-safe uninstall. |
| motion-canvas-driver-finalize | Keep abandoned broad address-space/debug experiments; current documented 4 GiB contract and protocol-v3 lifecycle supersede them. |
| figma-semantic-final-proof-chatgpt | Uncommitted typography and cumulative plan/validation-ticket machinery are post-release feature work. Current per-call budget/authorization is not a controller-wide cumulative guarantee. Preserve the draft. |
| figma-semantic-completeness | Do not import the experimental custom base64 decoder. PRE-011 instead uses the official Figma codecs and removes browser-codec mock injection; retain the historical draft. |
| Blender/Godot and mixed Windows worktrees | Assigned to their active owners. No source/worktree/index modified; any remaining critical delta must be reported before global freeze. |
| Audio-domain drafts, TIDELING, caches and untracked live evidence | New domains/demo deferred; caches are not source claims; evidence retained under its actual historical SHA. No deletion authorized. |

A broad comparison command was blocked by the tool and did not execute. Focused subsequent source reads/diffs established the comparisons above; any unreviewed remainder is retained, not inferred absent.

## Closure boundary

Audit-owned branch accounting is complete; application-owner acceptance is not. The global candidate remains unset until their required fixes and the final exact-main gates are resolved. R16 remains OPEN.
