# Integration security delta

Status: UNREVIEWED. This supplement is tied to `cd518748f742025a251b78028613aa1b16919e73` and is not an independent
security conclusion. The prior af109ca immutable packet is Actions run37035329221,
artifact11239081363; its source archive SHA256 is
`bf9445bc43796557cf9ea7c5777a6643582b28c4b2507fb0883fc357e489a8fa`.

The source archive contains all six owner security deltas in docs/composition,
docs/audio,docs/project-graph,docs/godot/authoring,docs/blender/authoring and
docs/effects. Review those alongside the12 areas in docs/security-review.md.

| Boundary | Integrated change | Evidence/limits |
| --- | --- | --- |
| Authorization/lifecycle | Fresh-child reconciliation binds original owner/root/vault incarnation, a fresh observation and new request; no budget refund or ledger reset | Source A50/D7 regressions (current37074022332); old and new tickets cannot cross |
| Effect evidence | Audio production adapter uses admitted decoded measurements through the real F evaluator | Prior af109ca B/F18 source cases, native AV37044706034 PASS and full B37045257965 PASS |
| Host isolation | Closed v8 sessions/tools for E/D/Motion/MLT; fixed templates and pinned runtimes | Scoped grants and limits retained; owner-configured Godot movie display is unavailable without typed Host display authority; no caller Python/GDScript/shell entry |
| Native identity | Provider qualifies numeric Godot wire PID with authenticated Host job incarnation | Numeric wire remains strict; wire-supplied Host qualifiers rejected; a job identity is not client authority |
| Export | Sealed Godot export helper validates fixed argument shape, pinned dependency and owner project/output mounts, with private HOME/XDG paths | Prior af109ca D37036800206 attempt2 PASS; initial control timeout retained |
| Diagnostics | Bounded UTF8-safe private diagnostic files and merged export stderr through existing bounded Host output | Existing opt-in owner scratch only; public error redaction remains |
| Fonts/media | Fixed pinned font packages and scoped media readers | Canonical non-symlink files and byte bounds; upstream engines remain TCB |
| AV publication | Artifact-bound post-encode PCM and raw sync measurement; contextual evidence does not replace native artifact pins | Required PASS/UNKNOWN/FAIL retained; coverage_complete=false remains where observed |
| Supply chain | Root lock reconciled with #201; reconstructive package dependency manifests versioned and tree/hash checked | Fresh audit/deny/supply-chain gates pending; no global lock exemption |
| Secret scan | Exact path/rule/line SHA classification for public cache labels and C14 determinant identifier | Complete ancestry/tree scan PASS_WITH_TRIAGED_NON_SECRETS;0untriaged; no body disclosure or whole-file exclusion |

Environment markers and hashes are not authorization. OS/Host confinement and
authenticated execution context remain the authority boundary. Tests on
disposable roots do not sandbox an unrelated already-open user application.
Fresh reopen does not prove crash durability. No cross-application ACID, exactly
once, global noninterference, aesthetic or independent-security claim is made.

R16 requires a dated independent report covering all12 areas on this exact
source, plus recorded remediation for any blocking findings.

The current typed export dependency API requires a bounded valid Host tool table
and exact dependency name/path. Foreign paths, undeclared names, empty/malformed
and duplicate tables are denied. SDK regression and unchanged zero-debt source
guard passed in37074022332. No fallback or extra execution grant is introduced.
The independent-review packet is regenerated for each product source. Historical source archives retain their original SHA; an UNREVIEWED packet is not security acceptance.

The current AV correction accepts stale classification only from the strict bounded closed-helper error envelope, without accepting an outcome flag from that envelope. Unknown codes or malformed/control-bearing envelopes remain BackendFailed. Write outcomes stay uncertain; sync.probe retains its read-only outcome semantics. New unit and live native stale-input checks pass on2111627. This does not authorize retries, reset a PlanVault budget or bypass reconciliation.

## Responsive Linux Host preparation

Current sourcecd51874 offloads strict binary verification and bounded sandbox startup to the blocking pool so control traffic stays responsive. All original authority/format/digest checks and deadlines remain. The launcher owns fresh sealed dependency descriptors until the child-metadata handshake finishes and checks cancellation before starting a child. Native v5-v8 controls, source contracts and two standalone exports passed on the exact source. Independent review still needs to assess lifecycle/cancellation races, blocking-pool resource pressure and the aggregate transport surface; no security guarantee is inferred from these tests. Previous2111627 and5843387 security packets remain historical.

Current exact-source review packet: run37076233575, artifact11256507417, archive SHA256 `237c3c95420d975e3e48b487b7de34e583963024b85d3c56bc3659212a97c425`. GH artifact ZIP digest `c08de4f6a99b2ee00f1ed7da5d4f9082f0fe9225d5d7c6970f7da69edb045140`. Kept remote and UNREVIEWED. Maintainer precheck MAINTAINER_PRECHECK_cd51874.json verified zero untriaged findings, without an independent security claim.
