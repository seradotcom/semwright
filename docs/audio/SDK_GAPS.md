# Audio SDK gaps and upstream restrictions

These are explicit gaps, not hidden fallbacks.

- Driver Package v2 can carry data companions but does not install executable companions as sealed tool grants. Audio therefore ships driver SWDPs plus runtime prerequisites/provisioning metadata.
- Ardour 8.4 `new_empty_session` does not expose master-channel selection; managed creation is explicitly stereo rather than inventing an unsupported flag. Headless Lua also does not expose the Editor-only import path used by Ardour's shipped `s_import_files.lua` example, so deep managed media import/relink is not advertised.
- Ardour snapshot v1 now enumerates bounded internal sends, route groups, plugin identities, parameter descriptors/current values and automation-point counts. The fixed adapter adds revision-bound internal-send edits, route-group edits, clip split, and owner-allowlisted plugin insert/remove/parameter/automation operations. Complete main-output routing topology, arbitrary plugin state-blob semantics, route reorder, region fade readback and native sidechain/delayed-feedback semantics remain incomplete; unknown/unbounded enumeration still degrades projection fidelity instead of being guessed.
- MIDI phrase/instrument authoring is modeled at the portable layer. Ardour 8.4 exposes MidiModel note-diff editing for existing MIDI regions, but the pinned headless Lua surface does not expose a certified factory for creating the initial MIDI source/region; full from-empty MIDI authoring therefore remains unadvertised rather than synthesizing session XML.
- Ardour experimental MCP HTTP/WebSocket surfaces are not selected for 8.4; no listener is enabled as a workaround.
- Audio analysis currently accepts explicit mono/stereo layouts for libebur128. Wider channel layouts require a channel-role contract before they can be certified.
- Faust sample playback is limited to one semantic SamplePlayer per monophonic graph, owner-granted RelativePath WAV/FLAC assets, exact sample rate, explicit mono-average input mapping, and hash-pinned immutable snapshots; Artifact-backed Sample sources remain unsupported until an explicit artifact materializer exists. Polyphonic Faust is limited to 1..64 voices and the upstream allocator policy pinned in Faust 2.37.3/2.70.3; same-pitch note overlap is rejected because upstream keyOff resolves by pitch, and only global CC120/123 on channel 0 is mapped.

These restrictions remain visible in ARDOUR_SURFACE_COVERAGE.json and FAUST_SURFACE_COVERAGE.json.
