# LibreOffice driver

This first-party driver demonstrates a deep non-browser, non-Blender integration using
LibreOffice's native UNO object model.

The driver is an ELF hosted by Semwright's App Driver sandbox. Production execution uses
Driver Protocol v8: Driver Host starts a session-bound runtime runner, and that runner owns the
headless LibreOffice + PyUNO lifetime inside the nested tool sandbox. The application driver
does not resolve or spawn Python, soffice or a system shell. A fixed, repository-owned Python
bridge translates typed driver operations to PyUNO. Agent input is JSON data only; there is no
arbitrary Python, macro or UNO method surface.

The Host contract grants the runner a writable workspace, a read-only executable LibreOffice
runtime, sealed `soffice.bin` and Python dependencies, and only the declared read-only
`/etc/libreoffice` and font configuration. Document paths remain relative to the workspace
and are materialized by the runner after containment checks.

Initial capabilities cover runtime status, Writer create/read, Calc create/get/set and PDF
export. This is intentionally narrower than the full UNO API; deeper introspection and object
references should be added through typed, versioned capability expansion rather than code eval.
