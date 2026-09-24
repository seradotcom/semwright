# figma-cli parity audit

Reference: `silships/figma-cli@2b1c22aaf8ff0ceeb7c08fcbaf553d6c1f963b5b`.

Semwright does not copy figma-cli's arbitrary-JavaScript execution model. The goal is to preserve its useful product workflows through typed operations and cross-provider composition.

| figma-cli family | Semwright equivalent | Status |
|---|---|---|
| tokens / variables / bindings | `variable.*`, `mode.*`, `design_system.*` | native semantic |
| create primitives / auto-layout | typed create + layout capabilities | native semantic |
| JSX render | `compose.apply` typed declarative AST | native semantic |
| render-batch | `compose.batch` | native semantic |
| modify / sizing / padding / gap / align | node/layout/property capabilities | native semantic |
| find / select / get | `node.search`, `node.query`, selection/node inspect | native semantic |
| canvas info / next / arrange | `canvas.info`, `canvas.next_position`, `canvas.arrange` | native semantic |
| duplicate / delete / tree / bindings | node and binding capabilities | native semantic |
| slots | `slot.*`, including native frame→slot conversion, preferred values, content, reset and `SlotSettings` | native semantic |
| Motion add/apply/presets/stagger | official `KeyframeField`/`ManualKeyframeTrackInput` primitives plus `motion.apply`, `motion.preset.apply`, `motion.stagger` | native semantic; no eval |
| screenshots / node export | artifact-backed `export.node` plus selection refs | native semantic |
| CSS / Tailwind | `design_system.export.css/tailwind` | native semantic |
| JSX / Storybook export | `node.export.jsx/storybook` | native semantic |
| lint / accessibility | `validate.lint`, `validate.a11y`, design-system validators | native semantic |
| color / typography / spacing / cluster analysis | `analysis.*` | native semantic |
| XPath/raw query | bounded typed `node.query`; no XPath evaluator | safer semantic replacement |
| component combos | `component.variant_matrix.create` | native semantic |
| size variants | `component.size_variants.create` | native semantic |
| FigJam primitives | `figjam.*` including diagrams/tables/timer | native semantic |
| remote image URL | Browser/download artifact -> `image.create` | cross-provider composition |
| website recreate/analyze/screenshot URL | Chromium semantic capture -> Figma `compose.*` | cross-provider composition |
| remove-bg external API | external image provider -> artifact -> Figma | provider composition |
| daemon/files connection UX | driver doctor/pairing/session capabilities | native Semwright |
| `eval`, `run`, FigJam eval | none | deliberately unsupported unsafe escape |
| app.asar/Yolo/CDP | none | deliberately unsupported private/control escape |

The cross-provider rows are not capability gaps in Figma's public object model. They are workflows whose authoritative source is another domain (web browsing, image processing, or external services). Semwright should compose providers rather than give the Figma driver arbitrary network/code authority.

The parity audit is intentionally about user-visible capability, not command-name equality. Semwright may expose a smaller number of higher-level operations while the generated property surface covers thousands of public Figma fields.
