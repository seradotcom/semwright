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
fn reusable_artifact(label: &str, dependencies: BTreeMap<String, Digest>) -> t::MediaArtifact {
    t::MediaArtifact {
        reference: format!("artifact:{label}"),
        owner: owner(),
        sha256: digest(&format!("artifact-{label}")),
        bytes: 1024,
        media_type: "video/x-matroska".into(),
        source_plan: digest(label),
        source_state: BaseStateSet(
            base()
                .0
                .into_iter()
                .filter(|state| state.key.resource == "motion")
                .collect(),
        ),
        metadata: t::MediaMetadata {
            duration: q(3, 1),
            encoded_duration: Some(q(3, 1)),
            video: Some(t::VideoMetadata {
                width: 640,
                height: 360,
                frame_rate: Rate::new(30, 1).unwrap(),
                frames: 90,
                alpha: false,
            }),
            audio: None,
        },
        dependencies,
        provenance: Some("synthetic contract fixture".into()),
        license: None,
        retention: t::Retention::PrivateCandidate,
    }
}

fn audio_artifact() -> t::MediaArtifact {
    t::MediaArtifact {
        reference: "artifact:audio-final".into(),
        owner: owner(),
        sha256: digest("audio-final-bytes"),
        bytes: 576_044,
        media_type: "audio/wav".into(),
        source_plan: digest("audio"),
        source_state: BaseStateSet(
            base()
                .0
                .into_iter()
                .filter(|state| state.key.resource == "audio")
                .collect(),
        ),
        metadata: t::MediaMetadata {
            duration: q(3, 1),
            encoded_duration: Some(q(3, 1)),
            video: None,
            audio: Some(t::AudioMetadata {
                sample_rate: 48_000,
                channels: 2,
                channel_layout: "stereo".into(),
                sample_frames: 144_000,
                priming_samples: None,
                padding_samples: None,
                latency_samples: None,
                tail_samples: None,
            }),
        },
        dependencies: BTreeMap::from([("fixture-source".into(), digest("audio"))]),
        provenance: Some("synthetic final-mix fixture".into()),
        license: None,
        retention: t::Retention::PrivateCandidate,
    }
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
            .map(|stage| {
                (
                    stage,
                    vec![CommandProof {
                        command: format!("fixture.{name}.{stage:?}"),
                        descriptor: digest(&format!("{stage:?}")),
                    }],
                )
            })
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
fn native_effect_receipt_count_and_string_limits_are_inclusive() {
    fn delivery_result(coordinator: &AvCoordinator) -> NativeResult {
        NativeResult::DeliveryPlanned {
            profile_digest: c::canonical_digest(&coordinator.plan().body.spec.delivery).unwrap(),
        }
    }

    let mut at_count_limit = AvCoordinator::new(plan()).unwrap();
    let call = next(&mut at_count_limit);
    let mut value = receipt(&call, delivery_result(&at_count_limit));
    value.effects = (0..4096).map(|_| "x".to_owned()).collect();
    at_count_limit.complete(value).unwrap();

    let mut over_count_limit = AvCoordinator::new(plan()).unwrap();
    let call = next(&mut over_count_limit);
    let mut value = receipt(&call, delivery_result(&over_count_limit));
    value.effects = (0..4097).map(|_| "x".to_owned()).collect();
    assert!(matches!(
        over_count_limit.complete(value),
        Err(Error::Limit(_))
    ));
    assert_eq!(over_count_limit.state(), AvState::Unknown);

    let mut at_string_limit = AvCoordinator::new(plan()).unwrap();
    let call = next(&mut at_string_limit);
    let mut value = receipt(&call, delivery_result(&at_string_limit));
    value.effects = vec!["x".repeat(512)];
    at_string_limit.complete(value).unwrap();

    let mut over_string_limit = AvCoordinator::new(plan()).unwrap();
    let call = next(&mut over_string_limit);
    let mut value = receipt(&call, delivery_result(&over_string_limit));
    value.effects = vec!["x".repeat(513)];
    assert!(matches!(
        over_string_limit.complete(value),
        Err(Error::Limit(_))
    ));
    assert_eq!(over_string_limit.state(), AvState::Unknown);

    let mut controls = AvCoordinator::new(plan()).unwrap();
    let call = next(&mut controls);
    let mut value = receipt(&call, delivery_result(&controls));
    value.effects = vec![
        "line
break"
            .into(),
    ];
    assert!(matches!(controls.complete(value), Err(Error::Limit(_))));
    assert_eq!(controls.state(), AvState::Unknown);
}

#[test]
fn mutating_stage_may_change_only_its_bound_provider_resource() {
    let mut coordinator = AvCoordinator::new(plan()).unwrap();
    let delivery = next(&mut coordinator);
    coordinator
        .complete(receipt(
            &delivery,
            NativeResult::DeliveryPlanned {
                profile_digest: c::canonical_digest(&coordinator.plan().body.spec.delivery)
                    .unwrap(),
            },
        ))
        .unwrap();

    let apply_motion = next(&mut coordinator);
    let mut value = receipt(&apply_motion, NativeResult::Applied);
    let motion = value
        .observed_base
        .0
        .iter_mut()
        .find(|state| state.key.provider == "fixture:motion")
        .unwrap();
    motion.revision = Revision::Counter(2);
    coordinator.complete(value).unwrap();
    assert_eq!(
        coordinator
            .expected_base()
            .0
            .iter()
            .find(|state| state.key.provider == "fixture:motion")
            .unwrap()
            .revision,
        Revision::Counter(2)
    );
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
    assert_eq!(report.observed_drift, None);
}
#[test]
fn sampled_decoder_cannot_certify_exhaustive_sync() {
    let mut p = probe(Q::ZERO);
    p.exhaustive_audio = false;
    let report = verify_sync(&sync(), &p).unwrap();
    assert_eq!(report.verdict, Verdict::Unknown);
    assert!(
        report
            .limitations
            .iter()
            .any(|value| value.contains("sampled media"))
    );
}
#[test]
fn fixture_evidence_does_not_certify_native_sync() {
    let mut p = probe(Q::ZERO);
    p.source = EvidenceSource::Fixture;
    let report = verify_sync(&sync(), &p).unwrap();
    assert_eq!(report.verdict, Verdict::Unknown);
    assert!(
        report
            .limitations
            .iter()
            .any(|value| value.contains("Non-native decoder evidence"))
    );
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
fn sampled_decoder_can_pass_when_full_scan_is_not_required() {
    let mut spec = sync();
    spec.require_full_scan = false;
    let mut value = probe(Q::ZERO);
    value.exhaustive_audio = false;
    let report = verify_sync(&spec, &value).unwrap();
    assert_eq!(report.verdict, Verdict::Pass);
    assert!(!report.exhaustive);
    assert_eq!(report.limitations.len(), 1);
}

#[test]
fn video_and_audio_cue_errors_fail_independently() {
    for video_side in [true, false] {
        let mut spec = sync();
        spec.max_offset = Q::ONE;
        spec.max_drift = Q::ONE;
        let mut value = probe(Q::ZERO);
        if video_side {
            value.flashes[0].presentation_time = value.flashes[0]
                .presentation_time
                .checked_add(q(1, 10))
                .unwrap();
        } else {
            value.impulses[0].presentation_time = value.impulses[0]
                .presentation_time
                .checked_add(q(1, 10))
                .unwrap();
        }
        assert_eq!(verify_sync(&spec, &value).unwrap().verdict, Verdict::Fail);
    }
}

#[test]
fn video_and_audio_uncertainty_can_independently_make_sync_unknown() {
    for video_side in [true, false] {
        let mut spec = sync();
        spec.max_offset = Q::ONE;
        spec.max_drift = Q::ONE;
        let mut value = probe(Q::ZERO);
        if video_side {
            value.flashes[0].uncertainty = q(1, 20);
        } else {
            value.impulses[0].uncertainty = q(1, 20);
        }
        assert_eq!(
            verify_sync(&spec, &value).unwrap().verdict,
            Verdict::Unknown
        );
    }
}

#[test]
fn cue_error_and_drift_thresholds_are_inclusive() {
    for video_side in [true, false] {
        let mut spec = sync();
        spec.max_offset = Q::ONE;
        spec.max_drift = Q::ONE;
        let mut value = probe(Q::ZERO);
        if video_side {
            value.flashes[0].presentation_time = value.flashes[0]
                .presentation_time
                .checked_add(spec.max_cue_error)
                .unwrap();
        } else {
            value.impulses[0].presentation_time = value.impulses[0]
                .presentation_time
                .checked_add(spec.max_cue_error)
                .unwrap();
        }
        assert_eq!(verify_sync(&spec, &value).unwrap().verdict, Verdict::Pass);
    }

    let mut spec = sync();
    spec.max_offset = Q::ONE;
    spec.max_cue_error = Q::ONE;
    let mut exact_drift = probe(Q::ZERO);
    exact_drift.impulses[1].presentation_time = exact_drift.impulses[1]
        .presentation_time
        .checked_add(spec.max_drift)
        .unwrap();
    let report = verify_sync(&spec, &exact_drift).unwrap();
    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(report.observed_drift, Some(spec.max_drift));
}

#[test]
fn drift_plus_uncertainty_equal_to_limit_is_still_pass() {
    let mut spec = sync();
    spec.max_offset = Q::ONE;
    spec.max_cue_error = Q::ONE;
    let half = q(1, 120);
    let mut value = probe(Q::ZERO);
    value.impulses[1].presentation_time = value.impulses[1]
        .presentation_time
        .checked_add(half)
        .unwrap();
    value.impulses[1].uncertainty = half;
    let report = verify_sync(&spec, &value).unwrap();
    assert_eq!(report.verdict, Verdict::Pass);
    assert_eq!(report.observed_drift, Some(half));
}

#[test]
fn sync_spec_rejects_duplicate_ids_and_negative_cue_times_independently() {
    let mut duplicate = sync();
    duplicate.cues[1].id = duplicate.cues[0].id.clone();
    assert!(duplicate.validate().is_err());

    let mut negative = sync();
    negative.cues[0].expected_time = q(-1, 1);
    assert!(negative.validate().is_err());
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

#[test]
fn sync_stage_accepts_raw_decoder_probe_shape_not_a_preverified_report() {
    let raw = NativeResult::SyncMeasured {
        probe: probe(Q::ZERO),
    };
    let bytes = serde_json::to_vec(&raw).unwrap();
    assert!(c::strict_decode::<NativeResult>(&bytes).is_ok());
    assert!(
        c::strict_decode::<NativeResult>(
            br#"{"kind":"sync_verified","report":{"verdict":"PASS"}}"#
        )
        .is_err()
    );
}

#[test]
fn visual_dependency_change_rebuilds_motion_but_reuses_independent_audio() {
    let cue = digest("cue-v1");
    let motion = reusable_artifact(
        "motion",
        BTreeMap::from([
            ("visual".into(), digest("visual-v1")),
            ("cue".into(), cue.clone()),
        ]),
    );
    let audio = reusable_artifact(
        "audio",
        BTreeMap::from([
            ("audio".into(), digest("audio-v1")),
            ("cue".into(), cue.clone()),
        ]),
    );
    let current = BTreeMap::from([
        ("visual".into(), digest("visual-v2")),
        ("audio".into(), digest("audio-v1")),
        ("cue".into(), cue),
    ]);

    assert_eq!(
        classify_reuse(&motion, &owner(), &current).unwrap(),
        ReuseDecision::Rebuild {
            invalidated: vec!["visual".into()]
        }
    );
    assert!(matches!(
        classify_reuse(&audio, &owner(), &current).unwrap(),
        ReuseDecision::Reuse { .. }
    ));
}

#[test]
fn cue_or_timing_change_invalidates_both_motion_and_audio_dependents() {
    let old_cue = digest("cue-v1");
    let motion = reusable_artifact(
        "motion",
        BTreeMap::from([
            ("visual".into(), digest("visual-v1")),
            ("cue".into(), old_cue.clone()),
        ]),
    );
    let audio = reusable_artifact(
        "audio",
        BTreeMap::from([
            ("audio".into(), digest("audio-v1")),
            ("cue".into(), old_cue),
        ]),
    );
    let current = BTreeMap::from([
        ("visual".into(), digest("visual-v1")),
        ("audio".into(), digest("audio-v1")),
        ("cue".into(), digest("cue-v2")),
    ]);

    for artifact in [&motion, &audio] {
        assert_eq!(
            classify_reuse(artifact, &owner(), &current).unwrap(),
            ReuseDecision::Rebuild {
                invalidated: vec!["cue".into()]
            }
        );
    }
}

#[test]
fn audio_only_change_keeps_motion_intermediate_but_fixed_graph_still_rebuilds_master() {
    let cue = digest("cue-v1");
    let motion = reusable_artifact(
        "motion",
        BTreeMap::from([
            ("visual".into(), digest("visual-v1")),
            ("cue".into(), cue.clone()),
        ]),
    );
    let current = BTreeMap::from([
        ("visual".into(), digest("visual-v1")),
        ("audio".into(), digest("audio-v2")),
        ("cue".into(), cue),
    ]);
    assert!(matches!(
        classify_reuse(&motion, &owner(), &current).unwrap(),
        ReuseDecision::Reuse { .. }
    ));

    let mux = Stage::ALL
        .iter()
        .position(|stage| *stage == Stage::Mux)
        .unwrap();
    let final_audio = Stage::ALL
        .iter()
        .position(|stage| *stage == Stage::VerifyFinalAudio)
        .unwrap();
    let sync = Stage::ALL
        .iter()
        .position(|stage| *stage == Stage::VerifySync)
        .unwrap();
    assert!(mux < final_audio && final_audio < sync);
}

#[test]
fn artifact_reuse_never_crosses_owner_sessions() {
    let artifact = reusable_artifact(
        "motion",
        BTreeMap::from([("visual".into(), digest("visual-v1"))]),
    );
    let mut other = owner();
    other.session = "another-owner-session".into();
    assert!(
        classify_reuse(
            &artifact,
            &other,
            &BTreeMap::from([("visual".into(), digest("visual-v1"))])
        )
        .is_err()
    );
}

#[test]
fn av_native_command_inventory_claims_only_delivery_bridge_not_audio_authoring() {
    for stage in [
        Stage::PlanDelivery,
        Stage::ApplyMotion,
        Stage::RenderMotion,
        Stage::VerifyMotion,
        Stage::TransferMotion,
        Stage::TransferAudio,
        Stage::Mux,
        Stage::VerifyFinalAudio,
        Stage::VerifySync,
    ] {
        let commands = av_stage_commands(stage).expect("AV stage mapping");
        assert!(!commands.is_empty());
        assert!(
            commands.iter().all(|command| {
                command.starts_with("driver.") || *command == "artifact.handoff"
            })
        );
    }
    assert_eq!(Stage::TransferMotion.service(), Service::Delivery);
    assert_eq!(Stage::TransferAudio.service(), Service::Artifacts);
    assert_eq!(
        av_stage_commands(Stage::TransferAudio).unwrap(),
        ["artifact.handoff"]
    );
    for stage in [
        Stage::ApplyAudio,
        Stage::RenderAudio,
        Stage::VerifyAudio,
        Stage::PreparePublication,
        Stage::Publish,
    ] {
        assert!(
            av_stage_commands(stage).is_none(),
            "The AV adapter must not silently claim {stage:?}"
        );
    }
}

#[test]
fn av_artifact_routes_are_host_configuration_not_arbitrary_mlt_roots() {
    AvArtifactRoutes {
        audio_source_root: "audio-output".into(),
        handoff_destination_root: "av-delivery".into(),
        mlt_media_root: "media".into(),
    }
    .validate()
    .unwrap();
    assert!(
        AvArtifactRoutes {
            audio_source_root: "audio-output".into(),
            handoff_destination_root: "av-delivery".into(),
            mlt_media_root: "anything".into(),
        }
        .validate()
        .is_err()
    );
}

#[test]
fn audio_source_locator_is_separate_from_provider_artifact_token() {
    let mut adapter = AvStageAdapter::with_artifact_routes(
        plan(),
        Some(AvArtifactRoutes {
            audio_source_root: "audio-output".into(),
            handoff_destination_root: "av-delivery".into(),
            mlt_media_root: "media".into(),
        }),
    )
    .unwrap();
    let artifact = audio_artifact();
    adapter
        .bind_audio_source_locator(&artifact, "final/master.wav")
        .unwrap();
    adapter
        .bind_audio_source_locator(&artifact, "final/master.wav")
        .unwrap();
    assert!(
        adapter
            .bind_audio_source_locator(&artifact, "other/master.wav")
            .is_err()
    );
    assert!(
        adapter
            .bind_audio_source_locator(&artifact, "../escape.wav")
            .is_err()
    );
    let mut wrong = artifact;
    wrong.owner.session = "other-session".into();
    assert!(
        adapter
            .bind_audio_source_locator(&wrong, "final/master.wav")
            .is_err()
    );
}

#[test]
fn audio_handoff_hint_is_path_data_not_authority_and_binds_master_digest() {
    let artifact = audio_artifact();
    ArtifactHandoffHint {
        version: 1,
        artifact_digest: artifact.sha256.clone(),
        relative_path: "final/master.wav".into(),
    }
    .validate()
    .unwrap();
    assert!(
        ArtifactHandoffHint {
            version: 1,
            artifact_digest: artifact.sha256.clone(),
            relative_path: "../escape.wav".into(),
        }
        .validate()
        .is_err()
    );
    assert!(
        ArtifactHandoffHint {
            version: 1,
            artifact_digest: artifact.sha256,
            relative_path: "file:///tmp/master.wav".into(),
        }
        .validate()
        .is_err()
    );
}

fn verification_report_on_base(
    plan_digest: Digest,
    rules: BTreeSet<String>,
    source: EvidenceSource,
    artifact: Digest,
    observed_base: BaseStateSet,
) -> c::VerificationReport {
    let checks = rules
        .iter()
        .map(|rule| c::RuleResult {
            rule: rule.clone(),
            version: 1,
            verdict: Verdict::Pass,
            evidence_class: c::EvidenceClass::Deterministic,
            evidence: vec![c::ObservationRef {
                id: format!("obs-{rule}"),
                base: observed_base.clone(),
                source,
                method: "fixture-native-read".into(),
                method_version: 1,
                scope: vec![],
                artifact: Some(artifact.clone()),
                exhaustive: true,
            }],
            reason: None,
        })
        .collect();
    c::VerificationReport {
        execution_status: ExecutionStatus::Completed,
        validation: c::ValidationReport {
            plan_digest,
            base: observed_base,
            required_rules: rules,
            checks,
        },
        support_level: c::SupportLevel::Native,
        effects_observed: vec![],
        effects_unobservable: vec![],
    }
}
fn verification_report(
    plan_digest: Digest,
    rules: BTreeSet<String>,
    source: EvidenceSource,
    artifact: Digest,
) -> c::VerificationReport {
    verification_report_on_base(plan_digest, rules, source, artifact, base())
}

fn encoded_artifact(plan: &AvPlan) -> t::MediaArtifact {
    t::MediaArtifact {
        reference: "artifact:encoded-master".into(),
        owner: owner(),
        sha256: digest("encoded-fixture"),
        bytes: 1_000_000,
        media_type: "video/mp4".into(),
        source_plan: plan.digest.clone(),
        source_state: base(),
        metadata: t::MediaMetadata {
            duration: q(3, 1),
            encoded_duration: Some(q(3, 1)),
            video: Some(t::VideoMetadata {
                width: 640,
                height: 360,
                frame_rate: Rate::new(30, 1).unwrap(),
                frames: 90,
                alpha: false,
            }),
            audio: Some(t::AudioMetadata {
                sample_rate: 48_000,
                channels: 2,
                channel_layout: "stereo".into(),
                sample_frames: 144_000,
                priming_samples: None,
                padding_samples: None,
                latency_samples: None,
                tail_samples: None,
            }),
        },
        dependencies: BTreeMap::from([
            ("motion-artifact".into(), digest("motion-mezzanine")),
            ("audio-artifact".into(), digest("audio-final-bytes")),
        ]),
        provenance: Some("synthetic encoded fixture".into()),
        license: None,
        retention: t::Retention::PrivateCandidate,
    }
}
fn decoded_audio_artifact(plan: &AvPlan, encoded: &t::MediaArtifact) -> t::MediaArtifact {
    t::MediaArtifact {
        reference: "artifact:decoded-final-audio".into(),
        owner: owner(),
        sha256: digest("decoded-final-audio-bytes"),
        bytes: 576_044,
        media_type: "audio/wav".into(),
        source_plan: plan.digest.clone(),
        source_state: base(),
        metadata: t::MediaMetadata {
            duration: q(3, 1),
            encoded_duration: Some(q(3, 1)),
            video: None,
            audio: Some(t::AudioMetadata {
                sample_rate: 48_000,
                channels: 2,
                channel_layout: "stereo".into(),
                sample_frames: 144_000,
                priming_samples: None,
                padding_samples: None,
                latency_samples: None,
                tail_samples: None,
            }),
        },
        dependencies: BTreeMap::from([("encoded-master".into(), encoded.sha256.clone())]),
        provenance: Some("synthetic post-encode decode fixture".into()),
        license: None,
        retention: t::Retention::PrivateCandidate,
    }
}

fn advance_to_mux() -> (AvCoordinator, AvPlan) {
    let plan = plan();
    let motion = reusable_artifact(
        "motion",
        BTreeMap::from([("visual".into(), digest("visual-v1"))]),
    );
    let audio = audio_artifact();
    let mut coordinator = AvCoordinator::new(plan.clone()).unwrap();

    let call = next(&mut coordinator);
    coordinator
        .complete(receipt(
            &call,
            NativeResult::DeliveryPlanned {
                profile_digest: c::canonical_digest(&plan.body.spec.delivery).unwrap(),
            },
        ))
        .unwrap();

    for result in [NativeResult::Applied, NativeResult::Applied] {
        let call = next(&mut coordinator);
        coordinator.complete(receipt(&call, result)).unwrap();
    }

    let call = next(&mut coordinator);
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Rendered {
                artifact: motion.clone(),
            },
        ))
        .unwrap();
    let call = next(&mut coordinator);
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Rendered {
                artifact: audio.clone(),
            },
        ))
        .unwrap();

    let call = next(&mut coordinator);
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Verified {
                artifact_digest: motion.sha256.clone(),
                report: verification_report(
                    plan.body.motion.plan_digest.clone(),
                    plan.body.motion.required_rules.clone(),
                    EvidenceSource::RendererState,
                    motion.sha256.clone(),
                ),
            },
        ))
        .unwrap();
    let call = next(&mut coordinator);
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Verified {
                artifact_digest: audio.sha256.clone(),
                report: verification_report(
                    plan.body.audio.plan_digest.clone(),
                    plan.body.audio.required_rules.clone(),
                    EvidenceSource::NativeApi,
                    audio.sha256.clone(),
                ),
            },
        ))
        .unwrap();

    let motion_transcode_digest = digest("motion-transcode");
    let call = next(&mut coordinator);
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Transferred {
                input: DeliveryInput {
                    token: "motion-input".into(),
                    source_digest: motion.sha256.clone(),
                    artifact_digest: motion_transcode_digest.clone(),
                    owner: owner(),
                    metadata: motion.metadata.clone(),
                    operation: TransferKind::VerifiedTranscode,
                    verification: Some(verification_report(
                        plan.digest.clone(),
                        BTreeSet::from(["decoded-frame-equivalence".into()]),
                        EvidenceSource::DecodedMedia,
                        motion_transcode_digest,
                    )),
                },
            },
        ))
        .unwrap();
    let call = next(&mut coordinator);
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Transferred {
                input: DeliveryInput {
                    token: "audio-input".into(),
                    source_digest: audio.sha256.clone(),
                    artifact_digest: audio.sha256.clone(),
                    owner: owner(),
                    metadata: audio.metadata.clone(),
                    operation: TransferKind::ByteCopy,
                    verification: None,
                },
            },
        ))
        .unwrap();

    assert_eq!(coordinator.next_stage(), Some(Stage::Mux));
    (coordinator, plan)
}

