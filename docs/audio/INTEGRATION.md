# Audio integration handoff

Role B owns the portable audio domain, audio authoring profile, Faust backend, native acoustic analysis and Ardour backend. A owns the shared Composition/media-time contracts and the combined AV candidate.

## Contract consumed

The branch integrated Composition C0 26602e4b25929be869d69ef28fef4dd9713180d7 by normal Git integration. Audio uses semwright-semantic-composition for ownership, base-state binding, typed plans, findings, verification, repair lifecycle and convergence budgets. It uses semwright-media-time for exact cue/time exchange while retaining sample frames as the native audio unit.

Do not replace A's kernel, media clock, Finding, VerificationReport, PlanVault or authority model with audio-local equivalents.

## Production surfaces

- semwright-audio-domain: neutral project/edit/routing/DSP/automation/render/analysis contracts, including ordered stems/buses, groups, markers/ranges, tempo changes and typed MIDI phrases.
- semwright-audio-authoring: AudioIntent -> plan -> apply -> measure -> validate -> repair -> reverify.
- semwright-faust-audio-driver: typed synthesis/SFX to generated Faust and sealed interpreter render.
- semwright-audio-analysis-driver: digest-bound WAV/FLAC analysis with fixed libebur128 and independent WAV statistics.
- semwright-ardour-audio-driver: official loopback OSC plus fixed-Lua deep sessions and pinned Ardour 8.4 create/export utilities.
- semwright-audio-production Skill: capability discovery, safe authoring, verification and recovery.

## AV handoff

A should consume audio artifacts by digest plus sample rate, channels and frame count, never by assuming a private pathname. Audio cues use the shared media-time contract. The checked-in synthetic `fixtures/audio/av-technical-fixture.json` exercises cue resolution, exact 44.1→48 kHz duration mapping, crossfade frame boundaries and 30000/1001 video-to-audio time mapping without user media. A visual-only change may reuse audio only when the audio dependency set is unchanged.

Faust starts cold per render. Ardour is session-stateful: save/reopen and source-revision checks are explicit. Native export verification does not make DAW edits transactionally atomic with video edits.

Headless Ardour 8.4 does not claim Editor-only import/relink, complete plugin state or full routing projection. Those remain explicit in ARDOUR_SURFACE_COVERAGE.json. Exact test/run evidence belongs in VERIFY.md and must be regenerated on A's combined SHA.
