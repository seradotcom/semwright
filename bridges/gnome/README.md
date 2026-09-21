# GNOME Shell bridge

This optional GJS extension exports a narrow version-1 D-Bus interface: Hello, Snapshot,
and typed Execute. Only the unique sender that currently owns `org.semwright.Broker`
is accepted. Operations are focus/move/resize/close on an exact id/fingerprint. No shell,
`eval`, arbitrary JavaScript, or portal consent bypass is exposed.

Candidate manifest shell versions are 46–49. This list is an implementation target, **not
live verification**. The source was syntax-checked and its shared validation contract
ran in Node; no GNOME Shell process loaded it here.

After review and a successful Rust build, copy this directory to
`$HOME/.local/share/gnome-shell/extensions/semwright@local/`, then explicitly enable it
using your desktop's extension manager. GNOME Wayland generally needs a new login session
to load a newly installed extension; record the actual procedure/version in your test.
Start the real broker so it owns the well-known bus name, then inspect doctor/window.list.
Never replace an installed extension without reviewing its existing files first.

An accepted window operation is a request to the compositor, not proof that the final
geometry/focus has settled. Observe again and compare the result. To remove it, disable
`semwright@local`, log out when required, and remove only the extension directory you
installed. No broker configuration, user project files or portal permissions are removed.
