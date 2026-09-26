# KWin bridge

The script publishes bounded window metadata to the broker's D-Bus mailbox and requests
queued typed commands. The broker checks that the caller owns `org.kde.KWin`. Replies are
correlated by request ID. Cancelled/expired broker requests are removed from the mailbox;
a heartbeat makes old cached snapshots unavailable rather than indefinitely “current”.

The JavaScript contract accepts only focus/move/resize/close and validates exact identity,
fingerprint and integer coordinate limits. No arbitrary JS code is received from an agent.
Mailbox cancellation cannot undo an operation that the compositor already accepted.

Install the reviewed package with your Plasma KWin script manager, using the matching
`kpackagetool5 --type KWin/Script` or `kpackagetool6 --type KWin/Script` tooling for the
installed Plasma generation, then enable it explicitly in KWin settings. The package root is
this directory (metadata.json plus contents/). The broker must be running with its session bus
name before the script begins polling; restart the script after a broker restart if its polling
loop has stopped.

The runtime selects only between the documented KWin workspace generations: KWin 5 uses
`clientList`, `activeClient` and `client*` lifecycle signals; KWin 6 uses `stackingOrder`,
`activeWindow` and `window*` signals. The command contract, identity checks and mutation surface
remain identical in both modes.

Uninstall by disabling the script and removing the `semwright` package through the same
manager. Do not delete unrelated KWin scripts. Shared contract/compatibility tests execute both
workspace API shapes. An isolated Plasma Wayland VM with KWin 5.27 also loaded the production
bridge and executed Wayland-to-Wayland focus, move and resize through the real broker mailbox;
see `verification/live-plasma-kwin5/kwin5-bridge-2026-09-26.json`. This does not substitute for
all physical Plasma/KWin hardware/session combinations.
