# Identity and revisions

Native cooperation distinguishes: durable application resource identity, application generation/incarnation, opaque application revision, Driver Host provider/session generation, and ephemeral Broker native reference. They are not interchangeable.

`RevisionToken` is a bounded opaque string. Hashes, database counters and native transaction IDs are not truncated to `u64`. `version_fingerprint` binds resource + generation + the full opaque revision.

`NativeDriver` may use a bounded serial in `NativeTarget.revision` for the live Host session, but retains and checks the complete application `ResourceVersion`. Old refs are valid only for the driver instance/session that emitted them.

Restart/rebind can preserve durable logical application identity while rotating provider/application generation and invalidating old refs. Delete/recreate or restore may rotate generation. Fork creates a fresh identity and never copies Broker refs or grants.

Graph durable locators are re-resolution hints, not authority. `graph_adapter::native_locator` carries provider/resource/stable ID without embedding an ephemeral Broker ref.
