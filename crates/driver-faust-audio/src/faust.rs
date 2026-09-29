use semwright_audio_domain::{
    Error, Result,
    hash::sha256,
    model::{
        AudioProject, DistortionAlgorithm, DynamicsDetector, Effect, FilterKind, Sample, Signal,
        SignalNodeKind, Synth, Waveform,
    },
    time::SampleRate,
    units::{MilliDb, MilliHz, Permille},
};
use std::collections::{BTreeMap, BTreeSet};

pub const TRANSLATOR_VERSION: u32 = 2;
pub const INSTRUMENT_TRANSLATOR_VERSION: u32 = 1;
pub const MAX_INSTRUMENT_POLYPHONY: u16 = 64;
pub const VOICE_POLICY: &str = "faust_first_free_then_oldest_release_then_oldest_playing";
const MAX_SOURCE_BYTES: usize = 60_000;
const MAX_INSTRUMENT_RELEASE_SECONDS: u64 = 30;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaustProgram {
    pub translator_version: u32,
    pub source: String,
    pub source_sha256: String,
    pub outputs: u16,
}

pub fn compile_project_synth(
    project: &AudioProject,
    synth_id: &str,
    duration_frames: u64,
    outputs: u16,
) -> Result<FaustProgram> {
    project.validate()?;
    let synth = project
        .synths
        .get(synth_id)
        .ok_or_else(|| Error::new("NotFound", "Faust synth is absent"))?;
    translate(synth, project.profile.sample_rate, duration_frames, outputs)
}

