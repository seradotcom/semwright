# Branch and worktree inventory

Comparison baseline: `241000c268d1bf1dc29d4e91a913097ac0d020cb`. All counts are snapshot counts, not a claim that parallel agents stopped.

- 333 refs; 212 distinct recorded heads; 85 refs ahead of initial main.
- 188 worktrees; 46 dirty; 14 detached. Every worktree retained.
- 63 distinct unmerged heads compared with merge-base, patch equivalence and changed-path trees.
- 15 heads resolved as patch-equivalent or changed paths identical to main; 48 remain behaviorally unresolved.

Patch equivalence excludes merge commits and does not prove absence of valuable verification files.
A differing blob may merely include later main evolution; it is not automatically a missing fix.
Process matching used an instantaneous /proc cwd/argument snapshot and does not prove inactivity.
No worktree satisfied every cleanup proof in this audit, so none was deleted, reset or cleaned.

## Priority unresolved work

Security-relevant dirty work includes `mcp-sandbox-v1`, `file-grant-driver-host`,
`sealed-tool-budget`, `release-closeout-v3`, `workflow-distillation-v1` and shared SDK/Host changes
in `godot-runtime-completeness`. The semantic-UI integration worktree contains unresolved merge
entries. Treat these as preserved evidence requiring comparison, not a reason to cherry-pick blindly.
Old AppContainer/AT-SPI backup variants, Motion Canvas diagnostic branches, browser variants and
platformization branches are grouped by SHA in `inventory/branch-tree-comparison.json`.

## Dirty worktrees

