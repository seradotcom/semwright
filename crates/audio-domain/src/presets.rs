use crate::{
    Result,
    model::{Envelope, Oscillator, Signal, SignalNodeKind, Synth, Waveform},
    time::SampleRate,
    units::{MilliDb, MilliHz, Permille},
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SfxPreset {
    Click,
    Whoosh,
    Impact,
    Riser,
    Sweep,
    Notification,
}
impl SfxPreset {
    pub const ALL: &'static [Self] = &[
        Self::Click,
        Self::Whoosh,
        Self::Impact,
        Self::Riser,
        Self::Sweep,
        Self::Notification,
    ];
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Click => "click",
            Self::Whoosh => "whoosh",
            Self::Impact => "impact",
            Self::Riser => "riser",
            Self::Sweep => "sweep",
            Self::Notification => "notification",
        }
    }
}

pub fn synth_for(
    preset: SfxPreset,
    id: impl Into<String>,
    sample_rate: SampleRate,
    duration_frames: u64,
    seed: u64,
) -> Result<Synth> {
    sample_rate.validate()?;
    let id = id.into();
    let fast = (u64::from(sample_rate.0) / 200).max(1);
    let medium = (u64::from(sample_rate.0) / 20).max(1);
    let release = duration_frames.saturating_sub(fast + medium).max(1);
    let env = Envelope {
        attack_frames: fast,
        decay_frames: medium,
        sustain: Permille(0),
        release_frames: release,
    };
    let (mut sources, output) = match preset {
        SfxPreset::Click => (
            vec![
                signal_osc("osc", Waveform::Square, 2_400_000, None, 650, None),
                unary("env", "osc", SignalNodeKind::Envelope { envelope: env }),
            ],
            "env",
        ),
        SfxPreset::Whoosh => (
            vec![
                signal_osc("noise", Waveform::Noise, 1_000, None, 900, Some(seed)),
                unary(
                    "filter",
                    "noise",
                    SignalNodeKind::Filter {
                        filter: crate::model::Filter {
                            kind: crate::model::FilterKind::LowPass,
                            cutoff: MilliHz(6_000_000),
                            resonance: Permille(200),
                        },
                    },
                ),
                unary("env", "filter", SignalNodeKind::Envelope { envelope: env }),
            ],
            "env",
        ),
        SfxPreset::Impact => (
            vec![
                signal_osc(
                    "tone",
                    Waveform::Sine,
                    72_000,
                    Some(MilliHz(45_000)),
                    900,
                    None,
                ),
                signal_osc("noise", Waveform::Noise, 1_000, None, 300, Some(seed)),
                Signal {
                    id: "mix".into(),
                    inputs: vec!["tone".into(), "noise".into()],
                    node: SignalNodeKind::Add,
                },
                unary("env", "mix", SignalNodeKind::Envelope { envelope: env }),
            ],
            "env",
        ),
        SfxPreset::Riser => (
            vec![
                signal_osc(
                    "osc",
                    Waveform::Saw,
                    110_000,
                    Some(MilliHz(1_760_000)),
                    650,
                    None,
                ),
                unary(
                    "filter",
                    "osc",
                    SignalNodeKind::Filter {
                        filter: crate::model::Filter {
                            kind: crate::model::FilterKind::LowPass,
                            cutoff: MilliHz(8_000_000),
                            resonance: Permille(150),
                        },
                    },
                ),
                unary("env", "filter", SignalNodeKind::Envelope { envelope: env }),
            ],
            "env",
        ),
        SfxPreset::Sweep => (
            vec![
                signal_osc(
                    "osc",
                    Waveform::Sine,
                    200_000,
                    Some(MilliHz(4_000_000)),
                    700,
                    None,
                ),
                unary("env", "osc", SignalNodeKind::Envelope { envelope: env }),
            ],
            "env",
        ),
        SfxPreset::Notification => (
            vec![
                signal_osc("a", Waveform::Sine, 880_000, None, 450, None),
                signal_osc("b", Waveform::Sine, 1_320_000, None, 350, None),
                Signal {
                    id: "mix".into(),
                    inputs: vec!["a".into(), "b".into()],
                    node: SignalNodeKind::Add,
                },
                unary(
                    "gain",
                    "mix",
                    SignalNodeKind::Gain {
                        gain: MilliDb(-3_000),
                    },
                ),
                unary("env", "gain", SignalNodeKind::Envelope { envelope: env }),
            ],
            "env",
        ),
    };
    let prefix = format!("{id}.");
    for signal in &mut sources {
        signal.id = format!("{prefix}{}", signal.id);
        for input in &mut signal.inputs {
            *input = format!("{prefix}{input}");
        }
    }
    let output = format!("{prefix}{output}");
    Ok(Synth {
        id,
        name: format!("Semwright {}", preset.as_str()),
        polyphony: 1,
        signals: sources,
        output,
    })
}

fn signal_osc(
    id: &str,
    waveform: Waveform,
    frequency: u32,
    end_frequency: Option<MilliHz>,
    amplitude: u16,
    seed: Option<u64>,
) -> Signal {
    Signal {
        id: id.into(),
        inputs: vec![],
        node: SignalNodeKind::Oscillator {
            oscillator: Oscillator {
                waveform,
                frequency: MilliHz(frequency),
                end_frequency,
                amplitude: Permille(amplitude),
                phase_millidegrees: 0,
                seed,
            },
        },
    }
}
fn unary(id: &str, input: &str, node: SignalNodeKind) -> Signal {
    Signal {
        id: id.into(),
        inputs: vec![input.into()],
        node,
    }
}
