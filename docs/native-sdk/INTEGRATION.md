# Integration with Core, Platform and consumers

| Concern | Canonical owner |
|---|---|
| app model/database/transaction | application |
| operation descriptor/risk | canonical descriptor + Native operation contract |
| authorization/approval/audit | Broker/Policy/Core |
| provider lifecycle/sandbox/runtime tools | Driver Host / Driver SDK |
| native ref resolution | Broker + live Driver Host |
| Project CURRENT/STALE/UNKNOWN | Project Graph |
| effect verdict/findings | Effect Conformance |
| jobs/progress/cancel/artifacts | Core / Driver Host |
| private app destination commit | application publication operation |
| accounts/Teams/Changes orchestration | Platform |

The Native SDK must not become an alternate Broker, Graph, scheduler, permission store or publication service.

Host conformance launches the real daemon, CLI, protocol MCP server, sandbox helper, Driver Host and native applications from one exact checkout. CLI and MCP target the same daemon; cross-session refs are rejected.

## Launchwright

Launchwright can consume public cooperation values/contracts and durable Graph locators while keeping its own storage. The SDK defines no Launchwright-specific model or repository. Clean external consumers intentionally exercise the public surface from outside the Semwright workspace.

## Platform gap

`NATIVE_TO_PLATFORM` / `REAL_CLIENT_HOST` remain external dependency levels. The SDK does not require or simulate a Cloud account, Teams backend, or physical certification. Independent SDK/Core/Host/Graph/Effects acceptance remains valid; the external gap stays explicit.
