use semwright_audio_domain::{
    model::{
        AudioProfile, AudioProject, DistortionAlgorithm, DynamicsDetector, Effect, Filter,
        FilterKind, Oscillator, ReverbAlgorithm, Signal, SignalNodeKind, Synth, Waveform,
    },
    presets::{self, SfxPreset},
    time::SampleRate,
    units::{MilliDb, MilliHz, Permille},
};
use semwright_faust_audio::faust::compile_project_synth;

fn project_with(synth: Synth) -> AudioProject {
    let mut project = AudioProject::new(AudioProfile::default()).unwrap();
    project.synths.insert(synth.id.clone(), synth);
    project.validate().unwrap();
    project
}

fn osc(id: &str, waveform: Waveform, phase: u32) -> Signal {
    Signal {
        id: id.into(),
        inputs: vec![],
        node: SignalNodeKind::Oscillator {
            oscillator: Oscillator {
                waveform,
                frequency: MilliHz(440_000),
                end_frequency: None,
                amplitude: Permille(800),
                phase_millidegrees: phase,
                seed: None,
            },
        },
    }
}

fn effect_project(effect: Effect) -> AudioProject {
    project_with(Synth {
        id: "fx".into(),
        name: "effect fixture".into(),
        polyphony: 1,
        signals: vec![
            osc("osc", Waveform::Sine, 0),
            Signal {
                id: "effect".into(),
                inputs: vec!["osc".into()],
                node: SignalNodeKind::Effect { effect },
            },
        ],
        output: "effect".into(),
    })
}

#[test]
fn every_sfx_preset_translates_deterministically() {
    for preset in SfxPreset::ALL {
        let mut project = AudioProject::new(AudioProfile {
            sample_rate: SampleRate(48_000),
            ..AudioProfile::default()
        })
        .unwrap();
        let synth = presets::synth_for(*preset, "sfx", SampleRate(48_000), 48_000, 42).unwrap();
        project.synths.insert("sfx".into(), synth);
        project.validate().unwrap();

        let a = compile_project_synth(&project, "sfx", 48_000, 2).unwrap();
        let b = compile_project_synth(&project, "sfx", 48_000, 2).unwrap();
        assert_eq!(a, b, "{preset:?}");
        assert!(a.source.contains("stdfaust.lib"));
        assert!(a.source.contains("process = "));
        assert_eq!(a.outputs, 2);
        assert_eq!(a.source_sha256.len(), 64);
    }
}

#[test]
fn fm_and_frequency_sweep_are_first_class() {
    let synth = Synth {
        id: "fm".into(),
        name: "fm sweep".into(),
        polyphony: 1,
        signals: vec![
            Signal {
                id: "sweep".into(),
                inputs: vec![],
                node: SignalNodeKind::Oscillator {
                    oscillator: Oscillator {
                        waveform: Waveform::Saw,
                        frequency: MilliHz(110_000),
                        end_frequency: Some(MilliHz(880_000)),
                        amplitude: Permille(500),
                        phase_millidegrees: 0,
                        seed: None,
                    },
                },
            },
            Signal {
                id: "fmnode".into(),
                inputs: vec![],
                node: SignalNodeKind::FmOscillator {
                    carrier_frequency: MilliHz(440_000),
                    modulator_frequency: MilliHz(220_000),
                    modulation_index_milli: 2_000,
                    amplitude: Permille(800),
                },
            },
            Signal {
                id: "mix".into(),
                inputs: vec!["sweep".into(), "fmnode".into()],
                node: SignalNodeKind::Add,
            },
        ],
        output: "mix".into(),
    };
    let program = compile_project_synth(&project_with(synth), "fm", 96_000, 2).unwrap();
    assert!(program.source.contains("polyblep_saw"));
    assert!(program.source.contains("float(t)"));
    assert!(program.source.matches("os.osc").count() >= 2);
}

#[test]
fn sine_phase_is_preserved_and_other_waveform_phase_fails_closed() {
    let sine = Synth {
        id: "phase".into(),
        name: "phase".into(),
        polyphony: 1,
        signals: vec![osc("sine", Waveform::Sine, 90_000)],
        output: "sine".into(),
    };
    let source = compile_project_synth(&project_with(sine), "phase", 48_000, 1)
        .unwrap()
        .source;
    assert!(source.contains("os.oscp"));
    assert!(source.contains("1.570796"));

    let saw = Synth {
        id: "phase".into(),
        name: "phase".into(),
        polyphony: 1,
        signals: vec![osc("saw", Waveform::Saw, 90_000)],
        output: "saw".into(),
    };
    assert!(compile_project_synth(&project_with(saw), "phase", 48_000, 1).is_err());
}