#[test]
fn verified_transcode_is_bound_to_the_motion_mux_slot() {
    let (mut coordinator, _) = advance_to_mux();
    let call = next(&mut coordinator);
    let StagePayload::Mux { motion, audio, .. } = call.payload else {
        panic!("expected mux payload");
    };
    assert_eq!(motion.token, "motion-input");
    assert!(matches!(motion.operation, TransferKind::VerifiedTranscode));
    assert_eq!(audio.token, "audio-input");
    assert!(matches!(audio.operation, TransferKind::ByteCopy));
}

#[test]
fn elapsed_budget_is_enforced_by_reserve_not_only_by_the_helper() {
    let mut body = plan().body;
    body.budget.max_elapsed_ms = 1;
    let constrained = AvPlan::prepare(body).unwrap();
    let mut coordinator = AvCoordinator::new(constrained).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(10));

    let stage = coordinator.next_stage().unwrap();
    let proof = coordinator
        .plan()
        .body
        .services
        .iter()
        .find(|candidate| candidate.service == stage.service())
        .unwrap()
        .clone();
    let fresh = coordinator.expected_base().clone();

    assert!(coordinator.reserve(&owner(), &proof, &fresh).is_err());
    assert_eq!(coordinator.state(), AvState::Exhausted);
}

#[test]
fn mux_rejects_zero_length_decoded_audio_at_the_exact_boundary() {
    let (mut coordinator, plan) = advance_to_mux();
    let encoded = encoded_artifact(&plan);
    let mut decoded_audio = decoded_audio_artifact(&plan, &encoded);
    decoded_audio.metadata.audio.as_mut().unwrap().sample_frames = 0;
    let handoff = ArtifactHandoffHint {
        version: 1,
        artifact_digest: decoded_audio.sha256.clone(),
        relative_path: "encoded-final-audio.wav".into(),
    };
    let call = next(&mut coordinator);
    assert!(
        coordinator
            .complete(receipt(
                &call,
                NativeResult::Encoded {
                    artifact: encoded,
                    decoded_audio: Box::new(decoded_audio),
                    decoded_audio_handoff: handoff,
                },
            ))
            .is_err()
    );
    assert_eq!(coordinator.state(), AvState::Unknown);
}

