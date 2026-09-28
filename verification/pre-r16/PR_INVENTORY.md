> **Historical first-pass report.** Current continuation, owner assignments and admission status are in [R16_HANDOFF_READINESS.md](R16_HANDOFF_READINESS.md), [OWNER_HANDOFF.md](OWNER_HANDOFF.md), [BRANCH_DISPOSITION_CLOSEOUT.md](BRANCH_DISPOSITION_CLOSEOUT.md) and `pre-r16-audit.json`. Earlier snapshots and failed logs below are preserved, not presented as current-head certification.

# PR inventory

Initial Git/CI reference: `241000c268d1bf1dc29d4e91a913097ac0d020cb`. Open PR snapshots are in `inventory/pr-*-latest.json`.
Three owner PRs were observed before the audit's own PR was created; their classifications are
single, explicit dispositions, not inferred from titles.

| PR | Head | State | Class | Action and reason |
|---|---|---|---|---|
| #153 | `ab5b4f95d2193a7e1070fc5158c037c0b8f3a281` | DRAFT | F — BROKEN / INCOMPLETE | The interactive harness still accepts exit-zero without positive test counts and uses generic self-hosted Windows labels; baseline ARM64 UIA failure also requires owner reconciliation. |
| #154 | `d46b241b97beb14093da71582c5106e6d616271c` | OPEN | F — BROKEN / INCOMPLETE | Owner-pinned runtime is relevant to isolation; the PR also expands GLB capabilities and has multiple failing checks. Resolve runtime/security requirements without automatically admitting new export surface. |
| #155 | `9acfa3c4f3c02c340db8cb579277f4754ef8d96d` | DRAFT | E — POST-RELEASE | Game/demo and cross-application expansion is outside the pre-release audit. Its Blender export overlaps PR154; reconcile that overlap without adding the demo to the review baseline. |

No incomplete mixed-scope PR has been approved wholesale as category A. Required remediation still
exists: resolve the Windows regression/harness and determine the necessary Blender runtime-security
subset. The new GLB path is not automatically a prerequisite for R16. #155 is deferred, not deleted.
The Blender source/schema overlap between #154 and #155 must be reconciled by their owners.

## Coordination

The Windows owner received exact run/job failure evidence and the zero-test/routing findings in
PR153 comment `5866493349`. No change was made to that branch. The later head still had these gaps.
The audit owns only its preflight branch, bundle/CLI/doctor fixes and root evidence documentation.

## Recent integration record

A merge is a starting point for source/test verification, not a blanket correctness certificate.

