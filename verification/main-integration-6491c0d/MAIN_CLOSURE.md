# Main integration complete

Main: [6491c0d838fa066938a494524d69ed507aa0dbe8](https://github.com/seradotcom/semwright/commit/6491c0d838fa066938a494524d69ed507aa0dbe8).

A–G, runtime #201, dependency fix #198 and CI #192 are integrated. Formal AV #204 and C14 #206 are included. Eleven PRs targeting main were automatically marked merged; nested #206 is closed as included, diagnostic #202 as superseded. All branches remain. Independent Dependabot upgrades #166/#165/#161/#157 remain open.

[Source identity 37096515334](https://github.com/seradotcom/semwright/actions/runs/37096515334) proves production code and Cargo.lock match the fully certified cd51874 engineering baseline. The only changes are four documentation files and the exact corrected Windows test fixture. [Standard Windows 37096430846](https://github.com/seradotcom/semwright/actions/runs/37096430846): four jobs PASS, including x64, ARM64 and sealed-tool compatibility. Historical baseline certificates retain their source identities; the original global failure is preserved with its explicit corrected-fixture disposition.

The existing automatic-main CI pause was active at promotion push; those skipped jobs are recorded and are not evidence of executed tests. Ordinary main CI is now restored for future events (`SEMWRIGHT_PAUSE_MAIN_CI=false`).

H benchmarks are deferred by the user for separate future work. Its branch and native preparation remain intact; no model comparison was run. R16 and physical/interactive desktop release blockers remain open. No release/tag was issued. Original A–G checkpoints are preserved byte-for-byte in `I_MAIN_OWNER_HANDOFFS.json`.
