# semwright-audio-domain

Backend-neutral professional audio semantics for Semwright.

The crate contains the portable project model, transactional edits, sample-frame time,
effects, synthesis graphs, automation, routing, refs, render intent, bounded analysis,
SFX presets, asset-provider provenance, backend contracts and differential conformance.

It intentionally contains no Ardour, Faust, OSC, Lua, shell, GUI-coordinate or
vendor-model types. Concrete backends must report unsupported or lossy semantics rather
than smuggling native state into the portable model.

See [docs/audio-domain.md](../../docs/audio-domain.md).
