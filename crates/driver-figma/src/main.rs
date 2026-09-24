use async_trait::async_trait;
use semwright_driver_sdk::{Capability, Driver, DriverChildEvent, DriverInterfaces};
use semwright_figma_driver::bridge::{BridgeError, BridgeHub, pairing_status};
use semwright_figma_driver::{model, schemas, snapshot};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Risk};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use tokio::sync::mpsc;

mod semantic_admin_ops;
mod semantic_more_ops;

const DRIVER_SCOPE: &str = "driver:figma";
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Copy)]
struct Op {
    name: &'static str,
    description: &'static str,
    risk: Risk,
    idem: Idempotency,
    dry: bool,
}

fn op(
    name: &'static str,
    description: &'static str,
    risk: Risk,
    idem: Idempotency,
    dry: bool,
) -> Op {
    Op {
        name,
        description,
        risk,
        idem,
        dry,
    }
}
fn operations() -> Vec<Op> {
    use Idempotency::{Idempotent, NonIdempotent};
    use Risk::MutatingReversible;
    vec![
        op(
            "doctor",
            "Bridge and plugin health without pairing secrets",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "pairing.begin",
            "Reveal the ephemeral local Figma plugin pairing code",
            Risk::SecretAccess,
            Idempotency::ReadOnly,
            false,
        ),
        op(
            "session.list",
            "List authenticated Figma sessions",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "document.status",
            "Current document identity and revision",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "document.inspect",
            "Bounded document inspection",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "document.snapshot",
            "Canonical document snapshot",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "document.diff",
            "Semantic snapshot diff",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "page.list",
            "List pages",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "page.create",
            "Create page",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "page.inspect",
            "Inspect page",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "page.rename",
            "Rename page",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "page.remove",
            "Remove page",
            Risk::Destructive,
            Idempotency::Destructive,
            true,
        ),
        op(
            "page.current.get",
            "Get current page",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "page.current.set",
            "Set current page",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "selection.get",
            "Read selection",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "selection.set",
            "Set selection",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "selection.clear",
            "Clear selection",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "node.get",
            "Inspect node",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "node.children",
            "List node children",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "node.ancestors",
            "List node ancestors",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "node.tree",
            "Bounded node tree",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "node.search",
            "Structured node search",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "node.query",
            "Bounded semantic scene-graph query without XPath or eval",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "node.patch",
            "Allowlisted node patch",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "node.rename",
            "Rename node",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "node.move",
            "Move node",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "node.resize",
            "Resize node",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "node.rotate",
            "Rotate node",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "node.remove",
            "Remove node",
            Risk::Destructive,
            Idempotency::Destructive,
            true,
        ),
        op(
            "node.clone",
            "Clone node",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "node.reparent",
            "Reparent node",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "node.reorder",
            "Reorder node",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "frame.create",
            "Create frame",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "section.create",
            "Create section",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "rect.create",
            "Create rectangle",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "ellipse.create",
            "Create ellipse",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "line.create",
            "Create line",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "polygon.create",
            "Create polygon",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "star.create",
            "Create star",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "text.create",
            "Create text with font preflight",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "vector.create",
            "Create bounded vector",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "svg.import",
            "Sanitized SVG import",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "layout.inspect",
            "Compact Auto Layout summary",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "layout.patch",
            "Patch Auto Layout and responsive sizing",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "paint.patch",
            "Patch fills",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "stroke.patch",
            "Patch strokes",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "effects.patch",
            "Patch effects",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "text.inspect",
            "Inspect rich text and mixed ranges",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "text.patch",
            "Patch text after font loading",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "component.create",
            "Create component",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "component.from_node",
            "Convert node to component",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "component.inspect",
            "Inspect component",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "component_set.create",
            "Combine components as variants",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "component_set.inspect",
            "Inspect component set",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "variant.list",
            "List variants",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "instance.create",
            "Create component instance",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "instance.inspect",
            "Inspect instance and overrides",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "instance.swap",
            "Swap instance component",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "instance.detach",
            "Detach instance",
            Risk::Destructive,
            Idempotency::Destructive,
            true,
        ),
        op(
            "variable.collection.list",
            "List variable collections",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "variable.collection.create",
            "Create variable collection",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "variable.list",
            "List variables",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "variable.create",
            "Create variable",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "variable.set_value",
            "Set variable mode value",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "variable.set_alias",
            "Set variable alias",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "variable.bind",
            "Bind compatible variable",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "mode.list",
            "List collection modes",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "mode.create",
            "Create collection mode",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "style.list",
            "List local styles",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "style.apply",
            "Apply local style",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "design_system.extract",
            "Extract canonical design system",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "design_system.import",
            "Import bounded design system",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "snapshot.page",
            "Canonical page snapshot",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "snapshot.subtree",
            "Canonical subtree snapshot",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "snapshot.selection",
            "Canonical selection snapshot",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "snapshot.design_system",
            "Canonical design-system snapshot",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "validate.layout",
            "Validate layout consistency",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "validate.design_system",
            "Validate design-system bindings",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "validate.a11y",
            "Deterministic accessibility checks",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "validate.components",
            "Validate component sets",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "validate.variables",
            "Validate variables and aliases",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "prototype.reaction.list",
            "List prototype reactions",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "prototype.reaction.set",
            "Replace reactions using async API",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "prototype.reaction.add",
            "Add prototype reaction",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "prototype.reaction.remove",
            "Remove prototype reaction",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "prototype.reaction.clear",
            "Clear prototype reactions",
            Risk::Destructive,
            Idempotency::Destructive,
            true,
        ),
        op(
            "prototype.flow.list",
            "List flow starting points",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "prototype.validate",
            "Validate prototype graph",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "motion.styles.list",
            "List Motion animation styles (Beta)",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "motion.node.inspect",
            "Inspect Motion state (Beta)",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "motion.keyframes.list",
            "List Motion keyframes (Beta)",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "motion.timelines.list",
            "List Motion timelines (Beta)",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "motion.style.apply",
            "Apply Motion style (Beta)",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "motion.style.remove",
            "Remove Motion style (Beta)",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "motion.keyframe.apply",
            "Apply manual keyframe track (Beta)",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "motion.keyframe.remove",
            "Remove manual keyframe track (Beta)",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "motion.timeline.set_duration",
            "Set Motion timeline duration (Beta)",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "motion.spring.normalize",
            "Normalize physical spring parameters (Beta)",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "motion.export",
            "Export animated node",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "figjam.sticky.create",
            "Create FigJam sticky",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "figjam.shape.create",
            "Create FigJam shape",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "figjam.connector.create",
            "Create semantic FigJam connector",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "figjam.section.create",
            "Create FigJam section",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "figjam.code_block.create",
            "Create FigJam code block",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "figjam.diagram.create",
            "Create bounded semantic FigJam graph",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "export.node",
            "Export PNG/JPG/SVG/PDF artifact",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "dev.css",
            "Read generated CSS where supported",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "compose.apply",
            "Create a bounded declarative Figma subtree",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "compose.batch",
            "Create multiple bounded declarative Figma subtrees",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "node.inspect.full",
            "Inspect the bounded semantic property surface of a node",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "node.flatten",
            "Flatten explicit nodes into a vector",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "node.outline_stroke",
            "Create outlined stroke geometry from a node",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "text.runs.inspect",
            "Inspect rich-text styled runs",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "text.range.patch",
            "Patch a bounded rich-text range",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "text.hyperlink.set",
            "Set or clear a hyperlink on a text range",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "text.variable.bind_range",
            "Bind a variable to a text range field",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "text.path.create",
            "Create text on an existing vector path",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "text.path.inspect",
            "Inspect text-path semantics",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "font.list",
            "List available Figma fonts",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "font.variation_axes",
            "Inspect variable-font axes",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "vector.inspect",
            "Inspect vector network and paths",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "vector.network.set",
            "Replace a bounded vector network",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "boolean.create",
            "Create an empty BooleanOperationNode",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "boolean.union",
            "Create a boolean union",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "boolean.subtract",
            "Create a boolean subtraction",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "boolean.intersect",
            "Create a boolean intersection",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "boolean.exclude",
            "Create a boolean exclusion",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "transform_group.create",
            "Create a Figma transform group",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "transform_group.inspect",
            "Inspect transform-group modifiers",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "style.inspect",
            "Inspect a local or imported style",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "style.create",
            "Create a local style",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "style.patch",
            "Patch a local style",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "style.remove",
            "Remove a local style",
            Risk::Destructive,
            Idempotency::Destructive,
            false,
        ),
        op(
            "component.property.add",
            "Add a component property",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "component.property.edit",
            "Edit a component property",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "component.property.delete",
            "Delete a component property",
            Risk::Destructive,
            Idempotency::Destructive,
            false,
        ),
        op(
            "component.instances.list",
            "List component instances",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "component.description.patch",
            "Patch component descriptions",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "instance.properties.patch",
            "Patch instance component properties",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "slot.create",
            "Create a component slot",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "slot.inspect",
            "Inspect a component slot",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "slot.reset",
            "Reset slot contents",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "library.variable_collections.list",
            "List enabled library variable collections",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "library.variables.list",
            "List variables in an enabled library collection",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "library.component.import",
            "Import a published component by key",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "library.component_set.import",
            "Import a published component set by key",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "library.style.import",
            "Import a published style by key",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "library.variable.import",
            "Import a published variable by key",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "variable.inspect",
            "Inspect variable metadata, values and publishing state",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "variable.rename",
            "Rename a local variable",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "variable.remove",
            "Remove a local variable",
            Risk::Destructive,
            Idempotency::Destructive,
            false,
        ),
        op(
            "variable.scopes.set",
            "Set variable picker scopes",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "variable.code_syntax.set",
            "Set variable platform code syntax",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "variable.code_syntax.remove",
            "Remove variable platform code syntax",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "variable.collection.inspect",
            "Inspect a variable collection",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "variable.collection.rename",
            "Rename a local variable collection",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "variable.collection.remove",
            "Remove a local variable collection",
            Risk::Destructive,
            Idempotency::Destructive,
            false,
        ),
        op(
            "mode.rename",
            "Rename a variable mode",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "mode.remove",
            "Remove a variable mode",
            Risk::Destructive,
            Idempotency::Destructive,
            false,
        ),
        op(
            "variable.mode.set_explicit",
            "Set an explicit variable mode on a node",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "variable.mode.clear_explicit",
            "Clear an explicit variable mode on a node",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "shader.list",
            "List shaders available to the file",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "shader.import",
            "Import a shader into the file",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "shader.apply_fill",
            "Apply a shader fill",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "shader.apply_stroke",
            "Apply a shader stroke",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "shader.apply_effect",
            "Apply a shader effect",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "viewport.inspect",
            "Inspect current viewport state",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "viewport.center",
            "Center the Figma viewport",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "viewport.zoom",
            "Set viewport zoom",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "viewport.fit",
            "Fit explicit nodes into the viewport",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "annotation.categories.list",
            "List annotation categories",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "annotation.category.create",
            "Create an annotation category",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "annotation.category.patch",
            "Patch an annotation category",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "annotation.category.remove",
            "Remove a custom annotation category",
            Risk::Destructive,
            Idempotency::Destructive,
            false,
        ),
        op(
            "annotation.node.inspect",
            "Inspect node annotations",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "annotation.node.set",
            "Replace bounded node annotations",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "dev.resources.list",
            "List Dev Mode resources attached to a node",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "dev.resources.add",
            "Add a Dev Mode resource",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "dev.resources.edit",
            "Edit a Dev Mode resource",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "dev.resources.remove",
            "Remove a Dev Mode resource",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "selection.colors",
            "Inspect native selection colors",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "file.version.save",
            "Save a named Figma version-history entry",
            Risk::Mutating,
            NonIdempotent,
            false,
        ),
        op(
            "figjam.table.create",
            "Create a bounded FigJam table",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "figjam.link_preview.create",
            "Create a FigJam link preview or embed",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "figjam.gif.create",
            "Create a FigJam GIF media node from an image hash",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "figjam.timer.status",
            "Inspect the FigJam timer",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "figjam.timer.start",
            "Start or reset the FigJam timer",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "figjam.timer.pause",
            "Pause the FigJam timer",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "figjam.timer.resume",
            "Resume the FigJam timer",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "figjam.timer.stop",
            "Stop the FigJam timer",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "canvas.grid.inspect",
            "Inspect the Slides/Buzz canvas grid",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "canvas.grid.set",
            "Replace the Slides/Buzz canvas grid",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "canvas.row.create",
            "Create a Slides/Buzz canvas row",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "canvas.nodes.move",
            "Move nodes to a Slides/Buzz grid coordinate",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "slides.slide.create",
            "Create a slide",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "slides.row.create",
            "Create a slide row",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "slides.view.get",
            "Inspect Slides grid/single-slide view",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "slides.view.set",
            "Set Slides grid/single-slide view",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "buzz.frame.create",
            "Create a Buzz frame",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "buzz.instance.create",
            "Create a Buzz component instance",
            MutatingReversible,
            NonIdempotent,
            true,
        ),
        op(
            "buzz.asset_type.get",
            "Inspect a Buzz asset type",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "buzz.asset_type.set",
            "Set a Buzz asset type",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "buzz.text_content.inspect",
            "Inspect Buzz dynamic text fields",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "buzz.text_content.set",
            "Set a Buzz dynamic text field",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "buzz.media_content.inspect",
            "Inspect Buzz dynamic media fields",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "buzz.smart_resize",
            "Smart-resize a Buzz asset",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "artifact.read",
            "Read a bounded chunk from an exported Figma artifact",
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            true,
        ),
        op(
            "artifact.release",
            "Release an exported Figma artifact",
            MutatingReversible,
            Idempotent,
            true,
        ),
    ]
}
fn advertised_operations() -> Vec<Op> {
    let mut operations = operations();
    operations.extend(semantic_more_ops::operations());
    operations.extend(semantic_admin_ops::operations());
    operations
}