pub fn translate(
    synth: &Synth,
    sample_rate: SampleRate,
    duration_frames: u64,
    outputs: u16,
) -> Result<FaustProgram> {
    sample_rate.validate()?;
    if duration_frames == 0 || duration_frames > semwright_audio_domain::time::MAX_SAMPLE_FRAME {
        return Err(Error::invalid("Faust render duration is invalid"));
    }
    if !(1..=16).contains(&outputs) {
        return Err(Error::invalid("Faust output channel count is invalid"));
    }
    if synth.polyphony != 1 {
        return Err(Error::unsupported(
            "Faust backend v1 accepts deterministic monophonic semantic synths",
        ));
    }
    if synth.signals.iter().any(|signal| {
        matches!(
            signal.node,
            SignalNodeKind::SamplePlayer { .. } | SignalNodeKind::Input { .. }
        )
    }) {
        return Err(Error::unsupported(
            "Faust backend v1 does not admit external/sample inputs",
        ));
    }

    // Validate graph shape without pretending sample-backed signals are available.
    synth.validate(&BTreeMap::new())?;

    let by_id: BTreeMap<&str, &Signal> = synth
        .signals
        .iter()
        .map(|signal| (signal.id.as_str(), signal))
        .collect();
    let mut visiting = BTreeSet::new();
    let mut memo = BTreeMap::new();
    let expression = expression(
        &synth.output,
        &by_id,
        &mut visiting,
        &mut memo,
        sample_rate,
        duration_frames,
        None,
    )?;
    let process = if outputs == 1 {
        expression
    } else {
        format!("({expression}) <: par(i, {outputs}, _)")
    };
    let definitions = memo
        .iter()
        .map(|(id, expr)| {
            let index = by_id
                .keys()
                .position(|key| *key == id.as_str())
                .expect("validated signal");
            format!("n{index} = {expr};\n")
        })
        .collect::<String>();
    let source = format!(
        "import(\"stdfaust.lib\");\n\
         declare name \"Semwright deterministic semantic synth\";\n\
         declare semwright_translator_version \"{TRANSLATOR_VERSION}\";\n\
         t = (+(1) ~ _) - 1;\n\
         seeded_noise(seed) = random / 2147483647.0 with {{ random = +(seed) ~ *(1103515245); }};\n\
         {definitions}\
         process = {process};\n"
    );
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Error::limit("Generated Faust source exceeds budget"));
    }
    let source_sha256 = sha256(source.as_bytes());
    Ok(FaustProgram {
        translator_version: TRANSLATOR_VERSION,
        source,
        source_sha256,
        outputs,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SampleProgram {
    pub program: FaustProgram,
    pub sample_id: String,
    pub looped: bool,
}

pub fn translate_sample_player(
    synth: &Synth,
    sample: &Sample,
    sample_rate: SampleRate,
    duration_frames: u64,
    outputs: u16,
) -> Result<SampleProgram> {
    sample_rate.validate()?;
    sample.validate()?;
    if duration_frames == 0 || duration_frames > semwright_audio_domain::time::MAX_SAMPLE_FRAME {
        return Err(Error::invalid("Faust sample render duration is invalid"));
    }
    if !(1..=16).contains(&outputs) {
        return Err(Error::invalid(
            "Faust sample output channel count is invalid",
        ));
    }
    if synth.polyphony != 1 {
        return Err(Error::unsupported(
            "Sample-backed Faust graphs are monophonic; MIDI/polyphony uses instrument.render",
        ));
    }
    if synth
        .signals
        .iter()
        .any(|signal| matches!(signal.node, SignalNodeKind::Input { .. }))
    {
        return Err(Error::unsupported(
            "Sample-backed Faust graphs do not accept a second external input",
        ));
    }
    let sample_nodes = synth
        .signals
        .iter()
        .filter_map(|signal| match &signal.node {
            SignalNodeKind::SamplePlayer { sample, looped } => Some((sample.as_str(), *looped)),
            _ => None,
        })
        .collect::<Vec<_>>();
    if sample_nodes.len() != 1 || sample_nodes[0].0 != sample.id {
        return Err(Error::unsupported(
            "Sample render requires exactly one SamplePlayer referencing the supplied Sample",
        ));
    }
    let samples = BTreeMap::from([(sample.id.clone(), sample.clone())]);
    synth.validate(&samples)?;

    let by_id: BTreeMap<&str, &Signal> = synth
        .signals
        .iter()
        .map(|signal| (signal.id.as_str(), signal))
        .collect();
    let mut visiting = BTreeSet::new();
    let mut memo = BTreeMap::new();
    let expression = expression(
        &synth.output,
        &by_id,
        &mut visiting,
        &mut memo,
        sample_rate,
        duration_frames,
        Some(sample.id.as_str()),
    )?;
    let process = if outputs == 1 {
        expression
    } else {
        format!("({expression}) <: par(i, {outputs}, _)")
    };
    let definitions = memo
        .iter()
        .map(|(id, expr)| {
            let index = by_id
                .keys()
                .position(|key| *key == id.as_str())
                .expect("validated signal");
            format!("n{index} = {expr};\n")
        })
        .collect::<String>();
    let source = format!(
        "import(\"stdfaust.lib\");\n\
         declare name \"Semwright deterministic sample-backed synth\";\n\
         declare semwright_translator_version \"{TRANSLATOR_VERSION}\";\n\
         t = (+(1) ~ _) - 1;\n\
         seeded_noise(seed) = random / 2147483647.0 with {{ random = +(seed) ~ *(1103515245); }};\n\
         {definitions}\
         process = {process};\n"
    );
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Error::limit(
            "Generated sample-backed Faust source exceeds budget",
        ));
    }
    Ok(SampleProgram {
        program: FaustProgram {
            translator_version: TRANSLATOR_VERSION,
            source_sha256: sha256(source.as_bytes()),
            source,
            outputs,
        },
        sample_id: sample.id.clone(),
        looped: sample_nodes[0].1,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstrumentProgram {
    pub program: FaustProgram,
    pub tail_frames: u64,
    pub reference_midi_note: u8,
}

pub fn translate_instrument(
    synth: &Synth,
    sample_rate: SampleRate,
    outputs: u16,
    reference_midi_note: u8,
) -> Result<InstrumentProgram> {
    sample_rate.validate()?;
    if !(1..=16).contains(&outputs) {
        return Err(Error::invalid(
            "Faust instrument output channel count is invalid",
        ));
    }
    if synth.polyphony == 0 || synth.polyphony > MAX_INSTRUMENT_POLYPHONY {
        return Err(Error::unsupported(
            "Faust instrument runtime supports 1..=64 voices",
        ));
    }
    if synth.signals.iter().any(|signal| {
        matches!(
            signal.node,
            SignalNodeKind::SamplePlayer { .. } | SignalNodeKind::Input { .. }
        )
    }) {
        return Err(Error::unsupported(
            "Faust instrument runtime does not combine external/sample inputs with polyphonic voices",
        ));
    }
    if !synth
        .signals
        .iter()
        .any(|signal| matches!(signal.node, SignalNodeKind::Envelope { .. }))
    {
        return Err(Error::invalid(
            "Polyphonic Faust instruments require an explicit semantic envelope",
        ));
    }
    synth.validate(&BTreeMap::new())?;
    let reference_hz = midi_frequency(reference_midi_note);
    let by_id: BTreeMap<&str, &Signal> = synth
        .signals
        .iter()
        .map(|signal| (signal.id.as_str(), signal))
        .collect();
    let mut visiting = BTreeSet::new();
    let mut memo = BTreeMap::new();
    let expression = instrument_expression(
        &synth.output,
        &by_id,
        &mut visiting,
        &mut memo,
        sample_rate,
        reference_hz,
    )?;
    let voice = format!("({expression}) * voice_gain");
    let process = if outputs == 1 {
        voice
    } else {
        format!("({voice}) <: par(i, {outputs}, _)")
    };
    let definitions = memo
        .iter()
        .map(|(id, expr)| {
            let index = by_id
                .keys()
                .position(|key| *key == id.as_str())
                .expect("validated signal");
            format!("n{index} = {expr};\n")
        })
        .collect::<String>();
    let source = format!(
        "import(\"stdfaust.lib\");\n\
         declare name \"Semwright deterministic polyphonic instrument\";\n\
         declare semwright_instrument_translator_version \"{INSTRUMENT_TRANSLATOR_VERSION}\";\n\
         voice_freq = hslider(\"freq\", {}, 8.0, 20000.0, 0.001);\n\
         voice_gate = button(\"gate\");\n\
         voice_gain = hslider(\"gain\", 1.0, 0.0, 1.0, 0.001);\n\
         seeded_noise(seed) = random / 2147483647.0 with {{ random = +(seed) ~ *(1103515245); }};\n\
         {definitions}\
         process = {process};\n",
        decimal(reference_hz)
    );
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Error::limit(
            "Generated polyphonic Faust source exceeds budget",
        ));
    }
    let tail_frames = instrument_tail_frames(synth, sample_rate)?;
    Ok(InstrumentProgram {
        program: FaustProgram {
            translator_version: TRANSLATOR_VERSION,
            source_sha256: sha256(source.as_bytes()),
            source,
            outputs,
        },
        tail_frames,
        reference_midi_note,
    })
}

fn instrument_expression(
    id: &str,
    by_id: &BTreeMap<&str, &Signal>,
    visiting: &mut BTreeSet<String>,
    memo: &mut BTreeMap<String, String>,
    sample_rate: SampleRate,
    reference_hz: f64,
) -> Result<String> {
    let index = by_id
        .keys()
        .position(|key| *key == id)
        .ok_or_else(|| Error::invalid("Missing instrument signal"))?;
    let symbol = format!("n{index}");
    if memo.contains_key(id) {
        return Ok(symbol);
    }
    if visiting.len() >= 128 {
        return Err(Error::limit(
            "Instrument graph depth exceeds translator limit",
        ));
    }
    if !visiting.insert(id.to_owned()) {
        return Err(Error::invalid(
            "Faust instrument translation encountered a signal cycle",
        ));
    }
    let signal = by_id
        .get(id)
        .ok_or_else(|| Error::invalid("Faust instrument signal is missing"))?;
    let inputs = signal
        .inputs
        .iter()
        .map(|input| instrument_expression(input, by_id, visiting, memo, sample_rate, reference_hz))
        .collect::<Result<Vec<_>>>()?;

    let value = match &signal.node {
        SignalNodeKind::Oscillator { oscillator } => {
            if oscillator.end_frequency.is_some() {
                return Err(Error::unsupported(
                    "Polyphonic instrument oscillator sweeps require explicit note-relative automation",
                ));
            }
            let amplitude = fraction(oscillator.amplitude);
            let ratio = decimal(oscillator.frequency.as_hz_f64() / reference_hz);
            let frequency = format!("(voice_freq * {ratio})");
            let osc = match oscillator.waveform {
                Waveform::Sine if oscillator.phase_millidegrees == 0 => {
                    format!("os.osc({frequency})")
                }
                Waveform::Sine => {
                    let radians =
                        f64::from(oscillator.phase_millidegrees) * std::f64::consts::PI / 180_000.0;
                    format!("os.oscp({frequency}, {})", decimal(radians))
                }
                Waveform::Saw if oscillator.phase_millidegrees == 0 => {
                    format!("os.polyblep_saw({frequency})")
                }
                Waveform::Square if oscillator.phase_millidegrees == 0 => {
                    format!("os.square({frequency})")
                }
                Waveform::Triangle if oscillator.phase_millidegrees == 0 => {
                    format!("os.triangle({frequency})")
                }
                Waveform::Saw | Waveform::Square | Waveform::Triangle => {
                    return Err(Error::unsupported(
                        "Polyphonic Faust backend does not discard non-sine oscillator phase",
                    ));
                }
                Waveform::Noise => {
                    let seed = oscillator
                        .seed
                        .ok_or_else(|| Error::invalid("Noise requires seed"))?
                        % 2_147_483_647;
                    format!("seeded_noise({})", seed.max(1))
                }
            };
            format!("({osc}) * {amplitude}")
        }
        SignalNodeKind::FmOscillator {
            carrier_frequency,
            modulator_frequency,
            modulation_index_milli,
            amplitude,
        } => {
            let carrier = decimal(carrier_frequency.as_hz_f64() / reference_hz);
            let modulator = decimal(modulator_frequency.as_hz_f64() / reference_hz);
            let index = decimal(f64::from(*modulation_index_milli) / 1000.0);
            let carrier = format!("voice_freq * {carrier}");
            let modulator = format!("voice_freq * {modulator}");
            let deviation = format!("({modulator})*({index})");
            format!(
                "(os.osc(({carrier}) + ((os.osc({modulator})) * ({deviation})))) * {}",
                fraction(*amplitude)
            )
        }
        SignalNodeKind::Constant { value_milli } => decimal(f64::from(*value_milli) / 1000.0),
        SignalNodeKind::Envelope { envelope } => {
            let input = one(&inputs)?;
            let sr = f64::from(sample_rate.0);
            format!(
                "({input}) * en.adsr({}, {}, {}, {}, voice_gate)",
                decimal(envelope.attack_frames as f64 / sr),
                decimal(envelope.decay_frames as f64 / sr),
                fraction(envelope.sustain),
                decimal(envelope.release_frames as f64 / sr),
            )
        }
        SignalNodeKind::Filter { filter } => {
            filter.validate()?;
            filter_expr(one(&inputs)?, filter.kind, filter.cutoff, filter.resonance)?
        }
        SignalNodeKind::Effect { effect } => effect_expr(one(&inputs)?, effect)?,
        SignalNodeKind::Gain { gain } => {
            format!("({}) * {}", one(&inputs)?, db_gain(*gain)?)
        }
        SignalNodeKind::Add => {
            if inputs.len() < 2 {
                return Err(Error::invalid("Add signal requires at least two inputs"));
            }
            format!("({})", inputs.join(" + "))
        }
        SignalNodeKind::Multiply => {
            if inputs.len() < 2 {
                return Err(Error::invalid(
                    "Multiply signal requires at least two inputs",
                ));
            }
            format!("({})", inputs.join(" * "))
        }
        SignalNodeKind::SamplePlayer { .. } | SignalNodeKind::Input { .. } => {
            return Err(Error::unsupported(
                "Polyphonic Faust instrument does not accept external/sample signal inputs",
            ));
        }
    };
    visiting.remove(id);
    let bytes = memo.values().map(String::len).sum::<usize>();
    if bytes.saturating_add(value.len()) > MAX_SOURCE_BYTES - 4096 {
        return Err(Error::limit(
            "Generated polyphonic graph exceeds sealed-tool budget",
        ));
    }
    memo.insert(id.to_owned(), value);
    Ok(symbol)
}

fn midi_frequency(note: u8) -> f64 {
    440.0 * 2_f64.powf((f64::from(note) - 69.0) / 12.0)
}

fn instrument_tail_frames(synth: &Synth, sample_rate: SampleRate) -> Result<u64> {
    let mut envelope_release = 0u64;
    let mut delay_tail = 0u64;
    for signal in &synth.signals {
        match &signal.node {
            SignalNodeKind::Envelope { envelope } => {
                envelope_release = envelope_release.max(envelope.release_frames);
            }
            SignalNodeKind::Effect {
                effect: Effect::Delay {
                    delay_ms, feedback, ..
                },
            } => {
                let delay_frames = u64::from(*delay_ms)
                    .checked_mul(u64::from(sample_rate.0))
                    .ok_or_else(|| Error::limit("Instrument delay-tail duration overflow"))?
                    / 1000;
                let repeats = if feedback.0 == 0 {
                    1.0
                } else {
                    let value = f64::from(feedback.0) / 1000.0;
                    (0.0005_f64.ln() / value.ln()).ceil().max(1.0)
                };
                if !repeats.is_finite() || repeats > 100_000.0 {
                    return Err(Error::limit(
                        "Instrument feedback tail exceeds deterministic voice budget",
                    ));
                }
                delay_tail = delay_tail.max(
                    delay_frames
                        .checked_mul(repeats as u64)
                        .ok_or_else(|| Error::limit("Instrument delay-tail frame overflow"))?,
                );
            }
            _ => {}
        }
    }
    let release = envelope_release
        .checked_add(delay_tail)
        .ok_or_else(|| Error::limit("Instrument release duration overflow"))?
        .max(1);
    let limit = u64::from(sample_rate.0) * MAX_INSTRUMENT_RELEASE_SECONDS;
    if release > limit {
        return Err(Error::limit(
            "Instrument release/tail exceeds 30-second runtime budget",
        ));
    }
    Ok(release)
}

fn expression(
    id: &str,
    by_id: &BTreeMap<&str, &Signal>,
    visiting: &mut BTreeSet<String>,
    memo: &mut BTreeMap<String, String>,
    sample_rate: SampleRate,
    duration_frames: u64,
    sample_source: Option<&str>,
) -> Result<String> {
    let index = by_id
        .keys()
        .position(|key| *key == id)
        .ok_or_else(|| Error::invalid("Missing signal"))?;
    let symbol = format!("n{index}");
    if memo.contains_key(id) {
        return Ok(symbol);
    }
    if visiting.len() >= 128 {
        return Err(Error::limit("Signal graph depth exceeds translator limit"));
    }
    if !visiting.insert(id.to_owned()) {
        return Err(Error::invalid(
            "Faust translation encountered a signal cycle",
        ));
    }
    let signal = by_id
        .get(id)
        .ok_or_else(|| Error::invalid("Faust translation signal is missing"))?;
    let inputs = signal
        .inputs
        .iter()
        .map(|input| {
            expression(
                input,
                by_id,
                visiting,
                memo,
                sample_rate,
                duration_frames,
                sample_source,
            )
        })
        .collect::<Result<Vec<_>>>()?;

    let value = match &signal.node {
        SignalNodeKind::Oscillator { oscillator } => {
            let amplitude = fraction(oscillator.amplitude);
            let frequency = sweep(
                oscillator.frequency,
                oscillator.end_frequency,
                duration_frames,
            );
            let osc = match oscillator.waveform {
                Waveform::Sine if oscillator.phase_millidegrees == 0 => {
                    format!("os.osc({frequency})")
                }
                Waveform::Sine => {
                    let radians =
                        f64::from(oscillator.phase_millidegrees) * std::f64::consts::PI / 180_000.0;
                    format!("os.oscp({frequency}, {})", decimal(radians))
                }
                Waveform::Saw if oscillator.phase_millidegrees == 0 => {
                    format!("os.polyblep_saw({frequency})")
                }
                Waveform::Square if oscillator.phase_millidegrees == 0 => {
                    format!("os.square({frequency})")
                }
                Waveform::Triangle if oscillator.phase_millidegrees == 0 => {
                    format!("os.triangle({frequency})")
                }
                Waveform::Saw | Waveform::Square | Waveform::Triangle => {
                    return Err(Error::unsupported(
                        "Faust backend does not silently discard non-sine oscillator phase",
                    ));
                }
                Waveform::Noise => {
                    let seed = oscillator
                        .seed
                        .ok_or_else(|| Error::invalid("Noise requires seed"))?
                        % 2_147_483_647;
                    format!("seeded_noise({})", seed.max(1))
                }
            };
            format!("({osc}) * {amplitude}")
        }
        SignalNodeKind::FmOscillator {
            carrier_frequency,
            modulator_frequency,
            modulation_index_milli,
            amplitude,
        } => {
            let carrier = decimal(carrier_frequency.as_hz_f64());
            let modulator = decimal(modulator_frequency.as_hz_f64());
            let index = decimal(f64::from(*modulation_index_milli) / 1000.0);
            let deviation = format!("({modulator})*({index})");
            format!(
                "(os.osc(({carrier}) + ((os.osc({modulator})) * ({deviation})))) * {}",
                fraction(*amplitude)
            )
        }
        SignalNodeKind::Constant { value_milli } => decimal(f64::from(*value_milli) / 1000.0),
        SignalNodeKind::Envelope { envelope } => {
            let input = one(&inputs)?;
            let total = envelope
                .attack_frames
                .checked_add(envelope.decay_frames)
                .and_then(|v| v.checked_add(envelope.release_frames))
                .ok_or_else(|| Error::limit("Envelope duration overflow"))?;
            if total > duration_frames {
                return Err(Error::invalid(
                    "Envelope attack/decay/release exceeds render duration",
                ));
            }
            let gate_end = duration_frames.saturating_sub(envelope.release_frames);
            let sr = f64::from(sample_rate.0);
            let attack = decimal(envelope.attack_frames as f64 / sr);
            let decay = decimal(envelope.decay_frames as f64 / sr);
            let release = decimal(envelope.release_frames as f64 / sr);
            let sustain = fraction(envelope.sustain);
            format!(
                "({input}) * en.adsr({attack}, {decay}, {sustain}, {release}, int(t < {gate_end}))"
            )
        }
        SignalNodeKind::Filter { filter } => {
            filter.validate()?;
            let input = one(&inputs)?;
            filter_expr(input, filter.kind, filter.cutoff, filter.resonance)?
        }
        SignalNodeKind::Effect { effect } => {
            let input = one(&inputs)?;
            effect_expr(input, effect)?
        }
        SignalNodeKind::Gain { gain } => {
            let input = one(&inputs)?;
            format!("({input}) * {}", db_gain(*gain)?)
        }
        SignalNodeKind::Add => {
            if inputs.len() < 2 {
                return Err(Error::invalid("Add signal requires at least two inputs"));
            }
            format!("({})", inputs.join(" + "))
        }
        SignalNodeKind::Multiply => {
            if inputs.len() < 2 {
                return Err(Error::invalid(
                    "Multiply signal requires at least two inputs",
                ));
            }
            format!("({})", inputs.join(" * "))
        }
        SignalNodeKind::SamplePlayer { sample, .. } => {
            if sample_source != Some(sample.as_str()) {
                return Err(Error::unsupported(
                    "Faust backend does not accept unbound sample signal inputs",
                ));
            }
            "_".into()
        }
        SignalNodeKind::Input { .. } => {
            return Err(Error::unsupported(
                "Faust backend does not accept arbitrary external signal inputs",
            ));
        }
    };
    visiting.remove(id);
    let bytes = memo.values().map(String::len).sum::<usize>();
    if bytes.saturating_add(value.len()) > MAX_SOURCE_BYTES - 4096 {
        return Err(Error::limit("Generated graph exceeds sealed-tool budget"));
    }
    memo.insert(id.to_owned(), value);
    Ok(symbol)
}

fn filter_expr(
    input: &str,
    kind: FilterKind,
    cutoff: MilliHz,
    resonance: Permille,
) -> Result<String> {
    let fc = decimal(cutoff.as_hz_f64());
    let q = decimal(0.5 + 19.5 * f64::from(resonance.0) / 1000.0);
    let value = match kind {
        FilterKind::LowPass => format!("({input}) : fi.resonlp({fc}, {q}, 1.0)"),
        FilterKind::HighPass => format!("({input}) : fi.resonhp({fc}, {q}, 1.0)"),
        FilterKind::BandPass => format!("({input}) : fi.resonbp({fc}, {q}, 1.0)"),
        FilterKind::Notch => {
            let width = decimal(
                (cutoff.as_hz_f64() / (0.5 + 19.5 * f64::from(resonance.0) / 1000.0)).max(1.0),
            );
            format!("({input}) : fi.notchw({width}, {fc})")
        }
    };
    Ok(value)
}

fn effect_expr(input: &str, effect: &Effect) -> Result<String> {
    effect.validate()?;
    match effect {
        Effect::Gain { gain } => Ok(format!("({input}) * {}", db_gain(*gain)?)),
        Effect::Filter { filter } => {
            filter_expr(input, filter.kind, filter.cutoff, filter.resonance)
        }
        Effect::Distortion {
            algorithm,
            drive,
            mix,
        } => {
            if *algorithm != DistortionAlgorithm::Tanh {
                return Err(Error::unsupported(
                    "Faust distortion mapping supports only the declared tanh algorithm",
                ));
            }
            let wet = fraction(*mix);
            let dry = decimal(1.0 - f64::from(mix.0) / 1000.0);
            let drive = db_gain(*drive)?;
            Ok(format!(
                "(({input})*{dry}) + (ma.tanh(({input})*{drive})*{wet})"
            ))
        }
        Effect::Eq { bands } => {
            let mut expression = input.to_owned();
            for band in bands {
                expression = format!(
                    "({expression}) : fi.peak_eq_cq({}, {}, {})",
                    db_value(band.gain)?,
                    decimal(band.frequency.as_hz_f64()),
                    decimal(f64::from(band.q_milli) / 1000.0),
                );
            }
            Ok(expression)
        }
        Effect::Compressor {
            threshold,
            ratio_milli,
            attack_ms,
            release_ms,
            knee,
            makeup,
            detector,
            channel_link,
        } => {
            if knee.0 != 0 || *detector != DynamicsDetector::Peak || channel_link.0 != 1000 {
                return Err(Error::unsupported(
                    "Faust compressor mapping requires zero knee, peak detection and fully linked channels",
                ));
            }
            Ok(format!(
                "(({input}) : co.compressor_mono({}, {}, {}, {})) * {}",
                decimal(f64::from(*ratio_milli) / 1000.0),
                db_value(*threshold)?,
                decimal(f64::from(*attack_ms) / 1000.0),
                decimal(f64::from(*release_ms) / 1000.0),
                db_gain(*makeup)?,
            ))
        }
        Effect::Delay {
            delay_ms,
            feedback,
            mix,
        } => {
            let seconds = decimal(f64::from(*delay_ms) / 1000.0);
            let feedback = fraction(*feedback);
            let wet = fraction(*mix);
            let dry = decimal(1.0 - f64::from(mix.0) / 1000.0);
            Ok(format!(
                "(({input})*{dry}) + ((({input}) : ef.echo({seconds}, {seconds}, {feedback}))*{wet})"
            ))
        }
        // The semantic limiter omits attack/lookahead/hold and reverb does not
        // yet name an algorithm/topology. Choosing hidden values here would
        // make two conforming backends render meaningfully different intent.
        Effect::Limiter { .. }
        | Effect::Reverb { .. }
        | Effect::GateExpander { .. }
        | Effect::ChannelMap { .. } => Err(Error::unsupported(
            "Faust backend has no fidelity-certified mapping for this semantic effect",
        )),
    }
}

fn one(inputs: &[String]) -> Result<&str> {
    if inputs.len() != 1 {
        return Err(Error::invalid(
            "Unary Faust node requires exactly one input",
        ));
    }
    Ok(&inputs[0])
}
fn fraction(value: Permille) -> String {
    decimal(f64::from(value.0) / 1000.0)
}
fn db_value(value: MilliDb) -> Result<String> {
    MilliDb::new(value.0)?;
    Ok(decimal(f64::from(value.0) / 1000.0))
}
fn db_gain(value: MilliDb) -> Result<String> {
    MilliDb::new(value.0)?;
    Ok(decimal(10_f64.powf(f64::from(value.0) / 20_000.0)))
}
fn sweep(start: MilliHz, end: Option<MilliHz>, duration_frames: u64) -> String {
    let start = start.as_hz_f64();
    match end {
        None => decimal(start),
        Some(end) => {
            let delta = end.as_hz_f64() - start;
            format!(
                "({} + ({}) * min(1.0, float(t)/{}))",
                decimal(start),
                decimal(delta),
                decimal(duration_frames.saturating_sub(1).max(1) as f64)
            )
        }
    }
}
fn decimal(value: f64) -> String {
    if value == 0.0 {
        return "0.0".into();
    }
    let mut text = format!("{value:.9}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.push('0');
    }
    text
}
