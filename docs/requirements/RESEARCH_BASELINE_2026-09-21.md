# Research Baseline — 2026-09-21

This file records the technical assumptions used to form the blueprint. The implementation chat should re-check versions before pinning dependencies.

## XDG Desktop Portal — RemoteDesktop

Official docs:
https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.RemoteDesktop.html

Verified baseline:
- interface documentation currently identifies RemoteDesktop version 2;
- supports requesting keyboard, pointer and touchscreen device types;
- session `Start()` generally presents user-facing selection/consent;
- integrates with ScreenCast and Clipboard;
- current portal APIs include session restore/persistence mechanisms where supported.

Implication:
Wayland control should use the portal/security model, not attempt to bypass it.

## XDG Desktop Portal — ScreenCast

Official docs:
https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.ScreenCast.html

Verified baseline:
- current docs describe ScreenCast version 6;
- streams are exposed through PipeWire;
- source types include monitor, window and virtual source;
- mapping IDs support alignment with remote-input regions;
- current documentation recommends stable PipeWire serial targeting over assuming node IDs are permanently unique;
- persistence/restore tokens exist.

Implication:
screen capture and coordinate mapping need session-aware metadata.

## Official MCP Rust SDK

Repository:
https://github.com/modelcontextprotocol/rust-sdk

Verified baseline:
- official Rust SDK is `rmcp`;
- current repository states support for stable MCP `2026-07-28` with compatibility for older protocol versions;
- supports tools, resources/prompts, negotiation, subscriptions/tasks and streamable HTTP/stdio depending on features.

Implication:
use the official SDK rather than inventing an MCP server implementation.

## KWin scripting

Official KDE docs:
https://develop.kde.org/docs/plasma/kwin/

Verified baseline:
- KWin exposes scripting for programmatic window behavior/manipulation;
- scripts are packageable and can access workspace/client abstractions.

Implication:
a first-party KWin bridge is a reasonable route for richer KDE window control.

## Landlock

Linux kernel docs:
https://kernel.org/doc/html/next/security/landlock.html

Verified baseline:
- Landlock is a Linux security module for scoped restrictions;
- it is designed so unprivileged processes can further restrict themselves;
- restrictions stack with existing system access controls.

Implication:
plugin/helper processes can gain kernel-enforced filesystem restrictions without requiring root, subject to kernel support.

## Bubblewrap

Repository:
https://github.com/containers/bubblewrap

Purpose:
unprivileged sandbox construction using Linux namespaces and related primitives.

Implication:
optional stronger plugin process isolation can be layered on top of the broker.

## Existing project: computer-use-linux

Repository:
https://github.com/agent-sh/computer-use-linux

Observed baseline:
- Rust CLI/MCP server;
- Wayland-first positioning;
- AT-SPI, portals and compositor-specific paths;
- GNOME/KDE/Hyprland/i3/COSMIC claims in current README;
- current project includes `doctor` and release packaging.

Implication:
do not build a clone. Differentiate on:
- capability broker;
- policy;
- typed command registry;
- plugin SDK;
- recipes;
- deterministic ambiguity semantics;
- audit/replay;
- app-native adapters;
- sandboxed extension model;
- CLI/MCP/TUI sharing one execution core.

Study its public implementation for interoperability lessons only in compliance with its license.

## Existing project: linux-desktop-mcp

Repository:
https://github.com/BeckhamLabsLLC/linux-desktop-mcp

Observed baseline:
- semantic element references;
- AT-SPI based targeting;
- X11/Wayland input fallbacks;
- Python implementation.

Implication:
semantic refs are already a validated interaction pattern; this project should take the concept further into a full broker/command platform.

## Rust portal wrapper

Docs:
https://docs.rs/ashpd/

Current docs expose Rust wrappers for XDG portals and use zbus underneath.

Implication:
prefer a maintained portal wrapper where it covers needed interfaces; drop to raw zbus only when necessary.

## Rust EIS/libei ecosystem

One available crate:
https://docs.rs/reis/

Current docs note that the crate is still incomplete/subject to change.

Implication:
wrap EIS/libei integration behind an internal backend trait so the project can replace the implementation without changing public commands.

## AT-SPI

AT-SPI is the Linux accessibility protocol over D-Bus used by assistive technologies and accessible applications.

Implementation should evaluate the current Rust `atspi` ecosystem at build time and pin a maintained compatible version.

Implication:
accessibility protocol types must be wrapped by our own normalized domain model; do not leak dependency-specific types into public APIs.

## Reference rule

The coding chat must re-check:
- current crate versions;
- license compatibility;
- minimum Rust version;
- portal versions;
- compositor APIs;
- current GitHub project names;
before creating final lockfiles/release metadata.
