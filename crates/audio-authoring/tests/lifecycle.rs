use semwright_audio_authoring::*;
use semwright_audio_domain::{
    model::{AudioProfile, AudioProject, Bus, BusSend, EffectChain},
    presets::SfxPreset,
    signal_analysis::PcmAnalyzer,
    units::MilliDb,
};
use semwright_media_time::{CueGraph, Interval, Rate, Rational};
use semwright_semantic_composition::*;
use std::collections::BTreeMap;
fn owner() -> Owner {
    Owner {
        session: "host-a".into(),
        principal: PrincipalBinding::HostSession,
    }
}
fn descriptor() -> ProfileDescriptor {
    profile(vec![CapabilityBinding {
        phase: Phase::Plan,
        command: "driver.fixture.composition.plan".into(),
        descriptor: Digest::of_bytes(b"unit-fixture"),
        effects: [EffectClass::Inspect].into(),
    }])
    .unwrap()
}
fn project() -> AudioProject {
    AudioProject::new(AudioProfile::default()).unwrap()
}
fn intent(p: &AudioProject) -> AudioIntent {
    AudioIntent {
        version: 1,
        id: "technical-fixture".into(),
        tracks: vec![TrackIntent {
            id: "sfx".into(),
            name: "Utility pulse".into(),
            role: BusRole::SoundEffect,
            existing_stem: None,
            output_bus: p.master_bus.clone(),
            channels: p.profile.channels,
            gain: MilliDb(-6000),
            pan_milli: 0,
            sends: vec![],
            effects: EffectChain::default(),
            clips: vec![ClipIntent {
                id: "pulse".into(),
                material: Material::SoundEffect {
                    preset: SfxPreset::Click,
                    seed: 42,
                },
                placement: Placement::Absolute {
                    start: Rational::ZERO,
                    duration: Rational::ONE,
                },
                source_offset_frames: 0,
                gain: MilliDb(0),
                fade_in_frames: 0,
                fade_out_frames: 100,
            }],
        }],
        ducking: vec![],
        cues: CueGraph {
            version: 1,
            cues: vec![],
        },
        dependencies: BTreeMap::new(),
        delivery: DeliveryProfile {
            id: "utility".into(),
            peak_ceiling_millidbfs: -1000,
            integrated_lufs_milli: None,
            loudness_tolerance_milli: 1000,
            true_peak_ceiling_millidbtp: None,
            allow_silence: true,
            minimum_master_gain: MilliDb(-24000),
            maximum_master_gain: MilliDb(6000),
        },
        budget: ConvergenceBudget {
            max_iterations: 4,
            max_operations: 64,
            max_findings: 64,
            max_observations: 16,
            max_elapsed_ms: 60000,
        },
    }
}
#[test]
fn plans_are_deterministic_and_leave_the_source_unchanged() {
    let p = project();
    let before = p.semantic_digest().unwrap();
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let first = plan(&p, base.clone(), owner(), intent(&p), &descriptor()).unwrap();
    let second = plan(&p, base, owner(), intent(&p), &descriptor()).unwrap();
    assert_eq!(first.plan.digest, second.plan.digest);
    let result = replay(&p, &first, &descriptor()).unwrap();
    assert_eq!(result.duration(), u64::from(p.profile.sample_rate.0));
    assert_eq!(p.semantic_digest().unwrap(), before);
    assert_eq!(result.stems[0].id, first.logical_bindings["sfx"]);
}
#[test]
fn authoring_plan_materializes_typed_stem_sends() {
    let mut p = project();
    p.buses.push(Bus {
        id: "fx".into(),
        name: "FX".into(),
        channels: 2,
        gain: MilliDb(0),
        pan_milli: 0,
        effects: EffectChain::default(),
        sends: vec![],
        automations: vec![],
    });
    p.validate().unwrap();
    let mut spec = intent(&p);
    spec.tracks[0].sends.push(BusSend {
        target_bus: "fx".into(),
        gain: MilliDb(-9_000),
        enabled: true,
        pre_fader: false,
        role: semwright_audio_domain::model::SendRole::Audio,
        delay_frames: 0,
    });
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = plan(&p, base, owner(), spec, &descriptor()).unwrap();
    let result = replay(&p, &planned, &descriptor()).unwrap();
    let stem = result.stem(&planned.logical_bindings["sfx"]).unwrap();
    assert_eq!(stem.sends.len(), 1);
    assert_eq!(stem.sends[0].target_bus, "fx");
    assert_eq!(stem.sends[0].gain, MilliDb(-9_000));
}

