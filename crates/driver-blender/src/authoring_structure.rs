//! Bounded direct-parent checks over the existing first-party managed Blender
//! source snapshot. The source graph is native evidence; parenting never proves
//! geometry contact or mesh welding. No additional native mutation is permitted.

use schemars::JsonSchema;
use semwright_driver_blender::authoring::{NativeSnapshot, local_id};
use semwright_semantic_composition as composition;
use semwright_types::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const MAX_RULES: usize = 16;
const MAX_NATIVE_ENTITIES: usize = 128;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CheckInput {
    pub island: String,
    pub checks: Vec<composition::DirectParentRule>,
}

impl CheckInput {
    pub(super) fn validate(&self) -> composition::Result<()> {
        local_id(&self.island)?;
        composition::ensure(
            !self.checks.is_empty() && self.checks.len() <= MAX_RULES,
            "structure check requires 1..16 direct-parent rules",
        )?;
        let mut graph = BTreeMap::new();
        for rule in &self.checks {
            rule.validate()?;
            local_id(&rule.child)?;
            local_id(&rule.parent)?;
            composition::ensure(
                graph
                    .insert(rule.child.clone(), rule.parent.clone())
                    .is_none(),
                "a child cannot have two expected direct parents",
            )?;
        }
        // Cycle detection is independent of Blender and does not query native
        // state. A cyclic hierarchy can never be an admissible desired scene.
        for child in graph.keys() {
            let mut current = child.as_str();
            let mut visited = BTreeSet::new();
            while let Some(parent) = graph.get(current) {
                composition::ensure(
                    visited.insert(current.to_owned()),
                    "direct-parent rules cannot form a cycle",
                )?;
                current = parent;
            }
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(super) struct CheckOutput {
    pub schema: String,
    pub island: String,
    pub native_session: String,
    pub fingerprint: composition::Digest,
    pub complete: bool,
    pub checks: Vec<composition::DirectParentAssessment>,
}

pub(super) fn input_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(CheckInput))
        .expect("Native structure input schema is serializable")
}

pub(super) fn output_schema() -> Value {
    serde_json::to_value(schemars::schema_for!(CheckOutput))
        .expect("Native structure output schema is serializable")
}

fn malformed() -> Error {
    Error::new(
        ErrorCode::PluginProtocolError,
        "Native source hierarchy returned an invalid or ambiguous managed identity",
    )
}

pub(super) fn assess(input: &CheckInput, snapshot: &NativeSnapshot) -> Result<CheckOutput> {
    input
        .validate()
        .map_err(|error| Error::invalid(error.to_string()))?;
    if snapshot.island.as_deref() != Some(input.island.as_str())
        || snapshot.native_session.is_empty()
        || snapshot.native_session.len() > 256
        || snapshot.total > MAX_NATIVE_ENTITIES
        || snapshot.items.len() > MAX_NATIVE_ENTITIES
    {
        return Err(malformed());
    }

    let mut items = BTreeMap::new();
    for row in &snapshot.items {
        let entity = row["entity"].as_str().ok_or_else(malformed)?;
        local_id(entity).map_err(|_| malformed())?;
        if row["native_island"].as_str() != Some(input.island.as_str())
            || items.insert(entity, row).is_some()
        {
            return Err(malformed());
        }
        let parent = row.get("parent").ok_or_else(malformed)?;
        if !(parent.is_string() || parent.is_null()) {
            return Err(malformed());
        }
    }
    let complete = snapshot.source_only
        && snapshot.exhaustive
        && !snapshot.drift
        && snapshot.total == items.len();

    let mut result = Vec::with_capacity(input.checks.len());
    for rule in &input.checks {
        let observed =
            items
                .get(rule.child.as_str())
                .map(|row| composition::DirectParentObservation {
                    parent: row["parent"].as_str().map(str::to_owned),
                    requested_parent_present: items.contains_key(rule.parent.as_str()),
                });
        result.push(
            composition::assess_direct_parent(rule, observed.as_ref(), complete)
                .map_err(|error| Error::invalid(error.to_string()))?,
        );
    }
    Ok(CheckOutput {
        schema: "composition-direct-parent/v1".into(),
        island: input.island.clone(),
        native_session: snapshot.native_session.clone(),
        fingerprint: snapshot.fingerprint.clone(),
        complete,
        checks: result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn input() -> CheckInput {
        serde_json::from_value(json!({
            "island":"tree",
            "checks":[{"child":"branch-left","parent":"trunk"}]
        }))
        .unwrap()
    }

    fn snapshot(parent: Value) -> NativeSnapshot {
        serde_json::from_value(json!({
            "native_session":"native-parent-test",
            "island":"tree",
            "fingerprint":"a".repeat(64),
            "drift":false,
            "total":2,
            "items":[
                {"entity":"trunk","native_island":"tree","parent":null},
                {"entity":"branch-left","native_island":"tree","parent":parent}
            ],
            "source_only":true,
            "exhaustive":true
        }))
        .unwrap()
    }

    #[test]
    fn directly_parented_branch_is_pass_without_geometric_weld_claim() {
        let observed = assess(&input(), &snapshot(json!("trunk"))).unwrap();
        let encoded = serde_json::to_value(&observed).unwrap();
        assert_eq!(encoded["schema"], "composition-direct-parent/v1");
        assert_eq!(encoded["checks"][0]["verdict"], "PASS");
        assert_eq!(encoded["checks"][0]["geometric_attachment_verified"], false);
        assert_eq!(encoded["complete"], true);
    }

    #[test]
    fn unparented_or_wrong_parent_are_confirmed_native_failures() {
        for parent in [Value::Null, json!("other")] {
            let result = assess(&input(), &snapshot(parent)).unwrap();
            assert_eq!(
                serde_json::to_value(&result).unwrap()["checks"][0]["verdict"],
                "FAIL"
            );
        }
    }

    #[test]
    fn missing_parent_incomplete_scene_or_manual_drift_cannot_pass() {
        let mut observation = snapshot(json!("trunk"));
        observation.items.remove(0);
        observation.total = 1;
        assert_eq!(
            serde_json::to_value(assess(&input(), &observation).unwrap()).unwrap()["checks"][0]["verdict"],
            "UNKNOWN"
        );
        let mut observation = snapshot(json!("trunk"));
        observation.drift = true;
        assert_eq!(
            serde_json::to_value(assess(&input(), &observation).unwrap()).unwrap()["checks"][0]["verdict"],
            "UNKNOWN"
        );
        let mut observation = snapshot(json!("trunk"));
        observation.exhaustive = false;
        assert_eq!(
            serde_json::to_value(assess(&input(), &observation).unwrap()).unwrap()["checks"][0]["verdict"],
            "UNKNOWN"
        );
    }

    #[test]
    fn ambiguous_native_identity_or_foreign_island_is_refused() {
        let mut observation = snapshot(json!("trunk"));
        observation.items[1]["entity"] = json!("trunk");
        assert!(assess(&input(), &observation).is_err());
        let mut observation = snapshot(json!("trunk"));
        observation.items[1]["native_island"] = json!("other");
        assert!(assess(&input(), &observation).is_err());
        let mut observation = snapshot(json!("trunk"));
        observation.items[1]["parent"] = json!(42);
        assert!(assess(&input(), &observation).is_err());
        let mut observation = snapshot(json!("trunk"));
        observation.items[1]
            .as_object_mut()
            .unwrap()
            .remove("parent");
        assert!(assess(&input(), &observation).is_err());
    }

    #[test]
    fn requested_parent_cycles_and_extra_fields_are_rejected() {
        let mutually_parented: CheckInput = serde_json::from_value(json!({
            "island":"tree",
            "checks":[{"child":"branch","parent":"trunk"},
                      {"child":"trunk","parent":"branch"}]
        }))
        .unwrap();
        assert!(mutually_parented.validate().is_err());

        let duplicate: CheckInput = serde_json::from_value(json!({
            "island":"tree",
            "checks":[{"child":"branch","parent":"trunk"},
                      {"child":"branch","parent":"root"}]
        }))
        .unwrap();
        assert!(duplicate.validate().is_err());

        let unknown_field: std::result::Result<CheckInput, _> = serde_json::from_value(json!({
            "island":"tree",
            "checks":[{"child":"branch","parent":"trunk","native_ops":"bpy.ops"}]
        }));
        assert!(unknown_field.is_err());
    }

    #[test]
    fn published_transport_schemas_accept_only_bounded_direct_parent_contract() {
        let input_validator = jsonschema::validator_for(&input_schema()).unwrap();
        let output_validator = jsonschema::validator_for(&output_schema()).unwrap();
        let input_json = serde_json::to_value(input()).unwrap();
        assert!(input_validator.is_valid(&input_json));
        let observed = assess(&input(), &snapshot(json!("trunk"))).unwrap();
        assert!(output_validator.is_valid(&serde_json::to_value(observed).unwrap()));
        let mut invalid = input_json;
        invalid["arbitrary_python"] = json!("print(1)");
        assert!(!input_validator.is_valid(&invalid));
    }
}
