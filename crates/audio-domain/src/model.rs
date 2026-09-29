use crate::{
    Error, Result,
    hash::sha256,
    provider::AssetGenerationReceipt,
    time::{MAX_SAMPLE_FRAME, SampleFrame, SampleRange, SampleRate},
    units::{MilliDb, MilliHz, Permille},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MODEL_VERSION: u32 = 1;
pub const MAX_SAMPLES: usize = 10_000;
pub const MAX_SYNTHS: usize = 512;
pub const MAX_SIGNALS: usize = 4096;
pub const MAX_STEMS: usize = 512;
pub const MAX_BUSES: usize = 256;
pub const MAX_CLIPS: usize = 100_000;
pub const MAX_EFFECTS: usize = 16_384;
pub const MAX_AUTOMATION_POINTS: usize = 1_000_000;
pub const MAX_GROUPS: usize = 256;
pub const MAX_MARKERS: usize = 4096;
pub const MAX_RANGES: usize = 4096;
pub const MAX_TEMPO_CHANGES: usize = 4096;
pub const MAX_MIDI_PHRASES: usize = 1024;
pub const MAX_MIDI_EVENTS: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioProfile {
    pub sample_rate: SampleRate,
    pub channels: u16,
    pub tempo_milli_bpm: u32,
    pub time_signature_numerator: u8,
    pub time_signature_denominator: u8,
}
impl Default for AudioProfile {
    fn default() -> Self {
        Self {
            sample_rate: SampleRate::default(),
            channels: 2,
            tempo_milli_bpm: 120_000,
            time_signature_numerator: 4,
            time_signature_denominator: 4,
        }
    }
}
impl AudioProfile {
    pub fn validate(&self) -> Result<()> {
        self.sample_rate.validate()?;
        if !(1..=64).contains(&self.channels)
            || !(20_000..=400_000).contains(&self.tempo_milli_bpm)
            || !(1..=32).contains(&self.time_signature_numerator)
            || !matches!(self.time_signature_denominator, 1 | 2 | 4 | 8 | 16 | 32)
        {
            return Err(Error::invalid("Invalid semantic audio profile"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SampleSource {
    RelativePath { path: String },
    Artifact { artifact_id: String, sha256: String },
    Silence,
}
impl SampleSource {
    fn validate(&self) -> Result<()> {
        match self {
            Self::RelativePath { path } => validate_relative_path(path),
            Self::Artifact {
                artifact_id,
                sha256,
            } => {
                validate_text("artifact ID", artifact_id, 256)?;
                validate_sha256(sha256)
            }
            Self::Silence => Ok(()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SampleOrigin {
    Imported,
    Recorded,
    Deterministic { generator: String, seed: u64 },
    Generated { receipt: AssetGenerationReceipt },
}
impl SampleOrigin {
    fn validate(&self) -> Result<()> {
        match self {
            Self::Imported | Self::Recorded => Ok(()),
            Self::Deterministic { generator, .. } => validate_id(generator),
            Self::Generated { receipt } => receipt.validate(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    pub id: String,
    pub name: String,
    pub channels: u16,
    pub sample_rate: SampleRate,
    pub frames: u64,
    pub source: SampleSource,
    pub origin: SampleOrigin,
}
impl Sample {
    pub fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        validate_text("sample name", &self.name, 4096)?;
        self.sample_rate.validate()?;
        if !(1..=64).contains(&self.channels) || self.frames == 0 || self.frames > MAX_SAMPLE_FRAME
        {
            return Err(Error::invalid("Invalid sample shape or duration"));
        }
        self.source.validate()?;
        self.origin.validate()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Waveform {
    Sine,
    Saw,
    Square,
    Triangle,
    Noise,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Oscillator {
    pub waveform: Waveform,
    pub frequency: MilliHz,
    pub end_frequency: Option<MilliHz>,
    pub amplitude: Permille,
    pub phase_millidegrees: u32,
    pub seed: Option<u64>,
}
impl Oscillator {
    pub fn validate(&self) -> Result<()> {
        MilliHz::new(self.frequency.0)?;
        if let Some(value) = self.end_frequency {
            MilliHz::new(value.0)?;
        }
        Permille::new(self.amplitude.0)?;
        if self.phase_millidegrees >= 360_000 {
            return Err(Error::invalid("Oscillator phase must be below 360 degrees"));
        }
        if self.waveform == Waveform::Noise && self.seed.is_none() {
            return Err(Error::invalid("Noise oscillator requires an explicit seed"));
        }
        if self.waveform != Waveform::Noise && self.seed.is_some() {
            return Err(Error::invalid("Only noise oscillators accept a seed"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub attack_frames: u64,
    pub decay_frames: u64,
    pub sustain: Permille,
    pub release_frames: u64,
}
impl Envelope {
    pub fn validate(&self) -> Result<()> {
        Permille::new(self.sustain.0)?;
        if [self.attack_frames, self.decay_frames, self.release_frames]
            .into_iter()
            .any(|value| value > MAX_SAMPLE_FRAME)
        {
            return Err(Error::limit(
                "Envelope segment exceeds audio duration budget",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FilterKind {
    LowPass,
    HighPass,
    BandPass,
    Notch,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Filter {
    pub kind: FilterKind,
    pub cutoff: MilliHz,
    pub resonance: Permille,
}
impl Filter {
    pub fn validate(&self) -> Result<()> {
        MilliHz::new(self.cutoff.0)?;
        Permille::new(self.resonance.0)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EqBand {
    pub frequency: MilliHz,
    pub gain: MilliDb,
    pub q_milli: u32,
}
impl EqBand {
    fn validate(&self) -> Result<()> {
        MilliHz::new(self.frequency.0)?;
        MilliDb::new(self.gain.0)?;
        if !(100..=50_000).contains(&self.q_milli) {
            return Err(Error::invalid("EQ Q is outside semantic bounds"));
        }
        Ok(())
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DynamicsDetector {
    #[default]
    Peak,
    Rms,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ReverbAlgorithm {
    #[default]
    Schroeder,
    Freeverb,
    Plate,
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum DistortionAlgorithm {
    #[default]
    Tanh,
    HardClip,
    SoftClip,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Effect {
    Eq {
        bands: Vec<EqBand>,
    },
    Compressor {
        threshold: MilliDb,
        ratio_milli: u32,
        attack_ms: u32,
        release_ms: u32,
        knee: MilliDb,
        makeup: MilliDb,
        detector: DynamicsDetector,
        channel_link: Permille,
    },
    GateExpander {
        threshold: MilliDb,
        ratio_milli: u32,
        attack_ms: u32,
        hold_ms: u32,
        release_ms: u32,
        range: MilliDb,
        detector: DynamicsDetector,
        channel_link: Permille,
    },
    Limiter {
        ceiling: MilliDb,
        attack_ms: u32,
        lookahead_ms: u32,
        hold_ms: u32,
        release_ms: u32,
        true_peak: bool,
        detector: DynamicsDetector,
        channel_link: Permille,
    },
    Reverb {
        algorithm: ReverbAlgorithm,
        room: Permille,
        damping: Permille,
        diffusion: Permille,
        mix: Permille,
        pre_delay_ms: u32,
    },
    Delay {
        delay_ms: u32,
        feedback: Permille,
        mix: Permille,
    },
    Distortion {
        algorithm: DistortionAlgorithm,
        drive: MilliDb,
        mix: Permille,
    },
    ChannelMap {
        input_channels: u16,
        output_channels: u16,
        matrix_milli: Vec<i32>,
    },
    Filter {
        filter: Filter,
    },
    Gain {
        gain: MilliDb,
    },
}
impl Effect {
    pub fn validate(&self) -> Result<()> {
        match self {
            Self::Eq { bands } => {
                if bands.is_empty() || bands.len() > 32 {
                    return Err(Error::invalid("EQ must contain 1..=32 bands"));
                }
                for band in bands {
                    band.validate()?;
                }
            }
            Self::Compressor {
                threshold,
                ratio_milli,
                attack_ms,
                release_ms,
                knee,
                makeup,
                detector: _,
                channel_link,
            } => {
                MilliDb::new(threshold.0)?;
                MilliDb::new(knee.0)?;
                MilliDb::new(makeup.0)?;
                Permille::new(channel_link.0)?;
                if !(1000..=100_000).contains(ratio_milli)
                    || *attack_ms > 60_000
                    || *release_ms > 120_000
                {
                    return Err(Error::invalid("Invalid compressor parameters"));
                }
            }
            Self::GateExpander {
                threshold,
                ratio_milli,
                attack_ms,
                hold_ms,
                release_ms,
                range,
                detector: _,
                channel_link,
            } => {
                MilliDb::new(threshold.0)?;
                MilliDb::new(range.0)?;
                Permille::new(channel_link.0)?;
                if !(1000..=100_000).contains(ratio_milli)
                    || *attack_ms > 60_000
                    || *hold_ms > 120_000
                    || *release_ms > 120_000
                    || range.0 > 0
                {
                    return Err(Error::invalid("Invalid gate/expander parameters"));
                }
            }
            Self::Limiter {
                ceiling,
                attack_ms,
                lookahead_ms,
                hold_ms,
                release_ms,
                true_peak: _,
                detector: _,
                channel_link,
            } => {
                MilliDb::new(ceiling.0)?;
                Permille::new(channel_link.0)?;
                if ceiling.0 > 0
                    || *attack_ms > 60_000
                    || *lookahead_ms > 10_000
                    || *hold_ms > 120_000
                    || *release_ms > 120_000
                {
                    return Err(Error::invalid("Invalid limiter parameters"));
                }
            }
            Self::Reverb {
                algorithm: _,
                room,
                damping,
                diffusion,
                mix,
                pre_delay_ms,
            } => {
                Permille::new(room.0)?;
                Permille::new(damping.0)?;
                Permille::new(diffusion.0)?;
                Permille::new(mix.0)?;
                if *pre_delay_ms > 10_000 {
                    return Err(Error::invalid("Invalid reverb pre-delay"));
                }
            }
            Self::Delay {
                delay_ms,
                feedback,
                mix,
            } => {
                Permille::new(feedback.0)?;
                Permille::new(mix.0)?;
                if *delay_ms == 0 || *delay_ms > 60_000 || feedback.0 >= 1000 {
                    return Err(Error::invalid("Invalid delay parameters"));
                }
            }
            Self::Distortion {
                algorithm: _,
                drive,
                mix,
            } => {
                MilliDb::new(drive.0)?;
                Permille::new(mix.0)?;
            }
            Self::ChannelMap {
                input_channels,
                output_channels,
                matrix_milli,
            } => {
                if !(1..=64).contains(input_channels)
                    || !(1..=64).contains(output_channels)
                    || matrix_milli.len()
                        != usize::from(*input_channels) * usize::from(*output_channels)
                    || matrix_milli
                        .iter()
                        .any(|value| !(-8_000..=8_000).contains(value))
                {
                    return Err(Error::invalid("Invalid channel-map matrix"));
                }
            }
            Self::Filter { filter } => filter.validate()?,
            Self::Gain { gain } => {
                MilliDb::new(gain.0)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectInstance {
    pub id: String,
    pub enabled: bool,
    pub effect: Effect,
}
impl EffectInstance {
    fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        self.effect.validate()
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EffectChain {
    pub effects: Vec<EffectInstance>,
}
impl EffectChain {
    fn validate(&self) -> Result<()> {
        if self.effects.len() > 256 {
            return Err(Error::limit("Effect-chain size exceeds limit"));
        }
        let mut ids = BTreeSet::new();
        for effect in &self.effects {
            effect.validate()?;
            if !ids.insert(&effect.id) {
                return Err(Error::invalid("Duplicate effect identity"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SignalNodeKind {
    Oscillator {
        oscillator: Oscillator,
    },
    FmOscillator {
        carrier_frequency: MilliHz,
        modulator_frequency: MilliHz,
        modulation_index_milli: u32,
        amplitude: Permille,
    },
    SamplePlayer {
        sample: String,
        looped: bool,
    },
    Input {
        channel: u16,
    },
    Constant {
        value_milli: i32,
    },
    Envelope {
        envelope: Envelope,
    },
    Filter {
        filter: Filter,
    },
    Effect {
        effect: Effect,
    },
    Gain {
        gain: MilliDb,
    },
    Add,
    Multiply,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Signal {
    pub id: String,
    pub inputs: Vec<String>,
    pub node: SignalNodeKind,
}
impl Signal {
    fn validate_shape(&self) -> Result<()> {
        validate_id(&self.id)?;
        if self.inputs.len() > 16 {
            return Err(Error::limit("Signal fan-in exceeds limit"));
        }
        let expected = match &self.node {
            SignalNodeKind::Oscillator { oscillator } => {
                oscillator.validate()?;
                0..=0
            }
            SignalNodeKind::FmOscillator {
                carrier_frequency,
                modulator_frequency,
                modulation_index_milli,
                amplitude,
            } => {
                MilliHz::new(carrier_frequency.0)?;
                MilliHz::new(modulator_frequency.0)?;
                Permille::new(amplitude.0)?;
                if *modulation_index_milli > 100_000 {
                    return Err(Error::invalid(
                        "FM modulation index exceeds semantic bounds",
                    ));
                }
                0..=0
            }
            SignalNodeKind::SamplePlayer { sample, .. } => {
                validate_id(sample)?;
                0..=0
            }
            SignalNodeKind::Input { channel } => {
                if *channel > 63 {
                    return Err(Error::invalid("Signal input channel exceeds limit"));
                }
                0..=0
            }
            SignalNodeKind::Constant { .. } => 0..=0,
            SignalNodeKind::Envelope { envelope } => {
                envelope.validate()?;
                1..=1
            }
            SignalNodeKind::Filter { filter } => {
                filter.validate()?;
                1..=1
            }
            SignalNodeKind::Effect { effect } => {
                effect.validate()?;
                1..=1
            }
            SignalNodeKind::Gain { gain } => {
                MilliDb::new(gain.0)?;
                1..=1
            }
            SignalNodeKind::Add | SignalNodeKind::Multiply => 2..=16,
        };
        if !expected.contains(&self.inputs.len()) {
            return Err(Error::invalid("Signal node has invalid input arity"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Synth {
    pub id: String,
    pub name: String,
    pub polyphony: u16,
    pub signals: Vec<Signal>,
    pub output: String,
}
impl Synth {
    pub fn validate(&self, samples: &BTreeMap<String, Sample>) -> Result<()> {
        validate_id(&self.id)?;
        validate_text("synth name", &self.name, 4096)?;
        if !(1..=256).contains(&self.polyphony)
            || self.signals.is_empty()
            || self.signals.len() > MAX_SIGNALS
        {
            return Err(Error::invalid("Invalid synth polyphony or signal count"));
        }
        let mut ids = BTreeSet::new();
        for signal in &self.signals {
            signal.validate_shape()?;
            if !ids.insert(signal.id.clone()) {
                return Err(Error::invalid("Duplicate signal identity"));
            }
            if let SignalNodeKind::SamplePlayer { sample, .. } = &signal.node
                && !samples.contains_key(sample)
            {
                return Err(Error::invalid("Synth signal references missing sample"));
            }
        }
        if !ids.contains(&self.output) {
            return Err(Error::invalid("Synth output signal is absent"));
        }
        for signal in &self.signals {
            for input in &signal.inputs {
                if !ids.contains(input) {
                    return Err(Error::invalid("Signal graph references missing input"));
                }
            }
        }
        validate_acyclic_signals(&self.signals)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AutomationCurve {
    Hold,
    Linear,
    Smooth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EffectParameter {
    GainDb,
    FrequencyHz,
    Q,
    ThresholdDb,
    Ratio,
    AttackMs,
    HoldMs,
    LookaheadMs,
    ReleaseMs,
    ChannelLink,
    Mix,
    Feedback,
    Room,
    Damping,
    Diffusion,
    PreDelayMs,
    DriveDb,
    CeilingDb,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SynthParameter {
    FrequencyHz,
    EndFrequencyHz,
    Amplitude,
    Phase,
    AttackFrames,
    DecayFrames,
    Sustain,
    ReleaseFrames,
    FilterCutoffHz,
    FilterResonance,
    GainDb,
    FmCarrierFrequencyHz,
    FmModulatorFrequencyHz,
    FmModulationIndex,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationTarget {
    StemGain,
    StemPan,
    BusGain,
    BusPan,
    StemSendGain {
        target_bus: String,
    },
    BusSendGain {
        target_bus: String,
    },
    EffectParameter {
        effect: String,
        parameter: EffectParameter,
    },
    SynthSignalParameter {
        synth: String,
        signal: String,
        parameter: SynthParameter,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AutomationPoint {
    pub frame: SampleFrame,
    pub value_milli: i64,
    pub curve: AutomationCurve,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Automation {
    pub id: String,
    pub target: AutomationTarget,
    pub points: Vec<AutomationPoint>,
}
impl Automation {
    fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        if self.points.is_empty() || self.points.len() > MAX_AUTOMATION_POINTS {
            return Err(Error::invalid("Automation point count is invalid"));
        }
        let mut previous = None;
        for point in &self.points {
            if point.frame.0 > MAX_SAMPLE_FRAME
                || previous.is_some_and(|frame| frame >= point.frame.0)
            {
                return Err(Error::invalid(
                    "Automation frames must be strictly increasing and bounded",
                ));
            }
            previous = Some(point.frame.0);
        }
        if let AutomationTarget::EffectParameter { effect, .. } = &self.target {
            validate_id(effect)?;
        }
        if let AutomationTarget::StemSendGain { target_bus }
        | AutomationTarget::BusSendGain { target_bus } = &self.target
        {
            validate_id(target_bus)?;
        }
        if let AutomationTarget::SynthSignalParameter { synth, signal, .. } = &self.target {
            validate_id(synth)?;
            validate_id(signal)?;
        }
        for point in &self.points {
            validate_automation_value(&self.target, point.value_milli)?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClipSource {
    Sample {
        sample: String,
    },
    Synth {
        synth: String,
        midi_note: u8,
        velocity: Permille,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioClip {
    pub id: String,
    pub name: String,
    pub source: ClipSource,
    pub start: SampleFrame,
    pub source_range: SampleRange,
    pub gain: MilliDb,
    pub fade_in_frames: u64,
    pub fade_out_frames: u64,
}
impl AudioClip {
    pub fn duration(&self) -> u64 {
        self.source_range.duration()
    }
    pub fn end(&self) -> Result<u64> {
        self.start
            .0
            .checked_add(self.duration())
            .filter(|value| *value <= MAX_SAMPLE_FRAME)
            .ok_or_else(|| Error::limit("Audio clip end exceeds duration budget"))
    }
}

#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SendRole {
    #[default]
    Audio,
    Sidechain,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BusSend {
    pub target_bus: String,
    pub gain: MilliDb,
    pub enabled: bool,
    pub pre_fader: bool,
    #[serde(default)]
    pub role: SendRole,
    /// Explicit sample delay on the feedback edge. Zero means algebraic/current block.
    #[serde(default)]
    pub delay_frames: u32,
}
impl BusSend {
    pub fn validate(&self) -> Result<()> {
        validate_id(&self.target_bus)?;
        MilliDb::new(self.gain.0)?;
        if self.delay_frames > 57_600_000 {
            return Err(Error::limit("Send delay exceeds bounded feedback memory"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Bus {
    pub id: String,
    pub name: String,
    pub channels: u16,
    pub gain: MilliDb,
    pub pan_milli: i16,
    pub effects: EffectChain,
    pub sends: Vec<BusSend>,
    pub automations: Vec<Automation>,
}
impl Bus {
    fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        validate_text("bus name", &self.name, 4096)?;
        if !(1..=64).contains(&self.channels) || !(-1000..=1000).contains(&self.pan_milli) {
            return Err(Error::invalid("Invalid bus channel count or pan"));
        }
        MilliDb::new(self.gain.0)?;
        self.effects.validate()?;
        if self.sends.len() > 256 {
            return Err(Error::limit("Bus send count exceeds limit"));
        }
        for send in &self.sends {
            send.validate()?;
        }
        validate_automations(&self.automations)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Stem {
    pub id: String,
    pub name: String,
    pub channels: u16,
    pub muted: bool,
    pub soloed: bool,
    pub gain: MilliDb,
    pub pan_milli: i16,
    pub output_bus: String,
    #[serde(default)]
    pub sends: Vec<BusSend>,
    pub clips: Vec<AudioClip>,
    pub effects: EffectChain,
    pub automations: Vec<Automation>,
}
impl Stem {
    fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        validate_text("stem name", &self.name, 4096)?;
        validate_id(&self.output_bus)?;
        if !(1..=64).contains(&self.channels) || !(-1000..=1000).contains(&self.pan_milli) {
            return Err(Error::invalid("Invalid stem channel count or pan"));
        }
        MilliDb::new(self.gain.0)?;
        self.effects.validate()?;
        if self.sends.len() > 256 {
            return Err(Error::limit("Stem send count exceeds limit"));
        }
        for send in &self.sends {
            send.validate()?;
        }
        if self.clips.len() > MAX_CLIPS {
            return Err(Error::limit("Stem clip count exceeds limit"));
        }
        for clip in &self.clips {
            validate_id(&clip.id)?;
            validate_text("clip name", &clip.name, 4096)?;
            MilliDb::new(clip.gain.0)?;
            SampleRange::new(clip.source_range.start.0, clip.source_range.end.0)?;
            if clip
                .fade_in_frames
                .checked_add(clip.fade_out_frames)
                .is_none_or(|sum| sum > clip.duration())
            {
                return Err(Error::invalid("Clip fades exceed clip duration"));
            }
            clip.end()?;
        }
        validate_automations(&self.automations)
    }
    pub fn duration(&self) -> u64 {
        self.clips
            .iter()
            .filter_map(|clip| clip.end().ok())
            .max()
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProjectMetadata {
    pub name: Option<String>,
    pub session_label: Option<String>,
    pub delivery_profile: Option<String>,
}
impl ProjectMetadata {
    fn validate(&self) -> Result<()> {
        for (label, value) in [
            ("project name", self.name.as_deref()),
            ("session label", self.session_label.as_deref()),
            ("delivery profile", self.delivery_profile.as_deref()),
        ] {
            if let Some(value) = value {
                validate_text(label, value, 4096)?;
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StemGroup {
    pub id: String,
    pub name: String,
    pub stems: Vec<String>,
}
impl StemGroup {
    fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        validate_text("stem group name", &self.name, 4096)?;
        if self.stems.is_empty() || self.stems.len() > MAX_STEMS {
            return Err(Error::invalid("Stem group membership count is invalid"));
        }
        let mut seen = BTreeSet::new();
        for stem in &self.stems {
            validate_id(stem)?;
            if !seen.insert(stem) {
                return Err(Error::invalid("Stem group repeats a member"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Marker {
    pub id: String,
    pub frame: SampleFrame,
    pub label: String,
}
impl Marker {
    fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        validate_text("marker label", &self.label, 4096)?;
        if self.frame.0 > MAX_SAMPLE_FRAME {
            return Err(Error::limit("Marker exceeds sample-frame budget"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NamedRange {
    pub id: String,
    pub range: SampleRange,
    pub label: String,
}
impl NamedRange {
    fn validate(&self) -> Result<()> {
        validate_id(&self.id)?;
        validate_text("range label", &self.label, 4096)?;
        SampleRange::new(self.range.start.0, self.range.end.0)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TempoChange {
    pub frame: SampleFrame,
    pub tempo_milli_bpm: u32,
    pub time_signature_numerator: u8,
    pub time_signature_denominator: u8,
}
impl TempoChange {
    fn validate(&self) -> Result<()> {
        if self.frame.0 == 0
            || self.frame.0 > MAX_SAMPLE_FRAME
            || !(20_000..=400_000).contains(&self.tempo_milli_bpm)
            || !(1..=32).contains(&self.time_signature_numerator)
            || !matches!(self.time_signature_denominator, 1 | 2 | 4 | 8 | 16 | 32)
        {
            return Err(Error::invalid("Invalid tempo/meter change"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MidiEvent {
    Note {
        id: String,
        start: SampleFrame,
        duration_frames: u64,
        channel: u8,
        note: u8,
        velocity: u8,
    },
    Control {
        id: String,
        frame: SampleFrame,
        channel: u8,
        controller: u8,
        value: u8,
    },
}
impl MidiEvent {
    fn id(&self) -> &str {
        match self {
            Self::Note { id, .. } | Self::Control { id, .. } => id,
        }
    }
    fn validate(&self) -> Result<()> {
        validate_id(self.id())?;
        match self {
            Self::Note {
                start,
                duration_frames,
                channel,
                note,
                velocity,
                ..
            } => {
                if *channel > 15
                    || *note > 127
                    || *velocity > 127
                    || *duration_frames == 0
                    || start
                        .0
                        .checked_add(*duration_frames)
                        .is_none_or(|end| end > MAX_SAMPLE_FRAME)
                {
                    return Err(Error::invalid("Invalid bounded MIDI note"));
                }
            }
            Self::Control {
                frame,
                channel,
                controller,
                value,
                ..
            } => {
                if *channel > 15 || *controller > 127 || *value > 127 || frame.0 > MAX_SAMPLE_FRAME
                {
                    return Err(Error::invalid("Invalid bounded MIDI control event"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MidiPhrase {
    pub id: String,
    pub name: String,
    pub instrument_synth: Option<String>,
    pub events: Vec<MidiEvent>,
}
impl MidiPhrase {
    fn validate(&self, synths: &BTreeMap<String, Synth>) -> Result<()> {
        validate_id(&self.id)?;
        validate_text("MIDI phrase name", &self.name, 4096)?;
        if self.events.is_empty() || self.events.len() > MAX_MIDI_EVENTS {
            return Err(Error::invalid("MIDI phrase event count is invalid"));
        }
        if let Some(synth) = &self.instrument_synth {
            validate_id(synth)?;
            if !synths.contains_key(synth) {
                return Err(Error::invalid("MIDI phrase instrument synth is missing"));
            }
        }
        let mut ids = BTreeSet::new();
        for event in &self.events {
            event.validate()?;
            if !ids.insert(event.id()) {
                return Err(Error::invalid("MIDI phrase repeats an event identity"));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioProject {
    pub model_version: u32,
    pub id: String,
    pub profile: AudioProfile,
    #[serde(default)]
    pub metadata: ProjectMetadata,
    pub samples: BTreeMap<String, Sample>,
    pub synths: BTreeMap<String, Synth>,
    pub stems: Vec<Stem>,
    pub buses: Vec<Bus>,
    pub master_bus: String,
    #[serde(default)]
    pub groups: Vec<StemGroup>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub ranges: Vec<NamedRange>,
    #[serde(default)]
    pub tempo_changes: Vec<TempoChange>,
    #[serde(default)]
    pub midi_phrases: Vec<MidiPhrase>,
}

impl AudioProject {
    pub fn new(profile: AudioProfile) -> Result<Self> {
        profile.validate()?;
        Ok(Self {
            model_version: MODEL_VERSION,
            id: "project".into(),
            buses: vec![Bus {
                id: "master".into(),
                name: "Master".into(),
                channels: profile.channels,
                gain: MilliDb(0),
                pan_milli: 0,
                effects: EffectChain::default(),
                sends: vec![],
                automations: vec![],
            }],
            master_bus: "master".into(),
            profile,
            metadata: ProjectMetadata::default(),
            samples: BTreeMap::new(),
            synths: BTreeMap::new(),
            stems: vec![],
            groups: vec![],
            markers: vec![],
            ranges: vec![],
            tempo_changes: vec![],
            midi_phrases: vec![],
        })
    }

    pub fn stem(&self, id: &str) -> Result<&Stem> {
        self.stems
            .iter()
            .find(|stem| stem.id == id)
            .ok_or_else(|| Error::new("NotFound", "Audio stem not found"))
    }
    pub fn stem_mut(&mut self, id: &str) -> Result<&mut Stem> {
        self.stems
            .iter_mut()
            .find(|stem| stem.id == id)
            .ok_or_else(|| Error::new("NotFound", "Audio stem not found"))
    }
    pub fn bus(&self, id: &str) -> Result<&Bus> {
        self.buses
            .iter()
            .find(|bus| bus.id == id)
            .ok_or_else(|| Error::new("NotFound", "Audio bus not found"))
    }
    pub fn bus_mut(&mut self, id: &str) -> Result<&mut Bus> {
        self.buses
            .iter_mut()
            .find(|bus| bus.id == id)
            .ok_or_else(|| Error::new("NotFound", "Audio bus not found"))
    }
    pub fn duration(&self) -> u64 {
        self.stems.iter().map(Stem::duration).max().unwrap_or(0)
    }

    pub fn validate(&self) -> Result<()> {
        if self.model_version != MODEL_VERSION {
            return Err(Error::unsupported(
                "Unsupported semantic audio model version",
            ));
        }
        validate_id(&self.id)?;
        self.profile.validate()?;
        self.metadata.validate()?;
        if self.samples.len() > MAX_SAMPLES
            || self.synths.len() > MAX_SYNTHS
            || self.stems.len() > MAX_STEMS
            || self.buses.is_empty()
            || self.buses.len() > MAX_BUSES
            || self.groups.len() > MAX_GROUPS
            || self.markers.len() > MAX_MARKERS
            || self.ranges.len() > MAX_RANGES
            || self.tempo_changes.len() > MAX_TEMPO_CHANGES
            || self.midi_phrases.len() > MAX_MIDI_PHRASES
        {
            return Err(Error::limit("Audio project collection budget exceeded"));
        }
        let mut ids = BTreeSet::new();
        unique(&mut ids, &self.id)?;
        for (key, sample) in &self.samples {
            sample.validate()?;
            if key != &sample.id {
                return Err(Error::invalid("Sample map key differs from semantic ID"));
            }
            unique(&mut ids, &sample.id)?;
        }
        for (key, synth) in &self.synths {
            if key != &synth.id {
                return Err(Error::invalid("Synth map key differs from semantic ID"));
            }
            unique(&mut ids, &synth.id)?;
            synth.validate(&self.samples)?;
            for signal in &synth.signals {
                unique(&mut ids, &signal.id)?;
            }
        }

        let bus_ids: BTreeSet<_> = self.buses.iter().map(|bus| bus.id.as_str()).collect();
        if !bus_ids.contains(self.master_bus.as_str()) || bus_ids.len() != self.buses.len() {
            return Err(Error::invalid(
                "Master bus missing or bus identities are duplicated",
            ));
        }

        let mut effect_count = 0usize;
        let mut automation_points = 0usize;
        let mut clip_count = 0usize;
        for bus in &self.buses {
            bus.validate()?;
            unique(&mut ids, &bus.id)?;
            validate_automation_targets(
                &bus.automations,
                &bus.effects,
                true,
                &bus.id,
                &self.synths,
                &bus_ids,
            )?;
            effect_count += bus.effects.effects.len();
            automation_points += bus
                .automations
                .iter()
                .map(|automation| automation.points.len())
                .sum::<usize>();
            for send in &bus.sends {
                if !bus_ids.contains(send.target_bus.as_str()) {
                    return Err(Error::invalid("Bus send target is missing"));
                }
                if send.enabled && send.target_bus == bus.id && send.delay_frames == 0 {
                    return Err(Error::invalid(
                        "Enabled self-feedback requires an explicit nonzero delay",
                    ));
                }
            }
        }
        validate_bus_acyclic(&self.buses)?;

        let mut clip_ids = BTreeSet::new();
        for stem in &self.stems {
            stem.validate()?;
            unique(&mut ids, &stem.id)?;
            validate_automation_targets(
                &stem.automations,
                &stem.effects,
                false,
                &stem.id,
                &self.synths,
                &bus_ids,
            )?;
            if !bus_ids.contains(stem.output_bus.as_str()) {
                return Err(Error::invalid("Stem output bus is missing"));
            }
            for send in &stem.sends {
                if !bus_ids.contains(send.target_bus.as_str()) {
                    return Err(Error::invalid("Stem send target bus is missing"));
                }
            }
            effect_count += stem.effects.effects.len();
            automation_points += stem
                .automations
                .iter()
                .map(|automation| automation.points.len())
                .sum::<usize>();
            for clip in &stem.clips {
                clip_count += 1;
                if !clip_ids.insert(clip.id.clone()) {
                    return Err(Error::invalid("Duplicate audio clip identity"));
                }
                unique(&mut ids, &clip.id)?;
                match &clip.source {
                    ClipSource::Sample { sample } => {
                        let source = self
                            .samples
                            .get(sample)
                            .ok_or_else(|| Error::invalid("Clip references missing sample"))?;
                        if clip.source_range.end.0 > source.frames {
                            return Err(Error::invalid("Clip exceeds referenced sample duration"));
                        }
                    }
                    ClipSource::Synth {
                        synth,
                        midi_note,
                        velocity,
                    } => {
                        if !self.synths.contains_key(synth) || *midi_note > 127 || velocity.0 > 1000
                        {
                            return Err(Error::invalid("Invalid synth clip source"));
                        }
                    }
                }
            }
        }

        let stem_ids: BTreeSet<_> = self.stems.iter().map(|stem| stem.id.as_str()).collect();
        let mut grouped = BTreeSet::new();
        for group in &self.groups {
            group.validate()?;
            unique(&mut ids, &group.id)?;
            for stem in &group.stems {
                if !stem_ids.contains(stem.as_str()) || !grouped.insert(stem.as_str()) {
                    return Err(Error::invalid(
                        "Stem group references a missing or multiply-grouped stem",
                    ));
                }
            }
        }

        let mut marker_ids = BTreeSet::new();
        for marker in &self.markers {
            marker.validate()?;
            if !marker_ids.insert(marker.id.as_str()) {
                return Err(Error::invalid("Duplicate marker identity"));
            }
        }
        let mut range_ids = BTreeSet::new();
        for range in &self.ranges {
            range.validate()?;
            if !range_ids.insert(range.id.as_str()) {
                return Err(Error::invalid("Duplicate range identity"));
            }
        }

        let mut previous_tempo_frame = 0u64;
        for change in &self.tempo_changes {
            change.validate()?;
            if change.frame.0 <= previous_tempo_frame {
                return Err(Error::invalid(
                    "Tempo/meter changes must be strictly increasing after frame zero",
                ));
            }
            previous_tempo_frame = change.frame.0;
        }

        let mut phrase_ids = BTreeSet::new();
        let mut midi_event_count = 0usize;
        for phrase in &self.midi_phrases {
            phrase.validate(&self.synths)?;
            if !phrase_ids.insert(phrase.id.as_str()) {
                return Err(Error::invalid("Duplicate MIDI phrase identity"));
            }
            midi_event_count = midi_event_count
                .checked_add(phrase.events.len())
                .ok_or_else(|| Error::limit("MIDI event count overflow"))?;
        }

        if clip_count > MAX_CLIPS
            || midi_event_count > MAX_MIDI_EVENTS
            || effect_count > MAX_EFFECTS
            || automation_points > MAX_AUTOMATION_POINTS
        {
            return Err(Error::limit("Audio project resource budget exceeded"));
        }
        Ok(())
    }

    pub fn semantic_digest(&self) -> Result<String> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)
            .map_err(|_| Error::new("BackendFailed", "Could not encode semantic audio model"))?;
        Ok(sha256(&bytes))
    }
}

fn validate_automations(values: &[Automation]) -> Result<()> {
    if values.len() > 4096 {
        return Err(Error::limit("Automation lane count exceeds limit"));
    }
    let mut ids = BTreeSet::new();
    for value in values {
        value.validate()?;
        if !ids.insert(&value.id) {
            return Err(Error::invalid("Duplicate automation identity"));
        }
    }
    Ok(())
}

fn validate_acyclic_signals(signals: &[Signal]) -> Result<()> {
    let by_id: BTreeMap<&str, &Signal> = signals.iter().map(|s| (s.id.as_str(), s)).collect();
    fn visit<'a>(
        id: &'a str,
        by_id: &BTreeMap<&'a str, &'a Signal>,
        temporary: &mut BTreeSet<&'a str>,
        permanent: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if permanent.contains(id) {
            return Ok(());
        }
        if temporary.len() >= 256 {
            return Err(Error::limit("Signal graph depth exceeds model budget"));
        }
        if !temporary.insert(id) {
            return Err(Error::invalid("Signal graph contains a cycle"));
        }
        let signal = by_id
            .get(id)
            .ok_or_else(|| Error::invalid("Signal graph references missing node"))?;
        for input in &signal.inputs {
            visit(input, by_id, temporary, permanent)?;
        }
        temporary.remove(id);
        permanent.insert(id);
        Ok(())
    }
    let mut temporary = BTreeSet::new();
    let mut permanent = BTreeSet::new();
    for signal in signals {
        visit(&signal.id, &by_id, &mut temporary, &mut permanent)?;
    }
    Ok(())
}

fn validate_bus_acyclic(buses: &[Bus]) -> Result<()> {
    let by_id: BTreeMap<&str, &Bus> = buses.iter().map(|bus| (bus.id.as_str(), bus)).collect();
    fn visit<'a>(
        id: &'a str,
        by_id: &BTreeMap<&'a str, &'a Bus>,
        temporary: &mut BTreeSet<&'a str>,
        permanent: &mut BTreeSet<&'a str>,
    ) -> Result<()> {
        if permanent.contains(id) {
            return Ok(());
        }
        if !temporary.insert(id) {
            return Err(Error::invalid("Bus routing graph contains a cycle"));
        }
        let bus = by_id
            .get(id)
            .ok_or_else(|| Error::invalid("Bus routing references missing bus"))?;
        for send in &bus.sends {
            if send.enabled && send.delay_frames == 0 {
                visit(&send.target_bus, by_id, temporary, permanent)?;
            }
        }
        temporary.remove(id);
        permanent.insert(id);
        Ok(())
    }
    let mut temporary = BTreeSet::new();
    let mut permanent = BTreeSet::new();
    for bus in buses {
        visit(&bus.id, &by_id, &mut temporary, &mut permanent)?;
    }
    Ok(())
}

fn unique(ids: &mut BTreeSet<String>, value: &str) -> Result<()> {
    validate_id(value)?;
    if !ids.insert(value.to_owned()) {
        return Err(Error::invalid("Semantic audio identity is reused"));
    }
    Ok(())
}

pub fn validate_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 256
        || value.contains("..")
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(Error::invalid("Invalid semantic audio identity"));
    }
    Ok(())
}

fn validate_relative_path(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 4096
        || value.starts_with('/')
        || value.contains('\0')
        || value.split('/').any(|part| matches!(part, "" | "." | ".."))
    {
        return Err(Error::invalid("Invalid relative audio asset path"));
    }
    Ok(())
}

fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::invalid("Invalid SHA-256 digest"));
    }
    Ok(())
}

fn validate_text(label: &str, value: &str, max: usize) -> Result<()> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(Error::invalid(format!("Invalid {label}")));
    }
    Ok(())
}

fn validate_automation_targets(
    values: &[Automation],
    effects: &EffectChain,
    owner_is_bus: bool,
    owner_id: &str,
    synths: &BTreeMap<String, Synth>,
    bus_ids: &BTreeSet<&str>,
) -> Result<()> {
    for automation in values {
        match &automation.target {
            AutomationTarget::StemGain | AutomationTarget::StemPan if owner_is_bus => {
                return Err(Error::invalid("Stem automation cannot belong to a bus"));
            }
            AutomationTarget::BusGain | AutomationTarget::BusPan if !owner_is_bus => {
                return Err(Error::invalid("Bus automation cannot belong to a stem"));
            }
            AutomationTarget::StemSendGain { target_bus } => {
                if owner_is_bus || !bus_ids.contains(target_bus.as_str()) {
                    return Err(Error::invalid("Invalid stem-send automation target"));
                }
            }
            AutomationTarget::BusSendGain { target_bus } => {
                if !owner_is_bus || target_bus == owner_id || !bus_ids.contains(target_bus.as_str())
                {
                    return Err(Error::invalid("Invalid bus-send automation target"));
                }
            }
            AutomationTarget::EffectParameter { effect, .. } => {
                if !effects.effects.iter().any(|item| item.id == *effect) {
                    return Err(Error::invalid(
                        "Effect automation references an effect outside its owner chain",
                    ));
                }
            }
            AutomationTarget::SynthSignalParameter {
                synth,
                signal,
                parameter,
            } => {
                let synth = synths
                    .get(synth)
                    .ok_or_else(|| Error::invalid("Synth automation references missing synth"))?;
                let signal = synth
                    .signals
                    .iter()
                    .find(|item| item.id == *signal)
                    .ok_or_else(|| Error::invalid("Synth automation references missing signal"))?;
                if !synth_parameter_supported(&signal.node, *parameter) {
                    return Err(Error::invalid(
                        "Synth automation parameter is incompatible with signal node",
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn synth_parameter_supported(node: &SignalNodeKind, parameter: SynthParameter) -> bool {
    match node {
        SignalNodeKind::Oscillator { .. } => matches!(
            parameter,
            SynthParameter::FrequencyHz
                | SynthParameter::EndFrequencyHz
                | SynthParameter::Amplitude
                | SynthParameter::Phase
        ),
        SignalNodeKind::FmOscillator { .. } => matches!(
            parameter,
            SynthParameter::FmCarrierFrequencyHz
                | SynthParameter::FmModulatorFrequencyHz
                | SynthParameter::FmModulationIndex
                | SynthParameter::Amplitude
        ),
        SignalNodeKind::Envelope { .. } => matches!(
            parameter,
            SynthParameter::AttackFrames
                | SynthParameter::DecayFrames
                | SynthParameter::Sustain
                | SynthParameter::ReleaseFrames
        ),
        SignalNodeKind::Filter { .. } => matches!(
            parameter,
            SynthParameter::FilterCutoffHz | SynthParameter::FilterResonance
        ),
        SignalNodeKind::Gain { .. } => parameter == SynthParameter::GainDb,
        SignalNodeKind::SamplePlayer { .. }
        | SignalNodeKind::Input { .. }
        | SignalNodeKind::Constant { .. }
        | SignalNodeKind::Effect { .. }
        | SignalNodeKind::Add
        | SignalNodeKind::Multiply => false,
    }
}

fn validate_automation_value(target: &AutomationTarget, value: i64) -> Result<()> {
    let valid = match target {
        AutomationTarget::StemGain
        | AutomationTarget::BusGain
        | AutomationTarget::StemSendGain { .. }
        | AutomationTarget::BusSendGain { .. } => (-120_000..=24_000).contains(&value),
        AutomationTarget::StemPan | AutomationTarget::BusPan => (-1000..=1000).contains(&value),
        AutomationTarget::EffectParameter { parameter, .. } => match parameter {
            EffectParameter::GainDb
            | EffectParameter::ThresholdDb
            | EffectParameter::DriveDb
            | EffectParameter::CeilingDb => (-120_000..=24_000).contains(&value),
            EffectParameter::FrequencyHz => (1..=192_000_000).contains(&value),
            EffectParameter::Q => (100..=50_000).contains(&value),
            EffectParameter::Ratio => (1000..=100_000).contains(&value),
            EffectParameter::AttackMs | EffectParameter::HoldMs | EffectParameter::ReleaseMs => {
                (0..=120_000).contains(&value)
            }
            EffectParameter::LookaheadMs | EffectParameter::PreDelayMs => {
                (0..=10_000).contains(&value)
            }
            EffectParameter::ChannelLink
            | EffectParameter::Mix
            | EffectParameter::Feedback
            | EffectParameter::Room
            | EffectParameter::Damping
            | EffectParameter::Diffusion => (0..=1000).contains(&value),
        },
        AutomationTarget::SynthSignalParameter { parameter, .. } => match parameter {
            SynthParameter::FrequencyHz
            | SynthParameter::EndFrequencyHz
            | SynthParameter::FilterCutoffHz
            | SynthParameter::FmCarrierFrequencyHz
            | SynthParameter::FmModulatorFrequencyHz => (1..=192_000_000).contains(&value),
            SynthParameter::Amplitude
            | SynthParameter::Sustain
            | SynthParameter::FilterResonance => (0..=1000).contains(&value),
            SynthParameter::Phase => (0..360_000).contains(&value),
            SynthParameter::AttackFrames
            | SynthParameter::DecayFrames
            | SynthParameter::ReleaseFrames => (0..=MAX_SAMPLE_FRAME as i64).contains(&value),
            SynthParameter::GainDb => (-120_000..=24_000).contains(&value),
            SynthParameter::FmModulationIndex => (0..=100_000).contains(&value),
        },
    };
    if !valid {
        return Err(Error::invalid(
            "Automation point value is outside target bounds",
        ));
    }
    Ok(())
}