fn capability(o: Op) -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: format!("driver.figma.{}", o.name),
            version: "1".into(),
            description: o.description.into(),
            input_schema: schemas::input_schema(o.name),
            output_schema: schemas::output_schema(o.name),
            requires: vec![DRIVER_SCOPE.into()],
            risk: o.risk,
            idempotency: o.idem,
            timeout_ms: 30_000,
            dry_run: o.dry,
            interactive_consent: o.risk.sensitive(),
            backends: vec![DRIVER_SCOPE.into()],
        },
        aliases: vec![],
        tags: vec!["figma".into(), "plugin-api".into(), "semantic".into()],
        object_types: o
            .name
            .split('.')
            .next()
            .map(|s| vec![s.into()])
            .unwrap_or_default(),
    }
}
struct FigmaDriver {
    descriptors: BTreeMap<String, String>,
    ops: BTreeMap<String, Op>,
    hub: BridgeHub,
    events: Option<mpsc::UnboundedReceiver<DriverChildEvent>>,
}

impl FigmaDriver {
    async fn new() -> semwright_types::Result<Self> {
        let mut descriptors = BTreeMap::new();
        let mut ops = BTreeMap::new();
        for operation in advertised_operations() {
            let capability = capability(operation);
            let digest = semwright_driver_sdk::descriptor_digest(&capability.descriptor)?;
            descriptors.insert(capability.descriptor.name.clone(), digest);
            ops.insert(capability.descriptor.name.clone(), operation);
        }
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let hub = BridgeHub::start(event_tx)
            .await
            .map_err(|_| Error::unavailable("Figma loopback bridge could not start"))?;
        Ok(Self {
            descriptors,
            ops,
            hub,
            events: Some(event_rx),
        })
    }