#[test]
fn repeated_sfx_presets_have_distinct_internal_signal_ids() {
    let p = project();
    let mut spec = intent(&p);
    let mut second = spec.tracks[0].clips[0].clone();
    second.id = "pulse-two".into();
    spec.tracks[0].clips.push(second);
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = plan(&p, base, owner(), spec, &descriptor()).unwrap();
    let result = replay(&p, &planned, &descriptor()).unwrap();
    assert_eq!(result.synths.len(), 2);
    result.validate().unwrap();
}
#[test]
fn another_host_session_cannot_execute_a_plan() {
    let p = project();
    let mut session = AudioSession::new(descriptor()).unwrap();
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = session
        .prepare(owner(), base.clone(), &p, intent(&p))
        .unwrap();
    let alien = Owner {
        session: "host-b".into(),
        principal: PrincipalBinding::HostSession,
    };
    assert!(
        session
            .begin_apply(&alien, &planned, &p, &base, "request-1")
            .is_err()
    );
}
#[test]
fn recomputing_the_digest_does_not_authorize_client_plan_changes() {
    let p = project();
    let mut session = AudioSession::new(descriptor()).unwrap();
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let mut planned = session
        .prepare(owner(), base.clone(), &p, intent(&p))
        .unwrap();
    planned.plan.body.intent.tracks[0].gain = MilliDb(6000);
    planned.plan.body.intent_digest = canonical_digest(&planned.plan.body.intent).unwrap();
    planned.plan.digest = canonical_digest(&planned.plan.body).unwrap();
    assert!(
        session
            .begin_apply(&owner(), &planned, &p, &base, "request-1")
            .is_err()
    );
}
#[test]
fn stale_revision_and_replay_are_rejected_before_mutation() {
    let p = project();
    let mut session = AudioSession::new(descriptor()).unwrap();
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = session
        .prepare(owner(), base.clone(), &p, intent(&p))
        .unwrap();
    let mut stale = base.clone();
    stale.0[0].generation = "restarted".into();
    assert!(
        session
            .begin_apply(&owner(), &planned, &p, &stale, "request-1")
            .is_err()
    );
    let candidate = session
        .begin_apply(&owner(), &planned, &p, &base, "request-1")
        .unwrap();
    session
        .finish_apply(candidate.permit, ExecutionStatus::Completed)
        .unwrap();
    assert!(
        session
            .begin_apply(&owner(), &planned, &p, &base, "request-2")
            .is_err()
    );
}
#[test]
fn cancellation_and_revocation_are_terminal_for_the_issued_plan() {
    let p = project();
    let mut session = AudioSession::new(descriptor()).unwrap();
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = session
        .prepare(owner(), base.clone(), &p, intent(&p))
        .unwrap();
    let candidate = session
        .begin_apply(&owner(), &planned, &p, &base, "request-1")
        .unwrap();
    session
        .finish_apply(candidate.permit, ExecutionStatus::Cancelled)
        .unwrap();
    assert_eq!(
        session
            .status(&owner(), planned.plan.digest.as_str())
            .unwrap()
            .0,
        State::Cancelled
    );
    session.revoke(&owner());
    assert!(session.get(&owner(), planned.plan.digest.as_str()).is_err());
}
fn duck(start: i64, end: i64) -> DuckingIntent {
    DuckingIntent {
        id: "duck".into(),
        track: "sfx".into(),
        foreground: vec![
            Interval::new(
                Rational::new(start, 1).unwrap(),
                Rational::new(end, 1).unwrap(),
            )
            .unwrap(),
        ],
        attenuation: MilliDb(-12000),
        attack_frames: 4800,
        release_frames: 9600,
        merge_gap_frames: 0,
        replace_existing_gain_automation: false,
    }
}
#[test]
fn ducking_uses_absolute_gain_and_explicit_sample_boundaries() {
    let curve = ducking_curve(&duck(1, 2), MilliDb(-6000), Rate::new(48000, 1).unwrap()).unwrap();
    let at = |frame| {
        curve
            .points
            .iter()
            .find(|point| point.frame.0 == frame)
            .unwrap()
    };
    assert_eq!(at(0).value_milli, -6000);
    assert_eq!(at(43200).value_milli, -6000);
    assert_eq!(at(48000).value_milli, -18000);
    assert_eq!(at(96000).value_milli, -18000);
    assert_eq!(at(105600).value_milli, -6000);
}
#[test]
fn ambiguous_overlapping_duck_ramps_require_an_explicit_resolution() {
    let mut config = duck(1, 2);
    config
        .foreground
        .push(Interval::new(Rational::new(21, 10).unwrap(), Rational::new(3, 1).unwrap()).unwrap());
    assert!(ducking_curve(&config, MilliDb(0), Rate::new(48000, 1).unwrap()).is_err());
    config.merge_gap_frames = 4800;
    assert!(ducking_curve(&config, MilliDb(0), Rate::new(48000, 1).unwrap()).is_ok());
}
fn measurement(p: &AudioProject, amplitude: f64) -> MeasuredAudio {
    let mut analyzer = PcmAnalyzer::new(
        p.profile.sample_rate,
        p.profile.channels,
        p.duration(),
        -90000,
        1,
    )
    .unwrap();
    let block = vec![amplitude; 2 * 1000];
    let mut remaining = p.duration();
    while remaining > 0 {
        let frames = remaining.min(1000) as usize;
        analyzer.push_interleaved(&block[..frames * 2]).unwrap();
        remaining -= frames as u64;
    }
    MeasuredAudio {
        artifact: Digest::of_bytes(b"unit-fixture-pcm-not-native-proof"),
        source_model: Digest::parse(p.semantic_digest().unwrap()).unwrap(),
        base: base_for(
            p,
            "driver:faust-audio",
            &owner(),
            "generation-1",
            Concurrency::CompareAndSwap,
        )
        .unwrap(),
        statistics: analyzer.finish().unwrap(),
        loudness: None,
        decoder: "unit-fixture-statistics".into(),
        decoder_version: 1,
        exhaustive: true,
    }
}

