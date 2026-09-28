use crate::osc::{ArdourStrip, StripList};
use semwright_audio_domain::{
    Result,
    backend::{
        BackendContract, BackendIdentity, ProjectionFidelity, ProjectionLoss, ProjectionLossImpact,
        ProjectionLossKind, ProjectionReport, SemanticAudioProjection,
    },
    model::{AudioProfile, AudioProject, Bus, EffectChain, Stem},
    support::{AudioOperation, OperationSupport},
    time::SampleRate,
    units::MilliDb,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArdourSnapshot {
    pub strip_list: StripList,
}

pub struct ArdourSemanticProjection;

impl ArdourSemanticProjection {
    pub fn report(snapshot: &ArdourSnapshot) -> Result<ProjectionReport> {
        let mut project = AudioProject::new(AudioProfile {
            sample_rate: SampleRate(snapshot.strip_list.sample_rate),
            channels: 2,
            ..AudioProfile::default()
        })?;
        project.id = "ardour_session".into();
        let mut losses = vec![
            loss(
                ProjectionLossKind::UnsupportedSemantic,
                ProjectionLossImpact::ReadOnly,
                "ardour.regions_unobserved",
                Some("stems".into()),
                "Ardour OSC strip discovery does not expose region/clip topology",
            ),
            loss(
                ProjectionLossKind::UnsupportedSemantic,
                ProjectionLossImpact::ReadOnly,
                "ardour.routing_unobserved",
                Some("routing".into()),
                "Ardour OSC strip discovery does not expose complete route connectivity",
            ),
            loss(
                ProjectionLossKind::UnsupportedSemantic,
                ProjectionLossImpact::ReadOnly,
                "ardour.plugins_unobserved",
                Some("effects".into()),
                "Ardour plug-ins require explicit descriptor queries and are not guessed as semantic effects",
            ),
        ];

        for strip in &snapshot.strip_list.strips {
            match strip.kind.as_str() {
                "AT" => project.stems.push(stem(strip, &mut losses)),
                "B" | "FB" => project.buses.push(bus(strip, &mut losses)),
                "MT" | "MB" | "V" => losses.push(loss(
                    ProjectionLossKind::UnsupportedSemantic,
                    ProjectionLossImpact::ReadOnly,
                    "ardour.non_audio_strip",
                    Some(format!("strip/{}", strip.ssid)),
                    format!(
                        "Ardour strip type {} is not represented by audio-domain v1",
                        strip.kind
                    ),
                )),
                _ => {}
            }
        }

        project.validate()?;
        let report = ProjectionReport {
            project,
            fidelity: ProjectionFidelity::LossyReadOnly,
            losses,
        };
        report.validate()?;
        Ok(report)
    }

    pub fn contract(snapshot: &ArdourSnapshot) -> Result<BackendContract> {
        BackendContract::from_support(
            BackendIdentity {
                backend_id: "ardour".into(),
                backend_version: None,
                adapter_id: "ardour-osc/1".into(),
            },
            ProjectionFidelity::LossyReadOnly,
            |operation| {
                let has_audio_tracks = snapshot
                    .strip_list
                    .strips
                    .iter()
                    .any(|strip| strip.kind == "AT");
                let has_audio_buses = snapshot
                    .strip_list
                    .strips
                    .iter()
                    .any(|strip| matches!(strip.kind.as_str(), "B" | "FB"));
                let support = match operation {
                    AudioOperation::StemRename
                    | AudioOperation::StemMute
                    | AudioOperation::StemSolo
                        if has_audio_tracks =>
                    {
                        OperationSupport::SafeRoundtrip
                    }
                    AudioOperation::StemGainSet | AudioOperation::StemPanSet
                        if has_audio_tracks =>
                    {
                        OperationSupport::MetadataRisk
                    }
                    AudioOperation::BusGainSet | AudioOperation::BusPanSet if has_audio_buses => {
                        OperationSupport::MetadataRisk
                    }
                    _ => OperationSupport::Unsupported,
                };
                let reason = match support {
                    OperationSupport::MetadataRisk => Some(
                        "Ardour OSC accepts this control but /strip/list does not echo its numeric value for verification"
                            .into(),
                    ),
                    OperationSupport::Unsupported => Some(
                        "Ardour OSC adapter does not claim this shared semantic operation".into(),
                    ),
                    _ => None,
                };
                (support, reason)
            },
        )
    }
}

impl SemanticAudioProjection<ArdourSnapshot> for ArdourSemanticProjection {
    fn contract(&self, native: &ArdourSnapshot) -> Result<BackendContract> {
        Self::contract(native)
    }

    fn project(&self, native: &ArdourSnapshot) -> Result<ProjectionReport> {
        Self::report(native)
    }
}

fn stem(strip: &ArdourStrip, losses: &mut Vec<ProjectionLoss>) -> Stem {
    Stem {
        id: format!("ardour_track_{}", strip.ssid),
        name: clean_name(&strip.name, strip.ssid, losses),
        channels: bounded_channels(strip.outputs, strip.ssid, losses),
        muted: strip.muted,
        soloed: strip.soloed,
        gain: MilliDb(0),
        pan_milli: 0,
        output_bus: "master".into(),
        clips: vec![],
        effects: EffectChain::default(),
        automations: vec![],
    }
}

fn bus(strip: &ArdourStrip, losses: &mut Vec<ProjectionLoss>) -> Bus {
    Bus {
        id: format!("ardour_bus_{}", strip.ssid),
        name: clean_name(&strip.name, strip.ssid, losses),
        channels: bounded_channels(strip.outputs, strip.ssid, losses),
        gain: MilliDb(0),
        pan_milli: 0,
        effects: EffectChain::default(),
        sends: vec![],
        automations: vec![],
    }
}

fn clean_name(value: &str, ssid: u32, losses: &mut Vec<ProjectionLoss>) -> String {
    let cleaned: String = value
        .chars()
        .filter(|character| !character.is_control())
        .take(4096)
        .collect();
    if cleaned.is_empty() || cleaned != value {
        losses.push(loss(
            ProjectionLossKind::NativeMetadataOnly,
            ProjectionLossImpact::Advisory,
            "ardour.name_sanitized",
            Some(format!("strip/{ssid}")),
            "Ardour strip name required bounded display sanitization",
        ));
    }
    if cleaned.is_empty() {
        format!("Ardour strip {ssid}")
    } else {
        cleaned
    }
}

fn bounded_channels(outputs: u32, ssid: u32, losses: &mut Vec<ProjectionLoss>) -> u16 {
    if (1..=64).contains(&outputs) {
        outputs as u16
    } else {
        losses.push(loss(
            ProjectionLossKind::UnsupportedSemantic,
            ProjectionLossImpact::Advisory,
            "ardour.channel_count_normalized",
            Some(format!("strip/{ssid}")),
            "Ardour strip output count cannot be represented directly; semantic projection uses stereo",
        ));
        2
    }
}

fn loss(
    kind: ProjectionLossKind,
    impact: ProjectionLossImpact,
    code: &str,
    semantic_path: Option<String>,
    detail: impl Into<String>,
) -> ProjectionLoss {
    ProjectionLoss {
        kind,
        impact,
        code: code.into(),
        semantic_path,
        detail: detail.into(),
    }
}
