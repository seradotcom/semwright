//! Deterministic, backend-neutral clearance checks from bounded spatial evidence.
//!
//! AABB separation is a conservative lower bound on distance between objects.
//! AABB overlap never proves physical contact. A positive native surface
//! intersection can prove failure; absent/incomplete evidence remains UNKNOWN.
//! These verdicts are per-observed-frame, never an animation-wide guarantee.

use crate::{Result, Verdict, bounded_id, ensure};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

const ROBUST_GAP_M: f64 = 0.000_001;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpatialClearanceRule {
    pub a: String,
    pub b: String,
    /// Required lower bound in meters. Zero requests strict non-contact.
    pub minimum_gap_m: f64,
}

impl SpatialClearanceRule {
    pub fn validate(&self) -> Result<()> {
        bounded_id(&self.a)?;
        bounded_id(&self.b)?;
        ensure(
            self.a != self.b
                && self.a.len() <= 128
                && self.b.len() <= 128
                && self.minimum_gap_m.is_finite()
                && (0.0..=1000.0).contains(&self.minimum_gap_m),
            "clearance rule requires distinct bounded entities and a finite meter gap",
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceIntersection {
    /// Surface triangles were directly observed intersecting.
    Intersect,
    /// Tested surface triangles did not intersect; containment is unproved.
    Separate,
    /// The native surface query is absent, incomplete, or unsupported.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SpatialPairObservation {
    /// Proven minimum distance between the two world-space AABBs.
    pub aabb_clearance_m: Option<f64>,
    pub surface_intersection: SurfaceIntersection,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SpatialClearanceAssessment {
    pub a: String,
    pub b: String,
    pub minimum_gap_m: f64,
    pub aabb_lower_bound_m: Option<f64>,
    pub verdict: Verdict,
    pub basis: String,
    pub scope: String,
    pub reason: String,
}

/// This is advisory evidence, not permission to mutate an application.
/// No operation is retried and UNKNOWN is never promoted to PASS.
pub fn assess_spatial_clearance(
    rule: &SpatialClearanceRule,
    observation: Option<SpatialPairObservation>,
) -> Result<SpatialClearanceAssessment> {
    rule.validate()?;
    let observed = observation.unwrap_or(SpatialPairObservation {
        aabb_clearance_m: None,
        surface_intersection: SurfaceIntersection::Unknown,
    });
    let clearance = observed
        .aabb_clearance_m
        .filter(|value| value.is_finite() && *value >= 0.0);
    let intersection = observed.surface_intersection;

    let (verdict, basis, reason) = if observed.aabb_clearance_m.is_some() && clearance.is_none() {
        (
            Verdict::Unknown,
            "invalid_observation",
            "AABB clearance is not a finite nonnegative measurement",
        )
    } else if intersection == SurfaceIntersection::Intersect
        && clearance.is_some_and(|gap| gap > ROBUST_GAP_M)
    {
        (
            Verdict::Unknown,
            "contradictory_native_evidence",
            "Surface intersection conflicts with positive AABB separation",
        )
    } else if intersection == SurfaceIntersection::Intersect {
        (
            Verdict::Fail,
            "native_triangle_surface_intersection",
            "Native geometry observed intersecting surfaces",
        )
    } else if clearance.is_some_and(|gap| gap > rule.minimum_gap_m + ROBUST_GAP_M) {
        (
            Verdict::Pass,
            "world_aabb_lower_bound",
            "Disjoint bounding boxes prove at least the requested separation",
        )
    } else {
        (
            Verdict::Unknown,
            "insufficient_spatial_evidence",
            "AABB overlap or insufficient clearance cannot prove contact or separation",
        )
    };

    Ok(SpatialClearanceAssessment {
        a: rule.a.clone(),
        b: rule.b.clone(),
        minimum_gap_m: rule.minimum_gap_m,
        aabb_lower_bound_m: clearance,
        verdict,
        basis: basis.into(),
        scope: "single_evaluated_frame".into(),
        reason: reason.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(gap: f64) -> SpatialClearanceRule {
        SpatialClearanceRule {
            a: "magnet-left".into(),
            b: "magnet-right".into(),
            minimum_gap_m: gap,
        }
    }

    #[test]
    fn disjoint_native_aabbs_prove_single_frame_separation() {
        let observed = SpatialPairObservation {
            aabb_clearance_m: Some(0.3),
            surface_intersection: SurfaceIntersection::Separate,
        };
        let result = assess_spatial_clearance(&rule(0.01), Some(observed)).unwrap();
        assert_eq!(result.verdict, Verdict::Pass);
        assert_eq!(result.basis, "world_aabb_lower_bound");
        assert_eq!(result.scope, "single_evaluated_frame");
    }

    #[test]
    fn overlapping_boxes_with_separate_triangles_cannot_prove_noncontact() {
        let result = assess_spatial_clearance(
            &rule(0.0),
            Some(SpatialPairObservation {
                aabb_clearance_m: Some(0.0),
                surface_intersection: SurfaceIntersection::Separate,
            }),
        )
        .unwrap();
        assert_eq!(result.verdict, Verdict::Unknown);
    }

    #[test]
    fn observed_surface_crossing_fails_noncontact_rule() {
        let result = assess_spatial_clearance(
            &rule(0.02),
            Some(SpatialPairObservation {
                aabb_clearance_m: Some(0.0),
                surface_intersection: SurfaceIntersection::Intersect,
            }),
        )
        .unwrap();
        assert_eq!(result.verdict, Verdict::Fail);
        assert_eq!(result.basis, "native_triangle_surface_intersection");
    }

    #[test]
    fn missing_or_inconsistent_geometry_never_passes() {
        assert_eq!(
            assess_spatial_clearance(&rule(0.0), None).unwrap().verdict,
            Verdict::Unknown
        );
        for gap in [f64::NAN, -1.0, f64::INFINITY] {
            let result = assess_spatial_clearance(
                &rule(0.0),
                Some(SpatialPairObservation {
                    aabb_clearance_m: Some(gap),
                    surface_intersection: SurfaceIntersection::Unknown,
                }),
            )
            .unwrap();
            assert_eq!(result.verdict, Verdict::Unknown);
        }
        let result = assess_spatial_clearance(
            &rule(0.0),
            Some(SpatialPairObservation {
                aabb_clearance_m: Some(0.5),
                surface_intersection: SurfaceIntersection::Intersect,
            }),
        )
        .unwrap();
        assert_eq!(result.verdict, Verdict::Unknown);
        assert_eq!(result.basis, "contradictory_native_evidence");
    }

    #[test]
    fn small_or_zero_box_gap_cannot_certify_minimum_clearance() {
        let result = assess_spatial_clearance(
            &rule(0.1),
            Some(SpatialPairObservation {
                aabb_clearance_m: Some(0.05),
                surface_intersection: SurfaceIntersection::Separate,
            }),
        )
        .unwrap();
        assert_eq!(result.verdict, Verdict::Unknown);
        assert_eq!(
            assess_spatial_clearance(
                &rule(0.1),
                Some(SpatialPairObservation {
                    aabb_clearance_m: Some(0.101),
                    surface_intersection: SurfaceIntersection::Unknown,
                }),
            )
            .unwrap()
            .verdict,
            Verdict::Pass
        );
    }

    #[test]
    fn invalid_rules_fail_closed() {
        for gap in [-0.01, f64::INFINITY, f64::NAN, 1001.0] {
            assert!(assess_spatial_clearance(&rule(gap), None).is_err());
        }
        let mut same = rule(0.0);
        same.b = same.a.clone();
        assert!(assess_spatial_clearance(&same, None).is_err());
    }
}
