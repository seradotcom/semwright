# Semantic UI v2

Semantic UI v2 is the portable semantic projection shared by Linux AT-SPI, macOS Accessibility and Windows UI Automation. It is intentionally a **union of proven semantics**, not the smallest common denominator.

## Contract

Every projected node keeps the stable v1 core (ref, role, name, states, actions, app, ancestry and bounds) and may add:

- help, accessibility identity and framework metadata;
- bounded native attributes that remain data, never authority;
- typed relations such as labelled_by and described_by;
- optional typed facets for text, value, selection, table, document, hypertext, scrolling, window state and transforms.

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
| Scroll facet | protocol-dependent | yes | yes |
| Window facet | yes | yes | yes |
| Transform facet | protocol-dependent | yes | yes |
| Native semantic hit-test | yes | yes | yes |

The table describes projection classes in the implementation. Native CI and live matrices remain the source of truth for verified support.

## Conformance direction

Platform implementations are tested against shared semantic expectations rather than forced into identical feature sets. A backend may expose a richer optional facet without requiring other operating systems to fabricate it.

The next hardening layers are:

- normalized semantic events over the existing provider event pipeline;
- native query pushdown where it preserves portable selector semantics;
- AT-SPI TableCell row/column/span/header projection;
- bounded text ranges, caret, selection and text-attribute semantics;
- live GTK/Qt/UIA/AX fixture coverage for rich facets and hit-testing;
- explicit event-loss, stale-ref, ambiguity and protected-control tests.

No v2 field is an authorization signal. Policy, consent, app scope and pre-mutation validation remain authoritative.
