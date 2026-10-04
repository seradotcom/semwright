# Effect Conformance research baseline — refreshed 2026-09-30

## Repository evidence
Frozen base `b736d41b61c4a4146c9e75c16796e251b025e69f`; Composition C0 `26602e4b25929be869d69ef28fef4dd9713180d7`; Project Graph P0 `6ee52b428310370d3ad438a13964086a63f48367`; E0 `dd6d22d6ec6c7c5ef378da58ed75ca18b25ba5ff`.
Composition defines reports, vault and canonicalization. Effect Conformance adds effect predicates, evidence/coverage normalization, enumeration audits and driver-quality evaluation without replacing those types.
Godot #171 scene-only persistence/readback and the fixed Blender Commands/GLB path are treated as conformance subjects, not inherited proof.

## Official sources used for design
Godot 4.7 PackedScene documentation: `PackedScene.pack` serializes owned scene nodes; The conformance layer therefore checks fresh reopen and external-resource sentinels rather than equating save ACK with persistence.
Godot 4.7 ResourceSaver documentation: save flags and resource handling motivated explicit scene-only/external-resource distinction.
Blender 4.5 API: glTF export `use_selection` and bounded native save/open behavior motivated decoded GLB membership plus fresh-process reopen.
GitHub Actions/CLI documentation: reruns preserve the original SHA, so every code fix used a new commit/run rather than reusing an old PASS.

## Runtime pins and observed evidence
Godot archive pin: 4.7.2-stable; CI records extracted binary SHA-256 `8d106cbe6144c2dc7e881d61d2429c1a8a76e6b22ef48bd5e48dcf934953f71e`.
Blender runtime: 4.5.14 LTS; CI records binary SHA-256 `050c02562f81fe80ba616a80198fa02d381e60f8b61b8d39add881f4bca0d7d8`.
Rust: 1.98.1. Effect Conformance release 36942492444 is exact to implementation SHA `d2cfd86a2ee064aa5de8f0a8944319edf6dbb060`; this SHA adds only the explicit Forbidden-obligation regression over the prior production implementation.
Official documentation informs expected semantics; only the exact-SHA executable evidence above proves this implementation exercised them.

No dependency upgrades or copied third-party implementation code were introduced by F. Workspace cargo-audit/cargo-deny and packaging/supply-chain gates for the tested SHA are recorded in RELEASE_IMPACT.md.
