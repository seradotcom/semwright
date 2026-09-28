# Research baseline

This file records technical sources that directly constrain the Composition/Media implementation. It is not evidence that a feature passed in Semwright.

## Repository baselines

A implementation baseline was frozen at be375a12e8afa4d779f9dc0de501b0d4a262a682. Common C0 is 26602e4b25929be869d69ef28fef4dd9713180d7.

Relevant existing product surfaces inspected before/while implementing include the Figma semantic authoring driver/plugin, Motion Canvas managed model/compiler/store/renderer, video-domain, MLT driver, Broker/Provider runtime, Driver SDK/Host, artifacts, Recipes and Skills.

## Upstream contracts used

- Figma Plugin API: native text/layout/bounds and font-dependent text mutation remain application-native concerns.
- Motion Canvas 3.17.2: signals, generators/layout and time evaluation are used by the pinned runtime. High-level cues/authoring do not imply Motion Canvas lacked timing primitives.
- MLT: final assembly/render is an explicit delivery provider step rather than hidden inside Motion authoring.
- GitHub Actions: reruns test the original SHA; code fixes require a new run for the new SHA.

The exact runtime symbols are validated by source/typecheck/native CI rather than inferred solely from current web documentation.

## Implementation decisions derived from those constraints

- exact rational project/media time until explicit frame/sample boundaries;
- no arbitrary TypeScript/eval in authoring data;
- conservative replay for stateful Motion generators;
- native Figma/Motion observations are not replaced with desired state;
- final encoded media is decoded/analyzed independently;
- Broker/Driver Host remains the sole native authority path;
- Skills remain procedural guidance rather than executable principals.

Before changing an upstream pin or relying on a newly documented feature, update this record with the tested version/commit and repeat the affected native evidence.
