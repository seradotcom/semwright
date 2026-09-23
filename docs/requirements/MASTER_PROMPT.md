# MASTER PROMPT — Build the complete Linux agentic computer interface

You are receiving a blueprint package for a serious open-source Linux project.

Your task is NOT to design another architecture proposal. Your task is to **build the entire repository**, verify everything that can be verified in your environment, and return a final ZIP.

Read every blueprint document before changing or creating code:

- `START_HERE.md`
- `PRODUCT_VISION.md`
- `ARCHITECTURE.md`
- `BACKENDS_AND_COMPATIBILITY.md`
- `COMMANDS_AND_PLUGIN_SDK.md`
- `SECURITY_THREAT_MODEL.md`
- `REPO_BLUEPRINT.md`
- `TESTING_RELEASE_AND_QUALITY.md`
- `ACCEPTANCE_CHECKLIST.md`
- `RESEARCH_BASELINE_2026-09-21.md`

Treat those documents as product requirements. If a low-level implementation detail must change for correctness, current APIs, compilation, security or maintainability, you may change it, but record the deviation in an ADR or `VERIFY.md`.

---

# 1. MISSION

Build a production-grade, local-first, open-source **agentic computer interface for Linux**.

It must convert Linux desktop/system/application capabilities into a typed, semantic, deterministic command surface usable through:

1. a CLI;
2. a user-level broker daemon;
3. an MCP server using the official Rust MCP SDK;
4. a TUI/inspector/debugging surface;
5. a plugin/adapter SDK;
6. a declarative recipe/workflow engine.

The project must prioritize:

```text
application-native API
    ↓
D-Bus / system API / compositor API
    ↓
AT-SPI accessibility
    ↓
XDG portal / libei approved input
    ↓
optional low-level input
    ↓
vision / screenshots only as last resort
```

This is explicitly NOT a screenshot-first “computer use” clone.

The agent should reason about:

```text
window.focus
ui.find
ui.invoke
blender.object.create
browser.dom.click
recipe.run
```

rather than raw coordinates whenever a semantic route exists.

---

# 2. PRODUCT STANDARD

Do not build an MVP.

Build a coherent **v1.0-grade release candidate**: the level of polish, documentation, reliability, extensibility and install experience expected from an open-source project that could plausibly become widely adopted.

Do not optimize for a flashy demo at the expense of architecture.

Do not use “10k GitHub stars” as a literal acceptance metric. Use it as a quality bar:
- immediate understandable value;
- elegant install;
- strong README;
- real engineering depth;
- useful even without an LLM;
- secure defaults;
- extensible architecture;
- excellent diagnostics;
- honest compatibility claims.

---

# 3. IMPORTANT EXECUTION RULES

## 3.1 Do the work, do not stop at planning

You must continue through:
- research;
- design reconciliation;
- implementation;
- compilation;
- tests;
- linting;
- docs;
- CI;
- packaging;
- final verification;
- ZIP creation.

Do not return only:
- an architecture;
- a skeleton;
- TODO-heavy scaffolding;
- a partial “MVP”;
- snippets.

## 3.2 Do not ask routine clarification questions

Use good engineering judgment.

If something cannot be tested because the current machine has no real GNOME/KDE/Wayland desktop, implement it behind interfaces, create contract/fake/headless tests, and label live verification pending.

Do not use lack of a graphical desktop as a reason to stop.

## 3.3 Never fabricate verification

For every feature distinguish:

```text
unit-tested
integration-tested
headless-tested
compiled only
implemented, live verification pending
manually verified
```

Never claim a compositor/app was tested if it was not.

## 3.4 Keep the repository buildable

After each substantial pass, run relevant compilation/tests.

Do not build 50,000 lines and wait until the end to discover the workspace never compiled.

## 3.5 Prefer current official APIs

Before pinning dependencies, re-check the latest:
- official MCP Rust SDK;
- XDG portal APIs;
- Rust D-Bus/portal crates;
- AT-SPI Rust crates;
- KWin/GNOME integration options;
- Wayland/libei ecosystem;
- dependency licenses.

Record important dependency choices.

---

# 4. PHASE 0 — RESEARCH AND NAMING, THEN FREEZE THE ARCHITECTURE

Do a short targeted current-state check.

