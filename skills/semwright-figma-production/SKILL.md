---
name: semwright-figma-production
description: Use when an agent must inspect, edit, export, or verify a Figma document through Semwright's first-party Figma driver and its authenticated official Plugin API/REST routes.
---

# Figma production

Use Figma through the live `driver.figma.*` capability catalog. Do not recreate the driver surface from memory.

## Procedure

1. Search Figma capabilities for the intent; describe the exact operation before calling it.
2. Inspect document status, search semantic nodes, or fetch a specific node before mutation. Reuse only current provider refs.
3. Prefer the narrow semantic capability that matches the requested change. Do not substitute arbitrary JavaScript, CDP, app patching, or coordinate automation.
4. After a mutation, re-inspect the affected object or call an explicit verification capability when available.
5. For exports, use the driver's export capability and preserve its returned artifact metadata. Driver artifact tokens are bounded provider-owned handles, not filesystem paths.
6. Read exported bytes only through the driver's artifact contract when needed; release tokenized artifacts when the workflow is finished.
7. Treat cloud operations as separately available: owner-provisioned credentials are never capability arguments, and an unavailable cloud route is not permission to bypass the transport.

Editor type, plugin manifest permissions, plan/team access, Motion Beta, document revision, and session generation can affect availability. Fail closed and re-inspect instead of guessing.

Read [inspection-and-mutation](references/inspection-and-mutation.md), [artifacts](references/artifacts.md), and [limitations](references/limitations.md) only when relevant.
