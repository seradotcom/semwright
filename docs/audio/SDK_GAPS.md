# Audio SDK gaps and upstream restrictions

These are explicit gaps, not hidden fallbacks.

- Driver Package v2 can carry data companions but does not install executable companions as sealed tool grants. Audio therefore ships driver SWDPs plus runtime prerequisites/provisioning metadata.
- Ardour 8.4 `new_empty_session` does not expose master-channel selection; managed creation is explicitly stereo rather than inventing an unsupported flag. Headless Lua also does not expose the Editor-only import path used by Ardour's shipped `s_import_files.lua` example, so deep managed media import/relink is not advertised.
- Ardour snapshot v1 can enumerate bounded internal sends and plugin identities through the pinned 8.4 Lua bindings, but complete routing topology and writable plugin state/automation remain incomplete. Unknown/unbounded enumeration degrades projection fidelity instead of being guessed.
- MIDI phrase/instrument authoring is modeled at the portable layer but not advertised through Ardour until a native headless route is verified.
- Ardour experimental MCP HTTP/WebSocket surfaces are not selected for 8.4; no listener is enabled as a workaround.
- Audio analysis currently accepts explicit mono/stereo layouts for libebur128. Wider channel layouts require a channel-role contract before they can be certified.
- Faust soundfile/sample playback and native polyphonic/MIDI voice allocation remain unadvertised until asset grants, memory bounds and runtime semantics are verified.

These restrictions remain visible in ARDOUR_SURFACE_COVERAGE.json and FAUST_SURFACE_COVERAGE.json.
