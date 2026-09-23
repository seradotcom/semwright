//! Differential conformance helpers for concrete video backends.
//!
//! A backend may keep arbitrary native round-trip metadata, but after a
//! supported mutation its semantic projection must match the shared edit
//! engine exactly. This module makes that rule executable.

use crate::{
    Error, Result,
    edit::{self, Edit, EditOutcome},
    model::Project,
};

pub fn verify_backend_mutation(
    before: &Project,
    edit: Edit,
    seed: &str,
    identity_base: &str,
    projected_after: &Project,
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
            "Backend mutation diverged from the portable semantic video edit engine",
        ));
    }
    Ok(expected)
}
