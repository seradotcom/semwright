# Audio runtime and license provenance

This file records engineering provenance, not legal advice or a relicensing decision.

| Component | Selected use | Distribution treatment |
|---|---|---|
| Semwright audio Rust/Lua/C++ source | product code and fixed adapters/helpers | repository MIT OR Apache-2.0 unless a file states otherwise |
| Faust compiler/libfaust | system runtime used to compile/interpret generated semantic DSP | external pinned system prerequisite; not copied into SWDP |
| Faust standard libraries | owner/runtime-provided .lib files hashed in runtime manifest | external runtime prerequisite; review actual library/file licenses before redistribution |
| libsndfile | WAV/FLAC I/O used by fixed helper and system/runtime | external system prerequisite in CI/deployment |
| libebur128 1.2.6 | standards-based loudness/true-peak meter | external pinned system prerequisite; not copied into SWDP |
| Ardour 8.4 utilities/libraries | managed native session create/Lua/export | GPL-2.0-or-later upstream runtime, external system prerequisite; no Ardour source copied into permissive core |

The audio package lane creates SWDPs containing Semwright driver binaries plus small runtime JSON data companions. It does not bundle Ardour, libfaust, Faust libraries, libsndfile or libebur128.

The Semwright fixed Faust interpreter helper and audio-meter helper are built as CI prerequisites for native evidence. They are not currently installed by SWDP because Driver Package v2 data companions do not establish executable sealed-tool grants. Public redistribution of helper binaries linked to native libraries requires the normal dependency/license review for the exact build inputs.

Ardour source was consulted at pinned tag 8.4 for API/utility semantics; implementation here calls the installed runtime and does not copy GPL implementation code.

See RESEARCH_BASELINE.md for versions/source references and PACKAGING.md for runtime provisioning boundaries.