    fn bridge_error(error: BridgeError, mutating: bool) -> Error {
        let mut mapped = match error {
            BridgeError::Auth => Error::new(
                ErrorCode::PermissionDenied,
                "Figma bridge authentication failed",
            ),
            BridgeError::Protocol | BridgeError::Duplicate => Error::new(
                ErrorCode::ProtocolMismatch,
                "Figma bridge protocol violation",
            ),
            BridgeError::Stale => Error::new(
                ErrorCode::StaleReference,
                "Figma session or revision is stale",
            ),
            BridgeError::Limit => Error::new(
                ErrorCode::ResourceExhausted,
                "Figma bridge resource limit exceeded",
            ),
            BridgeError::Unavailable | BridgeError::Io => {
                Error::unavailable("Figma plugin session unavailable")
            }
            BridgeError::Timeout => {
                Error::new(ErrorCode::Timeout, "Figma plugin request timed out")
            }
            BridgeError::Remote(code, outcome_known) => {
                let error = Error::new(
                    ErrorCode::BackendFailed,
                    format!("Figma plugin operation failed ({code})"),
                );
                if outcome_known {
                    error
                } else {
                    error.uncertain()
                }
            }
        };
        if mutating
            && matches!(mapped.code, ErrorCode::Unavailable | ErrorCode::Timeout)
            && mapped.outcome_known
        {
            mapped = mapped.uncertain();
        }
        mapped
    }
}

