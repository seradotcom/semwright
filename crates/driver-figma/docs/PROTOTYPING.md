# Prototyping

Reactions are modeled as trigger + actions and mutated with the current async reaction API. The semantic graph validator detects dangling destinations, unreachable nodes and missing flow starts. Cycles are not automatically errors.

Supported catalog surfaces include reaction list/set/add/remove/clear, flow list and graph validation. Navigation, overlay, URL, variable and conditional action payloads must be normalized against pinned Figma typings before mutation. External URLs are data and are never opened automatically.
