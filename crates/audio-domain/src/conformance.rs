//! Differential conformance helpers for concrete audio backends.

use crate::{
    Error, Result,
    edit::{self, Edit, EditOutcome},
    model::AudioProject,
};

pub fn verify_backend_mutation(
    before: &AudioProject,
    edit: Edit,
    seed: &str,
    identity_base: &str,
    projected_after: &AudioProject,
    backend_affected: &[String],
    backend_created: &[String],
) -> Result<EditOutcome> {
    let expected = edit::apply_with_identity_base(before, edit, seed, identity_base)?;
    if expected.result != *projected_after
        || expected.affected != backend_affected
        || expected.created != backend_created
        || expected.before_frames != before.duration()
        || expected.after_frames != projected_after.duration()
    {
        return Err(Error::new(
            "BackendFailed",
            "Backend mutation diverged from the portable semantic audio edit engine",
        ));
    }
    Ok(expected)
}
