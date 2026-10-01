# Faust audio driver

First-party deterministic DSP and synthesis backend for `semwright-audio-domain`.

Semantic synth graphs and curated SFX presets are translated to bounded Faust source.
Arbitrary Faust, arbitrary compiler flags, shell execution and network access are not
part of the agent-facing contract. A production runtime is owner-pinned by digest.

Unsupported effect semantics fail closed rather than being approximated silently.
