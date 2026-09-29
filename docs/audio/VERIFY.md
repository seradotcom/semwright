# Audio verification

All evidence is SHA-bound. A run from another SHA is diagnostic history only.

| Lane | Evidence |
|---|---|
| audio-domain | portable model, authoring lifecycle, schemas/catalog coverage, format/clippy |
| faust-native | pinned Faust translation/interpreter/render and Broker -> Driver Host path |
| analysis-native | sealed libebur128 meter, digest-bound artifact and independent WAV decode |
| ardour-native | real Ardour 8.4 managed session create/edit/save/reopen/export under Dummy backend |
| audio-package | real SWDP creation/inspection and development bundle hashes |
| audio-gate | every selected required lane ran and passed |

A missing prerequisite is a lane failure, not a successful skip.

The Ardour native lane is real application/library execution on a disposable GitHub runner with Dummy audio. It does not certify physical hardware latency, user plugins, a normal GUI profile or arbitrary existing projects.

Current native status remains NATIVE_ACCEPTANCE_PENDING until the final branch SHA has completed required jobs. Update this file only with exact run/job IDs for that SHA.
