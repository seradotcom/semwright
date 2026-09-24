use super::{op, Op};
use semwright_types::{Idempotency, Risk};

pub(super) fn operations() -> Vec<Op> {
    use Idempotency::{Idempotent, ReadOnly};
    use Risk::{MutatingReversible, ReadOnly as ReadRisk, SecretAccess};

    vec![
        op(
            "style.order.after",
            "Reorder a local style after another style of the same kind",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "style.folder.order.after",
            "Reorder a local style folder after a sibling folder",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "slides.grid.inspect",
            "Inspect the legacy Slides grid using the official Plugin API",
            ReadRisk,
            ReadOnly,
            true,
        ),
        op(
            "slides.grid.set",
            "Replace the legacy Slides grid with an explicit complete node grid",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "annotation.category.inspect",
            "Inspect one annotation category by ID",
            ReadRisk,
            ReadOnly,
            true,
        ),
        op(
            "font.load",
            "Load a font family/style for subsequent text mutation",
            ReadRisk,
            Idempotent,
            true,
        ),
        op(
            "dev.focused_node",
            "Inspect the focused node in Dev Mode, Slides, or Buzz",
            ReadRisk,
            ReadOnly,
            true,
        ),
        op(
            "codegen.status",
            "Inspect Figma Dev Mode codegen preferences and mode",
            ReadRisk,
            ReadOnly,
            true,
        ),
        op(
            "codegen.refresh",
            "Request a refresh of the active Dev Mode codegen output",
            MutatingReversible,
            Idempotent,
            true,
        ),
        op(
            "user.current",
            "Read the current Figma user when the currentuser permission is granted",
            SecretAccess,
            ReadOnly,
            false,
        ),
        op(
            "figjam.active_users",
            "Read bounded active FigJam collaborators when activeusers permission is granted",
            SecretAccess,
            ReadOnly,
            false,
        ),
    ]
}
