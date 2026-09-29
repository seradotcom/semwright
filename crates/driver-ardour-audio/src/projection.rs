use crate::native::{ArdourSnapshot, NativeRegion, NativeRoute, RouteKind};
use semwright_audio_domain::{
    Result,
    backend::{
        BackendContract, BackendIdentity, ProjectionFidelity, ProjectionLoss, ProjectionLossImpact,
        ProjectionLossKind, ProjectionReport, SemanticAudioProjection,
    },
    hash::sha256,
    model::{
        AudioClip, AudioProfile, AudioProject, Bus, BusSend, ClipSource, EffectChain, Sample,
        SampleOrigin, SampleSource, Stem,
    },
    support::{AudioOperation, OperationSupport},
    time::{SampleFrame, SampleRange, SampleRate},
    units::MilliDb,
};
use std::collections::BTreeMap;

pub struct ArdourProjection;

impl SemanticAudioProjection<ArdourSnapshot> for ArdourProjection {
    fn contract(&self, native: &ArdourSnapshot) -> Result<BackendContract> {
        native.validate()?;
        let fidelity = self.project(native)?.fidelity;
        BackendContract::from_support(
            BackendIdentity {
                backend_id: "ardour".into(),
                backend_version: Some(native.ardour_version.clone()),
                adapter_id: "ardour-luasession/1".into(),
            },
            fidelity,
            |operation| {
                let support = match operation {
                    AudioOperation::StemRemove
                    | AudioOperation::StemRename
                    | AudioOperation::StemMute
                    | AudioOperation::StemSolo
                    | AudioOperation::StemGainSet
                    | AudioOperation::ClipMove
                    | AudioOperation::ClipRemove
                    | AudioOperation::BusRemove
                    | AudioOperation::BusGainSet => OperationSupport::SafeRoundtrip,
                    AudioOperation::StemCreate
                    | AudioOperation::StemPanSet
                    | AudioOperation::ClipTrim
                    | AudioOperation::BusPanSet => OperationSupport::MetadataRisk,
                    _ => OperationSupport::Unsupported,
                };
                let reason = (support == OperationSupport::Unsupported).then(|| {
                    "Ardour backend v1 does not expose this semantic mutation through its fixed Lua adapter"
                        .into()
                });
                (support, reason)
            },
        )
    }

