# Semantic audio domain

`semwright-audio-domain` is the backend-neutral professional audio model shared by
Semwright audio drivers and asset providers. It is deliberately not an Ardour model,
a Faust AST, a plugin ABI or an AI-generation schema.

The portable model owns:

- projects, sample rates, channels and tempo/meter metadata;
- samples with provenance;
- stems, buses, sends and routing;
- sample- and synth-backed clips;
- synth signal graphs, oscillators, FM, envelopes, filters and gain;
- semantic effect chains: EQ, compressor, limiter, reverb, delay, distortion, filter and gain;
- typed automation targets and sample-frame automation points;
- deterministic SFX primitives and presets;
- analysis results for peak, RMS and optional standards-based integrated LUFS;
- backend-neutral WAV/FLAC render intent.

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

Production rendering uses an owner-pinned runtime manifest under
`/workspace/runtime/runtime.json` and emits new WAV/FLAC artifacts.

## Ardour

`semwright-ardour-audio-driver` uses two complementary official Ardour surfaces.

The live path is fixed-loopback OSC. It observes strips into snapshot-bound refs and
supports transport plus bounded track/bus controls. Mutations that can be re-observed
through Ardour's strip list are verified; numeric controls that the observation surface
cannot echo require explicit acknowledgement and force re-observation.

The deep path uses an owner-pinned `ardourN-lua` / `luasession` executable and a
Semwright-owned fixed Lua adapter. Agent values are positional arguments, never code.
The adapter loads only the mounted `/workspace/project` session and projects routes,
regions and media into the portable audio model. Native routing, sends and plugin
inventories remain explicitly incomplete until certified.

Deep mutation primitives exist internally only for operations implemented by the fixed
adapter. They are not advertised merely because Ardour could theoretically perform them.

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
