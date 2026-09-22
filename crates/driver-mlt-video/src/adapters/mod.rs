//! Application adapters preserve the original graph and expose mutation availability separately.
mod generic;
mod kdenlive;
mod shotcut;
use crate::{
    Error, Result,
    hash::sha256,
    json,
    model::*,
    time::FrameRange,
    xml::{self, Node},
};
pub use generic::{GenericMltAdapter, write_normal_form};
pub use kdenlive::KdenliveAdapter;
pub use shotcut::ShotcutAdapter;
use std::collections::BTreeMap;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Support {
    SafeRoundtrip,
    MetadataRisk,
    RenderOnly,
    Unsupported,
}
impl Support {
    pub fn name(self) -> &'static str {
        match self {
            Self::SafeRoundtrip => "SAFE_ROUNDTRIP",
            Self::MetadataRisk => "SUPPORTED_WITH_METADATA_RISK",
            Self::RenderOnly => "MLT_RENDER_ONLY",
            Self::Unsupported => "UNSUPPORTED",
        }
    }
}
pub trait ProjectAdapter {
    fn name(&self) -> &'static str;
    fn detect(&self, root: &Node) -> bool;
    fn parse(&self, root: Node) -> Result<Project>;
    fn serialize(&self, project: &Project) -> Result<String>;
    fn supported_mutation(&self, project: &Project, operation: &str) -> Support;
}
pub fn adapter(format: Format) -> Box<dyn ProjectAdapter> {
    match format {
        Format::GenericMlt => Box::new(GenericMltAdapter),
        Format::Kdenlive => Box::new(KdenliveAdapter),
        _ => Box::new(ShotcutAdapter),
    }
}
pub fn load(bytes: &[u8]) -> Result<Project> {
    let root = xml::parse(bytes)?;
    let kde = KdenliveAdapter.detect(&root);
    let shot = ShotcutAdapter.detect(&root);
    if kde && shot {
        return Err(Error::new(
            "AmbiguousTarget",
            "Both Kdenlive and Shotcut metadata detected",
        ));
    }
    if kde {
        KdenliveAdapter.parse(root)
    } else if shot {
        ShotcutAdapter.parse(root)
    } else {
        GenericMltAdapter.parse(root)
    }
}
pub fn save(project: &Project) -> Result<String> {
    adapter(project.format).serialize(project)
}
pub fn profile(root: &Node) -> Result<Profile> {
    let p = root
        .elements()
        .find(|n| n.name == "profile")
        .ok_or_else(|| {
            Error::unsupported("Explicit MLT profile is required; no guessed frame rate")
        })?;
    let n = |key: &str, default: u32| -> Result<u32> {
        p.a(key).map_or(Ok(default), |s| {
            s.parse()
                .map_err(|_| Error::invalid("Invalid profile integer"))
        })
    };
    let profile = Profile {
        width: n("width", 0)?,
        height: n("height", 0)?,
        fps: crate::time::FrameRate::new(n("frame_rate_num", 0)?, n("frame_rate_den", 0)?)?,
        progressive: n("progressive", 1)? != 0,
        sample_aspect: (n("sample_aspect_num", 1)?, n("sample_aspect_den", 1)?),
        display_aspect: (n("display_aspect_num", 16)?, n("display_aspect_den", 9)?),
        colorspace: n("colorspace", 709)?,
        audio_channels: n("semwright_audio_channels", 2)?,
    };
    profile.validate()?;
    Ok(profile)
}
fn ptext(n: &Node, keys: &[&str], fallback: &str) -> String {
    keys.iter()
        .find_map(|k| n.property(k))
        .unwrap_or_else(|| fallback.into())
}
fn id(n: &Node) -> Result<String> {
    n.a("id")
        .map(str::to_owned)
        .ok_or_else(|| Error::invalid("Service requires an ID"))
}
pub fn parse_resource(service: &str, raw: &str) -> Resource {
    if matches!(service, "color" | "colour") {
        Resource::Color(raw.into())
    } else if !matches!(
        service,
        "avformat" | "avformat-novalidate" | "qimage" | "pixbuf"
    ) {
        Resource::Opaque(raw.into())
    } else if raw.starts_with('/')
        || raw.contains(':')
        || raw.contains('\\')
        || raw.split('/').any(|s| s == "..")
    {
        Resource::External(raw.into())
    } else {
        Resource::Relative(raw.into())
    }
}
fn assets(root: &Node, fps: crate::time::FrameRate) -> Result<BTreeMap<String, MediaAsset>> {
    let mut result = BTreeMap::new();
    for n in root
        .elements()
        .filter(|n| matches!(n.name.as_str(), "producer" | "chain"))
    {
        if n.property("semwright:clip_asset").is_some() {
            continue;
        }
        let id = id(n)?;
        let service = n
            .property("mlt_service")
            .unwrap_or_else(|| "unknown".into());
        let raw = n.property("resource").unwrap_or_default();
        let mut resource = parse_resource(&service, &raw);
        if let (Some(root), Some(path)) = (
            n.property("semwright:resource_root"),
            n.property("semwright:resource_path"),
        ) {
            resource = Resource::Scoped { root, path };
        }
        let frames = if let Some(length) = n.property("length") {
            Some(fps.parse_mlt(&length)?)
        } else if let Some(out) = n.a("out") {
            Some(
                fps.parse_mlt(out)?
                    .checked_add(1)
                    .ok_or_else(|| Error::invalid("Asset duration overflow"))?,
            )
        } else {
            None
        };
        let kind = n.property("semwright:kind").unwrap_or_else(|| {
            if matches!(service.as_str(), "color" | "colour") {
                "color"
            } else if matches!(service.as_str(), "qimage" | "pixbuf") {
                "image"
            } else {
                "unknown"
            }
            .into()
        });
        let opaque = !matches!(
            service.as_str(),
            "color" | "colour" | "avformat" | "avformat-novalidate" | "qimage" | "pixbuf"
        );
        result.insert(
            id.clone(),
            MediaAsset {
                id: id.clone(),
                name: ptext(
                    n,
                    &["semwright:name", "kdenlive:clipname", "shotcut:caption"],
                    &id,
                ),
                kind,
                resource,
                frames,
                service,
                original: n
                    .property("kdenlive:originalurl")
                    .or_else(|| n.property("shotcut:originalResource")),
                proxy: n
                    .property("kdenlive:proxy")
                    .or_else(|| n.property("shotcut:resource")),
                opaque,
            },
        );
    }
    Ok(result)
}
fn effects(n: &Node, prefix: &str) -> Result<Vec<Effect>> {
    let mut out = vec![];
    for (index, f) in n.elements().filter(|n| n.name == "filter").enumerate() {
        let id = f.property("semwright:effect_id").unwrap_or_else(|| {
            format!(
                "ef_{}",
                &sha256(format!("{prefix}:{index}").as_bytes())[..24]
            )
        });
        let service = f
            .property("mlt_service")
            .unwrap_or_else(|| "unknown".into());
        let property = "level".to_string();
        let raw = f.property(&property).unwrap_or_else(|| "0".into());
        let mut value = 0;
        let mut keyframes = vec![];
        let mut opaque = true;
        if f.property("semwright:effect").as_deref() == Some("1")
            && matches!(service.as_str(), "volume" | "brightness")
        {
            opaque = false;
            if raw.contains('=') {
                for part in raw.split(';') {
                    let (frame, v) = part
                        .split_once('=')
                        .ok_or_else(|| Error::invalid("Invalid effect animation"))?;
                    let (frame, interpolation) = if let Some(frame) = frame.strip_suffix('|') {
                        (frame, Interpolation::Hold)
                    } else {
                        (frame, Interpolation::Linear)
                    };
                    let frame = frame
                        .parse()
                        .map_err(|_| Error::invalid("Invalid keyframe frame"))?;
                    keyframes.push(Keyframe {
                        frame,
                        value: parse_decimal(v)?,
                        interpolation,
                    });
                }
                value = keyframes.first().map_or(0, |k| k.value);
            } else {
                value = parse_decimal(&raw)?;
            }
        }
        out.push(Effect {
            id,
            service,
            property,
            value,
            enabled: f.property("disable").as_deref() != Some("1"),
            keyframes,
            opaque: if opaque { Some(f.clone()) } else { None },
        });
    }
    Ok(out)
}
fn lane(
    root: &Node,
    n: &Node,
    sequence: &str,
    assets: &mut BTreeMap<String, MediaAsset>,
    fps: crate::time::FrameRate,
) -> Result<Timeline> {
    let playlist = id(n)?;
    let mut cursor = 0u64;
    let mut ordinal = 0;
    let mut clips = vec![];
    for entry in n.elements() {
        match entry.name.as_str() {
            "blank" => {
                let d = fps.parse_mlt(
                    entry
                        .a("length")
                        .ok_or_else(|| Error::invalid("Blank missing length"))?,
                )?;
                cursor = cursor
                    .checked_add(d)
                    .ok_or_else(|| Error::invalid("Blank overflow"))?;
            }
            "entry" => {
                let producer = entry
                    .a("producer")
                    .ok_or_else(|| Error::invalid("Entry lacks producer"))?;
                let source = root
                    .by_id(producer)
                    .ok_or_else(|| Error::invalid("Missing producer"))?;
                let asset = source
                    .property("semwright:clip_asset")
                    .unwrap_or_else(|| producer.into());
                let asset = if assets.contains_key(&asset) {
                    asset
                } else {
                    let aid = format!("nested_asset_{producer}");
                    assets.entry(aid.clone()).or_insert(MediaAsset {
                        id: aid.clone(),
                        name: producer.into(),
                        kind: "composition".into(),
                        resource: Resource::Opaque(producer.into()),
                        frames: None,
                        service: source.name.clone(),
                        original: None,
                        proxy: None,
                        opaque: true,
                    });
                    aid
                };
                let start = fps.parse_mlt(entry.a("in").unwrap_or("0"))?;
                let end = fps.parse_mlt(entry.a("out").ok_or_else(|| {
                    Error::unsupported("Unbounded entry duration is inspection-unsupported")
                })?)?;
                let range = FrameRange::from_inclusive(start, end)?;
                let cid = source.property("semwright:clip_id").unwrap_or_else(|| {
                    format!(
                        "c_{}",
                        &sha256(
                            format!("{sequence}/{playlist}/{ordinal}/{producer}/{start}/{end}")
                                .as_bytes()
                        )[..24]
                    )
                });
                let name = ptext(
                    source,
                    &[
                        "semwright:clip_name",
                        "kdenlive:clipname",
                        "shotcut:caption",
                    ],
                    producer,
                );
                clips.push(Clip {
                    id: cid.clone(),
                    name,
                    asset,
                    start: cursor,
                    source: range,
                    effects: effects(source, &cid)?,
                    binding: Some(XmlBinding {
                        playlist: playlist.clone(),
                        entry: ordinal,
                    }),
                    speed: (1, 1),
                });
                cursor = cursor
                    .checked_add(range.duration())
                    .ok_or_else(|| Error::invalid("Timeline overflow"))?;
                ordinal += 1;
            }
            _ => {}
        }
        if clips.len() > MAX_CLIPS || cursor > crate::time::MAX_FRAME {
            return Err(Error::limit("Playlist budget exceeded"));
        }
    }
    Ok(Timeline {
        id: playlist,
        clips,
    })
}
/// A playlist is one lane, a track tractor may contain two lanes, never flattened.
fn track(
    root: &Node,
    service: &Node,
    sequence: &str,
    assets: &mut BTreeMap<String, MediaAsset>,
    fps: crate::time::FrameRate,
) -> Result<Track> {
    let tid = id(service)?;
    let mut lanes = vec![];
    let mut opaque = false;
    let mut muted = false;
    let mut hidden = false;
    if service.name == "playlist" {
        lanes.push(lane(root, service, sequence, assets, fps)?);
    } else {
        for child in service.elements().filter(|n| n.name == "track") {
            let hide = child.a("hide").unwrap_or("");
            muted |= matches!(hide, "audio" | "both");
            hidden |= matches!(hide, "video" | "both");
            if let Some(sub) = child.a("producer").and_then(|id| root.by_id(id)) {
                if sub.name == "playlist" {
                    lanes.push(lane(root, sub, sequence, assets, fps)?);
                } else {
                    opaque = true;
                }
            }
        }
    }
    let kind = if service.property("kdenlive:audio_track").as_deref() == Some("1")
        || service.property("shotcut:audio").as_deref() == Some("1")
    {
        String::from("audio")
    } else {
        service
            .property("semwright:track_kind")
            .unwrap_or_else(|| String::from("av"))
    };
    if let Some(v) = service.property("semwright:muted") {
        muted = v == "1";
    }
    if let Some(v) = service.property("semwright:hidden") {
        hidden = v == "1";
    }
    Ok(Track {
        id: if service.name == "playlist" {
            format!("track_{tid}")
        } else {
            tid.clone()
        },
        name: ptext(
            service,
            &["semwright:name", "kdenlive:track_name", "shotcut:name"],
            &tid,
        ),
        kind,
        muted,
        hidden,
        lanes,
        effects: effects(service, &tid)?,
        opaque,
    })
}
pub(crate) fn common(root: Node, format: Format, sequence_ids: Vec<String>) -> Result<Project> {
    let profile = profile(&root)?;
    let mut assets = assets(&root, profile.fps)?;
    let mut sequences = vec![];
    for sid in sequence_ids {
        let sn = root
            .by_id(&sid)
            .ok_or_else(|| Error::invalid("Sequence ID missing"))?;
        let mut tracks = vec![];
        let mut nested = vec![];
        let mut opaque = false;
        for child in sn.elements().filter(|n| n.name == "track") {
            let target = child
                .a("producer")
                .ok_or_else(|| Error::invalid("Track IDREF missing"))?;
            let service = root
                .by_id(target)
                .ok_or_else(|| Error::invalid("Track service missing"))?;
            if service.property("kdenlive:uuid").is_some() {
                nested.push(target.into());
                opaque = true;
                continue;
            }
            if !matches!(service.name.as_str(), "playlist" | "tractor") {
                opaque = true;
                continue;
            }
            let mut t = track(&root, service, &sid, &mut assets, profile.fps)?;
            let hide = child.a("hide").unwrap_or("");
            t.muted |= matches!(hide, "audio" | "both");
            t.hidden |= matches!(hide, "video" | "both");
            tracks.push(t);
        }
        let mut transitions = vec![];
        for (index, tr) in sn.elements().filter(|n| n.name == "transition").enumerate() {
            let a = tr
                .property("a_track")
                .and_then(|s| s.parse::<usize>().ok())
                .and_then(|i| tracks.get(i))
                .map(|t| t.id.clone())
                .unwrap_or_default();
            let b = tr
                .property("b_track")
                .and_then(|s| s.parse::<usize>().ok())
                .and_then(|i| tracks.get(i))
                .map(|t| t.id.clone())
                .unwrap_or_default();
            let service = tr.property("mlt_service").unwrap_or_default();
            let curated = tr.property("semwright:transition").as_deref() == Some("1");
            transitions.push(Transition {
                id: tr.property("semwright:transition_id").unwrap_or_else(|| {
                    format!("tr_{}", &sha256(format!("{sid}:{index}").as_bytes())[..24])
                }),
                kind: if curated {
                    match service.as_str() {
                        "luma" => "dissolve",
                        "mix" => "audio_mix",
                        _ => "opaque",
                    }
                    .into()
                } else {
                    "opaque".into()
                },
                a_track: a,
                b_track: b,
                range: FrameRange::from_inclusive(
                    profile.fps.parse_mlt(tr.a("in").unwrap_or("0"))?,
                    profile.fps.parse_mlt(tr.a("out").unwrap_or("0"))?,
                )?,
                reverse: tr.property("reverse").as_deref() == Some("1"),
                opaque: if curated { None } else { Some(tr.clone()) },
            });
        }
        let mut markers = vec![];
        if let Some(raw) = sn.property("semwright:markers") {
            for m in json::parse(raw.as_bytes())?.as_array()? {
                m.strict(
                    &["id", "frame", "label", "tags"],
                    &["id", "frame", "label", "tags"],
                )?;
                markers.push(Marker {
                    id: m.str("id")?.into(),
                    frame: m.uint("frame")?,
                    label: m.str("label")?.into(),
                    tags: m
                        .get("tags")?
                        .as_array()?
                        .iter()
                        .map(|v| v.string().map(str::to_owned))
                        .collect::<Result<_>>()?,
                });
            }
        }
        let mut subtitles = vec![];
        for f in sn.elements().filter(|n| n.name == "filter") {
            if f.property("mlt_service").as_deref() == Some("avfilter.subtitles") {
                subtitles.push(SubtitleReference {
                    resource: f.property("av.filename").unwrap_or_default(),
                    representation: "opaque_srt_filter".into(),
                });
            }
        }
        sequences.push(Sequence {
            id: sid.clone(),
            name: ptext(
                sn,
                &[
                    "semwright:name",
                    "kdenlive:sequenceproperties.name",
                    "kdenlive:sequenceproperty.name",
                ],
                &sid,
            ),
            tracks,
            transitions,
            markers,
            nested,
            subtitles,
            opaque,
        });
    }
    let mut p = Project {
        id: "project".into(),
        format,
        profile,
        assets,
        sequences,
        original: Some(root),
        format_version: None,
        warnings: vec![],
        source_root: None,
        source_dir: String::new(),
        generated: false,
    };
    if format != Format::GenericMlt {
        p.warnings.push(
            "Application metadata is untrusted; this is offline inspection, not live GUI control"
                .into(),
        );
    }
    // MLT permits a shared playlist/producer across multiple sequences. Give each model instance
    // a local identity while bindings retain the original XML identity. Such graphs are read-only.
    let mut seen = std::collections::BTreeSet::new();
    for s in &mut p.sequences {
        for t in &mut s.tracks {
            if !seen.insert(t.id.clone()) {
                t.id = format!("{}_{}", s.id, t.id);
                t.opaque = true;
            }
            for l in &mut t.lanes {
                if !seen.insert(l.id.clone()) {
                    l.id = format!("{}_{}", s.id, l.id);
                    t.opaque = true;
                }
            }
        }
    }
    p.validate()?;
    Ok(p)
}
pub fn rename_native_track(
    project: &mut Project,
    sequence: &str,
    track_id: &str,
    name: &str,
) -> Result<()> {
    let key = match project.format {
        Format::Kdenlive => "kdenlive:track_name",
        Format::Shotcut => "shotcut:name",
        _ => {
            return Err(Error::unsupported(
                "Native rename not supported for this format",
            ));
        }
    };
    let xml_id = if project.format == Format::Shotcut {
        track_id.strip_prefix("track_").unwrap_or(track_id)
    } else {
        track_id
    };
    let target = project
        .original
        .as_mut()
        .and_then(|n| n.by_id_mut(xml_id))
        .ok_or_else(|| Error::unsupported("Track does not have an unambiguous XML binding"))?;
    if target.property(key).is_none() {
        return Err(Error::unsupported(
            "Track name metadata is absent; refusing to invent it",
        ));
    }
    target.set_property(key, name);
    project.track_mut(sequence, track_id)?.name = name.into();
    Ok(())
}