#[test]
fn full_synthetic_av_graph_exercises_every_result_arm_before_ready() {
    let plan = plan();
    let motion = reusable_artifact(
        "motion",
        BTreeMap::from([("visual".into(), digest("visual-v1"))]),
    );
    let audio = audio_artifact();
    let mut coordinator = AvCoordinator::new(plan.clone()).unwrap();
    let mut request_ids = vec![];

    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    coordinator
        .complete(receipt(
            &call,
            NativeResult::DeliveryPlanned {
                profile_digest: c::canonical_digest(&plan.body.spec.delivery).unwrap(),
            },
        ))
        .unwrap();
    assert!(!coordinator.ready());

    for result in [NativeResult::Applied, NativeResult::Applied] {
        let call = next(&mut coordinator);
        request_ids.push(call.request_id.clone());
        coordinator.complete(receipt(&call, result)).unwrap();
        assert!(!coordinator.ready());
    }

    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Rendered {
                artifact: motion.clone(),
            },
        ))
        .unwrap();

    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Rendered {
                artifact: audio.clone(),
            },
        ))
        .unwrap();

    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Verified {
                artifact_digest: motion.sha256.clone(),
                report: verification_report(
                    plan.body.motion.plan_digest.clone(),
                    plan.body.motion.required_rules.clone(),
                    EvidenceSource::RendererState,
                    motion.sha256.clone(),
                ),
            },
        ))
        .unwrap();

    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Verified {
                artifact_digest: audio.sha256.clone(),
                report: verification_report(
                    plan.body.audio.plan_digest.clone(),
                    plan.body.audio.required_rules.clone(),
                    EvidenceSource::NativeApi,
                    audio.sha256.clone(),
                ),
            },
        ))
        .unwrap();

    let motion_input = DeliveryInput {
        token: "motion-input".into(),
        source_digest: motion.sha256.clone(),
        artifact_digest: digest("motion-mezzanine"),
        owner: owner(),
        metadata: motion.metadata.clone(),
        operation: TransferKind::LosslessMezzanine,
        verification: None,
    };
    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Transferred {
                input: motion_input.clone(),
            },
        ))
        .unwrap();

    let audio_input = DeliveryInput {
        token: "audio-input".into(),
        source_digest: audio.sha256.clone(),
        artifact_digest: audio.sha256.clone(),
        owner: owner(),
        metadata: audio.metadata.clone(),
        operation: TransferKind::ByteCopy,
        verification: None,
    };
    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Transferred {
                input: audio_input.clone(),
            },
        ))
        .unwrap();

    let encoded = encoded_artifact(&plan);
    let decoded_audio = decoded_audio_artifact(&plan, &encoded);
    let decoded_handoff = ArtifactHandoffHint {
        version: 1,
        artifact_digest: decoded_audio.sha256.clone(),
        relative_path: "encoded-final-audio.wav".into(),
    };
    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Encoded {
                artifact: encoded.clone(),
                decoded_audio: Box::new(decoded_audio.clone()),
                decoded_audio_handoff: decoded_handoff,
            },
        ))
        .unwrap();

    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    let StagePayload::VerifyFinalAudio {
        artifact, handoff, ..
    } = &call.payload
    else {
        panic!("expected final-audio verification payload");
    };
    assert_eq!(artifact.sha256, decoded_audio.sha256);
    assert_eq!(handoff.artifact_digest, decoded_audio.sha256);
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Verified {
                artifact_digest: decoded_audio.sha256.clone(),
                report: verification_report(
                    plan.digest.clone(),
                    plan.body.spec.required_final_audio_rules.clone(),
                    EvidenceSource::DecodedMedia,
                    decoded_audio.sha256.clone(),
                ),
            },
        ))
        .unwrap();

    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    coordinator
        .complete(receipt(
            &call,
            NativeResult::SyncMeasured {
                probe: probe(Q::ZERO),
            },
        ))
        .unwrap();

    let manifest = coordinator.manifest().unwrap();
    let candidate = PublicationCandidate {
        owner: owner(),
        manifest_digest: c::canonical_digest(&manifest).unwrap(),
        source_root: "candidate".into(),
        source_path: "candidate.json".into(),
        destination_root: "output".into(),
        destination_path: "ready.json".into(),
        bytes: 1024,
    };
    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    coordinator
        .complete(receipt(
            &call,
            NativeResult::PublicationPrepared {
                candidate: candidate.clone(),
            },
        ))
        .unwrap();
    assert!(!coordinator.ready());

    let call = next(&mut coordinator);
    request_ids.push(call.request_id.clone());
    coordinator
        .complete(receipt(
            &call,
            NativeResult::Published {
                manifest_digest: candidate.manifest_digest,
                pointer: candidate.destination_path,
            },
        ))
        .unwrap();

    assert_eq!(coordinator.state(), AvState::Verified);
    assert!(coordinator.ready());
    assert_eq!(coordinator.next_stage(), None);
    assert_eq!(request_ids.len(), Stage::ALL.len());
    let unique = request_ids
        .iter()
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(unique.len(), Stage::ALL.len());
    for (index, request) in request_ids.iter().enumerate() {
        assert!(request.ends_with(&(index + 1).to_string()), "{request}");
    }
}

