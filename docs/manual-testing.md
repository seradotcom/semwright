# Manual/live verification matrix

Some rows below now have recorded development evidence, while the remaining rows are still
procedures rather than implied successes. GNOME Wayland semantic GTK/AT-SPI, hosted X11/AT-SPI,
PipeWire, deep application and sandbox paths are recorded in `VERIFY.md`; do not generalize them to
untested desktop/version combinations. Use an owned disposable Linux account with a temporary
workspace containing only test data. Keep a second trusted recovery terminal; do not
connect an agent to an account containing important sessions or credentials.

## Record before every run

Commit/tree hash, exact Rust/Cargo and dependency lock, kernel/distro, desktop/compositor
version, Wayland/X11 session, portal/AT-SPI versions, app version, relevant config digest,
policy grants, architecture and test commands. Record observed outcome, including partial
state after errors. Never put secrets or raw application content in shared logs.

## Foundation gate

Compile, format, lint, docs, unit/property tests, fake-broker integration and fake-smoke
must pass first. Use the `Save` duplicate fixture to verify that discovery returns two
candidates and performs no action. Invoke one exact `Export` ref, then check that its old
generation becomes stale. Resume the same ticket across CLI processes; a different ticket
must fail the old ref. Disconnect during a slow fixture action and confirm cancellation
and uncertain outcome metadata rather than retry. Verify root real-mode refusal and
socket/ticket/config modes using a second user account, not chmod bypasses.

## Desktop matrix

| Session | Fixture and positive case | Negative/edge cases |
|---|---|---|
| GNOME Wayland, GNOME Shell 46.0 | **EXECUTED:** disposable Zenity/GTK fixture via production AT-SPI; discovery, snapshot, semantic text mutation, delta, close/resync and stale-ref rejection | Portal consent/revocation, optional bridge, different bus sender, locked screen, scaling/multi-monitor remain |
| Plasma Wayland | Qt/GTK accessibility; KWin mailbox focus/resize | Broker restart, stale heartbeat, cancelled queue, script disabled |
| Sway | Native window tree, focus/move and semantic UI | IPC endpoint ownership, window ID reuse, workspace change, failed command |
| Hyprland | Current native JSON client list and dispatch | Stale address/fingerprint, compositor restart, mixed scale/monitors |
| Native X11 + EWMH manager | Window lifecycle, focus, XTEST on a fixture | Missing WM, hung X server, server disconnect, destroyed/reused XID |

X11 I/O/lifecycle design fixes are implemented and the dedicated Xvfb lifecycle regression passes.
Xvfb without a full interactive EWMH desktop still does not certify the native-desktop matrix; do
not call a nested compositor test equivalent to every interactive desktop configuration.

Use `ui.snapshot` with several budgets, exact and regex selectors, duplicate labels,
ancestor refs, disabled/invisible controls, editable text, numeric values, toggle/select/
expand actions, disappearing objects and cycle/broken-child fixtures. Confirm semantic
actions succeed without global keyboard/pointer permissions. Force AT-SPI service loss
and verify stale invalidation, bounded errors and partial results, not endless waiting.

## Portal and capture

Start with no input/capture grant: commands must fail. Add only the needed capabilities,
use an explicit operator console when risk requires it, then request keyboard/pointer
through the compositor chooser. Reject it once; approve a later intentional request;
revoke during use. Check owner-session isolation, session close, pending request cleanup,
focus change between observation and action and cancellation without a stuck chooser.

Capture only a disposable fixture. Check artifact permissions, PNG metadata, normal expiry,
no bytes in audit and cleanup after normal/abnormal termination.

Multi-monitor scaling and mapping are not considered solved by either the relative Notify fallback
or EIS. The EIS protocol transport is implemented and contract-tested, PipeWire frame capture and
restore-token/clipboard persistence have executed fixtures, and a real GNOME Wayland run now
certifies owner-approved keyboard+pointer `ConnectToEIS` negotiation plus explicit stop/inactive
lifecycle. Release evidence still requires focused input/coordinate behavior, cancellation of an
in-flight input operation and broader portal-granted desktop coverage.

## App adapters

Blender: create/list/get/transform/delete a named cube, collections and materials, inspect
settings, render a small PNG, save a copy and reopen a trusted fixture with auto-run disabled.
Reject absolute paths, `..`, symlinks, hardlinks and unsupported suffixes. Test host queue
capacity, plugin disable, close during request and Rust-client reconnect. Use a private
workspace; do not use this to establish sandbox safety for untrusted `.blend` files.

Chromium: run the Rust adapter—not just the Python probe—against an explicit trusted local
origin. Verify the private profile is distinct from normal profiles; query/select/fill/click,
navigate/reload, node removal/reinsertion, frame changes, overlay hit-test rejection,
screenshot expiry, denied origins, disabled downloads, target loss and cancellation.
Inspect the preserved detached-node regression. Add quotas before permitting downloads.
Verify all owned processes and private profile/artifacts are cleaned after failure.

## Plugins, filesystem, frontend and release

Execute harmless sandbox canaries that try to read a designated fixture outside the grant,
write to a read-only mount, observe scrubbed env markers, open an ungranted network socket,
exceed output/memory/CPU limits, crash, and hang. Expect denial/child termination with the
broker still usable. These are tests against your own sandbox only. No bypass fallback.
Compare manifest/runtime protocol identity/schema/version and digest behavior.

For scoped files, repeat traversal, intermediate/final symlink, mount, hardlink, directory
rename and concurrent-write tests **through the Rust broker**. Confirm protected runtime,
state/config overlap grants fail. Normal application file I/O is a separate boundary.

MCP: real client initialize/list/describe/execute, structured results, cancellation, bad
arguments, concurrent calls, policy denial and no approval tool. TUI: resize, non-TTY/error
exit, terminal restoration, malicious OSC/bidi/control text and no mutation from filtering.
Installer: install in a clean account, preserve existing files, enable/stop user service,
reject root, uninstall unchanged binaries while retaining user data. Package/test both
architectures and distro targets. Record artifact checksums and actual CI provenance.

Use `verification/manual-result.template.json` for each case. Pending is not PASS.