    fn project(&self, native: &ArdourSnapshot) -> Result<ProjectionReport> {
        native.validate()?;
        let master = native
            .routes
            .iter()
            .find(|route| route.kind == RouteKind::Master);
        let channels = master.map_or(2, |route| route.channels);
        let mut project = AudioProject::new(AudioProfile {
            sample_rate: SampleRate(native.sample_rate),
            channels,
            ..AudioProfile::default()
        })?;
        project.id = format!("ardour_{}", &sha256(native.session_name.as_bytes())[..24]);

        let mut losses = vec![ProjectionLoss {
            kind: ProjectionLossKind::NativeMetadataOnly,
            impact: ProjectionLossImpact::Advisory,
            code: "ardour.tempo_map_not_projected".into(),
            semantic_path: None,
            detail: "Ardour backend v1 preserves session tempo/meter natively but projects the neutral default until tempo-map conformance is added".into(),
        }];

        if let Some(route) = master {
            let bus = project.bus_mut("master")?;
            bus.name = route.name.clone();
            bus.channels = route.channels;
            bus.gain = MilliDb(route.gain_millidb);
            bus.pan_milli = route.pan_milli;
            append_plugin_losses(&mut losses, route);
        }

        let mut route_to_bus = BTreeMap::new();
        route_to_bus.insert(
            master.map_or_else(|| "master".into(), |route| route.id.clone()),
            "master".to_string(),
        );

        for route in native
            .routes
            .iter()
            .filter(|route| route.kind == RouteKind::Bus)
        {
            let id = semantic_id("bus", &route.id);
            route_to_bus.insert(route.id.clone(), id.clone());
            project.buses.push(Bus {
                id,
                name: route.name.clone(),
                channels: route.channels,
                gain: MilliDb(route.gain_millidb),
                pan_milli: route.pan_milli,
                effects: EffectChain::default(),
                sends: vec![],
                automations: vec![],
            });
            append_route_completeness_losses(&mut losses, route);
            append_plugin_losses(&mut losses, route);
        }

        for route in native
            .routes
            .iter()
            .filter(|route| matches!(route.kind, RouteKind::Bus | RouteKind::Master))
        {
            let owner = route_to_bus
                .get(&route.id)
                .cloned()
                .unwrap_or_else(|| "master".into());
            let mut sends = Vec::new();
            for send in &route.sends {
                if let Some(target) = route_to_bus.get(&send.target_route) {
                    if target != &owner {
                        sends.push(BusSend {
                            target_bus: target.clone(),
                            gain: MilliDb(send.gain_millidb),
                            enabled: send.enabled,
                            pre_fader: send.pre_fader,
                        });
                    }
                } else {
                    loss(
                        &mut losses,
                        ProjectionLossKind::UnsupportedSemantic,
                        ProjectionLossImpact::ReadOnly,
                        "ardour.unresolved_send",
                        Some(format!("bus/{owner}")),
                        "Ardour send target was not present in the projected route graph",
                    );
                }
            }
            project.bus_mut(&owner)?.sends = sends;
        }

        let mut samples = BTreeMap::new();
        for route in native
            .routes
            .iter()
            .filter(|route| route.kind == RouteKind::Track)
        {
            let stem_id = semantic_id("stem", &route.id);
            let mut clips = Vec::new();
            for region in &route.regions {
                match project_region(region, native.sample_rate, &mut samples) {
                    Ok(clip) => clips.push(clip),
                    Err(detail) => loss(
                        &mut losses,
                        ProjectionLossKind::MissingMedia,
                        ProjectionLossImpact::ReadOnly,
                        "ardour.region_source_unprojectable",
                        Some(format!("stem/{stem_id}/region/{}", region.id)),
                        detail,
                    ),
                }
            }
            if !route.sends.is_empty() {
                loss(
                    &mut losses,
                    ProjectionLossKind::UnsupportedSemantic,
                    ProjectionLossImpact::ReadOnly,
                    "ardour.track_sends_not_projected",
                    Some(format!("stem/{stem_id}")),
                    "The v1 neutral model does not represent auxiliary sends owned directly by a stem",
                );
            }
            append_route_completeness_losses(&mut losses, route);
            append_plugin_losses(&mut losses, route);
            project.stems.push(Stem {
                id: stem_id,
                name: route.name.clone(),
                channels: route.channels,
                muted: route.muted,
                soloed: route.soloed,
                gain: MilliDb(route.gain_millidb),
                pan_milli: route.pan_milli,
                output_bus: "master".into(),
                clips,
                effects: EffectChain::default(),
                automations: vec![],
            });
        }
        project.samples = samples;

        for route in native
            .routes
            .iter()
            .filter(|route| route.kind == RouteKind::Other)
        {
            loss(
                &mut losses,
                ProjectionLossKind::OpaqueNativeObject,
                ProjectionLossImpact::ReadOnly,
                "ardour.opaque_route",
                Some(format!("route/{}", route.id)),
                "Native Ardour route type is not represented by the portable audio model",
            );
        }
        for warning in &native.warnings {
            loss(
                &mut losses,
                ProjectionLossKind::NativeMetadataOnly,
                ProjectionLossImpact::Advisory,
                "ardour.native_warning",
                None,
                warning.clone(),
            );
        }

        project.validate()?;
        let fidelity = if losses
            .iter()
            .any(|item| item.impact == ProjectionLossImpact::ReadOnly)
        {
            ProjectionFidelity::LossyReadOnly
        } else if losses.is_empty() {
            ProjectionFidelity::Exact
        } else {
            ProjectionFidelity::SemanticallyEquivalent
        };
        let report = ProjectionReport {
            project,
            fidelity,
            losses,
        };
        report.validate()?;
        Ok(report)
    }
}