#[test]
fn effect_contract_is_compiled_and_client_override_is_denied() {
    let p = project();
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = plan(&p, base.clone(), owner(), intent(&p), &descriptor()).unwrap();
    let contract = effects::contract(
        &base,
        &p.master_bus,
        &planned.plan.body.intent.delivery,
        &planned.resulting_model_digest,
        &planned.plan.body.changes.operations,
    )
    .unwrap();
    assert_eq!(
        planned.plan.body.dependencies["effects.contract"],
        contract.digest().unwrap()
    );
    assert_eq!(contract.required_rules(), required_rules());
    assert!(
        contract.rules.iter().all(|rule| planned
            .plan
            .body
            .observation_scope
            .contains(&rule.address))
    );
    let mut hostile = intent(&p);
    hostile.dependencies.insert(
        "effects.contract".into(),
        Digest::of_bytes(b"client-selected-verifier"),
    );
    assert!(plan(&p, base, owner(), hostile, &descriptor()).is_err());
}

#[test]
fn effect_consumer_preserves_decoder_provenance_and_never_claims_mutation_readback() {
    let p = project();
    let mut session = AudioSession::new(descriptor()).unwrap();
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = session
        .prepare(owner(), base.clone(), &p, intent(&p))
        .unwrap();
    let applied = session
        .begin_apply(&owner(), &planned, &p, &base, "effect-apply")
        .unwrap();
    let result = applied.model;
    session
        .finish_apply(applied.permit, ExecutionStatus::Completed)
        .unwrap();
    let measured = measurement(&result, 0.1);
    let raw_artifact = measured.artifact.clone();
    let raw_method = measured.decoder.clone();
    let id = session
        .record_measurement(&owner(), planned.plan.digest.as_str(), measured)
        .unwrap();
    let verified = session
        .verify(&owner(), planned.plan.digest.as_str(), &id, &result)
        .unwrap();
    assert_eq!(verified.verdict().unwrap(), Verdict::Pass);
    assert_eq!(verified.effects_observed.len(), 1);
    assert_eq!(verified.effects_observed[0].property, "decoded_master");
    assert_eq!(verified.validation.checks.len(), 7);
    for check in &verified.validation.checks {
        assert!(
            check
                .evidence
                .iter()
                .any(|e| e.method == "audio-decoded-constraints")
        );
        assert!(
            check
                .evidence
                .iter()
                .any(|e| e.artifact.as_ref() == Some(&raw_artifact) && e.method == raw_method)
        );
    }
}

