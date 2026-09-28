//! Synthetic contract tests only. These receipts are not native app evidence.
use semwright_av_composition::*;
use semwright_media_time::{self as t, Rate, Rational as Q};
use semwright_semantic_composition::{
    self as c, BaseState, BaseStateSet, Concurrency, Digest, EvidenceSource, ExecutionStatus,
    Owner, PrincipalBinding, ResourceKey, Revision, Verdict,
};
use std::collections::{BTreeMap, BTreeSet};
fn q(n: i64, d: i64) -> Q {
    Q::new(n, d).unwrap()
}
fn digest(label: &str) -> Digest {
    Digest::of_bytes(label.as_bytes())
}
fn owner() -> Owner {
    Owner {
        session: "contract-fixture-session".into(),
        principal: PrincipalBinding::HostSession,
    }
}
fn base() -> BaseStateSet {
    BaseStateSet(
        ["motion", "audio", "delivery", "artifacts", "decode"]
            .map(|name| BaseState {
                key: ResourceKey {
                    provider: format!("fixture:{name}"),
                    resource: name.into(),
                },
                document_id: format!("doc-{name}"),
                provider_session: "fixture-session".into(),
                generation: "1".into(),
                revision: Revision::Counter(1),
                concurrency: Concurrency::BestEffortRevalidate,
            })
            .to_vec(),
    )
}
fn sync() -> SyncSpec {
    SyncSpec {
        cues: vec![
            SyncCue {
                id: "cue-a".into(),
                expected_time: q(1, 1),
            },
            SyncCue {
                id: "cue-b".into(),
                expected_time: q(2, 1),
            },
        ],
        max_offset: q(1, 30),
        max_drift: q(1, 60),
        max_cue_error: q(1, 30),
        confidence_floor: 9000,
        require_full_scan: true,
    }
}
fn cues() -> t::CueGraph {
    c::strict_decode(br#"{"version":1,"cues":[{"id":"cue-a","anchor":{"kind":"absolute","time":{"num":"1","den":"1"}},"duration":{"num":"1","den":"10"},"source":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","method":"fixture","version":1,"confidence":10000},{"id":"cue-b","anchor":{"kind":"absolute","time":{"num":"2","den":"1"}},"duration":{"num":"1","den":"10"},"source":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","method":"fixture","version":1,"confidence":10000}]}"#).unwrap()
}
fn plan() -> AvPlan {
    let proof = |service: Service, name: &str| ServiceProof {
        service,
        provider: format!("fixture:{name}"),
        generation: 1,
        catalog_digest: digest(name),
        runtime_digest: digest("fixture-runtime"),
        commands: Stage::ALL
            .into_iter()
            .filter(|s| s.service() == service)
            .map(|s| (s, digest(&format!("{s:?}"))))
            .collect(),
        available: true,
    };
    let sub = |service: Service, name: &str| Subplan {
        version: 1,
        service,
        owner: owner(),
        plan_ref: format!("fixture-{name}-plan"),
        plan_digest: digest(name),
        base: BaseStateSet(
            base()
                .0
                .into_iter()
                .filter(|s| s.key.resource == name)
                .collect(),
        ),
        cue_digest: cues().digest().unwrap(),
        duration: q(3, 1),
        dependencies: BTreeMap::from([("fixture-source".into(), digest(name))]),
        required_rules: BTreeSet::from([format!("native-{name}")]),
    };
    AvPlan::prepare(AvPlanBody {
        version: 1,
        spec: AvSpec {
            version: 1,
            id: "fixture-av".into(),
            owner: owner(),
            cues: cues(),
            delivery: DeliveryProfile {
                codec: DeliveryCodec::Mp4H264Aac,
                frame_rate: Rate::new(30, 1).unwrap(),
                width: 640,
                height: 360,
                duration: q(3, 1),
                sample_rate: 48000,
                channels: 2,
                audio_is_final_mix: true,
                max_artifact_bytes: 64 * 1024 * 1024,
            },
            sync: sync(),
            required_final_audio_rules: BTreeSet::from([
                "decoded-audio-duration".into(),
                "decoded-audio-peak".into(),
            ]),
        },
        motion: sub(Service::Motion, "motion"),
        audio: sub(Service::Audio, "audio"),
        base: base(),
        services: vec![
            proof(Service::Motion, "motion"),
            proof(Service::Audio, "audio"),
            proof(Service::Delivery, "delivery"),
            proof(Service::Artifacts, "artifacts"),
            proof(Service::Decode, "decode"),
        ],
        budget: c::ConvergenceBudget {
            max_iterations: 4,
            max_operations: 32,
            max_findings: 64,
            max_observations: 64,
            max_elapsed_ms: 60000,
        },
    })
    .unwrap()
}
fn next(coordinator: &mut AvCoordinator) -> StageCall {
    let stage = coordinator.next_stage().unwrap();
    let proof = coordinator
        .plan()
        .body
        .services
        .iter()
        .find(|p| p.service == stage.service())
        .unwrap()
        .clone();
    let base = coordinator.expected_base().clone();
    let call = coordinator.reserve(&owner(), &proof, &base).unwrap();
    coordinator
        .before_dispatch(&call, &owner(), &proof, &base)
        .unwrap();
    call
}
fn receipt(call: &StageCall, result: NativeResult) -> NativeReceipt {
    NativeReceipt {
        request_id: call.request_id.clone(),
        av_plan_digest: call.av_plan_digest.clone(),
        owner: call.owner.clone(),
        stage: call.stage,
        proof: call.proof.clone(),
        observed_base: call.expected_base.clone(),
        status: ExecutionStatus::Completed,
        result: Some(result),
        effects: vec![],
    }
}
fn detections(offset: Q) -> Vec<Detection> {
    sync()
        .cues
        .iter()
        .map(|c| Detection {
            cue_id: c.id.clone(),
            presentation_time: c.expected_time.checked_add(offset).unwrap(),
            uncertainty: Q::ZERO,
            confidence: 10000,
        })
        .collect()
}
fn probe(offset: Q) -> DecodedSyncProbe {
    DecodedSyncProbe {
        version: 1,
        artifact_digest: digest("encoded-fixture"),
        decoder_method: "fixture-presentation-timestamps".into(),
        decoder_digest: digest("fixture-decoder"),
        source: EvidenceSource::DecodedMedia,
        exhaustive_video: true,
        exhaustive_audio: true,
        flashes: detections(Q::ZERO),
        impulses: detections(offset),
    }
}
#[test]
fn fixed_graph_has_no_ready_shortcut() {
    let c = AvCoordinator::new(plan()).unwrap();
    assert_eq!(Stage::ALL.len(), 14);
    assert!(!c.ready());
    assert!(c.manifest().is_err());
}
#[test]
fn missing_audio_never_negotiates_a_fallback() {
    let mut p = plan().body;
    p.services
        .iter_mut()
        .find(|p| p.service == Service::Audio)
        .unwrap()
        .available = false;
    assert!(AvPlan::prepare(p).is_err());
}
#[test]
fn every_required_operation_is_negotiated() {
    let mut p = plan().body;
    p.services[1].commands.remove(&Stage::RenderAudio);
    assert!(AvPlan::prepare(p).is_err());
}
#[test]
fn subplan_owner_cannot_be_rebound() {
    let mut p = plan().body;
    p.audio.owner.session = "different".into();
    assert!(AvPlan::prepare(p).is_err());
}
#[test]
fn subplan_cues_cannot_be_silently_changed() {
    let mut p = plan().body;
    p.audio.cue_digest = digest("different");
    assert!(AvPlan::prepare(p).is_err());
}
#[test]
fn narration_is_not_implicitly_stretched() {
    let mut p = plan().body;
    p.audio.duration = q(4, 1);
    assert!(AvPlan::prepare(p).is_err());
}
#[test]
fn mux_does_not_remix_final_audio() {
    let mut p = plan().body;
    p.spec.delivery.audio_is_final_mix = false;
    assert!(AvPlan::prepare(p).is_err());
}
#[test]
fn fractional_fps_profile_requires_exact_duration() {
    let mut p = plan().body;
    p.spec.delivery.frame_rate = Rate::new(30000, 1001).unwrap();
    assert!(AvPlan::prepare(p.clone()).is_err());
    let duration = q(3003, 1000);
    p.spec.delivery.duration = duration;
    p.motion.duration = duration;
    p.audio.duration = duration;
    assert!(AvPlan::prepare(p).is_ok());
}
#[test]
fn stale_provider_cannot_dispatch() {
    let mut c = AvCoordinator::new(plan()).unwrap();
    let mut proof = c.plan().body.services[2].clone();
    proof.generation += 1;
    assert!(c.reserve(&owner(), &proof, &base()).is_err());
    assert_eq!(c.state(), AvState::Conflicted);
}
#[test]
fn wrong_owner_cannot_reserve() {
    let mut c = AvCoordinator::new(plan()).unwrap();
    let proof = c.plan().body.services[2].clone();
    let mut owner = owner();
    owner.session = "other".into();
    assert!(c.reserve(&owner, &proof, &base()).is_err());
    assert_eq!(c.state(), AvState::Denied);
}
#[test]
fn recalculated_digest_does_not_replace_reserved_payload() {
    let mut c = AvCoordinator::new(plan()).unwrap();
    let proof = c.plan().body.services[2].clone();
    let mut call = c.reserve(&owner(), &proof, &base()).unwrap();
    if let StagePayload::PlanDelivery { profile } = &mut call.payload {
        profile.width = 1280;
    }
    assert!(c.before_dispatch(&call, &owner(), &proof, &base()).is_err());
}
#[test]
fn duplicate_dispatch_is_denied() {
    let mut c = AvCoordinator::new(plan()).unwrap();
    let call = next(&mut c);
    assert!(
        c.before_dispatch(&call, &owner(), &call.proof, &base())
            .is_err()
    );
}
#[test]
fn unrelated_resource_delta_is_not_attributed_to_a_stage() {
    let mut c = AvCoordinator::new(plan()).unwrap();
    let call = next(&mut c);
    let mut r = receipt(
        &call,
        NativeResult::DeliveryPlanned {
            profile_digest: c::canonical_digest(&c.plan().body.spec.delivery).unwrap(),
        },
    );
    r.observed_base.0[0].revision = Revision::Counter(2);
    assert!(c.complete(r).is_err());
    assert_eq!(c.state(), AvState::Unknown);
}
#[test]
fn prior_native_effects_survive_audio_failure() {
    let mut c = AvCoordinator::new(plan()).unwrap();
    let call = next(&mut c);
    c.complete(receipt(
        &call,
        NativeResult::DeliveryPlanned {
            profile_digest: c::canonical_digest(&c.plan().body.spec.delivery).unwrap(),
        },
    ))
    .unwrap();
    let call = next(&mut c);
    let mut r = receipt(&call, NativeResult::Applied);
    r.effects.push("fixture-owned-project-created".into());
    c.complete(r).unwrap();
    let call = next(&mut c);
    let mut r = receipt(&call, NativeResult::Applied);
    r.status = ExecutionStatus::Failed;
    r.result = None;
    assert!(c.complete(r).is_err());
    assert_eq!(c.ledger()[1].effects, ["fixture-owned-project-created"]);
    assert!(!c.ready());
    assert!(c.manifest().is_err());
}
#[test]
fn lost_mutation_receipt_never_allows_automatic_retry() {
    let mut c = AvCoordinator::new(plan()).unwrap();
    let _ = next(&mut c);
    c.lost_receipt().unwrap();
    assert_eq!(c.state(), AvState::Unknown);
    let proof = c.plan().body.services[2].clone();
    assert!(c.reserve(&owner(), &proof, &base()).is_err());
}
#[test]
fn cancellation_before_dispatch_prevents_ready() {
    let mut c = AvCoordinator::new(plan()).unwrap();
    c.cancel_before_dispatch().unwrap();
    assert_eq!(c.state(), AvState::Cancelled);
    assert!(!c.ready());
}
#[test]
fn active_mutation_needs_a_native_cancel_receipt() {
    let mut c = AvCoordinator::new(plan()).unwrap();
    let _ = next(&mut c);
    assert!(c.cancel_before_dispatch().is_err());
}
#[test]
fn aligned_presentation_timestamps_pass_sync_math() {
    let report = verify_sync(&sync(), &probe(Q::ZERO)).unwrap();
    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(report.observed_drift, Some(Q::ZERO));
}
#[test]
fn observed_offset_fails_before_tolerance_can_be_changed() {
    assert_eq!(
        verify_sync(&sync(), &probe(q(1, 10))).unwrap().verdict,
        Verdict::Fail
    );
}
#[test]
fn missing_impulse_is_unknown_not_time_zero() {
    let mut p = probe(Q::ZERO);
    p.impulses.pop();
    let report = verify_sync(&sync(), &p).unwrap();
    assert_eq!(report.verdict, Verdict::Unknown);
    assert_eq!(report.missing, ["cue-b"]);
}
#[test]
fn sampled_decoder_cannot_certify_exhaustive_sync() {
    let mut p = probe(Q::ZERO);
    p.exhaustive_audio = false;
    assert_eq!(verify_sync(&sync(), &p).unwrap().verdict, Verdict::Unknown);
}
#[test]
fn fixture_evidence_does_not_certify_native_sync() {
    let mut p = probe(Q::ZERO);
    p.source = EvidenceSource::Fixture;
    assert_eq!(verify_sync(&sync(), &p).unwrap().verdict, Verdict::Unknown);
}
#[test]
fn common_shift_does_not_cancel_out_expected_cue_error() {
    let mut p = probe(q(1, 2));
    p.flashes = detections(q(1, 2));
    assert_eq!(verify_sync(&sync(), &p).unwrap().verdict, Verdict::Fail);
}
#[test]
fn ambiguous_detection_is_rejected() {
    let mut p = probe(Q::ZERO);
    p.flashes.push(p.flashes[0].clone());
    assert!(verify_sync(&sync(), &p).is_err());
}
#[test]
fn decoder_uncertainty_is_not_hidden() {
    let mut p = probe(Q::ZERO);
    for i in &mut p.impulses {
        i.uncertainty = q(1, 10);
    }
    assert_eq!(verify_sync(&sync(), &p).unwrap().verdict, Verdict::Unknown);
}
#[test]
fn increasing_drift_is_detected() {
    let mut p = probe(Q::ZERO);
    p.impulses[1].presentation_time = q(202, 100);
    assert_eq!(verify_sync(&sync(), &p).unwrap().verdict, Verdict::Fail);
}
#[test]
fn publication_paths_are_not_artifact_tokens() {
    let candidate = PublicationCandidate {
        owner: owner(),
        manifest_digest: digest("manifest"),
        source_root: "candidate".into(),
        source_path: "../outside".into(),
        destination_root: "output".into(),
        destination_path: "ready.json".into(),
        bytes: 1,
    };
    assert!(candidate.validate().is_err());
}
#[test]
fn public_wire_rejects_executable_fields() {
    let mut p = serde_json::to_value(plan()).unwrap();
    p["shell"] = serde_json::json!("unexpected");
    assert!(c::strict_decode::<AvPlan>(&serde_json::to_vec(&p).unwrap()).is_err());
}
