//! Translate the existing Blender native spatial readback into generic,
//! conservative Composition verdicts. Never evaluate an inferred material,
//! visual theme, object name or intended real-world physical relationship.

use semwright_semantic_composition::{
    SpatialClearanceRule, SpatialPairObservation, SurfaceIntersection, assess_spatial_clearance,
};
use semwright_types::{Error, ErrorCode, Result};
use serde_json::{Value, json};

fn protocol_error() -> Error {
    Error::new(
        ErrorCode::PluginProtocolError,
        "Native spatial measurement omitted or changed its bounded evidence contract",
    )
}

fn pair_matches(row: &Value, a: &str, b: &str) -> bool {
    (row["a"].as_str() == Some(a) && row["b"].as_str() == Some(b))
        || (row["a"].as_str() == Some(b) && row["b"].as_str() == Some(a))
}

pub(super) fn assess(rules: &[SpatialClearanceRule], measurement: &Value) -> Result<Value> {
    if measurement["coverage"] != "single_frame"
        || measurement["pair_measurements"]["method"] != "world-origin-and-aabb-v1"
        || measurement["mesh_pair_intersections"]["method"] != "world-triangle-segment-coplanar-v1"
    {
        return Err(protocol_error());
    }
    let frame = measurement["items"]
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row["frame"].as_i64())
        .ok_or_else(protocol_error)?;
    let native_session = measurement["native_session"]
        .as_str()
        .filter(|id| !id.is_empty() && id.len() <= 256)
        .ok_or_else(protocol_error)?;
    let measured = measurement["pair_measurements"]["pairs"]
        .as_array()
        .ok_or_else(protocol_error)?;
    let collisions = measurement["mesh_pair_intersections"]["pairs"]
        .as_array()
        .ok_or_else(protocol_error)?;
    let mut assessments = Vec::with_capacity(rules.len());
    for rule in rules {
        let matches: Vec<_> = measured
            .iter()
            .filter(|row| pair_matches(row, &rule.a, &rule.b))
            .collect();
        if matches.len() != 1 {
            return Err(protocol_error());
        }
        let pair = matches[0];
        let clearance =
            if pair["bounds_available"] == true && pair["contact_evidence"] == "AABB_ONLY" {
                pair["aabb_clearance_m"].as_f64()
            } else {
                None
            };
        let crossing: Vec<_> = collisions
            .iter()
            .filter(|row| pair_matches(row, &rule.a, &rule.b))
            .collect();
        if crossing.len() > 1 {
            return Err(protocol_error());
        }
        let intersection = match crossing.first() {
            Some(row)
                if row["verdict"] == "INTERSECT"
                    && row["narrow_phase"] == "INTERSECT"
                    && row["triangle_pairs_tested"].as_u64().unwrap_or(0) > 0 =>
            {
                SurfaceIntersection::Intersect
            }
            Some(row) if row["verdict"] == "SEPARATE" => SurfaceIntersection::Separate,
            _ => SurfaceIntersection::Unknown,
        };
        let assessment = assess_spatial_clearance(
            rule,
            Some(SpatialPairObservation {
                aabb_clearance_m: clearance,
                surface_intersection: intersection,
            }),
        )
        .map_err(|error| Error::invalid(error.to_string()))?;
        assessments.push(assessment);
    }
    Ok(json!({
        "schema":"semantic-spatial-clearance/v1",
        "scope":"single_evaluated_frame",
        "method":"world-aabb-lower-bound-and-native-triangle-intersection-v1",
        "native_session":native_session,
        "frame":frame,
        "checks":assessments
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(gap: f64, narrow: &str) -> Value {
        let verdict = if narrow == "INTERSECT" {
            "INTERSECT"
        } else {
            "SEPARATE"
        };
        json!({
            "coverage":"single_frame",
            "native_session":"sandbox-test-only",
            "items":[{"frame":12}],
            "pair_measurements":{
                "method":"world-origin-and-aabb-v1",
                "pairs":[{"a":"magnet-b","b":"magnet-a",
                    "bounds_available":true,
                    "aabb_clearance_m":gap,
                    "contact_evidence":"AABB_ONLY"}]
            },
            "mesh_pair_intersections":{
                "method":"world-triangle-segment-coplanar-v1",
                "complete":true,
                "pairs":[{"a":"magnet-a","b":"magnet-b",
                    "verdict":verdict,
                    "narrow_phase":narrow,
                    "triangle_pairs_tested":4}]
            }
        })
    }

    fn requirement() -> SpatialClearanceRule {
        SpatialClearanceRule {
            a: "magnet-a".into(),
            b: "magnet-b".into(),
            minimum_gap_m: 0.02,
        }
    }

    #[test]
    fn native_aabb_disjoint_certifies_single_frame_clearance() {
        let result = assess(&[requirement()], &fixture(0.4, "SEPARATE")).unwrap();
        assert_eq!(result["schema"], "semantic-spatial-clearance/v1");
        assert_eq!(result["frame"], 12);
        assert_eq!(result["checks"][0]["verdict"], "PASS");
        assert_eq!(result["checks"][0]["basis"], "world_aabb_lower_bound");
    }

    #[test]
    fn native_collision_is_fail_without_conflating_overlap_with_contact() {
        let intersect = assess(&[requirement()], &fixture(0.0, "INTERSECT")).unwrap();
        assert_eq!(intersect["checks"][0]["verdict"], "FAIL");
        let no_contact_proof = assess(&[requirement()], &fixture(0.0, "SEPARATE")).unwrap();
        assert_eq!(no_contact_proof["checks"][0]["verdict"], "UNKNOWN");
    }

    #[test]
    fn missing_or_inconsistent_native_evidence_never_produces_false_pass() {
        let contradiction = assess(&[requirement()], &fixture(0.4, "INTERSECT")).unwrap();
        assert_eq!(contradiction["checks"][0]["verdict"], "UNKNOWN");
        let mut unknown = fixture(0.0, "SEPARATE");
        unknown["mesh_pair_intersections"]["pairs"] = json!([]);
        assert_eq!(
            assess(&[requirement()], &unknown).unwrap()["checks"][0]["verdict"],
            "UNKNOWN"
        );
        unknown["pair_measurements"]["pairs"] = json!([]);
        assert!(assess(&[requirement()], &unknown).is_err());
        let mut source_only = fixture(0.4, "SEPARATE");
        source_only["coverage"] = "source_only".into();
        assert!(assess(&[requirement()], &source_only).is_err());
        let mut missing_identity = fixture(0.4, "SEPARATE");
        missing_identity["native_session"] = Value::Null;
        assert!(assess(&[requirement()], &missing_identity).is_err());
    }
}