fn project_region(
    region: &NativeRegion,
    sample_rate: u32,
    samples: &mut BTreeMap<String, Sample>,
) -> std::result::Result<AudioClip, String> {
    let path = region.source_path.as_ref().ok_or_else(|| {
        "Region source is external, non-file, or outside the mounted session".to_string()
    })?;
    let sample_id = semantic_id("sample", &format!("{}:{path}", region.source_id));
    if !samples.contains_key(&sample_id) {
        let sample = Sample {
            id: sample_id.clone(),
            name: region.source_name.clone(),
            channels: region.source_channels,
            sample_rate: SampleRate(sample_rate),
            frames: region.source_frames,
            source: SampleSource::RelativePath { path: path.clone() },
            origin: SampleOrigin::Imported,
        };
        sample.validate().map_err(|error| {
            format!(
                "Region source cannot enter portable model: {}",
                error.message
            )
        })?;
        samples.insert(sample_id.clone(), sample);
    }
    let range = SampleRange::new(
        region.source_start,
        region.source_start.saturating_add(region.length),
    )
    .map_err(|error| error.message)?;
    Ok(AudioClip {
        id: semantic_id("clip", &region.id),
        name: region.name.clone(),
        source: ClipSource::Sample { sample: sample_id },
        start: SampleFrame(region.position),
        source_range: range,
        gain: MilliDb(0),
        fade_in_frames: 0,
        fade_out_frames: 0,
    })
}

fn append_route_completeness_losses(losses: &mut Vec<ProjectionLoss>, route: &NativeRoute) {
    if !route.routing_complete {
        loss(
            losses,
            ProjectionLossKind::UnsupportedSemantic,
            ProjectionLossImpact::ReadOnly,
            "ardour.routing_not_fully_projected",
            Some(format!("route/{}", route.id)),
            "Ardour route output/routing graph is not fully represented by the v1 native snapshot",
        );
    }
    if !route.sends_complete {
        loss(
            losses,
            ProjectionLossKind::UnsupportedSemantic,
            ProjectionLossImpact::ReadOnly,
            "ardour.sends_not_fully_projected",
            Some(format!("route/{}", route.id)),
            "Ardour auxiliary sends are not fully represented by the v1 native snapshot",
        );
    }
    if !route.plugins_complete {
        loss(
            losses,
            ProjectionLossKind::UnsupportedPlugin,
            ProjectionLossImpact::ReadOnly,
            "ardour.plugins_not_fully_projected",
            Some(format!("route/{}", route.id)),
            "Ardour processor/plugin inventory is incomplete; route remains conservatively read-only for effect semantics",
        );
    }
}

fn append_plugin_losses(losses: &mut Vec<ProjectionLoss>, route: &NativeRoute) {
    for plugin in &route.plugins {
        loss(
            losses,
            ProjectionLossKind::UnsupportedPlugin,
            ProjectionLossImpact::ReadOnly,
            "ardour.opaque_plugin",
            Some(format!("route/{}/plugin/{}", route.id, plugin.id)),
            format!(
                "Ardour plugin '{}' remains native/read-only until a semantic mapping is certified",
                plugin.name
            ),
        );
    }
}

pub(crate) fn semantic_id(prefix: &str, native: &str) -> String {
    format!("{prefix}_{}", &sha256(native.as_bytes())[..24])
}

