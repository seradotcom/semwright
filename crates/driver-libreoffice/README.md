# LibreOffice driver

This first-party driver demonstrates a deep non-browser, non-Blender integration using
LibreOffice's native UNO object model.

The driver is an ELF hosted by Semwright's App Driver sandbox. It starts its own headless
LibreOffice process inside the same bubblewrap/Landlock sandbox and talks to it over a local
UNO pipe. A fixed, repository-owned Python bridge translates typed driver operations to
PyUNO. Agent input is JSON data only; there is no arbitrary Python, macro or UNO method surface.

The manifest must grant a filesystem root named workspace. Inside the sandbox it appears at
/workspace/workspace. All document paths passed to capabilities are relative to that root.

Initial capabilities cover runtime status, Writer create/read, Calc create/get/set and PDF
export. This is intentionally narrower than the full UNO API; deeper introspection and object
references should be added through typed, versioned capability expansion rather than code eval.
