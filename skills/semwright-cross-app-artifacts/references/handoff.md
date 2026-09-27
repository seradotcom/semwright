# Broker-mediated handoff

The current `artifact.handoff` implementation copies bounded bytes between explicitly configured filesystem roots, computes SHA-256, checks an optional expected digest before writing, and performs an atomic destination write.

Use logical grant names plus relative source/destination paths from the current schema. Never replace them with arbitrary absolute paths.

If the producer returns a provider-owned artifact token, use that provider's artifact read/release capabilities unless it explicitly materializes a file under a granted root. Token identity and filesystem identity are distinct contracts.
