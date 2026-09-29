# Ardour sessions

The deep path targets the pinned Ardour 8.4 baseline. It uses fixed Semwright Lua plus sealed luasession, session-create and export utilities inside the Driver Host sandbox.

Workflow: inspect or create managed state -> apply one revision-bound typed edit -> reobserve -> save-as when required -> reopen candidate -> export -> decode and verify artifact. Existing output names fail closed.

The fixed headless Lua surface does not claim Editor-only import operations. Import/relink that requires the GUI remains an explicit upstream-restricted capability until a native safe route is implemented and tested. Destructive route/clip removal retains normal confirmation policy.

Live OSC is useful for transport and observed strip controls, but a sent datagram is not an acknowledgement. Prefer deep native state for persistence claims.
