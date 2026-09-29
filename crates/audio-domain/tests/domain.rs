use semwright_audio_domain::{
    analysis::analyze_pcm_i16_interleaved,
    backend::{
        BackendContract, BackendIdentity, ProjectionFidelity, ProjectionLoss, ProjectionLossImpact,
        ProjectionLossKind, ProjectionReport,
    },
    edit::{self, Edit},
    model::{
        AudioClip, AudioProfile, AudioProject, Automation, AutomationCurve, AutomationPoint,
        AutomationTarget, Bus, BusSend, ClipSource, EffectChain, Oscillator, Sample, SampleOrigin,
        SampleSource, Signal, SignalNodeKind, Stem, Synth, SynthParameter, Waveform,
    },
    presets::{self, SfxPreset},
    provider::{
        ASSET_PROVIDER_CONTRACT_VERSION, AssetGenerationReceipt, AssetProviderDescriptor,
        AssetProviderKind, GenerationKind,
    },
    refs::RefStore,
    render::{AudioFormat, BitDepth, RENDER_CONTRACT_VERSION, RenderIntent, RenderSource},
    support::{AudioOperation, OperationSupport},
    time::{SampleFrame, SampleRange, SampleRate},
    units::{MilliDb, MilliHz, Permille},
};

fn empty_project() -> AudioProject {
    AudioProject::new(AudioProfile::default()).unwrap()
}
fn stem(id: &str, output: &str) -> Stem {
    Stem {
        id: id.into(),
        name: id.into(),
        channels: 2,
        muted: false,
        soloed: false,
        gain: MilliDb(0),
        pan_milli: 0,
        output_bus: output.into(),
        sends: vec![],
        clips: vec![],
        effects: EffectChain::default(),
        automations: vec![],
    }
}
fn bus(id: &str) -> Bus {
    Bus {
        id: id.into(),
        name: id.into(),
        channels: 2,
        gain: MilliDb(0),
        pan_milli: 0,
        effects: EffectChain::default(),
        sends: vec![],
        automations: vec![],
    }
}

#[test]
fn model_roundtrips_and_rejects_unknown_fields() {
    let project = empty_project();
    project.validate().unwrap();
    let encoded = serde_json::to_value(&project).unwrap();
    let decoded: AudioProject = serde_json::from_value(encoded.clone()).unwrap();
    assert_eq!(decoded, project);
    let mut future = encoded;
    future
        .as_object_mut()
        .unwrap()
        .insert("future_backend_state".into(), serde_json::json!({"x": 1}));
    assert!(serde_json::from_value::<AudioProject>(future).is_err());
}

#[test]
fn all_sfx_presets_are_deterministic_valid_synths() {
    for preset in SfxPreset::ALL {
        let a = presets::synth_for(*preset, "synth", SampleRate(48_000), 48_000, 7).unwrap();
        let b = presets::synth_for(*preset, "synth", SampleRate(48_000), 48_000, 7).unwrap();
        assert_eq!(a, b, "{preset:?}");
        let mut project = empty_project();
        project.synths.insert(a.id.clone(), a);
        project.validate().unwrap();
    }
}

#[test]
fn fm_is_first_class_and_bounded() {
    let synth = Synth {
        id: "fm".into(),
        name: "FM".into(),
        polyphony: 1,
        signals: vec![Signal {
            id: "carrier".into(),
            inputs: vec![],
            node: SignalNodeKind::FmOscillator {
                carrier_frequency: MilliHz(440_000),
                modulator_frequency: MilliHz(220_000),
                modulation_index_milli: 2_000,
                amplitude: Permille(800),
            },
        }],
        output: "carrier".into(),
    };
    let mut project = empty_project();
    project.synths.insert("fm".into(), synth.clone());
    project.validate().unwrap();
    let mut invalid = synth;
    if let SignalNodeKind::FmOscillator {
        modulation_index_milli,
        ..
    } = &mut invalid.signals[0].node
    {
        *modulation_index_milli = 100_001;
    }
    project.synths.insert("fm".into(), invalid);
    assert!(project.validate().is_err());
}

#[test]
fn signal_cycles_fail_closed() {
    let synth = Synth {
        id: "cyclic".into(),
        name: "cyclic".into(),
        polyphony: 1,
        signals: vec![
            Signal {
                id: "a".into(),
                inputs: vec!["b".into()],
                node: SignalNodeKind::Gain { gain: MilliDb(0) },
            },
            Signal {
                id: "b".into(),
                inputs: vec!["a".into()],
                node: SignalNodeKind::Gain { gain: MilliDb(0) },
            },
        ],
        output: "a".into(),
    };
    let mut project = empty_project();
    project.synths.insert("cyclic".into(), synth);
    assert!(project.validate().is_err());
}