You must inspect, at minimum:
- `modelcontextprotocol/rust-sdk`;
- XDG `RemoteDesktop`;
- XDG `ScreenCast`;
- relevant `ashpd` support;
- Rust AT-SPI options;
- current libei/EIS Rust options;
- KWin scripting/current KDE route;
- GNOME route;
- existing `agent-sh/computer-use-linux`;
- at least one other Linux desktop MCP/automation project.

The goal is not an endless competitive research report.

The goal is to avoid stale APIs and accidental duplication.

Choose a short, memorable project name after checking obvious collisions:
- GitHub;
- crates.io;
- npm if used;
- common Linux packages.

The canonical user-facing CLI is `semwright`; companion executables use the `semwright-*`/`semwrightd` naming scheme.

Then freeze the core architecture and proceed.

---

# 5. IMPLEMENTATION LANGUAGE AND WORKSPACE

Use Rust for the core.

Create a Rust workspace with strong crate boundaries. You may merge/split the blueprint's proposed crates only if it materially improves buildability or API cleanliness.

Preferred ecosystem, subject to current verification:
- Tokio for async;
- serde/schemars for typed schemas;
- clap for CLI;
- tracing for logs;
- zbus for D-Bus;
- ashpd where suitable for portals;
- official `rmcp` for MCP;
- a maintained terminal UI crate for inspector/TUI;
- X11/Wayland crates appropriate to each backend.

Avoid dependency sprawl.

Use Python/JavaScript only where the host platform needs it:
- Blender add-on: Python;
- GNOME Shell extension: GJS/JavaScript;
- KWin script: JavaScript/QML if appropriate.

---

# 6. REQUIRED CORE

Implement a user-level broker daemon.

## Broker responsibilities

- environment detection;
- command registry;
- capability state;
- policy;
- session management;
- short-lived object refs;
- backend discovery;
- execution planning;
- cancellation;
- timeout;
- audit;
- plugin host;
- recipe execution.

The broker is the only authority for side effects.

CLI and MCP must route through the broker or the exact same policy/execution layer. Do not create a bypass.

## IPC

Default:
- Unix-domain socket under `$XDG_RUNTIME_DIR`;
- mode `0600`;
- peer UID validation;
- protocol version handshake;
- framed messages;
- request IDs;
- cancellation;
- events/subscriptions.

Do not expose a TCP listener by default.

---

# 7. CAPABILITY AND POLICY SYSTEM

Implement granular capabilities, at least:

```text
desktop.observe
app.observe
app.launch
app.close
window.observe
window.manage
ui.observe
ui.invoke
input.keyboard
input.pointer
screen.capture
clipboard.read
clipboard.write
process.observe
process.manage
filesystem.read:<scope>
filesystem.write:<scope>
shell.exec
plugin:<plugin>:<command>
```

Support profiles:
- observe;
- desktop;
- workspace;
- developer;
- explicit unsafe/full-user profile if you choose to include it.

Shell must be disabled by default.

Policy result types:
- allow;
- deny;
- require external confirmation;
- allow with restrictions/redactions.

The model may not approve its own confirmation.

---

# 8. COMMAND REGISTRY

Every command has:
- stable name;
- version;
- input schema;
- output schema;
- required capabilities;
- risk class;
- timeout;
- idempotency class;
- dry-run support;
- backend candidates.

Implement discovery and description APIs.

The registry is also the source for:
- CLI docs/completions where useful;
- MCP schemas;
- plugin validation;
- TUI inspection.

---

# 9. REFERENCE MODEL

Use opaque session-scoped refs:

```text
app:...
win:...
ui:...
screen:...
```

Implement:
- generation/revision;
- expiration;
- stale detection;
- no accidental reuse.

A stale ref must fail safely.

---

# 10. AT-SPI SEMANTIC DESKTOP

This is a core feature, not an optional demo.

Implement a normalized accessibility model independent of the chosen crate.

Required operations:
- enumerate accessible applications;
- enumerate roots/windows where appropriate;
- bounded tree snapshot;
- compact actionable snapshot;
- exact semantic find;
- ranked discovery find;
- invoke action;
- set/read text where supported;
- read/set value where supported;
- toggle/select/expand when supported;
- component geometry;
- state/action exposure;
- event handling or cache invalidation.

Robustness:
- disappearing object handling;
- D-Bus timeouts;
- partial accessibility coverage;
- cyclic/broken tree protection;
- node/depth budgets.

Ambiguity:
- mutating actions must not silently choose one of multiple plausible targets.

---

# 11. WINDOW/COMPOSITOR SUPPORT

