# Analysis and repair

driver.audio-analysis.artifact.measure snapshots a digest-pinned WAV/FLAC input before using the fixed libebur128 meter. WAV inputs also pass through Semwright's independent bounded RIFF/PCM decoder.

Keep sample peak/RMS separate from integrated, momentary and short-term loudness, LRA and dBTP. The receipt records method/version/window sizes. Silence, too-short material and non-finite input produce null/UNKNOWN where evidence is insufficient.

A gain-only repair must not chase incompatible peak and loudness targets. Return a conflict or propose an explicitly permitted limiter/compressor with a new plan and post-measurement.