#[test]
fn bus_routing_cycles_are_rejected() {
    let mut project = empty_project();
    project.buses.push(bus("fx"));
    project.buses[0].sends.push(BusSend {
        target_bus: "fx".into(),
        gain: MilliDb(0),
        enabled: true,
        pre_fader: false,
    });
    project.buses[1].sends.push(BusSend {
        target_bus: "master".into(),
        gain: MilliDb(0),
        enabled: true,
        pre_fader: false,
    });
    assert!(project.validate().is_err());
}

#[test]
fn referenced_bus_cannot_be_removed_transactionally() {
    let mut project = empty_project();
    project.buses.push(bus("fx"));
    project.stems.push(stem("dialogue", "fx"));
    project.validate().unwrap();
    let revision = edit::revision(&project).unwrap();
    let err = edit::apply(
        &project,
        &revision,
        Edit::BusRemove { bus: "fx".into() },
        "remove",
    )
    .unwrap_err();
    assert_eq!(err.code, "Conflict");
    assert_eq!(project.buses.len(), 2);
}

#[test]
fn stem_send_is_typed_and_protects_referenced_bus() {
    let mut project = empty_project();
    project.buses.push(bus("fx"));
    project.stems.push(stem("dialogue", "master"));
    project.validate().unwrap();

    let revision = edit::revision(&project).unwrap();
    let changed = edit::apply(
        &project,
        &revision,
        Edit::StemSendSet {
            stem: "dialogue".into(),
            send: BusSend {
                target_bus: "fx".into(),
                gain: MilliDb(-6_000),
                enabled: true,
                pre_fader: true,
            },
        },
        "stem-send",
    )
    .unwrap()
    .result;
    let sends = &changed.stem("dialogue").unwrap().sends;
    assert_eq!(sends.len(), 1);
    assert_eq!(sends[0].target_bus, "fx");
    assert_eq!(sends[0].gain, MilliDb(-6_000));
    assert!(sends[0].pre_fader);

    let revision = edit::revision(&changed).unwrap();
    let err = edit::apply(
        &changed,
        &revision,
        Edit::BusRemove { bus: "fx".into() },
        "remove-send-target",
    )
    .unwrap_err();
    assert_eq!(err.code, "Conflict");

    let revision = edit::revision(&project).unwrap();
    assert!(
        edit::apply(
            &project,
            &revision,
            Edit::StemSendSet {
                stem: "dialogue".into(),
                send: BusSend {
                    target_bus: "missing".into(),
                    gain: MilliDb(0),
                    enabled: true,
                    pre_fader: false,
                },
            },
            "bad-send",
        )
        .is_err()
    );
}

#[test]
fn gain_and_pan_are_semantic_edits() {
    let mut project = empty_project();
    project.stems.push(stem("music", "master"));
    project.validate().unwrap();
    let revision = edit::revision(&project).unwrap();
    let changed = edit::apply(
        &project,
        &revision,
        Edit::StemGainSet {
            stem: "music".into(),
            gain: MilliDb(-6_000),
        },
        "gain",
    )
    .unwrap()
    .result;
    assert_eq!(changed.stem("music").unwrap().gain, MilliDb(-6_000));
    let revision = edit::revision(&changed).unwrap();
    let changed = edit::apply(
        &changed,
        &revision,
        Edit::BusPanSet {
            bus: "master".into(),
            pan_milli: 250,
        },
        "pan",
    )
    .unwrap()
    .result;
    assert_eq!(changed.bus("master").unwrap().pan_milli, 250);
}

#[test]
fn synth_automation_is_typed_and_node_compatible() {
    let synth = Synth {
        id: "tone".into(),
        name: "Tone".into(),
        polyphony: 1,
        signals: vec![Signal {
            id: "osc".into(),
            inputs: vec![],
            node: SignalNodeKind::Oscillator {
                oscillator: Oscillator {
                    waveform: Waveform::Sine,
                    frequency: MilliHz(440_000),
                    end_frequency: None,
                    amplitude: Permille(800),
                    phase_millidegrees: 0,
                    seed: None,
                },
            },
        }],
        output: "osc".into(),
    };
    let mut project = empty_project();
    project.synths.insert("tone".into(), synth);
    let mut s = stem("music", "master");
    s.automations.push(Automation {
        id: "pitch".into(),
        target: AutomationTarget::SynthSignalParameter {
            synth: "tone".into(),
            signal: "osc".into(),
            parameter: SynthParameter::FrequencyHz,
        },
        points: vec![
            AutomationPoint {
                frame: SampleFrame(0),
                value_milli: 440_000,
                curve: AutomationCurve::Linear,
            },
            AutomationPoint {
                frame: SampleFrame(48_000),
                value_milli: 880_000,
                curve: AutomationCurve::Linear,
            },
        ],
    });
    project.stems.push(s);
    project.validate().unwrap();
    if let AutomationTarget::SynthSignalParameter { parameter, .. } =
        &mut project.stems[0].automations[0].target
    {
        *parameter = SynthParameter::FilterCutoffHz;
    }
    assert!(project.validate().is_err());
}

