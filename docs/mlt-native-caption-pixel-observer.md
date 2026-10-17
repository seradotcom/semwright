# Native MLT read-only caption-pixel conformance — bounded v1

**State: candidate implementation awaiting Linux Host CI and actual-media acceptance.**
Tracks [issue #271](https://github.com/seradotcom/semwright/issues/271).

This adds `driver.mlt-video.av.caption_pixels.verify` as a **read-only**,
idempotent Driver Host action. The tool owns an original output-root FFV1
Matroska video and the exact resulting H.264/AAC MP4. Both must be regular,
single-link, owner-root-relative media, freshly read through Linux openat2
and staged to sealed private scratch with exact SHA-256 and byte length.

Only Semwright Host ToolPath-owned FFmpeg and ffprobe perform frame
decoding/probing. The input does not permit a shell, process path,
filtergraph, arbitrary observation frames, host filesystem path, decoder
preset, output filename or second compositor.

## Fail-closed observed scope

Bounded early protocol: 2–900 frames, up to 1920×1080, one to eight
author-authenticated half-open caption cues, each with a preceding inactive
frame, at most 256 MiB per original file. Both streams must match the exact
rational output frame count and color geometry. FFV1 must contain only
video, while the MP4 must contain H.264 and 48 kHz stereo AAC. Source and
master names must be distinct output-granted relative files.

At the start and last frame of each cue, compare native FFV1 before vs
active RGB in a lower safe-region mask, then compare the same pixels in
the actual H.264 decoding. The source control area above the caption must
be stationary, so unrelated video motion cannot masquerade as glyph
evidence. When the cue ends before the video, separately verify the
inactive original and lossy master returned to inactive pixels.

The first bounded variant **rejects** moving backgrounds, cross-scene
transitions and the inability to sample a cue from an inactive prior frame;
it is not a general scene understanding or OCR capability. It does not
prove every interior frame of a long cue, human legibility, authentic
speech content/timing, compliance, consent or publication rights. It
cannot be used to bypass an existing owner-canonical revision check.

The output is a compact, closed JSON `PASS` with two original media
SHA-256 digests, frame count, sampled frame numbers, caption mask pixel
counts and numeric lossy/inactive comparison errors. No pixels,
transcripts, speaker identifiers, private file paths, job references
or native process logs cross the API boundary. Unknown/mismatched media
return errors; they are **never** silently upgraded to PASS.

## Tests and admission

The same Driver Host real-media acceptance constructs a disposable
six-frame 160×90 synthetic source with one short originally authored
glyph-shaped region, encodes source PNG frames to FFV1 through the
pinned driver, creates a matching 48 kHz stereo WAV for native MLT
H.264/AAC, then runs the new sealed read-only action against the
original output files. It also rejects forged digests, traversal and
invalid cue intervals. Pure Rust negative tests cover missing glyphs,
dropped lossy caption mask, changing backgrounds and malformed frames.

The catalog descriptor/digest/snapshot is regenerated with the existing
Semwright generator, not hand-written or made digest-optional.

**Downstream:** Motionwright #240 remains review-only until a separate
PR pins an upstream **merged and CI-passing** observer and proves a
production owner-local result. Synthetic CI samples do not mint
`DeliverLocal` consent or release approvals.