Implement a real backend strategy, not placeholders.

At minimum provide working code paths for:

## GNOME/Wayland
- environment detection;
- AT-SPI;
- portal input/capture path;
- a narrow first-party GNOME Shell bridge/extension if required for window operations not available generically.

Do NOT expose arbitrary shell/eval through the extension.

## KDE/Wayland
- AT-SPI;
- portal path;
- narrow KWin bridge/script for richer window management where necessary.

## Sway and/or wlroots
- direct IPC backend or robust typed adapter.

## Hyprland
- direct socket/IPC or robust typed adapter.

## X11
- semantic AT-SPI first;
- EWMH/window operations;
- X11 input fallback where needed.

Runtime detection must report what is actually available.

---

# 12. XDG PORTAL / WAYLAND INPUT

Implement the current XDG portal route for RemoteDesktop.

Requirements:
- keyboard/pointer selection;
- consent state;
- session lifecycle;
- cancellation;
- persistence/restore token handling when supported;
- secure token storage;
- integration with ScreenCast/Clipboard where relevant;
- EIS/libei route when available.

If the current Rust libei/EIS library is incomplete, hide it behind an internal backend trait and document the exact compatibility status.

Do not bypass user consent.

Optional uinput/ydotool-style helper may exist only as an explicit fallback:
- not required for semantic actions;
- not silently activated;
- policy-gated;
- setup documented.

---

# 13. SCREEN CAPTURE

Implement ScreenCast/Screenshot portal support to the extent current APIs allow.

Requirements:
- monitor/window source handling;
- PipeWire integration or a clean tested wrapper boundary;
- logical coordinate metadata;
- scaling awareness;
- mapping IDs if available;
- temporary artifact permissions;
- expiry;
- no screenshot bytes in audit logs.

Vision must be an optional adapter/fallback, not required to use the project.

---

# 14. CLI

Create a polished CLI.

Required examples:

```bash
<cmd> doctor
<cmd> capabilities list
<cmd> app list
<cmd> app launch ...
<cmd> window list
<cmd> window focus ...
<cmd> ui snapshot ...
<cmd> ui find ...
<cmd> ui invoke ...
<cmd> ui set-text ...
<cmd> input type ...
<cmd> pointer click ...
<cmd> screen capture ...
<cmd> clipboard read
<cmd> clipboard write ...
<cmd> plugin list
<cmd> recipe list
<cmd> recipe run ...
<cmd> audit tail
```

Required UX:
- human output;
- `--json`;
- stable exit codes;
- useful errors;
- shell completions;
- man page generation if practical;
- no ANSI contamination in JSON.

---

# 15. `doctor` IS A PRODUCT FEATURE

`doctor` must diagnose:
- session type;
- desktop;
- compositor;
- D-Bus;
- AT-SPI;
- portal availability/version;
- ScreenCast;
- input backend;
- GNOME/KWin bridge;
- plugin host;
- broker socket;
- config/state permissions.

Output statuses:

```text
SUPPORTED
SUPPORTED_WITH_CONSENT
SUPPORTED_WITH_HELPER
EXPERIMENTAL
UNAVAILABLE
```

Provide remediation text.

Both human and JSON output.

---

# 16. MCP

Use the official Rust MCP SDK.

MCP is a frontend to the broker, never a second execution engine.

Avoid dumping hundreds of tools into model context.

Always-visible core tools should include equivalents of:
- doctor;
- capabilities;
- search commands;
- describe command;
- execute;
- snapshot;
- find.

If current MCP supports dynamic tool list changes, implement session-enabled tools when practical.

Otherwise keep universal typed execution.

Support:
- structured outputs;
- command schemas;
- cancellation;
- long-running tasks where appropriate/currently supported;
- protocol negotiation;
- useful tool annotations.

Do not let MCP annotations substitute for real policy.

---

# 17. TUI / INSPECTOR

Ship a useful terminal inspector.

It should let a developer:
- see doctor status;
- list windows/apps;
- inspect accessibility tree;
- search controls;
- inspect a ref;
- see enabled capabilities;
- see policy decision;
- inspect recent redacted audit events;
- see plugin status.

This makes the project valuable and debuggable independent of an LLM.

---

# 18. RECIPE ENGINE

Implement a versioned declarative recipe format.

Requirements:
- typed inputs;
- typed outputs;
- variables;
- step outputs;
- assertions;
- conditions;
- explicit timeouts;
- explicit safe retry;
- cancellation;
- dry-run plan;
- secret values;
- no arbitrary shell interpolation by default;
- schema validation;
- fake-backend testing.