#[test]
fn clip_cannot_exceed_sample_duration() {
    let mut project = empty_project();
    project.samples.insert(
        "sample".into(),
        Sample {
            id: "sample".into(),
            name: "sample".into(),
            channels: 1,
            sample_rate: SampleRate(48_000),
            frames: 100,
            source: SampleSource::Silence,
            origin: SampleOrigin::Deterministic {
                generator: "fixture".into(),
                seed: 1,
            },
        },
    );
    let mut s = stem("s", "master");
    s.clips.push(AudioClip {
        id: "clip".into(),
        name: "clip".into(),
        source: ClipSource::Sample {
            sample: "sample".into(),
        },
        start: SampleFrame(0),
        source_range: SampleRange::new(0, 101).unwrap(),
        gain: MilliDb(0),
        fade_in_frames: 0,
        fade_out_frames: 0,
    });
    project.stems.push(s);
    assert!(project.validate().is_err());
}

#[test]
fn pcm_reference_analysis_is_bounded_and_predictable() {
    let data = vec![16_384i16; 4_800];
    let result = analyze_pcm_i16_interleaved(&data, 1, SampleRate(48_000)).unwrap();
    assert_eq!(result.frames, 4_800);
    assert!((-6_030..=-6_010).contains(&result.peak_millidbfs));
    assert!((-6_030..=-6_010).contains(&result.rms_millidbfs));
    assert_eq!(result.integrated_lufs_milli, None);
}

#[test]
fn render_intent_is_deterministic_and_validates_source() {
    let mut project = empty_project();
    let synth = presets::synth_for(
        SfxPreset::Notification,
        "notification",
        SampleRate(48_000),
        24_000,
        99,
    )
    .unwrap();
    project.synths.insert(synth.id.clone(), synth);
    project.validate().unwrap();
    let intent = RenderIntent {
        contract_version: RENDER_CONTRACT_VERSION,
        source: RenderSource::Synth {
            synth: "notification".into(),
        },
        range: Some(SampleRange::new(0, 24_000).unwrap()),
        format: AudioFormat::Wav,
        sample_rate: SampleRate(48_000),
        channels: 2,
        bit_depth: BitDepth::Pcm24,
        normalize_lufs_milli: None,
        dither_seed: Some(7),
    };
    assert_eq!(
        intent.semantic_digest(&project).unwrap(),
        intent.semantic_digest(&project).unwrap()
    );
}

#[test]
fn optional_ai_provider_is_asset_source_not_authority() {
    let descriptor = AssetProviderDescriptor {
        contract_version: ASSET_PROVIDER_CONTRACT_VERSION,
        id: "audio-ai/example".into(),
        kind: AssetProviderKind::ExternalAi,
        generation_kinds: vec![GenerationKind::SoundEffect],
        network_required: true,
    };
    descriptor.validate().unwrap();
    AssetGenerationReceipt {
        provider_id: descriptor.id,
        provider_request_id: Some("req-1".into()),
        model_id: Some("model-1".into()),
        content_sha256: "a".repeat(64),
        deterministic: false,
    }
    .validate()
    .unwrap();
    let bad = AssetProviderDescriptor {
        contract_version: ASSET_PROVIDER_CONTRACT_VERSION,
        id: "local".into(),
        kind: AssetProviderKind::DeterministicLocal,
        generation_kinds: vec![GenerationKind::SoundEffect],
        network_required: true,
    };
    assert!(bad.validate().is_err());
}

#[test]
fn backend_contract_is_complete_and_fidelity_cannot_lie() {
    let contract = BackendContract::from_support(
        BackendIdentity {
            backend_id: "fixture".into(),
            backend_version: Some("1".into()),
            adapter_id: "fixture/1".into(),
        },
        ProjectionFidelity::Exact,
        |_| (OperationSupport::SafeRoundtrip, None),
    )
    .unwrap();
    assert_eq!(contract.operations.len(), AudioOperation::ALL.len());
    assert_eq!(
        contract.support(AudioOperation::StemGainSet),
        OperationSupport::SafeRoundtrip
    );
    let report = ProjectionReport {
        project: empty_project(),
        fidelity: ProjectionFidelity::Exact,
        losses: vec![ProjectionLoss {
            kind: ProjectionLossKind::NativeMetadataOnly,
            impact: ProjectionLossImpact::Advisory,
            code: "fixture.loss".into(),
            semantic_path: None,
            detail: "native metadata exists".into(),
        }],
    };
    assert!(report.validate().is_err());
}

#[test]
fn refs_are_project_revision_and_kind_bound() {
    let mut refs = RefStore::new(2);
    let token = refs.issue("p", "r1", "stem", "s").unwrap();
    assert_eq!(refs.resolve(&token, "p", "r1", "stem").unwrap(), "s");
    assert!(refs.resolve(&token, "p", "r2", "stem").is_err());
    assert!(refs.resolve(&token, "p", "r1", "bus").is_err());
    refs.invalidate("p");
    assert!(refs.resolve(&token, "p", "r1", "stem").is_err());
}
