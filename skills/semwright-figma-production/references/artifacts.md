# Figma artifacts

Capabilities such as `driver.figma.export.node`, image export, Motion export, and design-system source exports can produce artifact metadata.

The plugin path holds exported bytes behind bounded artifact tokens/chunk reads and also surfaces artifact metadata through the Driver Protocol job/artifact contract. A token is not an absolute path and should never be passed directly to `artifact.handoff`.

If bytes need to become a cross-application file, first use the Figma driver's documented artifact-read/materialization route that is actually available, then move a real file only through a scoped filesystem workflow. Release provider-owned tokens when they are no longer needed.

Preserve media/semantic type and digest evidence when the capability provides it.