Ship examples.

Ship:

```bash
<cmd> recipe validate
<cmd> recipe run
<cmd> recipe scaffold
<cmd> recipe test --backend fake
```

A recipe is a deterministic workflow, not a disguised prompt.

---

# 19. PLUGIN SDK

Implement an out-of-process plugin system.

Requirements:
- versioned manifest;
- handshake;
- command schemas;
- requested permissions;
- health;
- version compatibility;
- timeout/watchdog;
- crash isolation;
- stdout/stderr handling;
- broker-controlled lifecycle.

Default plugin process security:
- environment scrub;
- narrow filesystem;
- Landlock where supported;
- optional bubblewrap;
- no network unless declared;
- no inherited secrets;
- resource limits where practical.

Provide:
- Rust SDK helper crate;
- example plugin;
- scaffold command/template;
- plugin install/remove/list/doctor.

Do not build a centralized marketplace.

---

# 20. FIRST-PARTY APP ADAPTERS

Implement at least two meaningful adapters, not hello-world examples.

## 20.1 Blender

Ship:
- Blender add-on;
- bridge/client;
- typed commands.

Minimum useful coverage:
- connection/status;
- scene inspect;
- object list/get/create/delete;
- transform;
- collections;
- materials;
- render settings;
- render;
- open/save under policy-scoped paths.

Default surface must NOT expose unrestricted Python execution.

A separate explicit high-risk developer command may exist if strongly justified.

Test:
- protocol unit tests;
- fake host;
- Blender background-mode tests if Blender can be installed/run.

## 20.2 Chromium-family browser

Use a CDP route.

Safety:
- launch an isolated browser profile by default;
- never silently attach to the user's normal profile;
- debug endpoint must be explicit.

Commands:
- browser status;
- tab list/open/close/focus;
- navigate;
- DOM query/snapshot;
- click/fill;
- JS-free common actions;
- screenshot;
- download/status;
- limited console/network diagnostics.

Arbitrary JS execution, if exposed at all, must be an explicit high-risk developer capability.

Test with headless Chromium if available.

## 20.3 Optional third adapter

Choose LibreOffice UNO or GIMP 3 only if you can implement it cleanly without destabilizing the core.

Do not sacrifice the core just to check a box.

---

# 21. SYSTEM COMMANDS

Demonstrate that not everything needs GUI.

Implement a small safe set through native services, for example:
- notifications;
- audio metadata/volume if reliable;
- network status (not necessarily destructive network reconfiguration);
- systemd user service status.

Do not expand into privileged sysadmin automation.

---

# 22. SECURITY

Implement the threat model from the blueprint.

Hard requirements:
- no root core daemon;
- no setuid;
- socket `0600`;
- peer UID checks;
- filesystem path confinement;
- no secret payloads in logs;
- clipboard treated as sensitive;
- screenshots treated as sensitive;
- low-level input focus checks where possible;
- no implicit sudo;
- shell off by default;
- plugin sandbox;
- Landlock support where possible;
- optional bubblewrap hardening;
- external confirmation for configured dangerous actions.

Create:
- `SECURITY.md`;
- security architecture doc;
- permission reference;
- vulnerability reporting instructions.

---

# 23. FILESYSTEM COMMANDS

If you expose filesystem commands:

They must be scoped by configured roots.

Protect against:
- `..`;
- symlink escape;
- path race where feasible;
- reading sensitive user locations outside grants.

Prefer safe directory-FD-relative operations where feasible.

Implement tests specifically for escape attacks.

---

# 24. SHELL

You may implement a developer-mode shell because it is useful, but:

- disabled by default;
- separate `shell.exec` capability;
- no silent `sh -c`;
- executable + argv preferred;
- cwd scoped;
- environment scrubbed;
- timeout;
- output size cap;
- no sudo;
- clearly documented as weakening confinement.

Do not make shell the mechanism used internally for every backend.

---

# 25. AUDIT

Every command records redacted metadata:
- command;
- request ID;
- client/session;
- policy result;
- backend;
- duration;
- success/failure;
- high-level target.

Never record:
- secret-typed arguments;
- full clipboard text by default;
- screenshot bytes;
- arbitrary document contents;
- browser auth material.

Ship an audit tail/inspect command.

---

# 26. TEST ARCHITECTURE