#[test]
fn native_receipt_binding_fields_are_independently_authenticated() {
    enum Corruption {
        Request,
        Owner,
        Plan,
        Stage,
        Proof,
    }
    for corruption in [
        Corruption::Request,
        Corruption::Owner,
        Corruption::Plan,
        Corruption::Stage,
        Corruption::Proof,
    ] {
        let mut coordinator = AvCoordinator::new(plan()).unwrap();
        let call = next(&mut coordinator);
        let mut value = receipt(
            &call,
            NativeResult::DeliveryPlanned {
                profile_digest: c::canonical_digest(&coordinator.plan().body.spec.delivery)
                    .unwrap(),
            },
        );
        match corruption {
            Corruption::Request => value.request_id = "foreign-request".into(),
            Corruption::Owner => value.owner.session = "foreign-session".into(),
            Corruption::Plan => value.av_plan_digest = digest("foreign-plan"),
            Corruption::Stage => value.stage = Stage::ApplyMotion,
            Corruption::Proof => value.proof.generation += 1,
        }
        assert!(coordinator.complete(value).is_err());
        assert_eq!(coordinator.state(), AvState::Unknown);
        assert!(!coordinator.ready());
    }
}

#[test]
fn native_terminal_receipts_preserve_distinct_av_states() {
    let cases = [
        (ExecutionStatus::Denied, AvState::Denied),
        (ExecutionStatus::Cancelled, AvState::Cancelled),
        (ExecutionStatus::Partial, AvState::PartiallyApplied),
        (ExecutionStatus::Unknown, AvState::Unknown),
        (ExecutionStatus::Failed, AvState::Failed),
    ];
    for (status, expected) in cases {
        let mut coordinator = AvCoordinator::new(plan()).unwrap();
        let call = next(&mut coordinator);
        let mut value = receipt(
            &call,
            NativeResult::DeliveryPlanned {
                profile_digest: c::canonical_digest(&coordinator.plan().body.spec.delivery)
                    .unwrap(),
            },
        );
        value.status = status;
        value.result = None;
        assert!(coordinator.complete(value).is_err(), "{status:?}");
        assert_eq!(coordinator.state(), expected, "{status:?}");
        assert!(!coordinator.ready());
    }
}

