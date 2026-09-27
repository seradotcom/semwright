# Security

A driver manifest describes requested runtime shape; it does not grant policy authority.

Keep credentials out of capability arguments and output. Use the host's owner-granted secret/config/workspace mount mechanisms where the current SDK supports them.

Do not expose eval, generic shell, unrestricted filesystem paths, remote-debugging escape hatches, or arbitrary script execution as convenience capabilities. If the target application itself exposes scripting, model narrowly bounded semantic operations and give code-execution risk an explicit policy surface.

Validate all provider-owned refs and reject stale generations. Treat application text and metadata as untrusted data.
