# Semantic audio domain

`semwright-audio-domain` is the backend-neutral professional audio model shared by
Semwright audio drivers and asset providers. It is deliberately not an Ardour model,
a Faust AST, a plugin ABI or an AI-generation schema.

The portable model owns:

- projects, delivery/session metadata, sample rates, channels and tempo/meter maps;
- samples with provenance;
- ordered stems/buses, groups, sends, routing and explicit send role/delay;
- sample- and synth-backed clips with move/trim/slip/split/fade semantics;
- timeline markers and named ranges;
- typed MIDI phrases with note/control events and optional semantic synth binding;
- synth signal graphs, oscillators, FM, envelopes, filters and gain;
- semantic effect chains with explicit algorithms/detectors/channel linking: EQ, compressor,
  gate/expander, limiter, reverb, delay, distortion, channel-map, filter and gain;
- typed automation targets, including stem/bus send gain, and sample-frame automation points;
- deterministic SFX primitives and presets;
- analysis results for sample peak/RMS plus optional standards-based loudness/true peak;
- backend-neutral WAV/FLAC render intent with explicit resampling quality and dither policy.

## Backend independence

Concrete backends implement `SemanticAudioProjection<Native>` and publish a
project/version-scoped `BackendContract`. Every shared `AudioOperation` is classified
as safe round-trip, metadata-risk, render-only or unsupported. Support is capability,
not authority; Semwright policy remains above every driver.

`ProjectionReport` carries explicit fidelity:

- `exact`
- `semantically_equivalent`
- `lossy_read_only`

Opaque plugins, missing media, unknown native versions and unprojected routing must be
reported as structured losses. A backend may not claim exact fidelity while hiding loss.

## Faust

`semwright-faust-audio-driver` is the deterministic DSP/render backend. Agent input is
semantic data only. Semwright generates bounded Faust source; callers cannot provide
Faust source, compiler flags, executable paths, shell fragments or environment values.

The v1 compiler handles the deterministic synthesis primitives and SFX presets plus
fidelity-certified mappings for filters, EQ, zero-knee compression, delay, gain and
distortion. Semantics that would require hidden implementation choices fail closed.
For example, limiter attack/lookahead/hold and reverb algorithm/topology are not guessed.

Production rendering reads an owner-granted `faust-libraries` mount containing
`semwright-runtime.json` plus pinned library hashes, and a Driver Host sealed
`faust-interpreter` tool. It emits only new WAV/FLAC artifacts into the explicit
output grant.

## Ardour

`semwright-ardour-audio-driver` uses two complementary official Ardour surfaces.

The live path is fixed-loopback OSC. It observes strips into snapshot-bound refs and
supports transport plus bounded track/bus controls. Mutations that can be re-observed
through Ardour's strip list are verified; numeric controls that the observation surface
cannot echo require explicit acknowledgement and force re-observation.

The deep path uses three owner-pinned Driver Host tools from the Ardour 8.4 runtime:
Lua session, new-session and export. A Semwright-owned fixed Lua adapter receives agent
values only as bounded positional arguments. Separate `ardour-runtime`,
`ardour-project` and `ardour-output` grants keep runtime metadata, managed session
state and artifacts distinct.

Advertised deep mutations are revision-bound and are re-read through the native adapter:
managed session creation/range, track and bus creation, route rename/mute/solo/gain/pan,
clip move/trim/remove, save-as/reopen and WAV export. Destructive remove operations keep
normal confirmation policy. The native snapshot also performs bounded send and plugin
identity enumeration when Ardour exposes a complete list; those observations remain
read-only metadata and never authorize plugin loading. Complete routing topology, writable
plugin state/automation, GUI-only media import and MIDI instrument authoring remain
explicitly incomplete or upstream-restricted.

## Generated/AI assets

AI generation is optional and remains an asset-provider concern.

`AssetProviderDescriptor`, `AssetGenerationRequest` and
`AssetGenerationReceipt` describe generated SFX, music, speech, Foley, ambience or
instrument samples with provider/model/request/content provenance. A generated asset
enters the same `Sample` model as imported or deterministic audio.

An AI provider does not receive execution authority and is never required for semantic
editing, DSP, rendering, analysis or DAW control.

## Evolution

Model v1 is strict and rejects unknown fields. New cross-backend semantics require an
explicit model-version change and conformance evidence. Backend-contract evolution is
versioned separately.

New backends should first project into this domain and declare honest support/fidelity.
Only semantics demonstrated to be meaningfully shared should be promoted into the common
model.
