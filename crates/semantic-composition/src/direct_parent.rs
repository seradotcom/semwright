//! Backend-neutral managed-hierarchy evidence. Native parenting is not
//! geometric welding or proof of contact between meshes.
//!
//! A rule can only PASS from an exhaustive native source snapshot whose
//! identities belong to the same managed graph. Absent or unbounded evidence
//! remains UNKNOWN; none of these verdicts grants mutation authority.
use crate::{Result, Verdict, bounded_id, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DirectParentRule {
    pub child: String,
    pub parent: String,
}

impl DirectParentRule {
    pub fn validate(&self) -> Result<()> {
        bounded_id(&self.child)?;
        bounded_id(&self.parent)?;
        ensure(
            self.child != self.parent && self.child.len() <= 128 && self.parent.len() <= 128,
            "direct-parent rule requires two distinct bounded identities",
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectParentObservation {
    /// Current managed source-only parent ID; None means a known root.
    pub parent: Option<String>,
    /// A reference to a nonexistent parent is not evidence of a valid edge.
    pub requested_parent_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DirectParentAssessment {
    pub child: String,
    pub parent: String,
    pub verdict: Verdict,
    pub basis: String,
    pub scope: String,
    pub reason: String,
    /// Parenting preserves transform hierarchy; it does NOT prove mesh weld.
    pub geometric_attachment_verified: bool,
}

pub fn assess_direct_parent(
    rule: &DirectParentRule,
    observation: Option<&DirectParentObservation>,
    source_exhaustive: bool,
) -> Result<DirectParentAssessment> {
    rule.validate()?;

    let (verdict, basis, reason) = match (source_exhaustive, observation) {
        (false, _) => (
            Verdict::Unknown,
            "incomplete_native_source",
            "The managed source hierarchy was not exhaustively observed",
        ),
        (true, None) => (
            Verdict::Unknown,
            "missing_managed_child",
            "The requested child identity is not present in the native snapshot",
        ),
        (true, Some(observed)) if !observed.requested_parent_present => (
            Verdict::Unknown,
            "missing_managed_parent",
            "The requested parent has not been observed in this managed source",
        ),
        (true, Some(observed)) if observed.parent.as_deref() == Some(rule.parent.as_str()) => (
            Verdict::Pass,
            "native_managed_parent_id",
            "The child directly references the requested managed parent",
        ),
        (true, Some(_)) => (
            Verdict::Fail,
            "native_managed_parent_id",
            "The child has no parent or a different observed direct parent",
        ),
    };

    Ok(DirectParentAssessment {
        child: rule.child.clone(),
        parent: rule.parent.clone(),
        verdict,
        basis: basis.into(),
        scope: "managed_source_hierarchy".into(),
        reason: reason.into(),
        geometric_attachment_verified: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule() -> DirectParentRule {
        DirectParentRule {
            child: "branch".into(),
            parent: "trunk".into(),
        }
    }

    #[test]
    fn observed_direct_parent_is_pass_without_claiming_mesh_weld() {
        let observed = DirectParentObservation {
            parent: Some("trunk".into()),
            requested_parent_present: true,
        };
        let result = assess_direct_parent(&rule(), Some(&observed), true).unwrap();
        assert_eq!(result.verdict, Verdict::Pass);
        assert!(!result.geometric_attachment_verified);
        assert_eq!(result.basis, "native_managed_parent_id");
    }

    #[test]
    fn changed_parent_or_known_root_is_a_failed_hierarchy_rule() {
        for parent in [Some("other".into()), None] {
            let observed = DirectParentObservation {
                parent,
                requested_parent_present: true,
            };
            let result = assess_direct_parent(&rule(), Some(&observed), true).unwrap();
            assert_eq!(result.verdict, Verdict::Fail);
        }
    }

    #[test]
    fn incomplete_or_missing_native_identity_never_passes() {
        let observed = DirectParentObservation {
            parent: Some("trunk".into()),
            requested_parent_present: true,
        };
        assert_eq!(
            assess_direct_parent(&rule(), Some(&observed), false)
                .unwrap()
                .verdict,
            Verdict::Unknown
        );
        assert_eq!(
            assess_direct_parent(&rule(), None, true).unwrap().verdict,
            Verdict::Unknown
        );
        let stale_target = DirectParentObservation {
            requested_parent_present: false,
            ..observed
        };
        assert_eq!(
            assess_direct_parent(&rule(), Some(&stale_target), true)
                .unwrap()
                .verdict,
            Verdict::Unknown
        );
    }

    #[test]
    fn invalid_identity_never_becomes_an_observation() {
        let mut self_parent = rule();
        self_parent.parent = self_parent.child.clone();
        assert!(assess_direct_parent(&self_parent, None, true).is_err());
        self_parent.parent = "x".repeat(129);
        assert!(assess_direct_parent(&self_parent, None, true).is_err());
    }
}