fn loss(
    losses: &mut Vec<ProjectionLoss>,
    kind: ProjectionLossKind,
    impact: ProjectionLossImpact,
    code: &str,
    semantic_path: Option<String>,
    detail: impl Into<String>,
) {
    losses.push(ProjectionLoss {
        kind,
        impact,
        code: code.into(),
        semantic_path,
        detail: detail.into(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::{NativePlugin, NativeRegion, NativeRoute, NativeSend, SNAPSHOT_VERSION};

    fn snapshot() -> ArdourSnapshot {
        ArdourSnapshot {
            snapshot_version: SNAPSHOT_VERSION,
            ardour_version: "9.0".into(),
            session_name: "fixture".into(),
            sample_rate: 48_000,
            session_start: 0,
            session_end: 0,
            routes: vec![
                NativeRoute {
                    id: "master1".into(),
                    name: "Master".into(),
                    kind: RouteKind::Master,
                    channels: 2,
                    muted: false,
                    soloed: false,
                    gain_millidb: 0,
                    pan_milli: 0,
                    regions: vec![],
                    sends: vec![],
                    plugins: vec![],
                    routing_complete: true,
                    sends_complete: true,
                    plugins_complete: true,
                },
                NativeRoute {
                    id: "track1".into(),
                    name: "Dialogue".into(),
                    kind: RouteKind::Track,
                    channels: 2,
                    muted: false,
                    soloed: false,
                    gain_millidb: -3_000,
                    pan_milli: 0,
                    regions: vec![NativeRegion {
                        id: "region1".into(),
                        name: "Take 1".into(),
                        position: 100,
                        source_start: 10,
                        length: 1000,
                        source_id: "source1".into(),
                        source_name: "take.wav".into(),
                        source_path: Some("interchange/take.wav".into()),
                        source_frames: 10_000,
                        source_channels: 2,
                        locked: false,
                    }],
                    sends: vec![],
                    plugins: vec![],
                    routing_complete: true,
                    sends_complete: true,
                    plugins_complete: true,
                },
            ],
            warnings: vec![],
        }
    }

    #[test]
    fn projects_tracks_regions_and_samples_without_native_ids_leaking() {
        let report = ArdourProjection.project(&snapshot()).unwrap();
        assert_eq!(report.project.stems.len(), 1);
        assert_eq!(report.project.samples.len(), 1);
        assert_eq!(report.project.stems[0].clips.len(), 1);
        assert!(!report.project.stems[0].id.contains("track1"));
        assert_eq!(report.fidelity, ProjectionFidelity::SemanticallyEquivalent);
    }

    #[test]
    fn opaque_plugins_and_external_media_degrade_to_read_only() {
        let mut native = snapshot();
        native.routes[1].plugins.push(NativePlugin {
            id: "plugin1".into(),
            name: "Mystery".into(),
            unique_id: Some("vendor.id".into()),
        });
        native.routes[1].regions[0].source_path = None;
        let report = ArdourProjection.project(&native).unwrap();
        assert_eq!(report.fidelity, ProjectionFidelity::LossyReadOnly);
        assert!(
            report
                .losses
                .iter()
                .any(|x| x.code == "ardour.opaque_plugin")
        );
        assert!(
            report
                .losses
                .iter()
                .any(|x| x.code == "ardour.region_source_unprojectable")
        );
    }

    #[test]
    fn contract_is_complete_and_session_mutations_are_explicit() {
        let contract = ArdourProjection.contract(&snapshot()).unwrap();
        assert_eq!(contract.operations.len(), AudioOperation::ALL.len());
        assert_eq!(
            contract.support(AudioOperation::StemGainSet),
            OperationSupport::SafeRoundtrip
        );
        assert_eq!(
            contract.support(AudioOperation::EffectAdd),
            OperationSupport::Unsupported
        );
    }

    #[test]
    fn unresolved_bus_send_fails_closed() {
        let mut native = snapshot();
        native.routes.push(NativeRoute {
            id: "bus1".into(),
            name: "FX".into(),
            kind: RouteKind::Bus,
            channels: 2,
            muted: false,
            soloed: false,
            gain_millidb: 0,
            pan_milli: 0,
            regions: vec![],
            sends: vec![NativeSend {
                target_route: "missing".into(),
                gain_millidb: -6_000,
                enabled: true,
                pre_fader: false,
            }],
            plugins: vec![],
            routing_complete: true,
            sends_complete: true,
            plugins_complete: true,
        });
        let report = ArdourProjection.project(&native).unwrap();
        assert_eq!(report.fidelity, ProjectionFidelity::LossyReadOnly);
    }
}
