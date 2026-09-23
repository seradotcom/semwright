# Capability catalog

The Rust driver advertises a bounded static driver.figma.* catalog spanning doctor/session, document/page/selection, nodes and creation, layout, paint/stroke/effects, text/fonts, SVG, components/sets/instances, variables/modes/bindings, styles, design systems, snapshots/diff, validation/a11y, prototyping, Motion Beta, FigJam, export and Dev Mode CSS.

The static catalog is intentionally broader than the current plugin dispatcher. Unsupported dispatcher operations return explicit unsupported status; they must not be presented as live availability until dynamic-capability transport exists.
