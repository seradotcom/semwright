# Audio security delta

This work expands parser, native-tool, media and DAW surfaces. It does not close R16 and does not create a second authority path.

## Authority and execution

Every advertised driver capability still requires its driver scope through the Broker. Skills, plans, manifests, receipts, hashes and support classifications are data, not permission. Destructive Ardour route/clip/send/group/plugin removal keeps the normal sensitive-operation confirmation boundary.

Faust and analysis use Driver Host digest-pinned tool mounts. Ardour deep execution uses fixed tool names and a fixed Semwright Lua adapter; agent values are validated argv. Plugin loading is additionally restricted to owner-pinned runtime IDs that resolve to fixed native names/types; parameter writes require the observed plugin unique identity plus parameter label/index. There is no public arbitrary Faust source, compiler flags, Lua source, raw OSC, plugin path/type, shell, remote host or executable field.

## Filesystem and media

Production paths resolve named owner grants. Output names are bounded relative names and existing artifacts are not overwritten. Analysis binds an expected SHA-256, snapshots input bytes before metering and rejects unsafe file shapes. WAV parsing has explicit byte/frame/chunk budgets and rejects truncation and inconsistent alignment. Faust sample playback uses a separate read-only audio-assets grant; only normalized relative WAV/FLAC paths are accepted, every path component is rejected if symlinked, bytes are copied to an immutable private scratch file, and the caller must bind the expected SHA-256 before libsndfile/Faust sees the sample.

Ardour runtime, project and output roots are separate grants. Save-as reopens the candidate and reobserves the protected source. Export reobserves source state and decodes the resulting WAV because Ardour 8.4 utility exit zero alone is not trusted as proof.

## Native boundary

A sandboxed driver does not imply a user's normal Ardour process is sandboxed. Native CI uses disposable runner state, the Dummy backend and only an explicitly allowlisted first-party Ardour Lua processor for plugin conformance. No microphone, physical speaker, external streaming, paid service or user plugin directory is required. Experimental Ardour HTTP/WebSocket surfaces are not enabled.

## Residual release review

Native libfaust, libebur128, libsndfile and Ardour remain TCB components. Save/reopen proves observed persistence, not crash consistency. OSC continuity differs from offline deep sessions. R16 must re-evaluate native-tool materialization, hostile media limits, runtime provenance and the managed Ardour boundary.