#[test]
fn effect_consumer_leaves_unavailable_loudness_and_incomplete_decode_unknown() {
    for exhaustive in [true, false] {
        let p = project();
        let mut session = AudioSession::new(descriptor()).unwrap();
        let base = base_for(
            &p,
            "driver:faust-audio",
            &owner(),
            "generation-1",
            Concurrency::CompareAndSwap,
        )
        .unwrap();
        let mut spec = intent(&p);
        spec.delivery.integrated_lufs_milli = Some(-16000);
        let planned = session.prepare(owner(), base.clone(), &p, spec).unwrap();
        let applied = session
            .begin_apply(&owner(), &planned, &p, &base, "unknown-effect-apply")
            .unwrap();
        let result = applied.model;
        session
            .finish_apply(applied.permit, ExecutionStatus::Completed)
            .unwrap();
        let mut measured = measurement(&result, 0.1);
        measured.exhaustive = exhaustive;
        let id = session
            .record_measurement(&owner(), planned.plan.digest.as_str(), measured)
            .unwrap();
        let verified = session
            .verify(&owner(), planned.plan.digest.as_str(), &id, &result)
            .unwrap();
        assert_eq!(verified.verdict().unwrap(), Verdict::Unknown);
        assert_eq!(
            verified
                .validation
                .checks
                .iter()
                .find(|c| c.rule == "audio.loudness")
                .unwrap()
                .verdict,
            Verdict::Unknown
        );
        if !exhaustive {
            assert!(
                verified
                    .validation
                    .checks
                    .iter()
                    .all(|c| c.verdict == Verdict::Unknown)
            );
        }
    }
}

