# Assets and provenance

- Keep asset identity separate from filesystem paths.
- Preserve digest, media type, source/origin, license/permission when known, duration, sample rate and channels.
- A generated or imported asset never grants network, secret or filesystem authority.
- Bind analysis and word/cue timing to the exact asset digest. Replacing bytes makes prior timing and measurement evidence stale.
- Do not attach large audio payloads to JSON. Use bounded artifact/file grants and returned receipts.