#[test]
fn read_only_stage_cannot_change_even_its_own_provider_resource() {
    let mut coordinator = AvCoordinator::new(plan()).unwrap();
    let call = next(&mut coordinator);
    let mut value = receipt(
        &call,
        NativeResult::DeliveryPlanned {
            profile_digest: c::canonical_digest(&coordinator.plan().body.spec.delivery).unwrap(),
        },
    );
    let delivery = value
        .observed_base
        .0
        .iter_mut()
        .find(|state| state.key.provider == "fixture:delivery")
        .unwrap();
    delivery.revision = Revision::Counter(2);
    assert!(coordinator.complete(value).is_err());
    assert_eq!(coordinator.state(), AvState::Unknown);
}

#[test]
fn sync_boundaries_are_inclusive_and_confidence_floor_is_exact() {
    let spec = sync();

    let mut exact_offset = probe(spec.max_offset);
    exact_offset.flashes = detections(Q::ZERO);
    let report = verify_sync(&spec, &exact_offset).unwrap();
    assert_eq!(report.verdict, Verdict::Pass);

    let mut exact_confidence = probe(Q::ZERO);
    for detection in exact_confidence
        .flashes
        .iter_mut()
        .chain(exact_confidence.impulses.iter_mut())
    {
        detection.confidence = spec.confidence_floor;
    }
    assert_eq!(
        verify_sync(&spec, &exact_confidence).unwrap().verdict,
        Verdict::Pass
    );

    for video_side in [true, false] {
        let mut below = exact_confidence.clone();
        if video_side {
            below.flashes[0].confidence = spec.confidence_floor - 1;
        } else {
            below.impulses[0].confidence = spec.confidence_floor - 1;
        }
        assert_eq!(
            verify_sync(&spec, &below).unwrap().verdict,
            Verdict::Unknown
        );
        below.impulses[0].presentation_time = q(3, 2);
        assert_eq!(verify_sync(&spec, &below).unwrap().verdict, Verdict::Fail);
    }
}

