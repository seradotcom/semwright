# Audio acceptance mapping

This maps AU requirements to implementation and evidence; it is not an automatic PASS declaration.

| IDs | Implementation | Evidence |
|---|---|---|
| AU01-AU04 | strict neutral model, 54 classified operations, ordered stems/buses, groups, typed sends, clip move/trim/slip/split/fades, markers/ranges, tempo map, MIDI phrases, automation and shared cues | portable unit/property/time tests plus synthetic AV fixture; native only where advertised |
| AU05-AU06 | typed Faust graph and parameterized seeded SFX | faust-native compiler/render/PCM |
| AU07-AU08 | Ardour OSC + fixed-Lua deep create/edit/save-as/reopen/export, native sends, groups, clip split and allowlisted plugin parameter/automation operations | ardour-native |
| AU09 | bounded plugin identity/parameter inventory plus owner-allowlisted insert/remove/parameter/automation; arbitrary loading excluded; opaque native state remains preserved but not semantically editable | coverage/deny evidence + ardour-native |
| AU10 | shared Composition authoring, ducking and repair lifecycle | audio-authoring lifecycle + combined AV gate |
| AU11-AU12 | PCM stats plus libebur128 loudness/true peak/LRA and defined silence/short/nonfinite/corrupt states | analysis-native + portable parser cases |
| AU13-AU16 | declared DSP semantics, platform/runtime matrix, stale handling, sandbox/resources | native/portable/security gates |
| AU17 | semwright-audio-production Skill and requirements | Skill validation |
| AU18 | runtime templates and real SWDP development packages | package lane |

Items marked pending or upstream_restricted in coverage JSON remain open for that feature and cannot be converted to PASS by relabeling.
