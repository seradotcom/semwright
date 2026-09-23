# Capability catalog

`semwright capabilities search "material" --provider blender-native --risk read_only`
returns a compact, deterministic catalog page. Quoted phrases are supported. Exact IDs
rank first; aliases, identifier tokens, tags and descriptions have fixed integer weights.
Ties use the capability ID, not hash-map order. No model, embeddings or network search
is required. Filters include provider, application, category, risk, tags, object types
and route availability. Limits are 1–100 items; revision-bound pagination rejects stale
catalog cursors rather than silently mixing provider generations.

`semwright capabilities describe blender.material.create` adds full input/output schemas.
`semwright capabilities execute <id> --args-json '{...}'` uses the existing broker
execution path, exactly like `semwright execute`. There is no independent permission
system in discovery. MCP keeps all eight original gateway tools and adds only
`capabilities_search` and `capabilities_describe`; the total is ten, not one tool per
application command. Original `commands search/describe` remain compatible.

Every entry includes source kind, provider, version, deterministic descriptor SHA-256,
application identity, tags and an explicit untrusted-metadata label. Plugin metadata is
untrusted. Trusted host registration can add driver, recipe or external-MCP sources to
the registry, but imports cannot overwrite built-ins, remove scopes, claim another
backend, or convert descriptions into policy. Native reference markers are interpreted
only for providers that explicitly support that internal contract, not arbitrary plugin
results. Source metadata is distinct from the selected runtime backend in execution evidence.

Availability means a matching **operation route probe**, not permission, human consent,
or an assertion that every possible target/context is valid. Target identity, focus,
app scopes and consent are checked again immediately before invocation. Unknown operation
probe mappings fail closed. A Screenshot portal does not make RemoteDesktop input
available. NetworkManager, notifications and user-systemd are probed independently;
`/proc` availability no longer enables unrelated D-Bus operations. Browser launch and
running-browser operations have separate states. Mutation invalidates the short probe cache.

Input and output JSON Schemas are validated once at registration and compiled validators
are shared across requests. Remote schema references and conflicting IDs are rejected.
The catalog itself does not grant execution authority or promise generic rollback.
