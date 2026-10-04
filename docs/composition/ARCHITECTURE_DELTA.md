# Architecture delta

Frozen integration baseline: be375a12e8afa4d779f9dc0de501b0d4a262a682. The prior inspection was 241000c268d1bf1dc29d4e91a913097ac0d020cb. Git diff confirmed no changes between those revisions in Figma, Motion Canvas, video-domain, Skills, Broker, Driver SDK or workspace Cargo.toml. New Windows and unrelated work is retained intact.

Existing: Figma's typed spec, native plugin authoring/measurement, authenticated bridge; managed Motion Canvas compiler/store/renderer; portable video-domain; MLT; Broker/Driver Host; Skills.

Extracted/new common mechanics: typed plan/base/profile/evidence envelopes; strict canonical codec; server-owned plan binding; cumulative budgets; pure bounded controller. New media-time reuses video-domain frame rates, adds checked exact rational conversion and cue/time maps.

Adapters must consume these in production, not merely import them in tests. Four source files survived the earlier isolated pass; unexported supporting crates did not survive. C0 is reconstructed here, not falsely attributed to a recovered ZIP or earlier commit. The old report's ten Node tests do not certify this Rust implementation.

Open integration scope: Figma native regression, Motion authoring/measurement/repair, audio-subsystem consumption, AV final artifact verification, native acceptance and packaging. No ready-for-demo or R16 closure is implied by C0.
