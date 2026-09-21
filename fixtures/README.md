# Disposable live-test applications

These fixtures contain no real credentials, file writes or remote services. They provide
an editable name, password-role input, duplicate Save buttons under different groups,
disabled/toggle/value/selection controls, object replacement with the same label, and a
small delayed state update. Do not put a real password into a fixture.

After installing the corresponding optional GUI bindings, run `python3 gtk/fixture.py`
(PyGObject + GTK4) or `python3 qt/fixture.py` (PySide6) in your disposable graphical session.
The files were syntax-checked, **not run** in a GTK/Qt desktop here. Actual accessible roles,
action names and app IDs depend on the toolkit/accessibility bridge; observe them instead
of inventing expected names. The duplicate controls deliberately force disambiguation.

For the browser, serve **only** `fixtures/browser/` on loopback using
`python3 -m http.server --bind 127.0.0.1 --directory fixtures/browser 8000` from the repo root.
Allow that exact origin in the private browser config. The separately executed Python CDP
probe used its own about:blank fixture, not this HTTP fixture; HTTP/origin integration
remains a distinct pending test.

Follow [manual-testing.md](../docs/manual-testing.md) and record actual versions/results.
