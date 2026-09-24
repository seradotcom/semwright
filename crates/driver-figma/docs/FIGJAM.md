# FigJam

Curated semantic surface: sticky, shape-with-text, connector, section, code block, tables, timer and bounded diagram composition. The plugin implements these operations through the official FigJam Plugin API and gates editor-specific mutations on `editorType=figjam`. Connector endpoints use semantic node IDs with Figma connector magnets rather than coordinate-only wiring.

`figjam.diagram.create` creates a bounded graph from explicit semantic nodes/edges, while the lower-level sticky/shape/connector/table operations remain available for incremental edits. Code-block creation is implemented in the main plugin dispatcher. Real-FigJam acceptance remains a separate protected live-evidence gate.
