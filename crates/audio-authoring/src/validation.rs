use crate::{PlannedAudio, address, domain};
use schemars::JsonSchema;
use semwright_audio_domain::{
    edit::Edit, model::AudioProject, signal_analysis::SignalStatistics, units::MilliDb,
};
use semwright_semantic_composition::*;
use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LoudnessMeasurement {
    pub method: String,
    pub version: String,
    pub integrated_lufs_milli: Option<i32>,
    pub momentary_lufs_milli: Option<i32>,
    pub short_term_lufs_milli: Option<i32>,
    pub loudness_range_milli: Option<u32>,
    pub true_peak_millidbtp: Option<i32>,
    pub unknown_reason: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MeasuredAudio {
    pub artifact: Digest,
    pub source_model: Digest,
    pub base: BaseStateSet,
    pub statistics: SignalStatistics,
    pub loudness: Option<LoudnessMeasurement>,
    pub decoder: String,
    pub decoder_version: u32,
    pub exhaustive: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioValidation {
    pub report: ValidationReport,
    pub findings: Vec<Finding<bool, Edit>>,
}
fn check(rule: &str, verdict: Verdict, reason: &str, evidence: &ObservationRef) -> RuleResult {
    RuleResult {
        rule: rule.into(),
        version: 1,
        verdict,
        evidence_class: EvidenceClass::Deterministic,
        evidence: vec![evidence.clone()],
        reason: Some(reason.into()),
    }
}
fn verdict(value: bool) -> Verdict {
    if value { Verdict::Pass } else { Verdict::Fail }
}
/// The caller must obtain this receipt from a decoder, never accept one as a client assertion.
pub fn validate(
    planned: &PlannedAudio,
    project: &AudioProject,
    measured: &MeasuredAudio,
) -> Result<AudioValidation> {
    domain(project.validate())?;
    measured.base.validate()?;
    let profile = &planned.plan.body.intent.delivery;
    profile.validate()?;
    let actual_model = Digest::parse(domain(project.semantic_digest())?)?;
    ensure(
        measured.source_model == actual_model,
        "measurement belongs to another model revision",
    )?;
    ensure(
        measured.base.0.len() == 1
            && measured.base.0[0].document_id == project.id
            && measured.base.0[0].revision == Revision::Fingerprint(actual_model.clone()),
        "measurement base does not bind the observed project",
    )?;
    ensure(
        measured.decoder_version > 0 && !measured.decoder.is_empty(),
        "decoder provenance is required",
    )?;
    let location = address(&measured.base, &project.master_bus, "decoded_master")?;
    let observation = ObservationRef {
        id: format!("pcm-{}", &measured.artifact.as_str()[..16]),
        base: measured.base.clone(),
        source: EvidenceSource::DecodedMedia,
        method: measured.decoder.clone(),
        method_version: measured.decoder_version,
        scope: vec![location.clone()],
        artifact: Some(measured.artifact.clone()),
        exhaustive: measured.exhaustive,
    };
    let pcm = &measured.statistics;
    let mut checks = vec![
        check(
            "audio.model",
            verdict(actual_model == planned.resulting_model_digest),
            "observed model compared with the replayed typed plan",
            &observation,
        ),
        check(
            "audio.frames",
            verdict(
                pcm.frames == project.duration()
                    && pcm.sample_rate == project.profile.sample_rate
                    && pcm.channels.len() == usize::from(project.profile.channels),
            ),
            "decoded frames, rate and channel count compared with master extent",
            &observation,
        ),
        check(
            "audio.finite",
            verdict(pcm.nonfinite_samples == 0),
            "decoded nonfinite samples must be zero",
            &observation,
        ),
        check(
            "audio.silence",
            verdict(profile.allow_silence || !pcm.entirely_silent),
            "configured silence policy, not a judgement of musical intent",
            &observation,
        ),
    ];
    let peak = pcm
        .peak_millidbfs
        .map_or(pcm.entirely_silent && profile.allow_silence, |v| {
            v <= profile.peak_ceiling_millidbfs
        });
    checks.push(check(
        "audio.sample_peak",
        verdict(peak && pcm.out_of_range_samples == 0),
        "decoded sample-peak ceiling; not true peak",
        &observation,
    ));
    let loudness = match profile.integrated_lufs_milli {
        None => check(
            "audio.loudness",
            Verdict::Pass,
            "no loudness constraint requested by this delivery profile",
            &observation,
        ),
        Some(target) => match measured
            .loudness
            .as_ref()
            .and_then(|m| m.integrated_lufs_milli)
        {
            Some(actual) => check(
                "audio.loudness",
                verdict(
                    (i64::from(actual) - i64::from(target)).unsigned_abs()
                        <= u64::from(profile.loudness_tolerance_milli),
                ),
                "measured integrated loudness compared with configured target/tolerance",
                &observation,
            ),
            None => check(
                "audio.loudness",
                Verdict::Unknown,
                "required integrated loudness is undefined or was not measured",
                &observation,
            ),
        },
    };
    checks.push(loudness);
    checks.push(match profile.true_peak_ceiling_millidbtp {
        None => check(
            "audio.true_peak",
            Verdict::Pass,
            "no true-peak constraint requested by this delivery profile",
            &observation,
        ),
        Some(ceiling) => match measured
            .loudness
            .as_ref()
            .and_then(|m| m.true_peak_millidbtp)
        {
            Some(actual) => check(
                "audio.true_peak",
                verdict(actual <= ceiling),
                "native true-peak method and version are recorded in the measurement",
                &observation,
            ),
            None => check(
                "audio.true_peak",
                Verdict::Unknown,
                "required true peak is undefined or was not measured",
                &observation,
            ),
        },
    });
    let findings = checks
        .iter()
        .filter(|c| c.verdict != Verdict::Pass)
        .map(|c| Finding {
            id: format!("{}-finding", c.rule),
            rule: c.rule.clone(),
            rule_version: c.version,
            subjects: vec![location.clone()],
            expected: true,
            actual: if c.verdict == Verdict::Unknown {
                None
            } else {
                Some(false)
            },
            severity: Severity::Error,
            evidence_class: c.evidence_class,
            observation: observation.clone(),
            uncertainty: c.reason.clone(),
            repairs: vec![],
        })
        .collect();
    Ok(AudioValidation {
        report: ValidationReport {
            plan_digest: planned.plan.digest.clone(),
            base: measured.base.clone(),
            required_rules: planned.plan.body.required_rules.clone(),
            checks,
        },
        findings,
    })
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GainRepair {
    pub master_bus: String,
    pub previous_gain: MilliDb,
    pub next_gain: MilliDb,
    pub delta_millidb: i32,
    pub artifact: Digest,
}
pub fn gain_repair(
    planned: &PlannedAudio,
    project: &AudioProject,
    measured: &MeasuredAudio,
) -> Result<Option<GainRepair>> {
    let validation = validate(planned, project, measured)?;
    if validation.report.verdict()? == Verdict::Pass {
        return Ok(None);
    }
    ensure(
        measured.statistics.nonfinite_samples == 0
            && measured.statistics.frames == project.duration(),
        "gain cannot repair corrupt media or wrong duration",
    )?;
    ensure(
        !measured.statistics.entirely_silent,
        "gain cannot repair undefined silent loudness",
    )?;
    let profile = &planned.plan.body.intent.delivery;
    let previous = domain(project.bus(&project.master_bus))?.gain;
    let mut low = i64::from(profile.minimum_master_gain.0) - i64::from(previous.0);
    let mut high = i64::from(profile.maximum_master_gain.0) - i64::from(previous.0);
    let peak = measured
        .statistics
        .peak_millidbfs
        .ok_or_else(|| ContractError::Unknown("sample peak unavailable".into()))?;
    high = high.min(i64::from(profile.peak_ceiling_millidbfs) - i64::from(peak));
    if let Some(ceiling) = profile.true_peak_ceiling_millidbtp {
        let actual = measured
            .loudness
            .as_ref()
            .and_then(|m| m.true_peak_millidbtp)
            .ok_or_else(|| {
                ContractError::Unknown("true peak unavailable; cannot approve a repair".into())
            })?;
        high = high.min(i64::from(ceiling) - i64::from(actual));
    }
    if let Some(target) = profile.integrated_lufs_milli {
        let actual = measured
            .loudness
            .as_ref()
            .and_then(|m| m.integrated_lufs_milli)
            .ok_or_else(|| {
                ContractError::Unknown(
                    "integrated loudness unavailable; cannot approve a repair".into(),
                )
            })?;
        let tolerance = i64::from(profile.loudness_tolerance_milli);
        low = low.max(i64::from(target) - tolerance - i64::from(actual));
        high = high.min(i64::from(target) + tolerance - i64::from(actual));
    }
    ensure(
        low <= high,
        "loudness and peak constraints conflict; gain-only repair cannot satisfy both",
    )?;
    let delta = i32::try_from(0_i64.clamp(low, high))
        .map_err(|_| ContractError::Limit("gain adjustment overflow".into()))?;
    ensure(
        delta != 0,
        "failed rule is not repairable by a nonzero master gain adjustment",
    )?;
    let next = domain(MilliDb::new(previous.0 + delta))?;
    Ok(Some(GainRepair {
        master_bus: project.master_bus.clone(),
        previous_gain: previous,
        next_gain: next,
        delta_millidb: delta,
        artifact: measured.artifact.clone(),
    }))
}