| PR | Title | Merge commit |
|---|---|---|
| #152 | feat(windows): support Plugin workspace mounts | `241000c268d1bf1dc29d4e91a913097ac0d020cb` |
| #151 | feat(windows): prove owner-gated ambient network authority | `fc9e9f7e5349e4b5635187f8b1c7f996e99130b8` |
| #150 | feat(figma): semantic authoring, verification and bounded convergence | `33a162e2fbe6775a61f431b40e0cc7e1ad55f8a0` |
| #149 | fix: make Godot full broker route conformant | `9ecf35fd9c3d6fbcbc1f8b72b8d4734c70037ffa` |
| #148 | fix(daemon): isolate driver file grants from directory backends | `0cd469083ffb24eedd48be2d9d369184eb041312` |
| #147 | feat: add Agent Skills interoperability layer | `849e253d87d44b0777ef99f618093e358734bf11` |
| #146 | feat(windows): add host-mediated driver loopback bridge | `a04f664969ac830e3cb5a96c6e62e8eefc40b5fe` |
| #145 | feat(windows): support private driver secret grants | `7eceb254a1bfc292966c1735de583df366391ff3` |
| #144 | feat(windows): support Host-sealed driver tools | `74f110dd4d64c81c21e82ff3aedafa4812b7c031` |
| #143 | ci(oss): reject environment-specific development traces | `b96d77bb2735eee3c4e564043493c8ae13fab0ee` |
| #142 | docs(oss): remove handoff-specific verification language | `2f19664f77992c553b57bca78411381eec431bdf` |
| #141 | feat(browser): complete semantic browser surface | `c14e9e4e26763a56edb9329182a9f2ed6a9a837a` |
| #140 | chore(oss): remove environment-specific verification traces | `d67796702cc96e214fc68e30d393a2390ccd4ff4` |
| #139 | fix(figma): normalize nullable bridge revisions | `3cd86958f70f7a8231d31e492b5505acff19dae0` |
| #138 | feat(windows): support read-only system config grants | `84bf6aeba644f2732bb4dab278ffcd2c9f0dc630` |
| #137 | test(windows): compose network and CPU sandbox spec | `44baa1a1c51adb752c218bd9e99f0341e9dbf058` |
| #136 | feat(drivers): harden session continuity across transports | `60b3120f85acf92519f329baf1491d0946ac0d95` |
| #134 | fix(blender): paginate broker capability smoke | `0bb5e111cbfdd0a3ea7a55efbf7af6bccdd8e2c3` |
| #133 | feat(windows): enforce per-operation Driver CPU budgets | `dc051b5bdb03401ab5f43e661116335e818be803` |
| #132 | security(windows): scope AppContainer outbound network capability | `e17c198cdf3624e17e2de7f0c8e273d210e75d35` |
| #131 | fix(figma): complete localhost dual-stack continuity | `96ad7a919e16ef7e98629f50592ed86541096987` |
| #130 | test(federation): retry catalog generation conflicts | `36f51ad1f84f9f27121221751af3c1b8578ddb70` |
| #129 | fix(figma): stop invalid pre-auth reconnect loops | `43bdd4a18088f0193d5104dab9c93a3a940ba9ac` |
| #128 | security(windows): grant bounded AppContainer workspaces | `54ec088571075481c94d1cb10ef08c6cdd6a9d65` |
| #127 | docs(linux): certify compositor reconnect progress | `5cb29e9b8ff2ef4549407e1b547b8e0d4647aee4` |
| #126 | fix(figma): keep bridge sessions resilient across disconnects | `94d9068b823b87fc1cc13f3c79ef9a5bf28e16b9` |
| #125 | feat(windows): secure Plugin Host and MCP child parity | `1b374a14e1a7de21331c2756c0adddabd1a64070` |
| #124 | docs(godot): reconcile closeout branch history | `e1760981b6b1eac4ba04396ae9727ca947b46cc9` |
| #123 | fix(linux): pace legacy portal text input | `050dbeaa631da599e52e17581bff8e5bf509ee9e` |
| #122 | fix(kwin): support Plasma 5 workspace API | `2119203645bf363fc0bc645d9c26556f70753c1a` |
| #120 | fix(linux): pace sustained EIS keycode text | `9d36c9cf99ace2bc0de30928475b1575429fd693` |
| #119 | feat(blender): semantic completeness | `512369323d355de22734b2ba967180b7a4938b9c` |
| #118 | test(linux): gate isolated EIS live certification | `60a6365c954e24c15d38882430f4eca26611dcb0` |
| #117 | test(linux): gate physical Hyprland certification | `1ebeb926d1925e0a7ec033d25c226bc428cec3ac` |
| #116 | docs(motion-canvas): close semantic completeness evidence | `e543a6bed1497840efdb06ce0f9b96748e9529a1` |
| #115 | docs(security): add reproducible review handoff | `296e0bb68fd49678bf79858affdedc9b9a6cbde9` |
| #114 | docs(linux): clarify EIS test isolation | `772a53b12c553eefad928b3517be8f58e3fb475e` |
| #113 | docs(linux): clarify invalidated EIS keyboard evidence | `7c717f6154b57c5fc874f6fcdae7e4bf97c57519` |
| #111 | fix(linux): drain bidirectional EIS feedback | `65db6d37907a7f9c5b7051d291a727c14a0435ab` |
| #109 | feat(motion-canvas): complete managed semantic authoring surface | `132355b5a182349eb165460307cc8ed0ddcef940` |
| #108 | feat(driver-host): add per-operation CPU budgets | `fda413e4962021b93a9941701ce644890d61d894` |
| #107 | fix(linux): harden Ubuntu Noble AT-SPI crash path | `8c5cecd9af6efb5190ef24d10cb4318bf88e5814` |
| #106 | Cancel in-flight EIS text input cooperatively | `d2aa0456f34cd48bcd23283957ac48331b570a44` |
| #105 | Certify hardware-backed Hyprland live matrix | `2cd0e161093d62ca26b290d7536aa8b6c418afa1` |
| #104 | feat(driver-registry): add safe multi-artifact package v2 | `b3d5a034810b5981ef8089a4127ea31a0dfe88b7` |
| #103 | perf(linux): add bounded AT-SPI Collection pushdown | `f9495c9e87125f946123524a73fddde02b92d683` |
| #102 | feat(driver): seal secondary executables in Driver Host | `7b7986674b3fb98d500eb769f5f187193eaeccb2` |
| #101 | feat(driver): add first-class owner secret mounts | `cdabeb50d1f9d774de88e2401daf5a5f5eb611c5` |
| #100 | feat(driver): enforce loopback-only driver authority | `c9d6132f55f9cc1076b1556932a1f7518d1edd7d` |
| #99 | feat(driver): broker-native refs for dynamic drivers | `8d76053ff8a63760b553aeebae9bb1733c28c1e2` |
| #98 | feat(godot): finalize generic semantic substrate | `f2f3ec470f95c2010df89a61d4835afe5c4926a1` |
| #96 | security(windows): add platform-owned secure child spawn | `e831bff8ceeff180b89c2cf15e6955b7239bc402` |
| #95 | security(windows): harden executable trust policy | `ae00db0dccdc5085e53ed6ff66871066470cac28` |
| #92 | feat(windows): close WGC D3D11 frame readback | `3ebaccfd3dafeeca4fcfcbed395dc488c936830f` |
| #91 | feat(godot): close semantic authoring domains | `9553489ccd97709976495da0ab5e2ef29cc3d1f0` |
| #90 | Translate EIS keycode keyboards via advertised XKB keymaps | `66f8ceab78fe78d6d207d48aa9ec4bfbe90b4cc3` |
| #89 | feat: add workflow distillation automatic proposals | `67202ff9f3ac9e06a66403742c29a1a65d39086b` |
| #88 | Fix hostile-suite security review commands | `1edfd5be0029d809d92e3cdb587fb8a6a4d2797b` |
| #87 | fix(figma): use Figma-valid localhost bridge URL | `3e563fc272266838ee64fdd601086846b5afe953` |
| #86 | feat: add workflow distillation pattern suggestions | `3d23cbda69d1356cbf8819e560152261e8e3759a` |
| #85 | Certify real GNOME ConnectToEIS lifecycle evidence | `7f608632c22d739723c65b8c7198dccbf21c00b0` |
| #84 | Sandbox external MCP stdio upstreams | `17b256c9d4ab9401d6a26bd3fffe0efb5789523e` |
| #83 | Add cross-platform Semantic UI v2 rich semantics and hit-testing | `b9dcf1bb64db4e2626aca3c2f22d8b08cd373c95` |
| #82 | Fix Landlock null-device stdio confinement | `1dd88c815cb43986f1ab5f9d00776c233fa09396` |
| #81 | feat: add generic cross-driver artifact handoff | `b0738914314a26aa8fefb7c3bcacef82c5fcb6e2` |
| #80 | feat(figma): close semantic completeness verification gaps | `b038d4f0ccd0091485ec3eda75aa593e06e430c2` |
| #79 | Reconcile live matrix and closed release blockers | `a7aca1fb58f37d007c4643f05ae76f1988afaed6` |
| #78 | Add workflow distillation v1 | `a19b8c0a083ff6ffdf6172d2d98cd6b42e651c6e` |
| #76 | Integrate Windows platform host foundation | `73ad946379ee4679280d7b80ba0ef602f3f5c6f8` |
| #75 | feat(godot): semantic domain completeness | `efa98e80c01d94db0fcd2f5960f2dbf2362c2830` |
| #74 | Prepare independent security review packet | `8bb81c35466a1c37d8f7e887cca7e1da488a33f9` |
| #73 | Fix reproducible CycloneDX SBOM attestations | `468b31a9e50961549753028b64f3fc66bea84b45` |
| #70 | Fix D-Bus broker name registration ordering | `ed0f18d93611d0a53b75f60253a34e6cfc3ff68d` |
| #69 | Fix GNOME keycode-only EIS keyboard negotiation | `49263f74397cb1b34b6d67f94bd239348fa4e486` |
| #68 | feat(driver): complete first-party Godot integration | `51d2d9fe6cf904ac51a814d9469e92faa9a2f95a` |
| #65 | Close jobs progress and inspector workflow gap | `ad6b3c39dc470a9e6545ea618dbcc048f1afe585` |
| #64 | Certify provider progress and job artifact flow | `12bfe63c26efedc772d4be2c4c0840600c5967ab` |
| #63 | feat(figma): drive semantic coverage toward completeness | `0fb4e7c9b5256a2f5d6aecad9550f4b332914d99` |
| #62 | Tighten built-in capability output schemas | `13b486aa67f89c039bc526321cccd869d1a68bd8` |
| #61 | Certify Nix SBOM and provenance supply chain | `c359f01e75c9ddd13b214ed34805118f32896b9c` |
| #59 | Fix post-merge live backend Context v2 regression | `a4bdd6173f83939a7d46f589ecf422e9ead63869` |
| #58 | feat(video): generalize backend-neutral render intent | `b7bb9920fa6406201db00e038ebebe351ee87494` |
| #57 | Video domain: formalize backend semantic contract | `7ef021f7597b0ec57251a5c4589497b72690f4e2` |
| #56 | feat(figma): add semantic Figma DriverProvider | `45958441c2392debbae80e2c894088590ff9dad0` |
| #55 | Add Driver Protocol v2 progress, events and cooperative cancellation | `4b24d4417198aae14193cf6bcb8bc145bbaf7d06` |
| #53 | test(video): harden shared semantic domain contract | `47db3ecd56d92075ffcacffae22ee79af93753fa` |
| #52 | feat(video): extract backend-neutral semantic video domain | `649f97fd991c1832181bea6594d0d495223647bd` |
| #51 | Exercise X11 backend through real Openbox EWMH session | `c31f3776a93e6e2832ebf2c56e40b8bb9b75fc85` |
| #50 | Exercise Plasma KWin 6 bridge on virtual Wayland | `08fb74ff900e98ba489f8db304ceeac49340f532` |
| #49 | Add semantic Motion Canvas driver and reproducible launch-film demo | `2d95e586b1d67471cdb1aa9d3a3dc3940bb44f58` |
| #48 | Certify reproducible packaging evidence | `d101f5a961e367225727088020ff1ec58f6dd9e0` |
| #47 | Exercise Sway IPC on real headless compositor | `3635a8caed92b109c0efa99873d5ae0c6df2f659` |
| #46 | Certify live GNOME Wayland semantic route | `7ce9f708e0546a7de8e1bd87060258cb4ad36898` |
| #45 | Certify reproducible release packaging | `94173f529db40886415d51560e5f849501d9a985` |
| #44 | Map Semwright jobs to MCP Tasks | `6bab0cca6948465752468d4a324fd4b7a6202860` |
| #43 | Harden Chromium download, crash and artifact lifecycle | `9f3759ebcad45fa0349eb59a82e0ddcdc22d7a13` |
| #42 | Certify Rust 1.88 MSRV and hosted static linters | `bcad3087e755dca42e1102f96be6b5881caffa0f` |
| #41 | Harden plugin sandbox and handshake attestation | `b3f808a195e847d0e60de9de2f5076cb8e64ebcc` |
| #40 | Certify universal runtime and current release gates | `25209e1ec1967c6d60406e6f6363804457f0a1e0` |
| #39 | Persist portal restore tokens and integrate clipboard | `2c8dbf1281fa2e18b2740f5b44d5cf0d18aff2a7` |
| #38 | Add portal PipeWire ScreenCast frame capture | `5830663785f9f47bd06af7965a88485329767fd7` |
| #37 | Rename canonical CLI and MCP tools to Semwright | `a14abd8328e092a8227584750e47c38a77449ffa` |
| #36 | Port AT-SPI delta recovery onto platformized Linux host | `2fffdb90b502c9e59bccb588bb48ef749753b3b0` |
| #34 | Close macOS platformization CI gaps | `336bec4bb876d4ee4ccb7deb3833dcfca7d131d9` |
| #32 | Add deep OBS realtime driver integration | `b0b4c2d3f8ea4911fc69e94b34e114af4eb9652f` |
| #31 | Port RemoteDesktop EIS transport onto platformized Linux host | `af8f9702afe75c4eecfc4d685dcbb278036c91a8` |
| #29 | Platformize runtime and add native macOS host foundation | `70c409fc619411b6ecb5fb3f723e27d00cac634e` |
| #27 | Certify static driver distribution evidence | `059d997e93fb72c248f8b111929d24acb7f0f7ae` |
| #26 | Add safe static driver registry and distribution | `4b657454eb46b38a416c66926066a5de5166f36a` |
| #23 | Harden universal X11 backend lifecycle and blocking I/O | `963f0ceecb22ccfadf66b0937524fb30a6269030` |
| #22 | Certify MLT driver against real melt runtime | `e94a2114786e03610b698b9996693bdb385f217a` |
| #21 | Certify interactive Blender add-on against real Blender | `f8a49bef3aead639448e076faadaaaa05280ef85` |
| #20 | Add sandboxed Blender deep driver and fix RNA type ranking | `3df4123fced2bc54fd6242214849ce585a09adfb` |
| #19 | Integrate bounded KiCad and MLT video drivers | `71b8cd78b86fbf0faedd9df9f411881207078d25` |
| #17 | Add sandboxed LibreOffice UNO driver | `7d6977eaed8bccd37bce8cf50c51c96e23037cd1` |
| #16 | Add provenance-aware events and session-scoped jobs | `1fa806ff5260858d9affe90352dd5f719645d6da` |
| #15 | Certify federation and App Driver SDK foundations | `cab62bf232baef37cc595fd6b5a07d4772c9e754` |
| #14 | Fix deterministic Chromium DOM reference invalidation | `39ea848322172fd39fc02b98b5a9c51ef479ad39` |
| #13 | Add persistent App Driver SDK and sandboxed conformance | `fa2489e4f4e13143fc9b6bba7c422fc28a76737e` |
| #12 | Federate external MCP servers through the provider runtime | `1ba7d5fc0a7ae9a6aeea14bc567c3fee9302a63f` |
| #11 | Introduce explicit provider runtime and governed dynamic capabilities | `dcaf8ca1d5dc067615fc3cf16cb714eb89d6d3c1` |
| #10 | Certify Rust baseline and expand the governed semantic runtime | `691e876682a3a1d3707c65cf62a35a10802b9eab` |
| #9 | chore(deps): bump tokio-tungstenite from 0.28.0 to 0.30.0 | `1bc91322406b0dc5aa5c10f7428f1311942a9877` |
| #8 | chore(deps): bump clap_mangen from 0.2.33 to 0.3.3 | `3a048cdde7f531811b7ffb7e126ad24346d6cd3a` |
| #6 | chore(deps): bump toml from 0.9.12+spec-1.1.0 to 1.1.6+spec-1.1.0 | `ad4d2aa9555b4e791033e68d817eaa491e23215f` |
| #5 | chore(deps): bump base64 from 0.22.1 to 0.23.1 | `4dcfa58c68fd4e733169bcc4e4ed6c1f706f5643` |
| #4 | chore(deps): bump actions/checkout from 4.4.0 to 7.0.1 | `a403114a6f5072dcaaa89453780b5d1f884704f9` |
| #3 | chore(deps): bump actions/setup-node from 4.4.0 to 7.0.0 | `4d9a17dfc543688cbdbef803cb1c9753872d4a1f` |
| #2 | chore(deps): bump actions/setup-python from 5.6.0 to 7.0.0 | `bf7415262d7cee791d210d9ec0c6f42ba311d666` |
| #1 | chore(deps): bump actions/upload-artifact from 4.6.2 to 7.0.1 | `a68920d01cf83dc9641c1a7574ca0a340ca2a7d9` |

