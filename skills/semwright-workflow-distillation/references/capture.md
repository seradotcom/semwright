# Capture

Recording wraps normal Semwright executions; it does not create a privileged execution path.

Use a specific workflow name and intent. Set `capture_values` only when concrete arguments/results are necessary to derive parameters. When capture is disabled, stored values are sanitized/redacted. Secret-access steps are not recorded as raw captured values even when capture was requested.

Stop a trace as failed when the intended workflow did not complete. Failed or invalid traces are evidence, not promotion candidates.

Do not record private application content by default. Prefer structural operations, refs, digests, and deliberately chosen parameters.
