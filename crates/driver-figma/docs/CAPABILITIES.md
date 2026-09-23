# Capability catalog

The Rust driver advertises a bounded static `driver.figma.*` catalog spanning doctor/session, document/page/selection, nodes and creation, layout, paint/stroke/effects, text/fonts, SVG, components/sets/instances, variables/modes/bindings, styles, design systems, snapshots/diff, validation/a11y, prototyping, Motion Beta, FigJam and Dev Mode CSS. Large binary export operations remain intentionally unadvertised until the protocol-v2 artifact path is integrated.

The advertised catalog is implementation-backed: 88 operations are handled by the typed plugin dispatcher and three operations are local to the Rust driver. Operations without a production handler are not advertised. Session-specific availability still depends on editor type, Figma Motion Beta support, document state and permissions; those conditions are checked at execution time and fail explicitly rather than changing the static catalog.
