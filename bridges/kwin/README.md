# KWin bridge

The script publishes bounded window metadata to the broker's D-Bus mailbox and requests
queued typed commands. The broker checks that the caller owns `org.kde.KWin`. Replies are
correlated by request ID. Cancelled/expired broker requests are removed from the mailbox;
a heartbeat makes old cached snapshots unavailable rather than indefinitely “current”.

The JavaScript contract accepts only focus/move/resize/close and validates exact identity,
fingerprint and integer coordinate limits. No arbitrary JS code is received from an agent.
Mailbox cancellation cannot undo an operation that the compositor already accepted.

Install the reviewed package with your Plasma KWin script manager, or the matching
`kpackagetool6 --type KWin/Script` tooling on a compatible Plasma installation, then enable
it explicitly in KWin settings. The package root is this directory (metadata.json plus
contents/). The broker must be running with its session bus name before the script begins
polling; restart the script after a broker restart if its polling loop has stopped.

Uninstall by disabling the script and removing the `semwright` package through the same
manager. Do not delete unrelated KWin scripts. Shared-contract Node tests passed, but
**actual KWin script loading, asynchronous D-Bus behavior and window control did not run**.
Record Plasma/KWin versions and rerun the full manual matrix before extending compatibility.