## Closed without merge

These require content/behavior comparison, not automatic loss or automatic supersession. The
branch-tree inventory resolves exact patch/tree equivalence where possible; remaining cases are
not falsely marked integrated. Browser #121 was followed by merged #141; the precise residual
branch differences still appear in the tree comparison. No closed PR was reopened or discarded.

| PR | Title | Preserved branch |
|---|---|---|
| #135 | fix(blender): paginate broker capability smoke | `fix/blender-broker-smoke-limit` |
| #121 | feat(browser): complete semantic browser surface | `feat/browser-semantic-completeness-chatgpt` |
| #112 | docs(linux): require isolated EIS live testing | `docs/eis-live-test-safety` |
| #110 | fix(linux): handle EIS socket backpressure | `fix/eis-flush-backpressure` |
| #97 | feat(godot): complete generic semantic substrate | `feat/godot-semantic-substrate-closeout` |
| #94 | test(windows): execute native UIA fixture | `feat/windows-uia-native-fixture` |
| #93 | feat(semantic-ui): add exact reference inspection | `feat/semantic-ui-v2-query-pushdown` |
| #77 | Fix Landlock execution rights for authorized Motion Canvas mounts | `fix/motion-landlock-exec-diagnostic` |
| #72 | Platformize local runtime for Windows and add native Windows host | `feat/semantic-ui-v2-complete` |
| #71 | Reconcile runtime and release certification | `docs/release-closeout-v3` |
| #67 | feat(driver): finalize first-party Godot integration | `feat/godot-driver-finalize-chatgpt-v2` |
| #66 | feat(driver): add first-party Godot integration | `feat/godot-driver-complete-sol` |
| #60 | Exercise Hyprland IPC on a headless Wayland compositor | `test/live-hyprland-headless` |
| #54 | feat(video): harden backend-neutral adapter contract | `feat/video-domain-hardening` |
| #35 | Close macOS platformization verification gaps | `verify/platformization-macos-closeout-chatgpt` |
| #33 | Close macOS platformization evidence and stabilize catalog refresh | `docs/platformization-macos-evidence-chatgpt` |
| #30 | Platformize host boundaries and add macOS foundation | `feat/platformization-macos-chatgpt` |
| #28 | Add RemoteDesktop EIS input transport | `feat/portal-eis-runtime` |
| #25 | Platformize runtime and add macOS host foundation | `feat/platformization-macos-integration` |
| #24 | Add AT-SPI delta snapshots and stale-reference recovery | `feat/atspi-delta-recovery` |
| #18 | Add sandboxed Blender deep driver and RNA introspection | `feat/blender-deep-driver` |
| #7 | chore(deps): update crossterm requirement from 0.28 to 0.29 | `dependabot/cargo/crossterm-0.29` |
