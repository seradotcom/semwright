//! Portable, explicitly authored semantic intent, not an inferred ontology.
//!
//! Role labels (branch, trunk, magnet, hinge, etc.) are advisory declarations.
//! Only explicit typed rules become checks. This module has no I/O, execution
//! authority, provider policy, model prompt or native dependencies.

use crate::{
    Digest, DirectParentRule, Result, SpatialClearanceRule, bounded_id, canonical_digest, ensure,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SEMANTIC_INTENT_VERSION: u32 = 1;
const MAX_ROLE_BINDINGS: usize = 64;
const MAX_INVARIANTS: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticRoleBinding {
    pub entity: String,
    /// An author-supplied tag, not a physical classifier or a permission.
    pub role: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SemanticInvariant {
    /// Blender parenting, DOM nesting, or another native tree relationship.
    /// Does not mean two meshes touch or are physically joined.
    DirectParent { child: String, parent: String },
    /// The explicitly requested minimum physical distance in meters.
    /// No such rule is automatically inferred from a role like "magnet".
    MinimumClearance {
        a: String,
        b: String,
        minimum_gap_m: f64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SemanticIntentManifest {
    pub version: u32,
    /// Caller-supplied provider/resource identity; the adapter must bind it
    /// to its currently authorized resource, never to a global name search.
    pub resource: String,
    pub roles: Vec<SemanticRoleBinding>,
    pub invariants: Vec<SemanticInvariant>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PreparedSemanticIntent {
    pub digest: Digest,
    pub roles: Vec<SemanticRoleBinding>,
    pub spatial_checks: Vec<SpatialClearanceRule>,
    pub direct_parents: Vec<DirectParentRule>,
}

fn valid_role_tag(name: &str) -> bool {
    name.len() <= 64
        && name.len() >= 2
        && name.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'-' | b'_' | b'.')
        })
}

impl SemanticIntentManifest {
    pub fn prepare(&self) -> Result<PreparedSemanticIntent> {
        ensure(
            self.version == SEMANTIC_INTENT_VERSION,
            "unsupported semantic intent version",
        )?;
        bounded_id(&self.resource)?;
        ensure(
            self.resource.len() <= 64
                && !self.roles.is_empty()
                && self.roles.len() <= MAX_ROLE_BINDINGS
                && !self.invariants.is_empty()
                && self.invariants.len() <= MAX_INVARIANTS,
            "semantic intent requires bounded declared roles and invariants",
        )?;

        let mut seen_roles = BTreeSet::new();
        for binding in &self.roles {
            bounded_id(&binding.entity)?;
            ensure(
                binding.entity.len() <= 64 && valid_role_tag(&binding.role),
                "semantic role requires bounded identity and lower-case role tag",
            )?;
            ensure(
                seen_roles.insert(binding.entity.as_str()),
                "duplicate semantic entity binding",
            )?;
        }

        let mut spatial_checks = Vec::new();
        let mut direct_parents = Vec::new();
        let mut parent_edges: BTreeMap<&str, &str> = BTreeMap::new();
        let mut spatial_edges = BTreeSet::new();

        for invariant in &self.invariants {
            match invariant {
                SemanticInvariant::DirectParent { child, parent } => {
                    let rule = DirectParentRule {
                        child: child.clone(),
                        parent: parent.clone(),
                    };
                    rule.validate()?;
                    ensure(
                        seen_roles.contains(child.as_str()) && seen_roles.contains(parent.as_str()),
                        "direct-parent subjects must have explicit role bindings",
                    )?;
                    ensure(
                        parent_edges
                            .insert(child.as_str(), parent.as_str())
                            .is_none(),
                        "a child has more than one expected direct parent",
                    )?;
                    direct_parents.push(rule);
                }
                SemanticInvariant::MinimumClearance {
                    a,
                    b,
                    minimum_gap_m,
                } => {
                    let rule = SpatialClearanceRule {
                        a: a.clone(),
                        b: b.clone(),
                        minimum_gap_m: *minimum_gap_m,
                    };
                    rule.validate()?;
                    ensure(
                        seen_roles.contains(a.as_str()) && seen_roles.contains(b.as_str()),
                        "clearance subjects must have explicit role bindings",
                    )?;
                    let edge = if a < b {
                        (a.as_str(), b.as_str())
                    } else {
                        (b.as_str(), a.as_str())
                    };
                    ensure(
                        spatial_edges.insert(edge),
                        "duplicate or contradictory clearance rule for one pair",
                    )?;
                    spatial_checks.push(rule);
                }
            }
        }

        for starting in parent_edges.keys() {
            let mut node = *starting;
            let mut visited = BTreeSet::new();
            while let Some(parent) = parent_edges.get(node) {
                ensure(
                    visited.insert(node),
                    "explicit parent relations cannot form a cycle",
                )?;
                node = parent;
            }
        }

        let digest = canonical_digest(self)?;
        Ok(PreparedSemanticIntent {
            digest,
            roles: self.roles.clone(),
            spatial_checks,
            direct_parents,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{canonical_bytes, strict_decode};
    use serde_json::json;

    fn tree_and_magnets() -> SemanticIntentManifest {
        SemanticIntentManifest {
            version: 1,
            resource: "scene-42".into(),
            roles: vec![
                SemanticRoleBinding {
                    entity: "trunk".into(),
                    role: "tree.trunk".into(),
                },
                SemanticRoleBinding {
                    entity: "branch".into(),
                    role: "tree.branch".into(),
                },
                SemanticRoleBinding {
                    entity: "magnet-left".into(),
                    role: "magnet".into(),
                },
                SemanticRoleBinding {
                    entity: "magnet-right".into(),
                    role: "magnet".into(),
                },
            ],
            invariants: vec![
                SemanticInvariant::DirectParent {
                    child: "branch".into(),
                    parent: "trunk".into(),
                },
                SemanticInvariant::MinimumClearance {
                    a: "magnet-left".into(),
                    b: "magnet-right".into(),
                    minimum_gap_m: 0.02,
                },
            ],
        }
    }

    #[test]
    fn explicit_invariants_compile_to_native_checks_and_stable_digest() {
        let source = tree_and_magnets();
        let prepared = source.prepare().unwrap();
        assert_eq!(prepared.direct_parents.len(), 1);
        assert_eq!(prepared.spatial_checks.len(), 1);
        assert_eq!(prepared.spatial_checks[0].minimum_gap_m, 0.02);
        assert_eq!(prepared.roles.len(), 4);
        let bytes = canonical_bytes(&source).unwrap();
        let recovered: SemanticIntentManifest = strict_decode(&bytes).unwrap();
        assert_eq!(recovered, source);
        assert_eq!(recovered.prepare().unwrap().digest, prepared.digest);
        assert_eq!(prepared.digest.as_str().len(), 64);
    }

    #[test]
    fn name_only_labels_never_generate_implicit_physical_rules() {
        let mut source = tree_and_magnets();
        source
            .invariants
            .retain(|item| matches!(item, SemanticInvariant::DirectParent { .. }));
        let prepared = source.prepare().unwrap();
        assert_eq!(prepared.spatial_checks.len(), 0);
        assert_eq!(prepared.direct_parents.len(), 1);
        source.invariants.clear();
        assert!(source.prepare().is_err());
    }

    #[test]
    fn missing_or_duplicate_subject_bindings_and_unknown_roles_are_denied() {
        let mut source = tree_and_magnets();
        source.roles.pop();
        assert!(source.prepare().is_err());
        let mut source = tree_and_magnets();
        source.roles.push(source.roles[0].clone());
        assert!(source.prepare().is_err());
        let mut source = tree_and_magnets();
        source.roles[0].role = "MAGNET_EXTERNAL_CODE".into();
        assert!(source.prepare().is_err());
    }

    #[test]
    fn duplicated_or_cyclic_invariants_and_nonfinite_clearance_fail() {
        let mut source = tree_and_magnets();
        source.invariants.push(SemanticInvariant::DirectParent {
            child: "trunk".into(),
            parent: "branch".into(),
        });
        assert!(source.prepare().is_err());
        let mut source = tree_and_magnets();
        source.invariants.push(SemanticInvariant::MinimumClearance {
            a: "magnet-right".into(),
            b: "magnet-left".into(),
            minimum_gap_m: 10.0,
        });
        assert!(source.prepare().is_err());
        let mut source = tree_and_magnets();
        source.invariants[1] = SemanticInvariant::MinimumClearance {
            a: "magnet-left".into(),
            b: "magnet-right".into(),
            minimum_gap_m: f64::NAN,
        };
        assert!(source.prepare().is_err());
    }

    #[test]
    fn version_and_extra_fields_are_rejected_without_execution() {
        let mut source = tree_and_magnets();
        source.version = 2;
        assert!(source.prepare().is_err());
        let mut json = serde_json::to_value(tree_and_magnets()).unwrap();
        json["execute_python"] = json!("bpy.ops.object.delete()");
        assert!(serde_json::from_value::<SemanticIntentManifest>(json).is_err());
        let duplicate_json =
            br#"{"version":1,"version":1,"resource":"scene","roles":[],"invariants":[]}"#;
        assert!(strict_decode::<SemanticIntentManifest>(duplicate_json).is_err());
    }

    #[test]
    fn digest_changes_when_intent_changes_not_when_wire_object_keys_reorder() {
        let source = tree_and_magnets();
        let baseline = source.prepare().unwrap().digest;
        let reordered_keys = json!({
            "invariants":source.invariants,
            "roles":source.roles,
            "resource":source.resource,
            "version":1
        });
        let canonical: SemanticIntentManifest =
            strict_decode(&serde_json::to_vec(&reordered_keys).unwrap()).unwrap();
        assert_eq!(canonical.prepare().unwrap().digest, baseline);
        let mut changed = canonical;
        changed.invariants[1] = SemanticInvariant::MinimumClearance {
            a: "magnet-left".into(),
            b: "magnet-right".into(),
            minimum_gap_m: 0.03,
        };
        assert_ne!(changed.prepare().unwrap().digest, baseline);
    }
}
