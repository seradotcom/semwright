# Synthesis and DSP

Use curated SFX presets or typed synth JSON. Generated Faust is inspectable derived source, not an input escape hatch.

Published effect mappings must preserve their declared algorithms/parameters. Do not silently translate an unsupported limiter/reverb/plugin to a merely similar effect. Seeded noise and bounded envelopes make deterministic fixtures reproducible.

Do not claim true-peak limiting from a sample-peak limiter. Avoid arbitrary plugin discovery/loading; plugin code is an execution boundary.

## Sample-backed graphs

Use driver.faust-audio.sample.render only for a semantic Sample with RelativePath under the owner-granted audio-assets root and an expected SHA-256. The runtime snapshots bytes before decode, requires exact sample rate, maps multichannel source audio to one Faust input by arithmetic mono-average, and makes loop versus EOF-silence explicit. Do not infer resampling or artifact materialization.

## Polyphonic instruments

Use driver.faust-audio.instrument.render for a semantic Synth + MidiPhrase. Polyphony is 1..64 and follows the pinned Faust allocator: first free voice, then oldest release, then oldest playing voice. Same-pitch overlapping notes are rejected because the upstream key-off API resolves by pitch, not note identity/channel. Only global CC 120/123 on channel 0 is accepted. The render range must include the final note-off plus the declared semantic tail.