#[test]
fn sync_spec_validates_collection_order_tolerance_and_confidence_boundaries() {
    let mut value = sync();
    value.confidence_floor = 10_000;
    value.max_offset = Q::ONE;
    value.max_drift = Q::ONE;
    value.max_cue_error = Q::ONE;
    value.validate().unwrap();

    let mut invalid = value.clone();
    invalid.confidence_floor = 10_001;
    assert!(invalid.validate().is_err());

    let mut invalid = value.clone();
    invalid.cues.clear();
    assert!(invalid.validate().is_err());

    let mut invalid = value.clone();
    invalid.cues = (0..513)
        .map(|index| SyncCue {
            id: format!("cue-{index}"),
            expected_time: q(index, 1),
        })
        .collect();
    assert!(invalid.validate().is_err());

    let mut invalid = value.clone();
    invalid.cues[1].expected_time = invalid.cues[0].expected_time;
    assert!(invalid.validate().is_err());

    let mut invalid = value;
    invalid.max_offset = q(1001, 1000);
    assert!(invalid.validate().is_err());
}

#[test]
fn imported_b_audio_receipt_advances_only_audio_stages_and_updates_only_audio_base() {
    let plan = plan();
    let mut coordinator = AvCoordinator::new(plan.clone()).unwrap();

    let delivery = next(&mut coordinator);
    coordinator
        .complete(receipt(
            &delivery,
            NativeResult::DeliveryPlanned {
                profile_digest: c::canonical_digest(&plan.body.spec.delivery).unwrap(),
            },
        ))
        .unwrap();

    let apply_motion = next(&mut coordinator);
    coordinator
        .complete(receipt(&apply_motion, NativeResult::Applied))
        .unwrap();
    assert_eq!(coordinator.next_stage(), Some(Stage::ApplyAudio));

    let mut observed_audio = plan.body.audio.base.clone();
    observed_audio.0[0].revision = Revision::Counter(2);
    let mut master = audio_artifact();
    master.source_state = observed_audio.clone();
    let mut verification = verification_report_on_base(
        plan.body.audio.plan_digest.clone(),
        plan.body.audio.required_rules.clone(),
        EvidenceSource::DecodedMedia,
        master.sha256.clone(),
        observed_audio.clone(),
    );
    for check in &mut verification.validation.checks {
        let mut context = check.evidence[0].clone();
        context.id = format!("context-{}", check.rule);
        context.method = "audio-decoded-constraints".into();
        context.artifact = None;
        check.evidence.push(context);
    }
    let imported = AudioConsumerReceipt {
        version: 1,
        project: plan.body.audio.clone(),
        master: master.clone(),
        stems: vec![],
        verification,
        cue_digest: plan.body.audio.cue_digest.clone(),
        handoff: Some(ArtifactHandoffHint {
            version: 1,
            artifact_digest: master.sha256.clone(),
            relative_path: "audio-final.wav".into(),
        }),
    };
    coordinator.import_audio_receipt(imported).unwrap();
    coordinator.complete_imported_audio_stage().unwrap();
    assert_eq!(coordinator.next_stage(), Some(Stage::RenderMotion));
    let audio_state = coordinator
        .expected_base()
        .0
        .iter()
        .find(|state| state.key.resource == "audio")
        .unwrap();
    assert_eq!(audio_state.revision, Revision::Counter(2));
    for state in coordinator
        .expected_base()
        .0
        .iter()
        .filter(|state| state.key.resource != "audio")
    {
        assert_eq!(state.revision, Revision::Counter(1));
    }
    assert_eq!(
        coordinator.ledger().last().unwrap().origin,
        CompletionOrigin::ImportedReceipt
    );
    assert_eq!(
        coordinator.ledger().last().unwrap().stage,
        Stage::ApplyAudio
    );

    let motion = reusable_artifact(
        "motion",
        BTreeMap::from([("visual".into(), digest("visual-v1"))]),
    );
    let render_motion = next(&mut coordinator);
    coordinator
        .complete(receipt(
            &render_motion,
            NativeResult::Rendered {
                artifact: motion.clone(),
            },
        ))
        .unwrap();
    assert_eq!(coordinator.next_stage(), Some(Stage::RenderAudio));
    coordinator.complete_imported_audio_stage().unwrap();
    assert_eq!(coordinator.next_stage(), Some(Stage::VerifyMotion));
    assert_eq!(
        coordinator.ledger().last().unwrap().origin,
        CompletionOrigin::ImportedReceipt
    );

    let verify_motion = next(&mut coordinator);
    coordinator
        .complete(receipt(
            &verify_motion,
            NativeResult::Verified {
                artifact_digest: motion.sha256.clone(),
                report: verification_report(
                    plan.body.motion.plan_digest.clone(),
                    plan.body.motion.required_rules.clone(),
                    EvidenceSource::RendererState,
                    motion.sha256,
                ),
            },
        ))
        .unwrap();
    assert_eq!(coordinator.next_stage(), Some(Stage::VerifyAudio));
    coordinator.complete_imported_audio_stage().unwrap();
    assert_eq!(coordinator.next_stage(), Some(Stage::TransferMotion));
    assert_eq!(
        coordinator
            .ledger()
            .iter()
            .filter(|entry| entry.origin == CompletionOrigin::ImportedReceipt)
            .map(|entry| entry.stage)
            .collect::<Vec<_>>(),
        [Stage::ApplyAudio, Stage::RenderAudio, Stage::VerifyAudio]
    );
}