#[test]
fn effect_consumer_denies_foreign_channel_measurement_and_unapplied_verification() {
    let p = project();
    let mut session = AudioSession::new(descriptor()).unwrap();
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = session.prepare(owner(), base, &p, intent(&p)).unwrap();
    let result = replay(&p, &planned, &descriptor()).unwrap();
    let mut foreign = measurement(&result, 0.1);
    foreign.base.0[0].provider_session = "another-channel".into();
    assert!(
        session
            .record_measurement(&owner(), planned.plan.digest.as_str(), foreign)
            .is_err()
    );
    let id = session
        .record_measurement(
            &owner(),
            planned.plan.digest.as_str(),
            measurement(&result, 0.1),
        )
        .unwrap();
    assert!(
        session
            .verify(&owner(), planned.plan.digest.as_str(), &id, &result)
            .is_err()
    );
}
#[test]
fn required_loudness_is_unknown_when_not_measured() {
    let p = project();
    let mut spec = intent(&p);
    spec.delivery.integrated_lufs_milli = Some(-16000);
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = plan(&p, base, owner(), spec, &descriptor()).unwrap();
    let result = replay(&p, &planned, &descriptor()).unwrap();
    let report = validate(&planned, &result, &measurement(&result, 0.1)).unwrap();
    assert_eq!(report.report.verdict().unwrap(), Verdict::Unknown);
}
#[test]
fn gain_repair_keeps_the_shared_lifecycle_and_reverifies_new_pcm() {
    let p = project();
    let mut session = AudioSession::new(descriptor()).unwrap();
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = session
        .prepare(owner(), base.clone(), &p, intent(&p))
        .unwrap();
    let applied = session
        .begin_apply(&owner(), &planned, &p, &base, "apply-1")
        .unwrap();
    let result = applied.model;
    session
        .finish_apply(applied.permit, ExecutionStatus::Completed)
        .unwrap();
    let measured = measurement(&result, 1.0);
    let current_base = measured.base.clone();
    let receipt = session
        .record_measurement(&owner(), planned.plan.digest.as_str(), measured)
        .unwrap();
    let report = session
        .validate_measurement(&owner(), planned.plan.digest.as_str(), &receipt, &result)
        .unwrap();
    assert_eq!(report.report.verdict().unwrap(), Verdict::Fail);
    let (repair, adjustment) = session
        .prepare_gain_repair(
            &owner(),
            planned.plan.digest.as_str(),
            &receipt,
            &result,
            current_base.clone(),
        )
        .unwrap();
    assert_eq!(adjustment.delta_millidb, -1000);
    let candidate = session
        .begin_apply(&owner(), &repair, &result, &current_base, "repair-1")
        .unwrap();
    let repair_contract = effects::contract(
        &repair.plan.body.base,
        &result.master_bus,
        &repair.plan.body.intent.delivery,
        &repair.resulting_model_digest,
        &repair.plan.body.changes.operations,
    )
    .unwrap();
    assert_eq!(
        repair.plan.body.dependencies["effects.contract"],
        repair_contract.digest().unwrap()
    );
    assert_ne!(
        repair.plan.body.dependencies["effects.contract"],
        planned.plan.body.dependencies["effects.contract"]
    );
    let repaired = candidate.model;
    session
        .finish_apply(candidate.permit, ExecutionStatus::Completed)
        .unwrap();
    let new_receipt = session
        .record_measurement(
            &owner(),
            repair.plan.digest.as_str(),
            measurement(&repaired, 0.5),
        )
        .unwrap();
    let verified = session
        .verify(
            &owner(),
            repair.plan.digest.as_str(),
            &new_receipt,
            &repaired,
        )
        .unwrap();
    assert_eq!(verified.verdict().unwrap(), Verdict::Pass);
    assert_eq!(
        session
            .status(&owner(), repair.plan.digest.as_str())
            .unwrap()
            .0,
        State::Verified
    );
    assert!(
        session
            .status(&owner(), planned.plan.digest.as_str())
            .is_err()
    );
}
#[test]
fn sample_peak_and_loudness_conflict_is_not_an_infinite_gain_loop() {
    let p = project();
    let mut spec = intent(&p);
    spec.delivery.integrated_lufs_milli = Some(-16000);
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = plan(&p, base, owner(), spec, &descriptor()).unwrap();
    let result = replay(&p, &planned, &descriptor()).unwrap();
    let mut measured = measurement(&result, 0.9);
    measured.loudness = Some(LoudnessMeasurement {
        method: "unit-fixture-supplied-meters".into(),
        version: "1".into(),
        integrated_lufs_milli: Some(-26000),
        momentary_lufs_milli: None,
        short_term_lufs_milli: None,
        loudness_range_milli: None,
        true_peak_millidbtp: None,
        unknown_reason: None,
    });
    assert!(gain_repair(&planned, &result, &measured).is_err());
}
#[test]
fn wrong_measurement_model_is_rejected() {
    let p = project();
    let base = base_for(
        &p,
        "driver:faust-audio",
        &owner(),
        "generation-1",
        Concurrency::CompareAndSwap,
    )
    .unwrap();
    let planned = plan(&p, base, owner(), intent(&p), &descriptor()).unwrap();
    let result = replay(&p, &planned, &descriptor()).unwrap();
    let mut measured = measurement(&result, 0.1);
    measured.source_model = Digest::of_bytes(b"different-model");
    assert!(validate(&planned, &result, &measured).is_err());
}
