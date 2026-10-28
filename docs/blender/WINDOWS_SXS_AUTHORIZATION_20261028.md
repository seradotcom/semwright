# Authorization recorded — Windows SxS diagnostics

On 2026-10-28 (America/Mexico_City), the owner explicitly approved the bounded
registryRead experiment proposed in WINDOWS_SXS_AUTHORIZATION_PROPOSAL.md and
similar tests that continue to respect all previously stated protections.

Owner message: "autorizo. y no pares. sigue trabajando. siempre y cuando sigas
respetando todo lo mencionado, te autorizo cuaquier prueba similar. adelante."

This authorizes continued local implementation and bounded first-party tests.
It does not authorize a generic capability list, network, system ACL changes,
owner-token Blender fallback, unrestricted descendants, runtime patching,
third-party Blender MCP, or external publication. Production authority must
remain explicit, typed, scoped, owner-visible and audited.

First execution: add registryRead alone in a test-only Host path; preserve LPAC,
exact capabilities/Job attestation, sealed runtime identity, private environment,
output limits, deadlines and confirmed process collection. Test the CPU activation
fixture first; proceed to pinned Blender --version only if it passes.
Further similar diagnostics remain authorized within those same protections.
No PASS is inferred from this authorization. A05 still requires the complete
broker → isolated Blender → typed mutation → readback → GLB → Godot chain.
