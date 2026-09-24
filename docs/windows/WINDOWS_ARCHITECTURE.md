# Windows architecture

The architecture remains `portable core -> platform boundary -> platform-windows`. `platform-windows` owns semantic Windows behavior and the UIA actor. `platform-windows-sys` owns Win32/WinRT/COM-adjacent system primitives. No `HWND`, `HANDLE`, SID, UIA COM interface or GraphicsCaptureItem is added to portable protocol types.

UI Automation is confined to a dedicated OS thread because the current `uiautomation` wrappers for UIA elements/patterns are not Send/Sync. Tokio-facing backend calls cross the thread through bounded request/reply messages; no COM object crosses that boundary. Snapshot traversal uses Control View and budgets: 2,000 nodes, depth 32, 256 children per node, 4 KiB strings and 1.5 seconds.

Refs are opaque Semwright `NativeTarget`s. UIA identity combines PID, process creation time, RuntimeId, AutomationId, ControlType and FrameworkId. Window refs keep HWND only in a private backend registry and combine it with PID + process creation time + title; every side effect re-enumerates/revalidates. PID/HWND reuse therefore becomes stale instead of silently retargeting.

Synthetic input is below semantic patterns in priority. Pointer/typing calls require a target and revalidate focus immediately before SendInput. Capture uses documented Windows.Graphics.Capture target acquisition; frame-pool/D3D11 readback is not advertised yet.
