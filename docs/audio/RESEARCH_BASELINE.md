# Audio upstream baseline

## Native Faust

The Ubuntu 24.04 development baseline is Faust `2.70.3+ds-1.1build2` (compiler 2.70.3). The narrow GitHub Actions native lane also exercises the supported Ubuntu 22.04 package `2.37.3~ds0-1`; the runtime manifest accepts only these explicitly tested compiler versions. The official interpreter API is used instead of generating an executable during an audio request. See https://faustdoc.grame.fr/manual/embedding/ and `architecture/faust/dsp/interpreter-dsp.h` at Faust tag 2.70.3. The system header is not copied into Semwright. The original helper links the system libfaust and libsndfile; their redistribution obligations remain separate from the permissive core.

## Native Ardour

The researched upstream tag is `8.4`, peeled commit `c35515e43d65bac23c89ae11cfbf2fed8c8f46b6`; Ubuntu 24.04 offers package `1:8.4.0+ds1-2ubuntu8`. Runtime acceptance is still pending.

Official source inspected: `session_utils/new_session.cc`, `session_utils/export.cc`, `luasession/wscript` under https://github.com/Ardour/ardour/tree/8.4 . The new-session utility uses the None (Dummy) backend and the native Session constructor. The export utility uses the native export handler and master-bus outputs. Neither requires synthesizing an Ardour session XML document. Both utilities have error-reporting paths that do not consistently produce a failing exit status; a driver must validate fresh native readback and produced artifacts, not rely on exit zero.

Ardour utilities and libraries are GPL-2.0-or-later components. No GPL implementation source is copied into the permissive audio-domain. System runtime loading, plugin/script activation and native-library search paths require their own audited grants and acceptance.

## Observation discipline

Legacy OSC strip numbers are not durable identities. Native session readback and save-as must be verified separately from transport control. The fixed Lua CLI is not the Editor; editor-only operations must not be invented.

References: https://manual.ardour.org/lua-scripting/ ; https://manual.ardour.org/using-control-surfaces/controlling-ardour-with-osc/ ; https://manual.ardour.org/using-control-surfaces/mcp-http/ ; https://manual.ardour.org/using-control-surfaces/websockets-server/ . Experimental HTTP/WebSocket documentation does not establish presence or safe binding in the pinned 8.4 binary. No listener is enabled by this research.


## Acoustic analysis

The production contract separates streaming PCM statistics from perceptual loudness. The native Linux lane pins Ubuntu 24.04 `libebur128-dev=1.2.6-1build1` and the helper rejects any loaded libebur128 version other than 1.2.6. Input is an owner-granted WAV/FLAC artifact bound to an expected SHA-256; the driver snapshots bytes before analysis and never treats RMS as LUFS.

The fixed helper reports integrated, momentary and short-term loudness, LRA and true peak only when its required evidence window is present. Silence, too-short material and non-finite samples produce explicit unknown/null fields. WAV results are cross-checked through Semwright's independent bounded RIFF decoder for sample count, sample peak, RMS, non-finite values and silence ranges.

Reference algorithm sources are ITU-R BS.1770 and EBU R128 as named in the master contract. The implementation records the concrete meter/version rather than claiming one delivery target is universal.
