# Snapshots, workspaces and private publication

These are optional application capabilities exposed as real registered operation descriptors, not universal flags. An application may omit all of them.

The inventory reference implements snapshot capture, fork to fresh logical identity, restore with generation rotation, owner close of an unused workspace, and private publication into an app-owned destination.

Fork never inherits Broker refs, secrets or caller grants. Restore does not rewind consumed request epochs or authorization history.

## Private publication

`driver.native-inventory.publish-private` is the concrete private-publication profile in the inventory reference application. The application transaction checks current destination revision/CAS, reviewed snapshot identity, exact candidate SHA-256, current application access and durable request identity.

A concurrent manual edit after preparation makes the destination base stale. A mismatched candidate digest is a conflict. The operation publishes only into an application destination configured private; it does not publish to the Internet, an app store, Semwright Platform or any external registry.

Platform/Teams/Publish may orchestrate this primitive later without redefining its transaction contract.
