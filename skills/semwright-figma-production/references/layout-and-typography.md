# Layout and typography

Prefer native Auto Layout for ordered stacks, rows, wrapping, alignment, distribution, padding and gaps. Use absolute positioning only for designs that semantically require overlays or free placement.

After writes, measure actual Figma geometry. Never assume requested width, font size or line height proves the final bounds.

UI copy should remain native text. Preserve font family/style intent, wrapping, intrinsic sizing, line height, letter spacing and explicit fit strategy. Do not silently shrink text until it fits. When copy does not fit, use the declared strategy or return a finding.

For section work, iterate section-by-section so measurement/validation failures do not cascade across a whole page.
