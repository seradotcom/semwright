use semwright_audio_domain::{
    Error, Result,
    hash::sha256,
    model::{AudioProject, Effect, FilterKind, Signal, SignalNodeKind, Synth, Waveform},
    time::SampleRate,
    units::{MilliDb, MilliHz, Permille},
};
use std::collections::{BTreeMap, BTreeSet};

pub const TRANSLATOR_VERSION: u32 = 1;
const MAX_SOURCE_BYTES: usize = 1_048_576;

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
    )?;
    let process = if outputs == 1 {
        expression
    } else {
        format!("({expression}) <: par(i, {outputs}, _)")
    };
    let source = format!(
        "import(\"stdfaust.lib\");\n\
         declare name \"Semwright deterministic semantic synth\";\n\
         declare semwright_translator_version \"{TRANSLATOR_VERSION}\";\n\
         t = +(1) ~ _;\n\
         seeded_noise(seed) = random / 2147483647.0 with {{ random = +(seed) ~ *(1103515245); }};\n\
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

fn expression(
    id: &str,
    by_id: &BTreeMap<&str, &Signal>,
    visiting: &mut BTreeSet<String>,
    memo: &mut BTreeMap<String, String>,
    sample_rate: SampleRate,
    duration_frames: u64,
) -> Result<String> {
    if let Some(value) = memo.get(id) {
        return Ok(value.clone());
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
        .map(|input| expression(input, by_id, visiting, memo, sample_rate, duration_frames))
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
        SignalNodeKind::SamplePlayer { .. } | SignalNodeKind::Input { .. } => {
            return Err(Error::unsupported(
                "Faust backend does not accept external/sample signal inputs",
            ));
        }
    };
    visiting.remove(id);
    memo.insert(id.to_owned(), value.clone());
    Ok(value)
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
        Effect::Distortion { drive, mix } => {
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
        } => {
            if knee.0 != 0 {
                return Err(Error::unsupported(
                    "Faust compressor mapping cannot preserve non-zero semantic knee",
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
        Effect::Limiter { .. } | Effect::Reverb { .. } => Err(Error::unsupported(
            "Faust backend requires a richer semantic contract for this effect",
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
