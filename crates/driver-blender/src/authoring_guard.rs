//! Require actual native evidence before an optional guarded Blender save.
//!
//! This inspects readbacks only. It does not write a sidecar, grant a file
//! capability, dispatch a mutation, or infer physics from semantic role tags.

use semwright_driver_blender::authoring::NativeSnapshot;
use semwright_semantic_composition as composition;
use semwright_types::{Error, ErrorCode, Result};
use serde_json::{Value, json};

use super::authoring_structure;

fn malformed() -> Error {
    Error::new(
        ErrorCode::PluginProtocolError,
        "Guarded save omitted source-bound native semantic evidence",
    )
}

fn no_pass() -> Error {
    Error::new(
        ErrorCode::Conflict,
        "Guarded save requires all declared native rules to have PASS evidence",
    )
}

pub(super) fn check(
    island: &str,
    manifest: &composition::SemanticIntentManifest,
    prepared: &composition::PreparedSemanticIntent,
    source: &NativeSnapshot,
    measurement: Option<&Value>,
) -> Result<Value> {
    if manifest.resource != island
        || source.island.as_deref() != Some(island)
        || source.native_session.is_empty()
        || source.drift
        || !source.exhaustive
        || !source.source_only
        || source.total != source.items.len()
    {
        return Err(no_pass());
    }

    let spatial = if prepared.spatial_checks.is_empty() {
        if measurement.is_some() {
            return Err(malformed());
        }
        None
    } else {
        let output = measurement.ok_or_else(malformed)?;
        if output["native_session"].as_str() != Some(source.native_session.as_str())
            || output["combined_source_consistent"] != true
            || output["combined_source_fingerprint"].as_str() != Some(source.fingerprint.as_str())
            || output["declared_intent"]["manifest_sha256"].as_str()
                != Some(prepared.digest.as_str())
            || output["declared_intent"]["inferred_rules"] != false
            || output["declared_intent"]["stored_in_native_scene"] != false
        {
            return Err(malformed());
        }
        let rows = output["spatial_rules"]["checks"]
            .as_array()
            .ok_or_else(malformed)?;
        if rows.len() != prepared.spatial_checks.len() {
            return Err(malformed());
        }
        for (row, rule) in rows.iter().zip(&prepared.spatial_checks) {
            if row["a"].as_str() != Some(rule.a.as_str())
                || row["b"].as_str() != Some(rule.b.as_str())
                || row["minimum_gap_m"].as_f64() != Some(rule.minimum_gap_m)
            {
                return Err(malformed());
            }
            if row["verdict"] != "PASS" {
                return Err(no_pass());
            }
        }
        Some(output["spatial_rules"].clone())
    };

    let hierarchy = if prepared.direct_parents.is_empty() {
        None
    } else {
        let bound = authoring_structure::assess(
            &authoring_structure::CheckInput {
                island: island.to_owned(),
                checks: prepared.direct_parents.clone(),
            },
            source,
        )?;
        let value = serde_json::to_value(bound)?;
        let rows = value["checks"].as_array().ok_or_else(malformed)?;
        if rows.len() != prepared.direct_parents.len()
            || value["complete"] != true
            || value["native_session"].as_str() != Some(source.native_session.as_str())
            || value["fingerprint"].as_str() != Some(source.fingerprint.as_str())
        {
            return Err(malformed());
        }
        for (row, rule) in rows.iter().zip(&prepared.direct_parents) {
            if row["child"].as_str() != Some(rule.child.as_str())
                || row["parent"].as_str() != Some(rule.parent.as_str())
                || row["geometric_attachment_verified"] != false
            {
                return Err(malformed());
            }
            if row["verdict"] != "PASS" {
                return Err(no_pass());
            }
        }
        Some(value)
    };

    Ok(json!({
        "schema":"semantic-guarded-persist/v1",
        "verdict":"PASS",
        "manifest_sha256":prepared.digest,
        "manifest":manifest,
        "native_session":source.native_session,
        "source_fingerprint":source.fingerprint,
        "spatial":spatial,
        "structural":hierarchy,
        "inferred_rules":false,
        "stored_in_native_scene":false,
        "requires_external_manifest_persistence":true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn manifest() -> composition::SemanticIntentManifest {
        serde_json::from_value(json!({
            "version":1,"resource":"managed-test",
            "roles":[
                {"entity":"tree","role":"tree.trunk"},
                {"entity":"branch","role":"tree.branch"},
                {"entity":"magnet-left","role":"magnet"},
                {"entity":"magnet-right","role":"magnet"}
            ],
            "invariants":[
                {"kind":"direct_parent","child":"branch","parent":"tree"},
                {"kind":"minimum_clearance","a":"magnet-left","b":"magnet-right",
                 "minimum_gap_m":0.02}
            ]
        }))
        .unwrap()
    }

    fn source() -> NativeSnapshot {
        serde_json::from_value(json!({
            "native_session":"native-test",
            "island":"managed-test",
            "fingerprint":"a".repeat(64),
            "drift":false,
            "total":4,
            "items":[
                {"entity":"tree","native_island":"managed-test","parent":null},
                {"entity":"branch","native_island":"managed-test","parent":"tree"},
                {"entity":"magnet-left","native_island":"managed-test","parent":null},
                {"entity":"magnet-right","native_island":"managed-test","parent":null}
            ],
            "source_only":true,"exhaustive":true
        }))
        .unwrap()
    }

    fn measurement(digest: &str, verdict: &str) -> Value {
        json!({
            "native_session":"native-test",
            "combined_source_consistent":true,
            "combined_source_fingerprint":"a".repeat(64),
            "declared_intent":{
                "manifest_sha256":digest,
                "inferred_rules":false,
                "stored_in_native_scene":false
            },
            "spatial_rules":{
                "checks":[{"a":"magnet-left","b":"magnet-right",
                           "minimum_gap_m":0.02,"verdict":verdict}]
            }
        })
    }

    #[test]
    fn all_pass_native_rules_produce_content_bound_new_only_receipt() {
        let manifest = manifest();
        let prepared = manifest.prepare().unwrap();
        let output = check(
            "managed-test",
            &manifest,
            &prepared,
            &source(),
            Some(&measurement(prepared.digest.as_str(), "PASS")),
        )
        .unwrap();
        assert_eq!(output["verdict"], "PASS");
        assert_eq!(output["manifest_sha256"], prepared.digest.as_str());
        assert_eq!(output["structural"]["checks"][0]["verdict"], "PASS");
        assert_eq!(output["stored_in_native_scene"], false);
        assert_eq!(output["requires_external_manifest_persistence"], true);
    }

    #[test]
    fn native_unknown_or_failed_clearance_refuses_save() {
        let manifest = manifest();
        let prepared = manifest.prepare().unwrap();
        for verdict in ["UNKNOWN", "FAIL"] {
            assert!(
                check(
                    "managed-test",
                    &manifest,
                    &prepared,
                    &source(),
                    Some(&measurement(prepared.digest.as_str(), verdict))
                )
                .is_err()
            );
        }
    }

    #[test]
    fn stale_session_or_changed_digest_or_missing_rows_never_pass() {
        let manifest = manifest();
        let prepared = manifest.prepare().unwrap();
        let mut stale = measurement(prepared.digest.as_str(), "PASS");
        stale["native_session"] = json!("other");
        assert!(
            check(
                "managed-test",
                &manifest,
                &prepared,
                &source(),
                Some(&stale)
            )
            .is_err()
        );

        let wrong = measurement(&"b".repeat(64), "PASS");
        assert!(
            check(
                "managed-test",
                &manifest,
                &prepared,
                &source(),
                Some(&wrong)
            )
            .is_err()
        );

        let mut empty = measurement(prepared.digest.as_str(), "PASS");
        empty["spatial_rules"]["checks"] = json!([]);
        assert!(
            check(
                "managed-test",
                &manifest,
                &prepared,
                &source(),
                Some(&empty)
            )
            .is_err()
        );
    }

    #[test]
    fn changed_parent_or_incomplete_source_refuses_save() {
        let manifest = manifest();
        let prepared = manifest.prepare().unwrap();
        let result = measurement(prepared.digest.as_str(), "PASS");
        let mut changed = source();
        changed.items[1]["parent"] = json!("magnet-left");
        assert!(
            check(
                "managed-test",
                &manifest,
                &prepared,
                &changed,
                Some(&result)
            )
            .is_err()
        );
        let mut incomplete = source();
        incomplete.exhaustive = false;
        assert!(
            check(
                "managed-test",
                &manifest,
                &prepared,
                &incomplete,
                Some(&result)
            )
            .is_err()
        );
        let mut drift = source();
        drift.drift = true;
        assert!(check("managed-test", &manifest, &prepared, &drift, Some(&result)).is_err());
    }

    #[test]
    fn hierarchy_only_uses_trusted_source_without_evaluated_measurement() {
        let mut manifest = manifest();
        manifest
            .invariants
            .retain(|value| matches!(value, composition::SemanticInvariant::DirectParent { .. }));
        let prepared = manifest.prepare().unwrap();
        let result = check("managed-test", &manifest, &prepared, &source(), None).unwrap();
        assert!(result["spatial"].is_null());
        assert_eq!(result["structural"]["checks"][0]["verdict"], "PASS");
        assert!(check("other", &manifest, &prepared, &source(), None).is_err());
    }
}