| Path (owner prefix normalized) | Branch | Changes | Process snapshot | Disposition |
|---|---|---|---|---|
| `$HOME/Documents/Projects/semwright` | `refs/heads/feat/blender-deep-driver` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-chromium-hardening` | `refs/heads/feat/chromium-hardening` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-obs-sol` | `refs/heads/feat/obs-driver-integration-sol` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-pipewire-runtime` | `refs/heads/feat/pipewire-screencast-runtime` | 5 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/audio-domain-faust-ardour` | `refs/heads/feat/audio-domain-faust-ardour` | 4 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/audio-domain-faust-ardour-chatgpt` | `refs/heads/feat/audio-domain-faust-ardour-chatgpt` | 5 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/audio-domain-professional` | `refs/heads/feat/audio-domain-professional` | 14 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/blender-semantic-closeout-final` | `refs/heads/feat/blender-semantic-release-candidate` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/blender-semantic-completeness` | `refs/heads/feat/blender-semantic-completeness` | 3 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/blender-semantic-docclose` | `refs/heads/docs/blender-semantic-closeout` | 7 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/driver-tool-mount-chatgpt` | `refs/heads/feat/driver-tool-mount-chatgpt` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/figma-pairing-retry-state` | `refs/heads/fix/figma-pairing-retry-state` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/figma-semantic-authoring` | `refs/heads/feat/figma-semantic-authoring` | 8 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/figma-semantic-completeness` | `refs/heads/feat/figma-semantic-completeness-closeout` | 3 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/figma-semantic-final-proof-chatgpt` | `refs/heads/feat/figma-semantic-authoring-hardening-chatgpt` | 8 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/file-grant-driver-host` | `refs/heads/fix/file-grants-driver-host` | 2 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/godot-driver-closeout-chatgpt` | `refs/heads/feat/godot-driver-closeout-chatgpt` | 4 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/godot-driver-integration` | `refs/heads/feat/godot-driver-integration` | 3 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/godot-final-closeout-docs-chatgpt` | `refs/heads/docs/godot-final-closeout-chatgpt` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/godot-generic-semantic-substrate` | `refs/heads/feat/godot-generic-semantic-substrate` | 3 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/godot-runtime-closeout-docs-chatgpt` | `refs/heads/docs/godot-runtime-closeout-chatgpt` | 3 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/godot-runtime-completeness` | `refs/heads/feat/godot-runtime-completeness` | 17 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/godot-semantic-completeness` | `refs/heads/feat/godot-semantic-completeness` | 2 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/godot-semantic-completeness-chatgpt` | `refs/heads/feat/godot-semantic-completeness-chatgpt` | 14 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/godot-semantic-substrate-final` | `refs/heads/feat/godot-semantic-substrate-final` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/jobs-progress-v2` | `refs/heads/feat/job-progress-artifacts` | 3 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/live-hyprland` | `refs/heads/test/live-hyprland-headless` | 3 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/live-portal-eis` | `refs/heads/test/live-portal-eis` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/live-portal-eis-closeout` | `refs/heads/test/live-portal-eis-closeout` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/mcp-sandbox-v1` | `refs/heads/feat/mcp-upstream-sandbox` | 13 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/motion-canvas-driver-finalize` | `DETACHED` | 12 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/platform-windows` | `refs/heads/feat/platform-windows` | 9 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/pre-r16-forensic-audit` | `refs/heads/audit/pre-r16-forensic` | 4 status entries | 2 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/release-closeout-v3` | `refs/heads/docs/release-closeout-v3` | 4 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/release-packaging` | `refs/heads/feat/release-packaging-certification` | 3 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/sealed-tool-budget` | `refs/heads/fix/sealed-tool-budget` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/semantic-ui-domain-v2-sol` | `refs/heads/feat/semantic-ui-domain-v2-sol` | 2 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/semantic-ui-v2` | `refs/heads/feat/semantic-ui-v2-windows-closeout` | 15 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/semantic-ui-v2-integration` | `refs/heads/feat/semantic-ui-v2-integration` | 84 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/semantic-ui-v2-platform-parity` | `refs/heads/assist/semantic-ui-v2-platform-parity` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/semantic-ui-v2-rich-chatgpt` | `refs/heads/feat/semantic-ui-v2-rich-chatgpt` | 4 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/tideling` | `refs/heads/feat/tideling` | 10 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/windows-capture-picker-artifacts` | `refs/heads/feat/windows-capture-picker-artifacts` | 2 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/windows-secure-spawn` | `refs/heads/feat/windows-secure-spawn` | 1 status entries | 0 observed process matches | KEEP |
| `$HOME/Documents/Projects/semwright-worktrees/workflow-distillation-v1` | `refs/heads/feat/workflow-distillation-v1` | 3 status entries | 0 observed process matches | KEEP |
| `/tmp/semwright-r02-vm-main` | `DETACHED` | 1 status entries | 0 observed process matches | KEEP |

## Unmerged groups

Full changed-path lists, unmatched commit identifiers and comparison methods are in the JSON.

| Head | Ref count | Ahead | Unmatched nonmerge patches | Disposition |
|---|---:|---:|---:|---|
| `e6798c63ceb1cedcf2f605e0f7caa97bd6bef347` | 1 | 21 | 15 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `d6ae8856ed9f7a3a866efc748e4418bd9cbf3fb3` | 1 | 27 | 17 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `776b0765a07887a39c1efa96d11699e6d64df1bf` | 1 | 5 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `eb9e0a801d9155d8ba3bd7d8bfca864e32cc4ab1` | 1 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `3f087430aa006889a9206a5121efa654f32118af` | 1 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `8dbe45ef881b10a1b0037cee78abd7f77b79c59f` | 1 | 1 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `b28a2bac5cf46018a3f0803434c12569865cd7f1` | 1 | 1 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `117afd2b9fdd01d981369f565108b4e341d3e26c` | 1 | 3 | 3 | CHANGED_PATHS_IDENTICAL_TO_MAIN |
| `b447cfadc6e115bd68e17daccf4fac7b4a0d5e17` | 1 | 1 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `0b6747384490c4ef3899c3fd69f4ab6c1a05dd80` | 2 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `0d551e9a1b5ba5535dfffdb7871304e739013852` | 1 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `6a4883dae2a04538203857e22aedc0e45e91b00e` | 2 | 3 | 2 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `bd6f9f4ce62648e5180b77f0118009ce2cdfd354` | 1 | 2 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `0a41b6fb7b039fd30aa4611dbecbfdeb7770c9df` | 1 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `6c067e90615ffb1f10acf7f1ef1d9598a47fc5b2` | 1 | 2 | 2 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `9d2d0c1bde367a82e8b39c0f19b3a85f0b8819a3` | 1 | 2 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `e682f52e4727d8fad17b29f8db454754cc8e5ac6` | 2 | 49 | 46 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `d46b241b97beb14093da71582c5106e6d616271c` | 2 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `d1d7fda2c7dbef2e7e9a9daaeef0535976f2b46c` | 2 | 29 | 26 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `8e0928605407b84088da2700f93e6dc4d93cd7c1` | 1 | 28 | 26 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `5b6946251e71936cbdda17de3387d02e8a6a97f3` | 1 | 27 | 26 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `8772956a81573e80441b96e78d5782038fee90e8` | 1 | 15 | 13 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `4a9641a815969dd869270fe9777375052a95337f` | 1 | 3 | 3 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `db46fd22b574ba91429daad7aac240adab64a929` | 1 | 1 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `10b88d81b24bcd9ded28873cc45ac7dedb81213b` | 2 | 1 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `0c21c2a9bb99a37dd5802dcccb88640b40909cfa` | 1 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `ca23b31583801af8c59a7cb1e343c41233cf06fb` | 2 | 2 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `95dd7f9a92d5f91270ff37bd8c1a5bed19dafec8` | 1 | 7 | 4 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `9e8b5813c2d3652b022b0300aca5b464c93f4616` | 2 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `ebbeb7b2dbc14d8f5f450954fd13966474bb02f5` | 1 | 1 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `e23c1675243256fb008aa541786466d7c9fdf3d1` | 1 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `f56e57e08d05f2e616a6111c8e58c4ce92757e6b` | 2 | 3 | 3 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `3a7bd313aa85e7a65ae5127a73ad73069ce80376` | 1 | 1 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `3046a9a3e8ca46214b361111d006b79b6c57fa3c` | 1 | 4 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `7986a5f8410960c17f73103c79752ad01f47c379` | 1 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `f90ca8cab72e0279397e33bc33462259703f9072` | 2 | 4 | 4 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `c4687cb493d1e86e050920f18f95b009bd3bb077` | 2 | 3 | 2 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `080f6444f41956c5986b73b7b8ad1a002e5bd970` | 1 | 1 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `8c2dddb2fcc6e68247b734d6fd4ebaa69004e25f` | 3 | 5 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `4bcac792702965c09409d0b53e9cc7e38ae63ec5` | 1 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `98246dbc66a29d9dde5ecc9a45d1842f12677faf` | 2 | 4 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `964b14db51c841c5cf7beb29cc3d8086e2436f0b` | 1 | 7 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `5edf45e5d0d625cf4e69f4cf1cbc2fc330b77aec` | 1 | 7 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `d53913c06083529df1831fe9cea01c5dc9265f63` | 1 | 4 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `af1a408c41ae7c22205eb6cf09162c993791a7af` | 1 | 6 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `fcab62931db10591a6f9f7ff50b8116350834459` | 1 | 2 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `fa314494865159c0dfb02c93cdc50eaad3ce0940` | 2 | 3 | 3 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `346df02e8c9fca296b75c2420b9a699a39bd9cf4` | 1 | 2 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `5808f5e7cd78925c448dac4b717119d7b765d9ec` | 2 | 2 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `c7ca999dea53886b5dbcccf47203881f505ec2c1` | 2 | 3 | 3 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `99287e7a72d7ad294d9c28d8faee6428d5169c2d` | 2 | 17 | 2 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `130442f33eec9e899014b91c3c0e7b1fc8ef84c0` | 1 | 39 | 38 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `bdbaafa1077175e14b7d9222d9bd116b5b101caa` | 2 | 2 | 2 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `a5b49cc87848f8f519fe1eb2f31fc962b90e7847` | 2 | 2 | 2 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `ff4ccb22d94331f8a1a76c130fdb2b637fd53c91` | 1 | 8 | 8 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `1c85875cdfcc5c3b1752e5e957d070b980770e0b` | 1 | 12 | 9 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `4190f4016e58cda8ce13864994553beb54451711` | 2 | 5 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `1eccc765fd2bfa02b14f335fc367c23d51d6da02` | 2 | 3 | 0 | PATCH_EQUIVALENT_TO_MAIN |
| `32e186ebffec31b2e56670ec1eca680029486367` | 1 | 9 | 7 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `8ec23daf106de66317eb91cb71e8cc752e9fba4e` | 1 | 2 | 2 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `2424c389eaafd89b80a6e32b7155d9c577a874b2` | 1 | 1 | 1 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `d1ef157d46bf6e349d680f1fa0ac6160be1c2cd6` | 1 | 40 | 24 | REQUIRES_BEHAVIORAL_DISPOSITION |
| `d91f9498b4040c062a555ea1b3b1279201491899` | 1 | 8 | 7 | REQUIRES_BEHAVIORAL_DISPOSITION |

## Subsequent reconciliation

See FOLLOWUP_FINDINGS.md: one Windows network backup is behaviorally superseded (47 heads remain),
the Blender provenance fault is identified, Windows ARM64 failure is reproduced at the audit head,
and the website public-proof observation is complete. Initial inventories remain historical snapshots.
