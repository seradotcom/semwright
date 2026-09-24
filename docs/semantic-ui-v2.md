# Semantic UI v2

Semantic UI v2 is the portable semantic projection shared by Linux AT-SPI, macOS Accessibility and Windows UI Automation. It is intentionally a **union of proven semantics**, not the smallest common denominator.

## Contract

Every projected node keeps the stable v1 core (ref, role, name, states, actions, app, ancestry and bounds) and may add:

- help, accessibility identity and framework metadata;
- bounded native attributes that remain data, never authority;
- typed relations such as labelled_by and described_by;
- optional typed facets for text, value, selection, table, document, hypertext, images, scrolling, window state and transforms.

Missing facets mean “not observed/supported”, not false. Platform-native handles, constants and object types never cross the portable contract.

## Safety invariants

1. Rich observation never grants additional authority.
2. Application text, names, help, attributes and relations are untrusted data.
3. Password/protected controls remain redacted.
4. Refs are revalidated before side effects.
5. Event loss invalidates cached identity and semantic assumptions.
6. Visual grounding may suggest a point, but native hit-testing should recover a semantic ref before mutation whenever possible.
7. Raw pointer input remains a fallback, not an implicit semantic action.
8. Legacy v1 node JSON remains deserializable.

## Platform projection

| Semantic surface | Linux / AT-SPI | macOS / AX | Windows / UIA |
|---|---|---|---|
| Core role/name/state/action | yes | yes | yes |
| Stable native identity inputs | bus/path + generation | PID + AX generation | PID/start + RuntimeId/AutomationId |
| Help/accessibility id/framework | yes | yes | yes |
| Relations | rich RelationSet | selected AX relations | selected UIA relations |
| Text facet | yes | yes | yes |
| Value facet | yes | yes | yes |
| Selection facet | yes | yes | yes |
| Table facet | yes | yes | yes |
| Document facet | yes | partial/future | partial/future |
| Hypertext facet | yes | partial/future | partial/future |
| Image facet | yes | partial/future | yes |
| Scroll facet | protocol-dependent | yes | yes |
| Window facet | yes | yes | yes |
| Transform facet | protocol-dependent | yes | yes |
| Native semantic hit-test | yes | yes | yes |

The table describes projection classes in the implementation. Native CI and live matrices remain the source of truth for verified support.

## Rich selectors

`ui.find` preserves the v1 selector surface and can additionally constrain observed semantic facets. Portable predicates include text editability/password/selection state, numeric value ranges, selection state and table row/column/size. These predicates operate only on projected semantics; they never inspect platform-native handles.

Ranked free-text `query` remains discovery-only and cannot select a mutating target. Mutations still require an unambiguous broker-resolved ref.

## Semantic events

The portable event taxonomy includes backend/window/structure/selection/text/focus/state/property/geometry/object changes. Linux AT-SPI projects native event streams into these kinds and invalidates generations conservatively on structural loss. Windows now installs bounded native UIA structure/property/focus/text/selection/window subscriptions when the host permits them, publishes portable provider events, and advances a structural generation so pre-change refs fail stale; native flood/loss fidelity still requires interactive Windows verification. macOS AX observers currently invalidate native state; publication through the provider event pipeline remains follow-up work.

Event loss is never treated as a complete history: caches/refs must be invalidated and refreshed.

## Conformance direction

Platform implementations are tested against shared semantic expectations rather than forced into identical feature sets. A backend may expose a richer optional facet without requiring other operating systems to fabricate it.

Implemented rich semantics include AT-SPI Table/TableCell coordinates/spans/headers, bounded caret/selections/text-attribute runs, UIA Text/Grid/Table/Scroll/Window/Transform patterns, AX/UIA/AT-SPI native hit-testing and cross-platform selector conformance.

Remaining hardening focuses on:

- native Windows UIA event loss/flood fidelity and interactive verification;
- macOS AX event publication through the provider pipeline;
- native query pushdown where it preserves portable selector semantics;
- live GTK/Qt/UIA/AX fixture coverage for rich facets and hit-testing;
- explicit native event-loss, stale-ref, ambiguity and protected-control tests.

No v2 field is an authorization signal. Policy, consent, app scope and pre-mutation validation remain authoritative.
