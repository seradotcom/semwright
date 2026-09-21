# Terminal inspector

After a successful Rust build, run `semwright-inspect` in the same login session as the
broker. `--socket` and `--session-file` can select a private fake session. The terminal UI
uses ratatui/crossterm and the same Unix client as the CLI/MCP frontend.

Panes cover doctor status, windows, apps, bounded accessibility output, policy/capabilities,
metadata audit and plugin status. Use Tab/arrow keys to switch panes, scroll the current
result, `r` to refresh, `/` to filter rendered text, and `q` to exit. Refresh/cancellation
closes the associated connection; it is not permission to retry a mutation. Terminal
control characters are escaped before rendering and terminal mode is restored on exit.

The delivered inspector is **read-only**. It does not invoke targets, approve commands,
read clipboard contents implicitly, provide a shell, or generate/edit executable plugins.
It is not a complete graphical debugger: direct ref drilling and richer selector/workflow
editing remain on the completion list. Terminal interaction itself was not run here.