#[async_trait]
impl Driver for FigmaDriver {
    fn id(&self) -> &str {
        "figma"
    }

    fn version(&self) -> &str {
        VERSION
    }

    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            events: true,
            health: true,
            ..DriverInterfaces::default()
        }
    }

    fn take_events(&mut self) -> Option<mpsc::UnboundedReceiver<DriverChildEvent>> {
        self.events.take()
    }

    async fn capabilities(&mut self) -> semwright_types::Result<Vec<Capability>> {
        Ok(advertised_operations()
            .into_iter()
            .map(capability)
            .collect())
    }

    async fn execute(
        &mut self,
        command: &str,
        digest: &str,
        args: Value,
    ) -> semwright_types::Result<Value> {
        let Some(expected_digest) = self.descriptors.get(command) else {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Figma capability is not implemented",
            ));
        };
        if expected_digest != digest {
            return Err(Error::new(ErrorCode::Conflict, "descriptor digest changed"));
        }

        if command == "driver.figma.doctor" {
            let sessions = self.hub.sessions().await;
            return Ok(json!({
                "healthy": true,
                "bridge_protocol": model::BRIDGE_PROTOCOL_VERSION,
                "listen_host": "127.0.0.1",
                "listen_port": self.hub.port(),
                "pairing_required": true,
                "connected_sessions": sessions.len(),
                "motion": "beta",
                "plugin_api": "official"
            }));
        }
        if command == "driver.figma.pairing.begin" {
            let sessions = self.hub.sessions().await;
            return Ok(pairing_status(
                self.hub.port(),
                &self.hub.pairing_code(),
                &sessions,
            ));
        }
        if command == "driver.figma.session.list" {
            return serde_json::to_value(self.hub.sessions().await).map_err(|_| {
                Error::new(ErrorCode::Internal, "Could not serialize Figma sessions")
            });
        }

        let Some(operation) = self.ops.get(command).copied() else {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Figma capability is not implemented",
            ));
        };
        let mut object = args
            .as_object()
            .cloned()
            .ok_or_else(|| Error::invalid("Figma capability arguments must be an object"))?;
        let session_id = match object.remove("session_id") {
            Some(Value::String(value)) if !value.is_empty() && value.len() <= 128 => Some(value),
            Some(_) => return Err(Error::invalid("session_id must be a bounded string")),
            None => None,
        };
        let expected_revision = match object.remove("expected_revision") {
            Some(Value::Number(value)) => value
                .as_u64()
                .ok_or_else(|| Error::invalid("expected_revision must be an unsigned integer"))
                .map(Some)?,
            Some(_) => {
                return Err(Error::invalid(
                    "expected_revision must be an unsigned integer",
                ));
            }
            None => None,
        };
        let operation_name = command.strip_prefix("driver.figma.").ok_or_else(|| {
            Error::new(ErrorCode::Unsupported, "Invalid Figma capability namespace")
        })?;
        let snapshot_mode = object
            .get("mode")
            .and_then(Value::as_str)
            .map(|mode| match mode {
                "identity" => Ok(snapshot::SnapshotMode::Identity),
                "portable" => Ok(snapshot::SnapshotMode::Portable),
                _ => Err(Error::invalid("snapshot mode must be identity or portable")),
            })
            .transpose()?
            .unwrap_or(snapshot::SnapshotMode::Portable);
        let mutating = operation.risk != Risk::ReadOnly;
        let value = self
            .hub
            .execute(
                session_id.as_deref(),
                operation_name,
                expected_revision,
                Value::Object(object),
            )
            .await
            .map_err(|error| Self::bridge_error(error, mutating))?;
        if operation_name.starts_with("snapshot.") {
            Ok(snapshot::canonicalize(&value, snapshot_mode))
        } else {
            Ok(value)
        }
    }

    async fn health(&mut self) -> semwright_types::Result<Value> {
        let sessions = self.hub.sessions().await;
        Ok(json!({
            "healthy": true,
            "bridge_protocol": model::BRIDGE_PROTOCOL_VERSION,
            "listen_host": "127.0.0.1",
            "listen_port": self.hub.port(),
            "connected_sessions": sessions.len(),
            "motion": "beta",
            "plugin_api": "official"
        }))
    }
}

#[tokio::main]
async fn main() -> semwright_types::Result<()> {
    let driver = FigmaDriver::new().await?;
    semwright_driver_sdk::serve(driver).await
}
#[cfg(test)]
mod catalog_tests {
    use super::*;

    #[test]
    fn every_advertised_operation_has_strict_input_and_output_schema() {
        let unsupported = json!({"not":{}});
        for operation in advertised_operations() {
            let input = schemas::input_schema(operation.name);
            let output = schemas::output_schema(operation.name);
            assert_ne!(
                input, unsupported,
                "missing input schema: {}",
                operation.name
            );
            assert_ne!(
                output, unsupported,
                "missing output schema: {}",
                operation.name
            );
            assert_eq!(
                input.get("type").and_then(Value::as_str),
                Some("object"),
                "input must be an object schema: {}",
                operation.name
            );
            assert_eq!(
                input.get("additionalProperties"),
                Some(&Value::Bool(false)),
                "input must reject unknown properties: {}",
                operation.name
            );
        }
    }

    #[test]
    fn advertised_catalog_has_no_duplicate_names() {
        let operations = advertised_operations();
        let unique = operations
            .iter()
            .map(|op| op.name)
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(operations.len(), unique.len());
    }
}
