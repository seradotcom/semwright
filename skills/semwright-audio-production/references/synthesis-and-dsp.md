# Synthesis and DSP

Use curated SFX presets or typed synth JSON. Generated Faust is inspectable derived source, not an input escape hatch.

Published effect mappings must preserve their declared algorithms/parameters. Do not silently translate an unsupported limiter/reverb/plugin to a merely similar effect. Seeded noise and bounded envelopes make deterministic fixtures reproducible.

Do not claim true-peak limiting from a sample-peak limiter. Avoid arbitrary plugin discovery/loading; plugin code is an execution boundary.
