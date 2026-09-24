# Live Windows 11 acceptance matrix

All rows are `WINDOWS_INTERACTIVE_PENDING` in this source drop because no interactive Windows session was used.

| area | test | required evidence |
|---|---|---|
| UIA fixture | snapshot + invoke + edit + checkbox + radio + slider + combo/list + tree/table | correct semantic roles/actions; bounded snapshot |
| Notepad | read/set text through UIA | no SendInput when Value/Text pattern works |
| Calculator | invoke buttons/read result | semantic pattern path |
| Explorer | read-only browse | no credential/system shell mutation |
| WinUI app | snapshot/patterns | modern provider compatibility |
| classic Win32 | fixture/Notepad | native provider compatibility |
| Chromium | top-level/window + semantic driver ranking | app provider outranks generic UIA where intended |
| integrity | normal -> normal | actions work within policy |
| UIPI | normal -> elevated target | explicit denial; no bypass |
| UAC | prompt/secure desktop | no interaction |
| display | 96/125/150/200%, mixed DPI, negative coordinates | coordinate conversions documented/verified |
| lifecycle | close/recreate, process restart, simulated PID/HWND reuse | old refs become StaleReference |
| clipboard | contention and >4 MiB | bounded retry/ResourceExhausted |
| capture | picker/programmatic target, resize/close/device loss | WGC single frame bounded; no private API |
| session | lock/unlock, sleep/wake | invalidate/reprobe |
| IPC | wrong SID/session, remote client, DACL inspection | denied + impersonation reverted |
| Driver Host | safe fixture child | only after secure-spawn contract lands |
