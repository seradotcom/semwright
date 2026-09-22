use super::*;
use crate::xml::{Child, property};
pub struct GenericMltAdapter;
impl ProjectAdapter for GenericMltAdapter {
    fn name(&self) -> &'static str {
        "generic-mlt/1"
    }
    fn detect(&self, root: &Node) -> bool {
        root.name == "mlt" && !KdenliveAdapter.detect(root) && !ShotcutAdapter.detect(root)
    }
    fn parse(&self, root: Node) -> Result<Project> {
        let own = root.a("semwright_normal_form") == Some("1");
        let mut sequence_ids: Vec<_> = root
            .elements()
            .filter(|n| {
                n.name == "tractor" && n.property("semwright:sequence").as_deref() == Some("1")
            })
            .filter_map(|n| n.a("id").map(str::to_owned))
            .collect();
        if sequence_ids.is_empty() {
            if let Some(last) = root.elements().filter(|n| n.name == "tractor").last() {
                sequence_ids.push(id(last)?);
            } else {
                return Err(Error::unsupported(
                    "MLT clip-only documents without a sequence tractor are inspection-unsupported",
                ));
            }
        }
        let mut p = common(root, Format::GenericMlt, sequence_ids)?;
        if own {
            p.format_version = Some("semwright-normal-form-1".into());
            let original = xml::serialize(
                p.original
                    .as_ref()
                    .ok_or_else(|| Error::invalid("Original missing"))?,
            )?;
            let regenerated = xml::serialize(&write_normal_form(&p, None)?)?;
            p.generated = original == regenerated;
            if !p.generated {
                p.warnings.push("Normal-form marker is not proof: extra or changed XML requires read-only preservation".into());
            }
        }
        if !p.generated {
            p.warnings.push("Imported generic graph is preserved read-only; deep editing requires the verified normal form".into());
        }
        Ok(p)
    }
    fn serialize(&self, p: &Project) -> Result<String> {
        p.validate()?;
        if p.generated {
            xml::serialize(&write_normal_form(p, None)?)
        } else {
            xml::serialize(
                p.original
                    .as_ref()
                    .ok_or_else(|| Error::invalid("Original XML absent"))?,
            )
        }
    }
    fn supported_mutation(&self, p: &Project, _op: &str) -> Support {
        if p.generated {
            Support::SafeRoundtrip
        } else {
            Support::Unsupported
        }
    }
}
fn asset_node(
    asset: &MediaAsset,
    id: &str,
    render_paths: Option<&BTreeMap<String, String>>,
) -> Result<Node> {
    let mut n = Node::new(if asset.service.starts_with("avformat") {
        "chain"
    } else {
        "producer"
    })
    .attr("id", id);
    if let Some(frames) = asset.frames {
        if frames > 0 {
            n.set("out", frames - 1);
        }
        n.push(property("length", &frames.to_string()));
    }
    let service = if asset.service == "colour" {
        "color"
    } else {
        &asset.service
    };
    n.push(property("mlt_service", service));
    let resource = if let Some(map) = render_paths {
        match &asset.resource {
            Resource::Color(color) => color.clone(),
            _ => map
                .get(&asset.id)
                .cloned()
                .ok_or_else(|| Error::new("PermissionDenied", "Render asset is not staged"))?,
        }
    } else {
        match &asset.resource {
            Resource::Scoped { root, path } => format!("../{root}/{path}"),
            _ => asset.resource.text(),
        }
    };
    n.push(property("resource", &resource));
    n.push(property("semwright:name", &asset.name));
    n.push(property("semwright:kind", &asset.kind));
    if let Resource::Scoped { root, path } = &asset.resource {
        n.push(property("semwright:resource_root", root));
        n.push(property("semwright:resource_path", path));
    }
    Ok(n)
}
fn effect_node(effect: &Effect, start: u64, duration: u64) -> Result<Node> {
    if let Some(n) = &effect.opaque {
        return Ok(n.clone());
    }
    effect.validate(duration)?;
    let mut f = Node::new("filter")
        .attr("in", start)
        .attr("out", start + duration - 1);
    f.push(property("mlt_service", &effect.service));
    f.push(property("semwright:effect", "1"));
    f.push(property("semwright:effect_id", &effect.id));
    f.push(property("disable", if effect.enabled { "0" } else { "1" }));
    let v = |n: i64| {
        if effect.service == "volume" {
            format!("{}dB", decimal(n))
        } else {
            decimal(n)
        }
    };
    let value = if effect.keyframes.is_empty() {
        v(effect.value)
    } else {
        effect
            .keyframes
            .iter()
            .map(|k| {
                format!(
                    "{}{}={}",
                    k.frame,
                    if k.interpolation == Interpolation::Hold {
                        "|"
                    } else {
                        ""
                    },
                    v(k.value)
                )
            })
            .collect::<Vec<_>>()
            .join(";")
    };
    f.push(property(&effect.property, &value));
    Ok(f)
}
/// Deterministic owned normal form; rendering uses a newly constructed graph with staged media.
pub fn write_normal_form(
    p: &Project,
    render_paths: Option<&BTreeMap<String, String>>,
) -> Result<Node> {
    p.validate()?;
    let mut root = Node::new("mlt")
        .attr("LC_NUMERIC", "C")
        .attr("semwright_normal_form", "1");
    let q = &p.profile;
    root.push(
        Node::new("profile")
            .attr("description", "Semwright")
            .attr("width", q.width)
            .attr("height", q.height)
            .attr("frame_rate_num", q.fps.num)
            .attr("frame_rate_den", q.fps.den)
            .attr("progressive", u8::from(q.progressive))
            .attr("sample_aspect_num", q.sample_aspect.0)
            .attr("sample_aspect_den", q.sample_aspect.1)
            .attr("display_aspect_num", q.display_aspect.0)
            .attr("display_aspect_den", q.display_aspect.1)
            .attr("colorspace", q.colorspace)
            .attr("semwright_audio_channels", q.audio_channels),
    );
    for a in p.assets.values() {
        root.push(asset_node(a, &a.id, render_paths)?);
    }
    for s in &p.sequences {
        for t in &s.tracks {
            for l in &t.lanes {
                for c in &l.clips {
                    let asset = p
                        .assets
                        .get(&c.asset)
                        .ok_or_else(|| Error::invalid("Missing asset"))?;
                    let mut n = asset_node(asset, &format!("source_{}", c.id), render_paths)?;
                    n.push(property("semwright:clip_asset", &c.asset));
                    n.push(property("semwright:clip_id", &c.id));
                    n.push(property("semwright:clip_name", &c.name));
                    for e in &c.effects {
                        n.push(effect_node(e, c.source.start.0, c.duration())?);
                    }
                    root.push(n);
                }
                let mut pl = Node::new("playlist").attr("id", &l.id);
                let mut cursor = 0;
                for c in &l.clips {
                    if c.start > cursor {
                        pl.push(Node::new("blank").attr("length", c.start - cursor));
                    }
                    pl.push(
                        Node::new("entry")
                            .attr("producer", format!("source_{}", c.id))
                            .attr("in", c.source.start.0)
                            .attr("out", c.source.end.0 - 1),
                    );
                    cursor = c.end()?;
                }
                root.push(pl);
            }
            let mut tr = Node::new("tractor").attr("id", &t.id);
            tr.push(property("semwright:name", &t.name));
            tr.push(property("semwright:track_kind", &t.kind));
            tr.push(property("semwright:muted", if t.muted { "1" } else { "0" }));
            tr.push(property(
                "semwright:hidden",
                if t.hidden { "1" } else { "0" },
            ));
            for l in &t.lanes {
                let mut node = Node::new("track").attr("producer", &l.id);
                let audio = t.muted || t.kind == "video";
                let video = t.hidden || t.kind == "audio";
                if audio || video {
                    node.set(
                        "hide",
                        match (audio, video) {
                            (true, true) => "both",
                            (true, false) => "audio",
                            _ => "video",
                        },
                    );
                }
                tr.push(node);
            }
            for e in &t.effects {
                tr.push(effect_node(e, 0, t.duration().max(1))?);
            }
            root.push(tr);
        }
        let mut seq = Node::new("tractor").attr("id", &s.id);
        if s.duration() > 0 {
            seq.set("out", s.duration() - 1);
        }
        seq.push(property("semwright:sequence", "1"));
        seq.push(property("semwright:name", &s.name));
        let markers = json::array(s.markers.iter().map(|m| {
            json::obj([
                ("id", m.id.clone().into()),
                ("frame", m.frame.into()),
                ("label", m.label.clone().into()),
                ("tags", json::array(m.tags.iter().cloned().map(Into::into))),
            ])
        }))
        .encode();
        seq.push(property("semwright:markers", &markers));
        for t in &s.tracks {
            seq.push(Node::new("track").attr("producer", &t.id));
        }
        for tr in &s.transitions {
            if let Some(n) = &tr.opaque {
                seq.push(n.clone());
                continue;
            }
            let mut n = Node::new("transition")
                .attr("in", tr.range.start.0)
                .attr("out", tr.range.end.0 - 1);
            n.push(property(
                "mlt_service",
                if tr.kind == "dissolve" { "luma" } else { "mix" },
            ));
            n.push(property("semwright:transition", "1"));
            n.push(property("semwright:transition_id", &tr.id));
            for (k, id) in [("a_track", &tr.a_track), ("b_track", &tr.b_track)] {
                let index = s
                    .tracks
                    .iter()
                    .position(|t| &t.id == id)
                    .ok_or_else(|| Error::invalid("Transition track missing"))?;
                n.push(property(k, &index.to_string()));
            }
            n.push(property("reverse", if tr.reverse { "1" } else { "0" }));
            if tr.kind == "audio_mix" {
                n.push(property("start", "0"));
                n.push(property("end", "1"));
            }
            seq.push(n);
        }
        root.push(seq);
    }
    // There is no user-supplied consumer in this graph. The runner supplies a curated consumer.
    if root
        .children
        .iter()
        .any(|c| matches!(c,Child::Element(n)if n.name=="consumer"))
    {
        return Err(Error::invalid(
            "Consumer is not part of project serialization",
        ));
    }
    xml::validate_graph(&root)?;
    Ok(root)
}
