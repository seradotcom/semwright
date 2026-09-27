# Design systems

Inspect a bounded design-system context before serious interface construction when components, variants, variables or styles are relevant.

Resolve exact refs first. Semantic name lookup must be unambiguous; ambiguity is a stop/review condition, not permission to pick the first match.

Reuse components and preserve instance semantics. Do not detach instances for convenience. Bind variables/styles where a real token exists and the composition declares that requirement. Do not invent a token merely because a hard-coded literal would be less convenient.

When a required component, variable, style or mode is absent, report the gap or explicitly use a token/component creation capability if that is part of the authorized task.
