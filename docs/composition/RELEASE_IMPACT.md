# Release impact — Composition / Media / AV

Status: development integration. No release/tag/publication is authorized by this work.

## Source additions

The Composition/AV branch adds or extends:

- semantic-composition;
- media-time;
- motion-authoring;
- AV coordination contracts;
- Figma Composition production binding;
- Motion Canvas high-level authoring and renderer observations;
- video production Skill;
- targeted Composition/driver diagnostics;
- benchmark and production handoff documentation.

Audio-domain, Faust, Ardour, audio-authoring and the audio production Skill remain owned by the audio subsystem and are not imported into the AV integration until an AUDIO_READY_FOR_INTEGRATION revision is published.

## Compatibility

Figma retains the existing eight public Composition capability names and domain payloads. Internal authorization/budget handling is stricter; a caller that previously relied on a client-recomputed plan digest or over-budget repair now fails closed. The private in-memory `BeginPermit` also becomes incarnation-bound: permits minted before revoke/expiry/root recreation, or by another PlanVault instance, are rejected without changing any public wire schema.

Legacy semwright-motion.json projects keep the old generated-source shape when no authoring binding exists. New authoring metadata and rational fps denominator fields are optional on the legacy model. High-level authoring projects intentionally reject direct edits to the derived projection.

The new crates are development-version workspace packages. Their wire formats are versioned but not a stable public release promise until the combined candidate passes its final gates.

## Runtime and packaging consequences

Motion authoring uses the already pinned Motion Canvas runtime/browser and adds one fixed first-party runtime source file plus observation receipts. It does not add arbitrary npm dependencies or network requirements.

The video production Skill must be included wherever first-party Skills are packaged and must receive the same static/reference validation as existing Skills.

AV final delivery requires the existing MLT/video-domain path plus the public audio provider delivered by B. No hidden ffmpeg/shell fallback is introduced by the AV contract.

Development packages must record source SHA, Cargo.lock/runtime lock digests, provider manifests and any native runtime version used by evidence.

## Evidence invalidated by later integration

Any Composition-only evidence becomes insufficient for the combined candidate when:

- the audio-ready revision is merged;
- C0/C1 contracts change;
- current main is reconciled;
- Cargo.lock changes;
- a runtime/manifest/tool hash changes;
- a descriptor/schema changes.

The candidate must rebuild/retest affected paths on one exact combined SHA. Green runs from separate branches cannot be assembled into a final certification.

## Release blockers

This historical checkpoint did not close or weaken the release blockers that were open at the time; R16 was still open at this point. The combined candidate also had to re-run security/supply-chain/package gates affected by the additional crates, Skill and generated runtime. Current review and release-gate status is recorded in [VERIFY.md](../../VERIFY.md) and [RELEASE_BLOCKERS.md](../../RELEASE_BLOCKERS.md).

READY_FOR_DEMO_PRODUCTION is a product-development gate for the future demo workflow, not a public release declaration.
