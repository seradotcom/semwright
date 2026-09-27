use super::{Op, op};
use semwright_types::{Idempotency, Risk};

pub(super) fn operations() -> Vec<Op> {
    use Idempotency::{Idempotent, NonIdempotent, ReadOnly};
    use Risk::{MutatingReversible, ReadOnly as ReadRisk};
    vec![
        op(
            "composition.inspect",
            "Inspect bounded Figma composition and design-system semantics",
            ReadRisk,
            ReadOnly,
            true,
        ),
        op(
            "composition.plan",
            "Compile bounded structured composition intent into a revision-bound Figma ChangeSet",
            ReadRisk,
            ReadOnly,
            true,
        ),
        op(
            "composition.apply",
            "Apply an authorized revision-bound semantic Figma ChangeSet",
            MutatingReversible,
            NonIdempotent,
            false,
        ),
        op(
            "composition.measure",
            "Measure actual bounded Figma geometry and native semantic state",
            ReadRisk,
            ReadOnly,
            true,
        ),
        op(
            "composition.validate",
            "Validate deterministic composition constraints against observed Figma state",
            ReadRisk,
            ReadOnly,
            true,
        ),
        op(
            "composition.repair.plan",
            "Compile deterministic findings into a bounded revision-bound repair ChangeSet",
            ReadRisk,
            ReadOnly,
            true,
        ),
        op(
            "composition.repair.apply",
            "Apply an authorized deterministic repair ChangeSet and report observed effects",
            MutatingReversible,
            Idempotent,
            false,
        ),
        op(
            "composition.verify",
            "Return semantic measurement, validation findings and artifact-backed visual evidence",
            ReadRisk,
            ReadOnly,
            true,
        ),
    ]
}
