mod common;
use common::*;
use semwright_effect_conformance::*;
use semwright_effect_conformance::composition::*;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn figma_motion_audio_consumers_use_identical_A_reports_without_native_claims() {
    for (profile,units,value) in [("figma.layout","pixel",640.0),("motion.frame","frame",24.0),("audio.peak","dBFS",-1.0)] {
        let (mut contract, mut context) = fixture();
        contract.profile = profile.into(); contract.rules.truncate(1);
        contract.rules[0].predicate = Predicate::Within { expected: value, tolerance: 0.0, units: units.into() };
        context.contract_digest = contract.digest().unwrap();
        let batch = collect(&contract,&context,&mut ModelAdapter::default()).unwrap();
        let evaluation = evaluate(&contract,&context,&batch).unwrap();
        let common: VerificationReport = evaluation.report.clone();
        assert_eq!(common.verdict().unwrap(),Verdict::Pass);
        let quality = workflow_quality(WorkflowIdentity { driver: profile.into(), driver_version: "consumer-v1".into(),
            runtime: "contractual-only".into(), os: "portable".into(), workflow: profile.into(), fixture: "F-E0".into(),
            source_sha: "0123456789012345678901234567890123456789".into(), route: EvidenceRoute::Contractual },
            &BTreeMap::from([(QualityDimension::NativeEvidence,BTreeSet::from(["position".into()]))]), &evaluation, BTreeSet::new()).unwrap();
        assert_eq!(quality.dimensions.iter().find(|d|d.dimension==QualityDimension::NativeEvidence).unwrap().verdict,Verdict::Unknown);
        // C is a storage consumer of the same serialized A report, not an auth oracle.
        let encoded=canonical_bytes(&common).unwrap();
        let stored: VerificationReport = strict_decode(&encoded).unwrap();
        assert_eq!(stored.validation.plan_digest,context.plan_digest);
        assert_eq!(stored.effects_unobservable,common.effects_unobservable);
    }
}
#[test]
fn incomplete_quality_mapping_and_missing_negative_proofs_stay_unknown() {
    let evaluation = run(&mut ModelAdapter::default());
    let identity = WorkflowIdentity { driver: "godot".into(), driver_version: "v1".into(), runtime: "model".into(),os: "portable".into(),workflow: "save".into(),fixture: "fixture".into(),source_sha:"0123456789012345678901234567890123456789".into(),route:EvidenceRoute::Contractual };
    let quality=workflow_quality(identity,&BTreeMap::from([(QualityDimension::Conformance,BTreeSet::from(["position".into()]))]),&evaluation,BTreeSet::new()).unwrap();
    assert_eq!(quality.dimensions.len(),9);
    assert!(quality.dimensions.iter().all(|d|d.verdict==Verdict::Unknown));
}
#[test]
fn wire_flags_match_A_and_unknown_cannot_be_renamed_to_success() {
    let evaluation = run(&mut ModelAdapter { trusted: false, ..Default::default() });
    let json = serde_json::to_value(&evaluation).unwrap();
    assert_eq!(json["report"]["execution_status"],"completed");
    assert_eq!(json["report"]["validation"]["checks"][0]["verdict"],"UNKNOWN");
    assert_eq!(evaluation.verdict().unwrap(),Verdict::Unknown);
    assert!(strict_decode::<VerificationReport>(br#"{"execution_status":"success","validation":{},"support_level":"native","effects_observed":[],"effects_unobservable":[]}"#).is_err());
}
