# Non-secret driver config samples

`default.json` matches in-code defaults. `authenticated-agent.json` requests the
owner's in-memory Unix credential agent at a fixed path; it contains no password.
The driver reads `/etc/semwright-obs/config.json` only if the owner separately grants
that protected system directory. Defaults need no filesystem mounts.

These are not OBS Studio profiles. Never put password, host URL, certificate-bypass,
raw-request or approval fields in this format: unknown fields are rejected.
