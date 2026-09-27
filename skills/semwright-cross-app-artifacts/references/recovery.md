# Recovery

If export fails, do not start handoff.

If a digest check fails, stop before import/rescan and re-export or re-inspect the producer artifact. Do not disable the digest check to make the workflow proceed.

If handoff succeeds but import/rescan fails, keep the copied artifact as evidence unless the user's procedure explicitly asks for cleanup. Inspect consumer state and retry only a documented idempotent ingestion step.

Verify the consumer object or catalog after ingestion; file presence alone is not destination verification.