#[test]
fn fidelity_certified_effects_map_to_explicit_faust_primitives() {
    let cases = [
        (
            Effect::Eq {
                bands: vec![semwright_audio_domain::model::EqBand {
                    frequency: MilliHz(2_000_000),
                    gain: MilliDb(3_000),
                    q_milli: 1_000,
                }],
            },
            "fi.peak_eq_cq",
        ),
        (
            Effect::Compressor {
                threshold: MilliDb(-12_000),
                ratio_milli: 4_000,
                attack_ms: 10,
                release_ms: 100,
                knee: MilliDb(0),
                makeup: MilliDb(2_000),
                detector: DynamicsDetector::Peak,
                channel_link: Permille(1000),
            },
            "co.compressor_mono",
        ),
        (
            Effect::Delay {
                delay_ms: 250,
                feedback: Permille(300),
                mix: Permille(400),
            },
            "ef.echo",
        ),
        (
            Effect::Filter {
                filter: Filter {
                    kind: FilterKind::Notch,
                    cutoff: MilliHz(2_000_000),
                    resonance: Permille(500),
                },
            },
            "fi.notchw",
        ),
        (
            Effect::Distortion {
                algorithm: DistortionAlgorithm::Tanh,
                drive: MilliDb(6_000),
                mix: Permille(500),
            },
            "ma.tanh",
        ),
    ];

    for (effect, primitive) in cases {
        let source = compile_project_synth(&effect_project(effect), "fx", 48_000, 1)
            .unwrap()
            .source;
        assert!(source.contains(primitive), "{primitive}: {source}");
    }
}

#[test]
fn effects_with_unmodeled_semantics_are_rejected_not_approximated() {
    let compressor_with_knee = Effect::Compressor {
        threshold: MilliDb(-12_000),
        ratio_milli: 4_000,
        attack_ms: 10,
        release_ms: 100,
        knee: MilliDb(3_000),
        makeup: MilliDb(0),
        detector: DynamicsDetector::Peak,
        channel_link: Permille(1000),
    };
    assert!(compile_project_synth(&effect_project(compressor_with_knee), "fx", 48_000, 1).is_err());

    let limiter = Effect::Limiter {
        ceiling: MilliDb(-1_000),
        attack_ms: 0,
        lookahead_ms: 5,
        hold_ms: 0,
        release_ms: 100,
        true_peak: true,
        detector: DynamicsDetector::Peak,
        channel_link: Permille(1000),
    };
    assert!(compile_project_synth(&effect_project(limiter), "fx", 48_000, 1).is_err());

    let reverb = Effect::Reverb {
        algorithm: ReverbAlgorithm::Schroeder,
        room: Permille(500),
        damping: Permille(500),
        diffusion: Permille(700),
        mix: Permille(500),
        pre_delay_ms: 10,
    };
    assert!(compile_project_synth(&effect_project(reverb), "fx", 48_000, 1).is_err());
}

#[test]
fn fidelity_sensitive_dynamics_and_channel_maps_fail_closed() {
    let rms = Effect::Compressor {
        threshold: MilliDb(-18_000),
        ratio_milli: 2_000,
        attack_ms: 5,
        release_ms: 80,
        knee: MilliDb(0),
        makeup: MilliDb(0),
        detector: DynamicsDetector::Rms,
        channel_link: Permille(1000),
    };
    assert!(compile_project_synth(&effect_project(rms), "fx", 48_000, 1).is_err());

    let gate = Effect::GateExpander {
        threshold: MilliDb(-40_000),
        ratio_milli: 4_000,
        attack_ms: 5,
        hold_ms: 20,
        release_ms: 100,
        range: MilliDb(-60_000),
        detector: DynamicsDetector::Peak,
        channel_link: Permille(1000),
    };
    assert!(compile_project_synth(&effect_project(gate), "fx", 48_000, 1).is_err());

    let map = Effect::ChannelMap {
        input_channels: 1,
        output_channels: 2,
        matrix_milli: vec![1000, 1000],
    };
    assert!(compile_project_synth(&effect_project(map), "fx", 48_000, 1).is_err());
}

#[test]
fn project_wrapper_rejects_unknown_synth() {
    let project = AudioProject::new(AudioProfile::default()).unwrap();
    assert!(compile_project_synth(&project, "missing", 48_000, 2).is_err());
}
