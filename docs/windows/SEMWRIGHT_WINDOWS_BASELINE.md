# Historical Windows implementation baseline

`BASELINE_SHA=13b486aa67f89c039bc526321cccd869d1a68bd8`

This file records the frozen baseline used when the Windows port began. It is historical provenance, not the current Windows capability or release-status document. Current behavior and evidence are maintained in `README.md`, `WINDOWS_VERIFY.md`, `WINDOWS_SECURITY_BOUNDARIES.md` and `LIVE_WINDOWS_TEST_MATRIX.md`.

The SHA was captured once after `git fetch origin --prune`. The canonical checkout was not modified. Exact-SHA GitHub Actions observed green before implementation: Quality gates `35942804030`; Dependency, coverage and fuzz `35942803985`; Native application integration `35942803946`; Platformization and macOS `35942804105`; Packaging certification `35942804144`; Plasma Wayland live `35942803932`; Native X11 EWMH live `35942803954`.

The snapshot contained the completed platformization/macOS line plus subsequent Linux integrations, RemoteDesktop EIS, AT-SPI recovery work, Driver Registry/SDK/Host, Events/Jobs/MCP federation and application-driver integrations. The baseline intentionally remains fixed at the SHA above so later Windows implementation evidence is not retroactively attributed to the original snapshot.
