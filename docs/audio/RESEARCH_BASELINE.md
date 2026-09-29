# Audio upstream baseline

## Native Faust

The Ubuntu 24.04 development baseline is Faust `2.70.3+ds-1.1build2` (compiler 2.70.3). The narrow GitHub Actions native lane also exercises the supported Ubuntu 22.04 package `2.37.3~ds0-1`; the runtime manifest accepts only these explicitly tested compiler versions. The official interpreter API is used instead of generating an executable during an audio request. See https://faustdoc.grame.fr/manual/embedding/ and `architecture/faust/dsp/interpreter-dsp.h` at Faust tag 2.70.3. The system header is not copied into Semwright. The original helper links the system libfaust and libsndfile; their redistribution obligations remain separate from the permissive core.

The Faust standard library is treated as a versioned input closure rather than an ambient system directory. The owner stages a bounded recursive tree of `.lib` files; the runtime validates strict relative paths, rejects symlinks/untrusted writers, requires `stdfaust.lib`, verifies the exact inventory and SHA-256 of every file, and only then passes that grant to the fixed interpreter.
Faust polyphonic behavior was checked directly against upstream 2.37.3 and 2.70.3 poly-dsp.h. Both versions allocate the first free voice, then steal the oldest release voice, then the oldest playing voice; keyOff resolves a playing voice by pitch rather than MIDI channel. Both versions use a 0.5-second default release-detection window, while setReleaseLength is available only in newer Faust and therefore is not used by Semwright. The semantic instrument tail remains an explicit render-horizon budget rather than being misrepresented as Faust voice-release configuration.

Sample playback does not enable Faust soundfile(), arbitrary imports, or host paths. Semwright snapshots a SHA-256-bound WAV/FLAC from the read-only audio-assets grant, libsndfile decodes it inside the fixed helper, channel mapping is explicit mono-average, resampling is exact-rate-only, and the resulting single input is processed by generated Faust DSP.


## Native Ardour

The researched upstream tag is `8.4`, peeled commit `c35515e43d65bac23c89ae11cfbf2fed8c8f46b6`; Ubuntu 24.04 offers package `1:8.4.0+ds1-2ubuntu8`. Runtime acceptance is still pending.

Official source inspected: `session_utils/new_empty_session.cc`, `session_utils/export.cc`, `luasession/luasession.cc`, `luasession/wscript` and the relevant Lua bindings under https://github.com/Ardour/ardour/tree/8.4 . `ardour8-new_empty_session` exposes sample-rate selection but no master-channel option; managed creation is deliberately constrained to stereo and accepts the result only after a clean native reopen reports a two-channel master. The Lua CLI also uses the None (Dummy) backend for headless session work. The export utility renders the verified session range through master-bus outputs. Neither path requires synthesizing an Ardour session XML document. Utility exit status alone is not accepted as success: the driver requires fresh native state/artifact readback.

Ardour utilities and libraries are GPL-2.0-or-later components. No GPL implementation source is copied into the permissive audio-domain. System runtime loading, plugin/script activation and native-library search paths require their own audited grants and acceptance.

Ardour 8.4 Lua binding source was also checked for Session:add_internal_sends, Session:new_route_group, Session:remove_route_group, RouteGroup:add/remove, Playlist:split_region, LuaAPI:new_plugin, LuaAPI:set_plugin_insert_param, LuaAPI:plugin_automation, AutomationList/ControlList, PluginInsert, and MidiModel note-diff commands. Ardour own send_to_bus.lua, s_pluginutils.lua, s_plugin_automation.lua, addscopes.lua, and scope.lua examples corroborate those bindings. ACE Inline Scope is selected as the first-party acceptance plugin through a runtime allowlist; agent input never supplies a raw native plugin name/type. The same source inspection found MIDI note editing for existing MidiRegion/MidiModel, but no headless source/region factory equivalent to the Editor import/create path, so from-empty MIDI region authoring remains a documented upstream restriction.


## Observation discipline

Legacy OSC strip numbers are not durable identities. Native session readback and save-as must be verified separately from transport control. The fixed Lua CLI is not the Editor; editor-only operations must not be invented.

References: https://manual.ardour.org/lua-scripting/ ; https://manual.ardour.org/using-control-surfaces/controlling-ardour-with-osc/ ; https://manual.ardour.org/using-control-surfaces/mcp-http/ ; https://manual.ardour.org/using-control-surfaces/websockets-server/ . Experimental HTTP/WebSocket documentation does not establish presence or safe binding in the pinned 8.4 binary. No listener is enabled by this research.


## Acoustic analysis

The production contract separates streaming PCM statistics from perceptual loudness. The native Linux lane pins Ubuntu 24.04 `libebur128-dev=1.2.6-1build1` and the helper rejects any loaded libebur128 version other than 1.2.6. Input is an owner-granted WAV/FLAC artifact bound to an expected SHA-256; the driver snapshots bytes before analysis and never treats RMS as LUFS.

The fixed helper reports integrated, momentary and short-term loudness, LRA and true peak only when its required evidence window is present. Silence, too-short material and non-finite samples produce explicit unknown/null fields. WAV results are cross-checked through Semwright's independent bounded RIFF decoder for sample count, sample peak, RMS, non-finite values and silence ranges.

Reference algorithm sources are ITU-R BS.1770 and EBU R128 as named in the master contract. The implementation records the concrete meter/version rather than claiming one delivery target is universal.