Do not rely on having a live GUI.

Build a deterministic fake desktop backend that supports:
- apps;
- windows;
- tree;
- focus;
- actions;
- changes;
- stale refs;
- duplicates;
- backend failures;
- consent state.

Run end-to-end recipes against it.

Required test classes:
- unit;
- property;
- golden;
- fake integration;
- protocol;
- security regression;
- adapter;
- headless where possible;
- fuzz targets.

Use `proptest` or equivalent for critical invariants.

---

# 27. HEADLESS / CI GUI TESTING

Attempt meaningful automation:

## X11
Use Xvfb or equivalent where practical.

## Wayland
Use a headless/nested compositor route where practical.

## D-Bus
Private/session-bus tests where practical.

## Blender
Background mode if available.

## Chromium
Headless CDP if available.

If something cannot be made reliable in CI, keep a contract test + exact manual test instructions.

---

# 28. FUZZING

Create fuzz targets for:
- local broker protocol;
- plugin protocol/handshake;
- selectors;
- recipes;
- path/policy normalization.

Seed corpora.

Run at least smoke fuzzing if the environment supports it.

---

# 29. QUALITY GATES

Before final handoff, attempt and fix until passing:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-targets
cargo doc --workspace --no-deps
cargo audit
cargo deny check
```

Also relevant:
- shellcheck;
- actionlint;
- JS lint;
- Python lint/format;
- package validation.

If a gate cannot run because the tool is unavailable, say that in `VERIFY.md`. Do not pretend it passed.

---

# 30. COVERAGE AND BENCHMARKS

Generate code coverage if feasible.

Targets:
- core/pure crates should aim >=90% line coverage;
- do not fake aggregate coverage by excluding difficult code.

Benchmarks:
- selector exact/ranked;
- policy;
- snapshot compaction;
- protocol encode/decode;
- registry search;
- fake end-to-end action.

Do not invent benchmark results.

---

# 31. PACKAGING

Ship:
- systemd user unit;
- release tarballs configuration;
- checksums;
- x86_64 release build;
- aarch64 build/cross-build setup;
- `.deb` if reliable;
- `.rpm` if reliable;
- Nix flake/package if feasible;
- install/uninstall docs.

Installer:
- no silent sudo;
- verifies checksums for downloaded artifacts;
- explains optional helper permissions.

---

# 32. CI / GITHUB

Create polished GitHub workflows for:
- format/check/clippy/test;
- security audit/deny;
- coverage;
- headless integration where practical;
- release build;
- artifact checksums;
- dependency review where applicable.

Also:
- issue templates;
- PR template;
- Dependabot/Renovate choice;
- release/changelog workflow.

---

# 33. OSS PROJECT QUALITY

Ship:
- dual MIT/Apache-2.0 if dependency/legal review remains compatible;
- `README.md`;
- `CONTRIBUTING.md`;
- `CODE_OF_CONDUCT.md`;
- `SECURITY.md`;
- `GOVERNANCE.md`;
- `CHANGELOG.md`;
- developer setup;
- architecture docs;
- command docs;
- MCP docs;
- plugin docs;
- recipes docs;
- security/permissions docs;
- compatibility matrix;
- Wayland explanation;
- troubleshooting;
- manual test checklist.

The public repo documentation should be in English.

No fake screenshots.

---

# 34. README QUALITY

The README must make the value obvious in 30 seconds.

Include:
- one-line pitch;
- terminal example;
- semantic-first diagram;
- why not screenshot-first;
- installation;
- quick start;
- `doctor`;
- MCP configuration;
- compatibility matrix;
- security model summary;
- plugin/recipe examples;
- links to docs.

Do not write marketing claims unsupported by tests.

---

# 35. COMPETITIVE DIFFERENTIATION

Do not simply recreate `computer-use-linux`.

The project must visibly add a broader platform:

1. least-privilege capability broker;
2. policy engine;
3. deterministic target semantics;
4. stable command registry;
5. CLI + MCP + TUI over one execution core;
6. app-native adapters;
7. plugin SDK;
8. recipe compiler/workflows;
9. sandboxed plugins;
10. structured audit;
11. scoped filesystem/shell;
12. comprehensive fake/headless test architecture;
13. excellent `doctor`;
14. release-quality packaging.

If, after researching current projects, one of these is already common, preserve it but emphasize the complete integrated system rather than novelty theater.

---

# 36. IMPLEMENTATION PASSES

Use passes so the repo stays healthy.

## Pass 1 — foundation
- workspace;
- types;
- protocol;
- registry;
- policy;
- audit;
- fake backend;
- daemon IPC;
- CLI skeleton backed by real broker;
- unit tests.

Do not stop.

## Pass 2 — semantic Linux core
- environment detector;
- D-Bus;
- AT-SPI;
- selectors;
- refs;
- window abstraction;
- doctor.

Do not stop.

## Pass 3 — Wayland/X11 backends
- portal;
- screen capture path;
- GNOME/KDE bridges;
- Sway/Hyprland;
- X11.

Do not stop.

## Pass 4 — MCP/TUI
- official MCP integration;
- command discovery;
- TUI inspector.

Do not stop.

## Pass 5 — recipes/plugins/security
- recipe engine;
- plugin SDK;
- sandbox host;
- Landlock;
- optional bubblewrap;
- permission profiles.

Do not stop.

## Pass 6 — first-party adapters
- Blender;
- Chromium;
- optional third.

Do not stop.

## Pass 7 — tests/CI/packaging/docs
- headless harness;
- fuzz;
- coverage;
- GitHub workflows;
- release;
- docs.

Do not stop.

## Pass 8 — final verification
Run the acceptance checklist and fix real failures.

---

# 37. WHEN AN API IS NOT AVAILABLE

Do not fill critical production code with `todo!()`.

Choose one:

1. implement a real alternative;
2. compile-gate an optional feature with clear capability status;
3. implement the protocol boundary and complete contract tests;
4. label live verification pending.

Placeholders are acceptable only for truly platform-external conditions, not as substitutes for implementation.

---

# 38. NO ARCHITECTURE DRIFT

Do not gradually turn the project into:
- an LLM framework;
- a cloud agent;
- a generic remote desktop;
- a browser-only automation tool;
- a wrapper around shell commands;
- a computer-vision benchmark.

The core identity remains:

> A semantic, typed, secure, extensible computer interface for Linux agents and automation.

---

# 39. VERIFY.md

At repository root create `VERIFY.md`.

It must include:

## Environment
- OS;
- Rust;
- relevant tool versions.

## Commands actually executed
Exact commands.

## Results
Counts where available.

## Live GUI limitations
Example:

```text
GNOME Wayland live execution: NOT TESTED in this build environment.
AT-SPI protocol tests: PASS.
Portal contract tests: PASS.
GNOME extension lint/build: PASS.
Manual validation instructions: docs/manual-testing.md.
```

## Known issues
Real only.

## Packaging status
What was built vs configured.

This document is mandatory.

---

# 40. FINAL ACCEPTANCE PASS

Before creating the ZIP, explicitly inspect `ACCEPTANCE_CHECKLIST.md`.

Resolve every item into:
- PASS;
- NOT APPLICABLE with reason;
- LIVE VERIFICATION PENDING;
- FAIL.

Critical core items may not remain FAIL.

If something significant is impossible in the environment, implementation plus tests/documented manual validation is acceptable, but be precise.

---

# 41. FINAL ZIP

Create a clean ZIP of the complete repository.

Exclude:
- `target/`;
- `.git/` if huge/unnecessary for handoff;
- caches;
- virtual environments;
- node_modules;
- secrets;
- temporary screenshots;
- downloaded browsers/toolchains.

Include:
- all source;
- lockfiles;
- docs;
- schemas;
- tests;
- CI;
- packaging;
- licenses;
- `VERIFY.md`.

Compute and report SHA-256.

---

# 42. FINAL RESPONSE FORMAT

The final response must be concise and contain:

1. link to the ZIP;
2. project name and one-line description;
3. what was actually verified;
4. live desktop/app tests still pending;
5. ZIP SHA-256.

Do not paste the entire codebase in the final response.

Do not claim “production ready” if critical checks failed.

---

# 43. FINAL PRINCIPLE

When choosing between:

> “let the model improvise again”

and:

> “turn the reliable operation into a typed command”

prefer the typed command.

When choosing between:

> “give the model more privilege”

and:

> “create a narrower capability”

prefer the narrower capability.

When choosing between:

> “guess which control the user meant”

and:

> “return ambiguity with candidates”

return ambiguity.

When choosing between:

> “fake broad support”

and:

> “report exact capability status”

report exact capability status.

Now read all blueprint files, perform the targeted current research, build the repository through all passes, run the verification gates available in your environment, and deliver the final ZIP.