#[test]
fn imported_b_audio_receipt_rejects_substitution_and_rebinding() {
    let plan = plan();
    let mut coordinator = AvCoordinator::new(plan.clone()).unwrap();
    let mut master = audio_artifact();
    let audio_base = plan.body.audio.base.clone();
    master.source_state = audio_base.clone();
    let valid = AudioConsumerReceipt {
        version: 1,
        project: plan.body.audio.clone(),
        master: master.clone(),
        stems: vec![],
        verification: verification_report_on_base(
            plan.body.audio.plan_digest.clone(),
            plan.body.audio.required_rules.clone(),
            EvidenceSource::NativeApi,
            master.sha256.clone(),
            audio_base,
        ),
        cue_digest: plan.body.audio.cue_digest.clone(),
        handoff: Some(ArtifactHandoffHint {
            version: 1,
            artifact_digest: master.sha256.clone(),
            relative_path: "audio-final.wav".into(),
        }),
    };

    let mut wrong_plan = valid.clone();
    wrong_plan.project.plan_digest = digest("substituted-audio-plan");
    assert!(coordinator.import_audio_receipt(wrong_plan).is_err());

    let mut wrong_handoff = valid.clone();
    wrong_handoff.handoff.as_mut().unwrap().artifact_digest = digest("other-bytes");
    assert!(coordinator.import_audio_receipt(wrong_handoff).is_err());

    coordinator.import_audio_receipt(valid.clone()).unwrap();
    assert!(coordinator.import_audio_receipt(valid).is_err());
    assert!(coordinator.complete_imported_audio_stage().is_err());
}
