//! Curated capability dispatch. Caller arguments never supply an executable, shell, environment or XML mutation.
use crate::{
    Error, Result, adapters,
    catalog::{self, Capability},
    edit::{self, Edit},
    fs::{PrivateDir, Root},
    hash::{Sha256, random_id, reader_hash, sha256, valid_digest},
    jobs::{self, Jobs},
    json::{Value, array, display, obj},
    model::*,
    refs::RefStore,
    runtime::{RenderProfile, Runtime},
    time::{FrameRange, FrameRate},
};
use semwright_video_domain::support::VideoOperation;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    os::unix::fs::MetadataExt,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};
pub const VERSION: &str = "0.1.0";
#[derive(Clone)]
struct SourcePin {
    root: String,
    path: String,
    sha256: String,
    device: u64,
    inode: u64,
}
#[derive(Clone)]
struct Loaded {
    project: Project,
    revision: String,
    source: Option<SourcePin>,
}
pub struct App {
    pub capabilities: Vec<Capability>,
    projects: BTreeMap<String, Loaded>,
    refs: RefStore,
    pub roots: Arc<BTreeMap<String, Arc<Root>>>,
    pub runtime: Option<Arc<Runtime>>,
    pub jobs: Jobs,
    pub runtime_reason: String,
}
impl App {
    pub fn new(roots: BTreeMap<String, Arc<Root>>, runtime: Option<Arc<Runtime>>) -> Result<Self> {
        Ok(Self {
            capabilities: catalog::capabilities()?,
            projects: BTreeMap::new(),
            refs: RefStore::default(),
            roots: Arc::new(roots),
            runtime,
            jobs: Jobs::new(),
            runtime_reason:
                "Runtime is not configured or could not pass pinned-tool confinement probes".into(),
        })
    }
    pub fn production() -> Result<Self> {
        let mut roots = BTreeMap::new();
        for (name, writable) in [("project", false), ("media", false), ("output", true)] {
            if let Ok(path) = semwright_driver_sdk::workspace_mount(name)
                && path.is_dir()
            {
                roots.insert(name.into(), Arc::new(Root::open(&path, true, writable)?));
            }
        }
        let mut reason = "No owner-provided read-only runtime configuration mount".to_string();
        let config = semwright_driver_sdk::workspace_mount("runtime").ok();
        let runtime = if config.as_ref().is_some_and(|path| path.is_dir()) {
            let config = config.as_deref().expect("checked runtime mount");
            let attempt = (|| -> Result<Arc<Runtime>> {
                let root = Root::open(config, true, false)?;
                let mut file = root.read_file("runtime.json", 16384)?;
                if file.metadata()?.mode() & 0o022 != 0 {
                    return Err(Error::new(
                        "PermissionDenied",
                        "Runtime configuration must not be group/other writable",
                    ));
                }
                let mut bytes = vec![];
                file.read_to_end(&mut bytes)?;
                Ok(Arc::new(Runtime::load(&crate::json::parse(&bytes)?)?))
            })();
            match attempt {
                Ok(r) => {
                    reason = "Pinned melt and ffprobe passed bounded bubblewrap service discovery"
                        .into();
                    Some(r)
                }
                Err(e) => {
                    let detail: String = e
                        .message
                        .chars()
                        .filter(|ch| !ch.is_control())
                        .take(512)
                        .collect();
                    reason = format!("Runtime unavailable: {}: {detail}", e.code);
                    None
                }
            }
        } else {
            None
        };
        let mut app = Self::new(roots, runtime)?;
        app.runtime_reason = reason;
        Ok(app)
    }
    pub fn doctor(&self) -> Value {
        obj([
            ("driver_version", VERSION.into()),
            ("driver_protocol", 1u64.into()),
            (
                "adapters",
                array([
                    "generic-mlt/1".into(),
                    "kdenlive-generation5-inspection/1".into(),
                    "shotcut-annotations-inspection/1".into(),
                ]),
            ),
            ("render_available", self.runtime.is_some().into()),
            (
                "mlt_version",
                self.runtime
                    .as_ref()
                    .map_or(Value::Null, |r| r.catalog.version.clone().into()),
            ),
            ("capabilities", self.capabilities.len().into()),
            (
                "filesystem_mounts",
                array(self.roots.keys().cloned().map(Into::into)),
            ),
            ("reason", self.runtime_reason.clone().into()),
            ("network", false.into()),
            ("native_gui_verified", false.into()),
        ])
    }
    pub fn execute(&mut self, command: &str, digest: &str, args: Value) -> Result<Value> {
        let capability = self
            .capabilities
            .iter()
            .find(|c| c.name == command)
            .cloned()
            .ok_or_else(|| Error::new("NotFound", "Capability not registered"))?;
        if capability.digest != digest {
            return Err(Error::new("Conflict", "Pinned descriptor digest mismatch"));
        }
        capability.input(&args)?;
        let operation = command
            .strip_prefix(catalog::PREFIX)
            .ok_or_else(|| Error::invalid("Driver namespace mismatch"))?;
        let value = self.dispatch(operation, &args)?;
        if let Err(mut e) = capability.output(&value) {
            if capability.mutates() {
                e.outcome_known = false;
            }
            return Err(e);
        }
        if value.encode().len() > 900_000 {
            let mut e = Error::limit("Result exceeds response frame budget");
            e.outcome_known = !capability.mutates();
            return Err(e);
        }
        Ok(value)
    }
    fn identify(&mut self, token: &str) -> Result<String> {
        let target = self.refs.get(token, "project")?;
        let loaded = self
            .projects
            .get(&target.project)
            .ok_or_else(Error::stale)?;
        if target.revision != loaded.revision {
            return Err(Error::stale());
        }
        let valid = if let Some(pin) = &loaded.source {
            let attempt = (|| -> Result<bool> {
                let root = self.roots.get(&pin.root).ok_or_else(Error::stale)?;
                let file = root.read_file(&pin.path, crate::xml::MAX_XML as u64)?;
                let m = file.metadata()?;
                if m.ino() != pin.inode || m.dev() != pin.device {
                    return Ok(false);
                }
                let (hash, _) = reader_hash(file, crate::xml::MAX_XML as u64)?;
                Ok(hash == pin.sha256)
            })();
            attempt.unwrap_or(false)
        } else {
            true
        };
        if !valid {
            self.refs.invalidate(&target.project);
            self.projects.remove(&target.project);
            return Err(Error::stale());
        }
        Ok(target.project)
    }
    fn resolve(
        &self,
        args: &Value,
        key: &str,
        project: &str,
        revision: &str,
        kind: &str,
    ) -> Result<String> {
        Ok(self.refs.resolve(args.str(key)?, project, revision, kind)?)
    }
    fn project_handle(&mut self, id: &str) -> Result<Value> {
        let loaded = self.projects.get(id).ok_or_else(Error::stale)?;
        let r = self
            .refs
            .issue(id, &loaded.revision, "project", "project")?;
        Ok(obj([
            ("project", r.into()),
            ("revision", loaded.revision.clone().into()),
            ("format", loaded.project.format.name().into()),
            ("warnings", warnings(&loaded.project.warnings)),
        ]))
    }
    fn add_project(&mut self, project: Project, source: Option<SourcePin>) -> Result<Value> {
        if self.projects.len() >= 4 {
            return Err(Error::limit(
                "At most four loaded projects; close an unused project first",
            ));
        }
        let id = random_id()?;
        let revision = edit::revision(&project)?;
        self.projects.insert(
            id.clone(),
            Loaded {
                project,
                revision,
                source,
            },
        );
        self.project_handle(&id)
    }
    fn dispatch(&mut self, op: &str, a: &Value) -> Result<Value> {
        match op {
            "doctor" => return Ok(self.doctor()),
            "project.create" => {
                let profile = a
                    .opt("profile")
                    .map(parse_profile)
                    .transpose()?
                    .unwrap_or_default();
                return self.add_project(Project::new(profile)?, None);
            }
            "project.open" => {
                let root_name = a.str("root")?;
                let path = a.str("path")?;
                let root = self
                    .roots
                    .get(root_name)
                    .ok_or_else(|| Error::new("Unavailable", "Project mount is absent"))?;
                let mut file = root.read_file(path, crate::xml::MAX_XML as u64)?;
                let m = file.metadata()?;
                let mut bytes = vec![];
                (&mut file)
                    .take(crate::xml::MAX_XML as u64 + 1)
                    .read_to_end(&mut bytes)?;
                let mut p = adapters::load(&bytes)?;
                p.source_root = Some(root_name.into());
                p.source_dir = path
                    .rsplit_once('/')
                    .map_or(String::new(), |(d, _)| d.into());
                return self.add_project(
                    p,
                    Some(SourcePin {
                        root: root_name.into(),
                        path: path.into(),
                        sha256: sha256(&bytes),
                        device: m.dev(),
                        inode: m.ino(),
                    }),
                );
            }
            "frames.encode" => return self.frames_encode(a),
            "sync.probe" => return self.sync_probe(a),
            "render.profiles" => {
                return Ok(obj([
                    (
                        "profiles",
                        array(
                            RenderProfile::all()
                                .iter()
                                .map(|p| p.json(self.runtime.as_ref().map(|r| &r.catalog))),
                        ),
                    ),
                    (
                        "mlt_version",
                        self.runtime
                            .as_ref()
                            .map_or(Value::Null, |r| r.catalog.version.clone().into()),
                    ),
                ]));
            }
            "render.status" | "render.result" => {
                let s = self.jobs.get(a.str("job")?)?;
                if op == "render.result" && s.state != jobs::State::Succeeded {
                    return Err(Error::new(
                        "Unavailable",
                        "No validated successful artifact is available",
                    ));
                }
                return Ok(s.json());
            }
            "render.cancel" => return self.jobs.cancel(a.str("job")?).map(|s| s.json()),
            "effect.search" => {
                let query = a
                    .opt("query")
                    .map(Value::string)
                    .transpose()?
                    .unwrap_or("")
                    .to_lowercase();
                return Ok(obj([
                    (
                        "items",
                        array(
                            ["volume", "brightness"]
                                .into_iter()
                                .filter(|s| s.contains(&query))
                                .map(|s| self.effect_info(s)),
                        ),
                    ),
                    (
                        "mlt_version",
                        self.runtime
                            .as_ref()
                            .map_or(Value::Null, |r| r.catalog.version.clone().into()),
                    ),
                ]));
            }
            "effect.describe" => return Ok(self.effect_info(a.str("service")?)),
            _ => {}
        }
        let pid = self.identify(a.str("project")?)?;
        let loaded = self.projects.get(&pid).cloned().ok_or_else(Error::stale)?;
        let p = &loaded.project;
        let revision = &loaded.revision;
        if let Some(expected) = a.opt("expected_revision") {
            if expected.string()? != revision {
                return Err(Error::stale());
            }
        }
        let sequence = if a.opt("sequence").is_some() {
            Some(self.resolve(a, "sequence", &pid, revision, "sequence")?)
        } else {
            None
        };
        match op {
            "project.inspect" => {
                let mut sequences = vec![];
                for s in &p.sequences {
                    sequences.push(self.sequence_dto(&pid, revision, s)?);
                }
                Ok(obj([
                    (
                        "project",
                        self.refs
                            .issue(&pid, revision, "project", "project")?
                            .into(),
                    ),
                    ("revision", revision.clone().into()),
                    ("format", p.format.name().into()),
                    ("profile", p.profile.json()),
                    ("sequences", array(sequences)),
                    ("asset_count", p.assets.len().into()),
                    (
                        "track_count",
                        p.sequences
                            .iter()
                            .map(|s| s.tracks.len())
                            .sum::<usize>()
                            .into(),
                    ),
                    (
                        "clip_count",
                        p.sequences
                            .iter()
                            .flat_map(|s| &s.tracks)
                            .flat_map(|t| &t.lanes)
                            .map(|l| l.clips.len())
                            .sum::<usize>()
                            .into(),
                    ),
                    ("deep_editable", p.generated.into()),
                    ("warnings", warnings(&p.warnings)),
                ]))
            }
            "project.validate" => {
                p.validate()?;
                Ok(obj([
                    ("valid", true.into()),
                    ("revision", revision.clone().into()),
                    ("warnings", warnings(&p.warnings)),
                ]))
            }
            "project.format" => {
                let contract = crate::domain::contract(p)?;
                Ok(obj([
                    ("format", p.format.name().into()),
                    (
                        "version",
                        p.format_version.clone().map_or(Value::Null, Into::into),
                    ),
                    (
                        "mutations",
                        array(
                            self.capabilities
                                .iter()
                                .filter(|c| c.mutates())
                                .filter_map(|c| c.name.strip_prefix(catalog::PREFIX))
                                .filter(|op| {
                                    !matches!(
                                        *op,
                                        "project.create"
                                            | "project.open"
                                            | "project.save_as"
                                            | "project.close"
                                            | "render.start"
                                            | "render.cancel"
                                    )
                                })
                                .map(|operation| {
                                    let support = operation
                                        .parse::<VideoOperation>()
                                        .map_or(adapters::Support::Unsupported, |operation| {
                                            contract.support(operation)
                                        });
                                    obj([
                                        ("operation", operation.into()),
                                        ("support", adapters::support_name(support).into()),
                                    ])
                                }),
                        ),
                    ),
                    ("warnings", warnings(&p.warnings)),
                ]))
            }
            "project.profile.get" => Ok(p.profile.json()),
            "project.close" => {
                self.refs.invalidate(&pid);
                self.projects.remove(&pid);
                Ok(obj([("closed", true.into())]))
            }
            "project.diff" => {
                let other = self.identify(a.str("other")?)?;
                let other = self.projects.get(&other).ok_or_else(Error::stale)?;
                Ok(edit::diff(p, &other.project))
            }
            "project.save_as" => {
                let path = a.str("path")?;
                let root = self
                    .roots
                    .get("output")
                    .ok_or_else(|| Error::new("Unavailable", "Output mount absent"))?;
                crate::fs::validate_relative(path)?;
                if root.exists(path)? {
                    return Err(Error::new(
                        "Conflict",
                        "Save-as target exists; no overwrite capability",
                    ));
                }
                let xml = adapters::save(p)?;
                let reopened = adapters::load(xml.as_bytes())?;
                if reopened.semantic_json() != p.semantic_json() {
                    return Err(Error::new(
                        "BackendFailed",
                        "Save validation changed semantics",
                    ));
                }
                let preview = a.flag("preview", false)?;
                let artifact = if preview {
                    Value::Null
                } else {
                    let artifact = root.write_new("output", path, xml.as_bytes())?;
                    let checked = (|| -> Result<bool> {
                        let stored = root.read(path, crate::xml::MAX_XML)?;
                        Ok(sha256(&stored) == artifact.sha256
                            && adapters::load(&stored)?.semantic_json() == p.semantic_json())
                    })();
                    if !matches!(checked, Ok(true)) {
                        let mut e = Error::new(
                            "BackendFailed",
                            "Published file failed read-back validation",
                        );
                        e.outcome_known = false;
                        return Err(e);
                    }
                    artifact.json()
                };
                Ok(obj([("written",(!preview).into()),("artifact",artifact),("project_revision",revision.clone().into()),("warnings",array(["Save-as preserves resource spelling, not media placement; maintain the referenced directory layout".into(),"The original file is never overwritten and live GUI coordination is not provided".into()]))]))
            }
            "sequence.duration" => {
                let s = p.sequence(
                    sequence
                        .as_deref()
                        .ok_or_else(|| Error::invalid("Explicit sequence required"))?,
                )?;
                Ok(obj([
                    ("frames", s.duration().into()),
                    ("fps_num", u64::from(p.profile.fps.num).into()),
                    ("fps_den", u64::from(p.profile.fps.den).into()),
                ]))
            }
            "render.plan" => {
                let profile = RenderProfile::get(a.str("profile")?)?;
                jobs::render_plan(
                    p,
                    sequence
                        .as_deref()
                        .ok_or_else(|| Error::invalid("Sequence required"))?,
                    revision,
                    &profile,
                    a.str("output")?,
                    self.runtime.as_deref(),
                    &self.roots,
                )
            }
            "render.start" => {
                let runtime = self.runtime.clone().ok_or_else(|| {
                    Error::new("Unavailable", "Constrained MLT runtime is not available")
                })?;
                let profile = RenderProfile::get(a.str("profile")?)?;
                self.jobs
                    .start(
                        runtime,
                        p.clone(),
                        sequence.ok_or_else(|| Error::invalid("Sequence required"))?,
                        revision.clone(),
                        profile,
                        a.str("output")?.into(),
                        self.roots.clone(),
                    )
                    .map(|s| s.json())
            }
            "audio.volume.get" => {
                let seq = sequence
                    .as_deref()
                    .ok_or_else(|| Error::invalid("Sequence required"))?;
                let clip = self.resolve(a, "clip", &pid, revision, "clip")?;
                let c = p.clip(seq, &clip)?;
                let es: Vec<_> = c.effects.iter().filter(|e| e.service == "volume").collect();
                if es.len() > 1 {
                    return Err(Error::new(
                        "AmbiguousTarget",
                        "Multiple volume filters; inspect explicit effects",
                    ));
                }
                if let Some(e) = es.first() {
                    Ok(obj([
                        (
                            "value_milli",
                            if e.opaque.is_some() || !e.keyframes.is_empty() {
                                Value::Null
                            } else {
                                e.value.into()
                            },
                        ),
                        ("animated", (!e.keyframes.is_empty()).into()),
                        (
                            "effect",
                            self.refs.issue(&pid, revision, "effect", &e.id)?.into(),
                        ),
                    ]))
                } else {
                    Ok(obj([
                        ("value_milli", 0i64.into()),
                        ("animated", false.into()),
                        ("effect", Value::Null),
                    ]))
                }
            }
            "sequence.list" | "sequence.inspect" | "track.list" | "track.inspect" | "clip.list"
            | "clip.inspect" | "asset.list" | "asset.inspect" | "asset.missing"
            | "transition.list" | "transition.inspect" | "effect.list" | "effect.inspect"
            | "keyframe.list" | "marker.list" => {
                self.read_entities(op, a, &pid, revision, p, sequence.as_deref())
            }
            "search" => self.search(a, &pid, revision, p, sequence.as_deref()),
            _ => {
                let edit = self.make_edit(op, a, &pid, revision, p, sequence.as_deref())?;
                let mut seed = a.object()?.clone();
                seed.remove("preview");
                seed.remove("allow_metadata_risk");
                let seed = format!("{op}:{}", Value::Object(seed).encode());
                let plan = edit::plan(
                    p,
                    revision,
                    edit,
                    &seed,
                    a.flag("allow_metadata_risk", false)?,
                )?;
                let preview = a.flag("preview", false)?;
                let mut value = plan.json(!preview);
                let mut created_refs = vec![];
                let current = if preview {
                    revision.clone()
                } else {
                    let new_revision = plan.resulting_revision.clone();
                    self.projects.insert(
                        pid.clone(),
                        Loaded {
                            project: plan.result.clone(),
                            revision: new_revision.clone(),
                            source: loaded.source.clone(),
                        },
                    );
                    self.refs.invalidate(&pid);
                    for id in &plan.created {
                        let kind = kind_from_id(id)?;
                        created_refs.push(obj([
                            ("id", id.clone().into()),
                            ("kind", kind.into()),
                            (
                                "reference",
                                self.refs.issue(&pid, &new_revision, kind, id)?.into(),
                            ),
                        ]));
                    }
                    new_revision
                };
                if let Value::Object(ref mut m) = value {
                    m.insert(
                        "project".into(),
                        self.refs
                            .issue(&pid, &current, "project", "project")?
                            .into(),
                    );
                    m.insert("created_refs".into(), array(created_refs));
                }
                Ok(value)
            }
        }
    }
    fn effect_info(&self, service: &str) -> Value {
        let (min, max) = if service == "volume" {
            (-60000, 24000)
        } else {
            (0, 2000)
        };
        obj([
            ("service", service.into()),
            ("property", "level".into()),
            ("minimum_milli", (min as i64).into()),
            ("maximum_milli", (max as i64).into()),
            (
                "available",
                self.runtime
                    .as_ref()
                    .is_some_and(|r| r.catalog.has("filters", service))
                    .into(),
            ),
            ("metadata_untrusted", true.into()),
        ])
    }
    fn sequence_dto(&mut self, pid: &str, revision: &str, s: &Sequence) -> Result<Value> {
        Ok(obj([
            (
                "reference",
                self.refs.issue(pid, revision, "sequence", &s.id)?.into(),
            ),
            ("id", display(&s.id).into()),
            ("name", display(&s.name).into()),
            ("duration", s.duration().into()),
            ("track_count", s.tracks.len().into()),
            (
                "nested_sequences",
                array(s.nested.iter().map(|s| display(s).into())),
            ),
            ("opaque", s.opaque.into()),
        ]))
    }
    fn track_dto(&mut self, pid: &str, revision: &str, t: &Track) -> Result<Value> {
        Ok(obj([
            (
                "reference",
                self.refs.issue(pid, revision, "track", &t.id)?.into(),
            ),
            ("id", display(&t.id).into()),
            ("name", display(&t.name).into()),
            ("kind", display(&t.kind).into()),
            ("muted", t.muted.into()),
            ("hidden", t.hidden.into()),
            ("lane_count", t.lanes.len().into()),
            (
                "clip_count",
                t.lanes.iter().map(|l| l.clips.len()).sum::<usize>().into(),
            ),
            ("duration", t.duration().into()),
            ("opaque", t.opaque.into()),
        ]))
    }
    fn clip_dto(&mut self, pid: &str, revision: &str, c: &Clip) -> Result<Value> {
        Ok(obj([
            (
                "reference",
                self.refs.issue(pid, revision, "clip", &c.id)?.into(),
            ),
            ("id", display(&c.id).into()),
            ("name", display(&c.name).into()),
            ("asset", display(&c.asset).into()),
            ("start", c.start.into()),
            ("duration", c.duration().into()),
            ("source_in", c.source.start.0.into()),
            ("source_out", c.source.end.0.into()),
            ("effect_count", c.effects.len().into()),
            ("speed_num", u64::from(c.speed.0).into()),
            ("speed_den", u64::from(c.speed.1).into()),
        ]))
    }
    fn asset_dto(&mut self, pid: &str, revision: &str, a: &MediaAsset) -> Result<Value> {
        Ok(obj([
            (
                "reference",
                self.refs.issue(pid, revision, "asset", &a.id)?.into(),
            ),
            ("id", display(&a.id).into()),
            ("name", display(&a.name).into()),
            ("kind", display(&a.kind).into()),
            ("resource", display(&a.resource.text()).into()),
            ("resource_status", a.resource.status().into()),
            ("frames", a.frames.map_or(Value::Null, Into::into)),
            ("service", display(&a.service).into()),
            (
                "original",
                a.original
                    .as_ref()
                    .map_or(Value::Null, |s| display(s).into()),
            ),
            (
                "proxy",
                a.proxy.as_ref().map_or(Value::Null, |s| display(s).into()),
            ),
            ("opaque", a.opaque.into()),
        ]))
    }
    fn effect_dto(&mut self, pid: &str, revision: &str, e: &Effect) -> Result<Value> {
        Ok(obj([
            (
                "reference",
                self.refs.issue(pid, revision, "effect", &e.id)?.into(),
            ),
            ("id", display(&e.id).into()),
            ("service", display(&e.service).into()),
            ("property", display(&e.property).into()),
            (
                "value_milli",
                if e.opaque.is_some() {
                    Value::Null
                } else {
                    e.value.into()
                },
            ),
            ("enabled", e.enabled.into()),
            ("keyframe_count", e.keyframes.len().into()),
            ("opaque", e.opaque.is_some().into()),
        ]))
    }
    fn transition_dto(&mut self, pid: &str, revision: &str, t: &Transition) -> Result<Value> {
        Ok(obj([
            (
                "reference",
                self.refs.issue(pid, revision, "transition", &t.id)?.into(),
            ),
            ("id", display(&t.id).into()),
            ("kind", display(&t.kind).into()),
            ("a_track", display(&t.a_track).into()),
            ("b_track", display(&t.b_track).into()),
            ("start", t.range.start.0.into()),
            ("end", t.range.end.0.into()),
            ("reverse", t.reverse.into()),
            ("opaque", t.opaque.is_some().into()),
        ]))
    }
    fn marker_dto(&mut self, pid: &str, revision: &str, m: &Marker) -> Result<Value> {
        Ok(obj([
            (
                "reference",
                self.refs.issue(pid, revision, "marker", &m.id)?.into(),
            ),
            ("id", display(&m.id).into()),
            ("frame", m.frame.into()),
            ("label", display(&m.label).into()),
            ("tags", array(m.tags.iter().map(|s| display(s).into()))),
        ]))
    }
    fn read_entities(
        &mut self,
        op: &str,
        a: &Value,
        pid: &str,
        revision: &str,
        p: &Project,
        sequence: Option<&str>,
    ) -> Result<Value> {
        let (offset, limit) = pagination(a, revision)?;
        let mut items = vec![];
        let total;
        match op {
            "sequence.list" | "sequence.inspect" => {
                let list: Vec<_> = p
                    .sequences
                    .iter()
                    .filter(|s| sequence.is_none_or(|id| s.id == id))
                    .collect();
                total = list.len();
                for s in list.into_iter().skip(offset).take(limit) {
                    items.push(self.sequence_dto(pid, revision, s)?);
                }
            }
            "asset.list" | "asset.inspect" | "asset.missing" => {
                let target = a
                    .opt("asset")
                    .map(|_| self.resolve(a, "asset", pid, revision, "asset"))
                    .transpose()?;
                let list: Vec<_> = p
                    .assets
                    .values()
                    .filter(|s| target.as_ref().is_none_or(|id| &s.id == id))
                    .collect();
                total = list.len();
                for asset in list.into_iter().skip(offset).take(limit) {
                    if op == "asset.missing" {
                        let (status, accessed) = match &asset.resource {
                            Resource::Color(_) => ("generator", false),
                            Resource::Opaque(_) | Resource::External(_) => ("unresolved", false),
                            _ => {
                                match jobs::media_location(p, asset).and_then(|(root, path)| {
                                    self.roots
                                        .get(&root)
                                        .ok_or_else(|| {
                                            Error::new("PermissionDenied", "Root absent")
                                        })?
                                        .exists(&path)
                                }) {
                                    Ok(true) => ("available", true),
                                    Ok(false) => ("missing", false),
                                    Err(_) => ("denied", false),
                                }
                            }
                        };
                        items.push(obj([
                            ("asset", display(&asset.id).into()),
                            ("status", status.into()),
                            ("resource_accessed", accessed.into()),
                        ]));
                    } else {
                        items.push(self.asset_dto(pid, revision, asset)?);
                    }
                }
            }
            "track.list" | "track.inspect" => {
                let seq =
                    p.sequence(sequence.ok_or_else(|| Error::invalid("Sequence required"))?)?;
                let target = a
                    .opt("track")
                    .map(|_| self.resolve(a, "track", pid, revision, "track"))
                    .transpose()?;
                let list: Vec<_> = seq
                    .tracks
                    .iter()
                    .filter(|t| target.as_ref().is_none_or(|id| &t.id == id))
                    .collect();
                total = list.len();
                for t in list.into_iter().skip(offset).take(limit) {
                    items.push(self.track_dto(pid, revision, t)?);
                }
            }
            "clip.list" | "clip.inspect" => {
                let seq =
                    p.sequence(sequence.ok_or_else(|| Error::invalid("Sequence required"))?)?;
                let target = a
                    .opt("clip")
                    .map(|_| self.resolve(a, "clip", pid, revision, "clip"))
                    .transpose()?;
                let track = a
                    .opt("track")
                    .map(|_| self.resolve(a, "track", pid, revision, "track"))
                    .transpose()?;
                let list: Vec<_> = seq
                    .tracks
                    .iter()
                    .filter(|t| track.as_ref().is_none_or(|id| &t.id == id))
                    .flat_map(|t| &t.lanes)
                    .flat_map(|l| &l.clips)
                    .filter(|c| target.as_ref().is_none_or(|id| &c.id == id))
                    .collect();
                total = list.len();
                for c in list.into_iter().skip(offset).take(limit) {
                    items.push(self.clip_dto(pid, revision, c)?);
                }
            }
            "transition.list" | "transition.inspect" => {
                let seq =
                    p.sequence(sequence.ok_or_else(|| Error::invalid("Sequence required"))?)?;
                let target = a
                    .opt("transition")
                    .map(|_| self.resolve(a, "transition", pid, revision, "transition"))
                    .transpose()?;
                let list: Vec<_> = seq
                    .transitions
                    .iter()
                    .filter(|t| target.as_ref().is_none_or(|id| &t.id == id))
                    .collect();
                total = list.len();
                for t in list.into_iter().skip(offset).take(limit) {
                    items.push(self.transition_dto(pid, revision, t)?);
                }
            }
            "effect.list" | "effect.inspect" | "keyframe.list" => {
                let seq = sequence.ok_or_else(|| Error::invalid("Sequence required"))?;
                let clip = self.resolve(a, "clip", pid, revision, "clip")?;
                let c = p.clip(seq, &clip)?;
                let target = a
                    .opt("effect")
                    .map(|_| self.resolve(a, "effect", pid, revision, "effect"))
                    .transpose()?;
                if op == "keyframe.list" {
                    let e = c
                        .effects
                        .iter()
                        .find(|e| target.as_ref() == Some(&e.id))
                        .ok_or_else(|| Error::new("NotFound", "Effect missing"))?;
                    if e.opaque.is_some() {
                        return Err(Error::unsupported("Opaque effect animation"));
                    }
                    total = e.keyframes.len();
                    for k in e.keyframes.iter().skip(offset).take(limit) {
                        items.push(obj([
                            ("frame", k.frame.into()),
                            ("value_milli", k.value.into()),
                            ("interpolation", k.interpolation.name().into()),
                        ]));
                    }
                } else {
                    let list: Vec<_> = c
                        .effects
                        .iter()
                        .filter(|e| target.as_ref().is_none_or(|id| &e.id == id))
                        .collect();
                    total = list.len();
                    for e in list.into_iter().skip(offset).take(limit) {
                        items.push(self.effect_dto(pid, revision, e)?);
                    }
                }
            }
            "marker.list" => {
                let seq =
                    p.sequence(sequence.ok_or_else(|| Error::invalid("Sequence required"))?)?;
                total = seq.markers.len();
                for m in seq.markers.iter().skip(offset).take(limit) {
                    items.push(self.marker_dto(pid, revision, m)?);
                }
            }
            _ => return Err(Error::new("NotFound", "Read operation missing")),
        }
        Ok(page(revision, items, total, offset, limit, &p.warnings))
    }
    fn make_edit(
        &self,
        op: &str,
        a: &Value,
        pid: &str,
        revision: &str,
        p: &Project,
        sequence: Option<&str>,
    ) -> Result<Edit> {
        let s = || {
            sequence
                .map(str::to_owned)
                .ok_or_else(|| Error::invalid("Explicit sequence reference required"))
        };
        let r = |key: &str, kind: &str| self.resolve(a, key, pid, revision, kind);
        let source = || -> Result<FrameRange> {
            Ok(FrameRange::new(
                a.uint("source_in")?,
                a.uint("source_out")?,
            )?)
        };
        let range =
            || -> Result<FrameRange> { Ok(FrameRange::new(a.uint("start")?, a.uint("end")?)?) };
        let value = || a.get("value_milli")?.i64();
        Ok(match op {
            "project.profile.set" => Edit::Profile(parse_profile(a.get("profile")?)?),
            "sequence.create" => Edit::SequenceCreate {
                name: a.str("name")?.into(),
            },
            "asset.import" => Edit::AssetImport {
                asset: self.import_asset(p, a)?,
            },
            "asset.relink" => {
                let asset = r("asset", "asset")?;
                let (old_frames, old_kind) = {
                    let old = p
                        .assets
                        .get(&asset)
                        .ok_or_else(|| Error::new("NotFound", "Asset not found"))?;
                    (old.frames, old.kind.clone())
                };
                let info = self.probe_resource(a.str("root")?, a.str("path")?)?;
                let frames = media_frames(&info, p.profile.fps)?;
                if old_frames.is_some_and(|old| frames < old) || old_kind == "image" {
                    return Err(Error::unsupported(
                        "Relink target has insufficient duration or unsupported image semantics",
                    ));
                }
                Edit::AssetRelink {
                    id: asset,
                    expected: a.str("expected_resource")?.into(),
                    resource: Resource::Scoped {
                        root: a.str("root")?.into(),
                        path: a.str("path")?.into(),
                    },
                }
            }
            "track.create" => Edit::TrackCreate {
                sequence: s()?,
                name: a.str("name")?.into(),
                kind: a.str("kind")?.into(),
            },
            "track.remove" => Edit::TrackRemove {
                sequence: s()?,
                track: r("track", "track")?,
            },
            "track.rename" => Edit::TrackRename {
                sequence: s()?,
                track: r("track", "track")?,
                name: a.str("name")?.into(),
            },
            "track.mute" => Edit::TrackMute {
                sequence: s()?,
                track: r("track", "track")?,
                value: a.get("value")?.boolean()?,
            },
            "track.hide" => Edit::TrackHide {
                sequence: s()?,
                track: r("track", "track")?,
                value: a.get("value")?.boolean()?,
            },
            "track.reorder" => Edit::TrackReorder {
                sequence: s()?,
                track: r("track", "track")?,
                index: a.uint("index")? as usize,
            },
            "clip.insert" => Edit::Insert {
                sequence: s()?,
                track: r("track", "track")?,
                asset: r("asset", "asset")?,
                start: a.uint("start")?,
                source: source()?,
                name: a.str("name")?.into(),
                ripple: a.flag("ripple", false)?,
            },
            "clip.move" => Edit::Move {
                sequence: s()?,
                clip: r("clip", "clip")?,
                track: r("track", "track")?,
                start: a.uint("start")?,
            },
            "clip.duplicate" => Edit::Duplicate {
                sequence: s()?,
                clip: r("clip", "clip")?,
                track: r("track", "track")?,
                start: a.uint("start")?,
            },
            "clip.trim" => Edit::Trim {
                sequence: s()?,
                clip: r("clip", "clip")?,
                source: source()?,
            },
            "clip.split" => Edit::Split {
                sequence: s()?,
                clip: r("clip", "clip")?,
                at: a.uint("at")?,
            },
            "clip.remove" => Edit::Remove {
                sequence: s()?,
                clip: r("clip", "clip")?,
                ripple: a.flag("ripple", false)?,
            },
            "transition.add" => Edit::TransitionAdd {
                sequence: s()?,
                kind: a.str("kind")?.into(),
                a_track: r("a_track", "track")?,
                b_track: r("b_track", "track")?,
                range: range()?,
            },
            "transition.patch" => Edit::TransitionPatch {
                sequence: s()?,
                id: r("transition", "transition")?,
                range: range()?,
                reverse: a.get("reverse")?.boolean()?,
            },
            "transition.remove" => Edit::TransitionRemove {
                sequence: s()?,
                id: r("transition", "transition")?,
            },
            "effect.add" => Edit::EffectAdd {
                sequence: s()?,
                clip: r("clip", "clip")?,
                service: a.str("service")?.into(),
                value: value()?,
            },
            "effect.patch" => Edit::EffectPatch {
                sequence: s()?,
                clip: r("clip", "clip")?,
                id: r("effect", "effect")?,
                value: value()?,
            },
            "effect.remove" => Edit::EffectRemove {
                sequence: s()?,
                clip: r("clip", "clip")?,
                id: r("effect", "effect")?,
            },
            "effect.enable" | "effect.disable" => Edit::EffectEnable {
                sequence: s()?,
                clip: r("clip", "clip")?,
                id: r("effect", "effect")?,
                value: op == "effect.enable",
            },
            "keyframe.set" => Edit::KeyframeSet {
                sequence: s()?,
                clip: r("clip", "clip")?,
                effect: r("effect", "effect")?,
                keyframe: Keyframe {
                    frame: a.uint("frame")?,
                    value: value()?,
                    interpolation: if a.str("interpolation")? == "hold" {
                        Interpolation::Hold
                    } else {
                        Interpolation::Linear
                    },
                },
            },
            "keyframe.remove" => Edit::KeyframeRemove {
                sequence: s()?,
                clip: r("clip", "clip")?,
                effect: r("effect", "effect")?,
                frame: a.uint("frame")?,
            },
            "marker.add" => Edit::MarkerAdd {
                sequence: s()?,
                frame: a.uint("frame")?,
                label: a.str("label")?.into(),
            },
            "marker.patch" => Edit::MarkerPatch {
                sequence: s()?,
                id: r("marker", "marker")?,
                frame: a.uint("frame")?,
                label: a.str("label")?.into(),
            },
            "marker.remove" => Edit::MarkerRemove {
                sequence: s()?,
                id: r("marker", "marker")?,
            },
            "audio.volume.set" => Edit::AudioVolume {
                sequence: s()?,
                clip: r("clip", "clip")?,
                value: value()?,
            },
            "audio.fade_in" | "audio.fade_out" => Edit::AudioFade {
                sequence: s()?,
                clip: r("clip", "clip")?,
                frames: a.uint("frames")?,
                fade_in: op == "audio.fade_in",
            },
            _ => return Err(Error::new("NotFound", "Capability implementation missing")),
        })
    }
    fn import_asset(&self, p: &Project, a: &Value) -> Result<MediaAsset> {
        let name = a.str("name")?.to_owned();
        let (kind, resource, frames, service) = if a.str("kind")? == "color" {
            if a.opt("root").is_some() || a.opt("path").is_some() || a.opt("image_frames").is_some()
            {
                return Err(Error::invalid("Color generators cannot contain file paths"));
            }
            let color = a.str("color")?;
            if !jobs::valid_color(color) {
                return Err(Error::invalid("Unsupported color token"));
            }
            let frames = a.uint("frames")?;
            (
                "color".to_owned(),
                Resource::Color(color.into()),
                frames,
                "color".to_owned(),
            )
        } else {
            if a.opt("color").is_some() || a.opt("frames").is_some() {
                return Err(Error::invalid(
                    "File duration must come from a confined media probe",
                ));
            }
            let root = a.str("root")?;
            let path = a.str("path")?;
            let info = self.probe_resource(root, path)?;
            let still = info.duration_num == 0
                && info.video
                && info
                    .codecs
                    .iter()
                    .any(|c| matches!(c.as_str(), "png" | "mjpeg"));
            let (frames, kind, service) = if still {
                let f = a.uint("image_frames")?;
                if !self
                    .runtime
                    .as_ref()
                    .is_some_and(|r| r.catalog.has("producers", "pixbuf"))
                {
                    return Err(Error::new("Unavailable", "Image producer is unavailable"));
                }
                (f, "image", "pixbuf")
            } else {
                if a.opt("image_frames").is_some() {
                    return Err(Error::invalid(
                        "image_frames only applies to a probed still image",
                    ));
                }
                (
                    media_frames(&info, p.profile.fps)?,
                    if info.video { "video" } else { "audio" },
                    "avformat",
                )
            };
            (
                kind.into(),
                Resource::Scoped {
                    root: root.into(),
                    path: path.into(),
                },
                frames,
                service.into(),
            )
        };
        if frames == 0 || frames > crate::time::MAX_FRAME {
            return Err(Error::limit("Asset duration must be positive and bounded"));
        }
        Ok(MediaAsset {
            id: String::new(),
            name,
            kind,
            resource,
            frames: Some(frames),
            service,
            original: None,
            proxy: None,
            opaque: false,
        })
    }
    fn frames_encode(&self, a: &Value) -> Result<Value> {
        a.strict(
            &[
                "root",
                "manifest_path",
                "expected_manifest_sha256",
                "output_path",
                "max_bytes",
            ],
            &[
                "root",
                "manifest_path",
                "expected_manifest_sha256",
                "output_path",
                "max_bytes",
            ],
        )?;
        let root_name = a.str("root")?;
        if !matches!(root_name, "project" | "media" | "output") {
            return Err(Error::new(
                "PermissionDenied",
                "Motion frame manifest root is not granted",
            ));
        }
        let manifest_path = a.str("manifest_path")?;
        crate::fs::validate_relative(manifest_path)?;
        let expected_manifest_sha256 = a.str("expected_manifest_sha256")?;
        if !valid_digest(expected_manifest_sha256) {
            return Err(Error::invalid("Motion frame manifest digest is malformed"));
        }
        let output_path = a.str("output_path")?;
        crate::fs::validate_relative(output_path)?;
        if !output_path.ends_with(".mkv") {
            return Err(Error::invalid("Motion mezzanine output must use .mkv"));
        }
        let max_bytes = a.uint("max_bytes")?;
        if max_bytes == 0 || max_bytes > jobs::MAX_ARTIFACT_BYTES {
            return Err(Error::limit("Motion mezzanine output byte budget"));
        }
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            Error::new(
                "Unavailable",
                "Frame encoding requires the pinned confined ffmpeg runtime",
            )
        })?;
        let mount = self
            .roots
            .get(root_name)
            .ok_or_else(|| Error::new("PermissionDenied", "Motion frame root absent"))?;
        let manifest_bytes = mount.read(manifest_path, 8 * 1024 * 1024)?;
        if sha256(&manifest_bytes) != expected_manifest_sha256 {
            return Err(Error::new(
                "StaleReference",
                "Motion frame manifest digest changed before encoding",
            ));
        }
        let manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)
            .map_err(|_| Error::invalid("Motion frame manifest is malformed"))?;
        let plan = manifest
            .get("plan")
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| Error::invalid("Motion frame manifest omitted render plan"))?;
        let required_u64 = |name: &str| -> Result<u64> {
            plan.get(name)
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| Error::invalid(format!("Motion render plan omitted {name}")))
        };
        let width = u32::try_from(required_u64("width")?)
            .map_err(|_| Error::limit("Motion render width"))?;
        let height = u32::try_from(required_u64("height")?)
            .map_err(|_| Error::limit("Motion render height"))?;
        let fps_num = u32::try_from(required_u64("fps")?)
            .map_err(|_| Error::limit("Motion render fps numerator"))?;
        let fps_den = u32::try_from(
            plan.get("fps_denominator")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1),
        )
        .map_err(|_| Error::limit("Motion render fps denominator"))?;
        let first_frame = required_u64("first_frame")?;
        let frame_count = required_u64("frame_count")?;
        if frame_count == 0
            || frame_count > 36_000
            || first_frame
                .checked_add(frame_count)
                .is_none_or(|end| end > 100_000_000)
        {
            return Err(Error::limit("Motion frame manifest count/range"));
        }
        let frames = manifest
            .get("frames")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| Error::invalid("Motion frame manifest omitted frames"))?;
        if frames.len() as u64 != frame_count {
            return Err(Error::new(
                "Conflict",
                "Motion frame manifest count does not match its render plan",
            ));
        }
        let parent = manifest_path
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("");
        let inputs = PrivateDir::new(Path::new("/tmp"))?;
        let work = PrivateDir::new(Path::new("/tmp"))?;
        let mut total = 0u64;
        for (index, frame) in frames.iter().enumerate() {
            let index_u64 = index as u64;
            if frame.get("index").and_then(serde_json::Value::as_u64) != Some(index_u64) {
                return Err(Error::invalid(
                    "Motion frame manifest index is not contiguous",
                ));
            }
            let number = first_frame
                .checked_add(index_u64)
                .ok_or_else(|| Error::limit("Motion frame number overflow"))?;
            let expected_file = format!("frames/{number:06}.png");
            let file = frame
                .get("file")
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| Error::invalid("Motion frame manifest omitted file"))?;
            if file != expected_file {
                return Err(Error::invalid(
                    "Motion frame manifest contains an unexpected basename",
                ));
            }
            let expected_bytes = frame
                .get("bytes")
                .and_then(serde_json::Value::as_u64)
                .filter(|bytes| *bytes > 8 && *bytes <= 32 * 1024 * 1024)
                .ok_or_else(|| Error::limit("Motion PNG byte budget"))?;
            let expected_sha256 = frame
                .get("sha256")
                .and_then(serde_json::Value::as_str)
                .filter(|digest| valid_digest(digest))
                .ok_or_else(|| Error::invalid("Motion PNG digest is malformed"))?;
            let source_path = if parent.is_empty() {
                file.to_owned()
            } else {
                format!("{parent}/{file}")
            };
            crate::fs::validate_relative(&source_path)?;
            let mut source = mount.read_file(&source_path, expected_bytes)?;
            let destination_name = format!("{number:06}.png");
            let mut destination = inputs.create(&destination_name)?;
            let mut hasher = Sha256::new();
            let mut size = 0u64;
            let mut signature = Vec::with_capacity(8);
            let mut buffer = [0u8; 65_536];
            loop {
                let count = source.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                if signature.len() < 8 {
                    let needed = 8 - signature.len();
                    signature.extend_from_slice(&buffer[..count.min(needed)]);
                }
                size = size
                    .checked_add(count as u64)
                    .ok_or_else(|| Error::limit("Motion PNG size overflow"))?;
                total = total
                    .checked_add(count as u64)
                    .ok_or_else(|| Error::limit("Motion frame-set size overflow"))?;
                if size > expected_bytes || total > jobs::MAX_JOB_INPUT_BYTES {
                    return Err(Error::limit("Motion frame-set staging budget"));
                }
                hasher.update(&buffer[..count]);
                destination.write_all(&buffer[..count])?;
            }
            destination.sync_all()?;
            drop(destination);
            if size != expected_bytes
                || hasher.finish() != expected_sha256
                || signature.as_slice() != b"\x89PNG\r\n\x1a\n"
            {
                return Err(Error::new(
                    "StaleReference",
                    "Motion PNG bytes no longer match the verified render manifest",
                ));
            }
            inputs.seal(&destination_name)?;
        }
        let media = runtime.encode_frames(
            inputs.path(),
            work.path(),
            first_frame,
            frame_count,
            fps_num,
            fps_den,
            width,
            height,
            &AtomicBool::new(false),
        )?;
        let output = self
            .roots
            .get("output")
            .ok_or_else(|| Error::new("Unavailable", "Output mount is absent"))?;
        let source = Root::open(work.path(), true, false)?.read_file("mezzanine.mkv", max_bytes)?;
        let artifact = output.publish("output", output_path, source, max_bytes)?;
        Ok(obj([
            ("artifact", artifact.json()),
            ("source_manifest_sha256", expected_manifest_sha256.into()),
            ("codec", "ffv1".into()),
            ("container", "matroska".into()),
            ("frame_count", frame_count.into()),
            ("width", u64::from(width).into()),
            ("height", u64::from(height).into()),
            ("fps_num", u64::from(fps_num).into()),
            ("fps_den", u64::from(fps_den).into()),
            ("media", media.json()),
        ]))
    }

    fn sync_probe(&self, a: &Value) -> Result<Value> {
        a.strict(
            &["root", "path", "expected_sha256", "window_us", "cues"],
            &["root", "path", "expected_sha256", "window_us", "cues"],
        )?;
        let root_name = a.str("root")?;
        if !matches!(root_name, "project" | "media" | "output") {
            return Err(Error::new(
                "PermissionDenied",
                "Sync media root not granted",
            ));
        }
        let path = a.str("path")?;
        let expected_sha256 = a.str("expected_sha256")?;
        if !valid_digest(expected_sha256) {
            return Err(Error::invalid("Sync artifact SHA-256 is malformed"));
        }
        let window_us = a.uint("window_us")?;
        if !(crate::sync::MIN_WINDOW_US..=crate::sync::MAX_WINDOW_US).contains(&window_us) {
            return Err(Error::invalid(
                "Sync cue window is outside supported bounds",
            ));
        }
        let cue_values = a.get("cues")?.as_array()?;
        if cue_values.is_empty() || cue_values.len() > crate::sync::MAX_SYNC_CUES {
            return Err(Error::limit("Sync cue count must be 1..16"));
        }
        let mut seen = std::collections::BTreeSet::new();
        let mut cues = Vec::with_capacity(cue_values.len());
        for cue in cue_values {
            cue.strict(&["id", "expected_us"], &["id", "expected_us"])?;
            let id = cue.str("id")?;
            let expected_us = cue.uint("expected_us")?;
            if !crate::sync::valid_cue_id(id)
                || expected_us > 600_000_000
                || !seen.insert(id.to_owned())
            {
                return Err(Error::invalid("Sync cue ID/time is invalid or duplicated"));
            }
            cues.push(crate::sync::CueWindow {
                id: id.to_owned(),
                expected_us,
            });
        }
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            Error::new(
                "Unavailable",
                "Sync probing requires the pinned confined ffprobe runtime",
            )
        })?;
        let mount = self
            .roots
            .get(root_name)
            .ok_or_else(|| Error::new("PermissionDenied", "Sync media mount absent"))?;
        let mut source = mount.read_file(path, jobs::MAX_MEDIA_BYTES)?;
        let inputs = PrivateDir::new(Path::new("/tmp"))?;
        let work = PrivateDir::new(Path::new("/tmp"))?;
        let mut destination = inputs.create("sync-media.bin")?;
        let mut hasher = Sha256::new();
        let mut copied = 0u64;
        let mut buffer = [0u8; 65_536];
        loop {
            let count = source.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            copied = copied
                .checked_add(count as u64)
                .ok_or_else(|| Error::limit("Sync media size overflow"))?;
            if copied > jobs::MAX_MEDIA_BYTES {
                return Err(Error::limit("Sync media exceeds staging budget"));
            }
            hasher.update(&buffer[..count]);
            destination.write_all(&buffer[..count])?;
        }
        destination.flush()?;
        destination.sync_all()?;
        drop(destination);
        let actual_sha256 = hasher.finish();
        if actual_sha256 != expected_sha256 {
            return Err(Error::new(
                "StaleReference",
                "Sync media digest changed before decode",
            ));
        }
        inputs.seal("sync-media.bin")?;
        let result = runtime.sync_probe(
            inputs.path(),
            work.path(),
            "sync-media.bin",
            &cues,
            window_us,
            &AtomicBool::new(false),
        )?;
        let detection = |value: crate::sync::Detection| {
            obj([
                ("cue_id", value.cue_id.into()),
                ("presentation_time_us", value.presentation_time_us.into()),
                ("uncertainty_us", value.uncertainty_us.into()),
                ("confidence", u64::from(value.confidence).into()),
            ])
        };
        Ok(obj([
            ("artifact_sha256", actual_sha256.into()),
            ("decoder", "ffprobe-lavfi-sync-v1".into()),
            ("decoder_sha256", runtime.ffprobe.sha256.clone().into()),
            ("coverage", "cue_windows".into()),
            ("window_us", window_us.into()),
            ("flashes", array(result.flashes.into_iter().map(detection))),
            (
                "impulses",
                array(result.impulses.into_iter().map(detection)),
            ),
            (
                "missing_video",
                array(result.missing_video.into_iter().map(Into::into)),
            ),
            (
                "missing_audio",
                array(result.missing_audio.into_iter().map(Into::into)),
            ),
        ]))
    }

    fn probe_resource(&self, root: &str, path: &str) -> Result<crate::runtime::MediaInfo> {
        if !matches!(root, "project" | "media" | "output") {
            return Err(Error::new("PermissionDenied", "Media root not granted"));
        }
        let runtime = self.runtime.as_ref().ok_or_else(|| {
            Error::new(
                "Unavailable",
                "File import requires a pinned confined ffprobe/MLT runtime",
            )
        })?;
        let mount = self
            .roots
            .get(root)
            .ok_or_else(|| Error::new("PermissionDenied", "Media mount absent"))?;
        let mut file = mount.read_file(path, jobs::MAX_MEDIA_BYTES)?;
        let inputs = PrivateDir::new(Path::new("/tmp"))?;
        let work = PrivateDir::new(Path::new("/tmp"))?;
        let mut dest = inputs.create("probe.bin")?;
        let copied = std::io::copy(&mut (&mut file).take(jobs::MAX_MEDIA_BYTES + 1), &mut dest)?;
        if copied > jobs::MAX_MEDIA_BYTES {
            return Err(Error::limit("Media grew during staging"));
        }
        dest.flush()?;
        dest.sync_all()?;
        drop(dest);
        inputs.seal("probe.bin")?;
        runtime.probe(
            inputs.path(),
            work.path(),
            "probe.bin",
            &AtomicBool::new(false),
        )
    }
    fn search(
        &mut self,
        a: &Value,
        pid: &str,
        revision: &str,
        p: &Project,
        sequence: Option<&str>,
    ) -> Result<Value> {
        struct Hit {
            kind: &'static str,
            id: String,
            name: String,
            sequence: Option<String>,
            track: Option<String>,
            start: Option<u64>,
            duration: Option<u64>,
            haystack: String,
        }
        let query = a.str("query")?.to_lowercase();
        let words: Vec<_> = query.split_whitespace().collect();
        let kind = a.opt("kind").map(Value::string).transpose()?;
        let track = a
            .opt("track")
            .map(|_| self.resolve(a, "track", pid, revision, "track"))
            .transpose()?;
        let start = a.opt("start").map(Value::u64).transpose()?;
        let end = a.opt("end").map(Value::u64).transpose()?;
        if start.zip(end).is_some_and(|(s, e)| s >= e) {
            return Err(Error::invalid("Search interval must be ordered"));
        }
        let mut hits = vec![];
        if sequence.is_none() && track.is_none() {
            for asset in p.assets.values() {
                hits.push(Hit {
                    kind: "asset",
                    id: asset.id.clone(),
                    name: asset.name.clone(),
                    sequence: None,
                    track: None,
                    start: None,
                    duration: None,
                    haystack: format!("{} {} {}", asset.name, asset.resource.text(), asset.kind)
                        .to_lowercase(),
                });
            }
        }
        for s in p
            .sequences
            .iter()
            .filter(|s| sequence.is_none_or(|id| s.id == id))
        {
            for t in s
                .tracks
                .iter()
                .filter(|t| track.as_ref().is_none_or(|id| &t.id == id))
            {
                hits.push(Hit {
                    kind: "track",
                    id: t.id.clone(),
                    name: t.name.clone(),
                    sequence: Some(s.id.clone()),
                    track: Some(t.id.clone()),
                    start: Some(0),
                    duration: Some(t.duration()),
                    haystack: format!("{} {}", t.name, t.kind).to_lowercase(),
                });
                for l in &t.lanes {
                    for c in &l.clips {
                        hits.push(Hit {
                            kind: "clip",
                            id: c.id.clone(),
                            name: c.name.clone(),
                            sequence: Some(s.id.clone()),
                            track: Some(t.id.clone()),
                            start: Some(c.start),
                            duration: Some(c.duration()),
                            haystack: c.name.to_lowercase(),
                        });
                        for e in &c.effects {
                            hits.push(Hit {
                                kind: "effect",
                                id: e.id.clone(),
                                name: e.service.clone(),
                                sequence: Some(s.id.clone()),
                                track: Some(t.id.clone()),
                                start: Some(c.start),
                                duration: Some(c.duration()),
                                haystack: e.service.to_lowercase(),
                            });
                        }
                    }
                }
            }
            if track.is_none() {
                for m in &s.markers {
                    hits.push(Hit {
                        kind: "marker",
                        id: m.id.clone(),
                        name: m.label.clone(),
                        sequence: Some(s.id.clone()),
                        track: None,
                        start: Some(m.frame),
                        duration: Some(1),
                        haystack: format!("{} {}", m.label, m.tags.join(" ")).to_lowercase(),
                    });
                }
            }
        }
        hits.retain(|h| {
            kind.is_none_or(|k| h.kind == k)
                && words.iter().all(|w| h.haystack.contains(w))
                && start.is_none_or(|s| {
                    h.start
                        .zip(h.duration)
                        .is_some_and(|(p, d)| p.saturating_add(d) > s)
                })
                && end.is_none_or(|e| h.start.is_some_and(|p| p < e))
        });
        hits.sort_by(|a, b| (a.kind, &a.name, &a.id).cmp(&(b.kind, &b.name, &b.id)));
        let total = hits.len();
        let (offset, limit) = pagination(a, revision)?;
        let mut items = vec![];
        for h in hits.into_iter().skip(offset).take(limit) {
            items.push(obj([
                ("kind", h.kind.into()),
                (
                    "reference",
                    self.refs.issue(pid, revision, h.kind, &h.id)?.into(),
                ),
                ("id", display(&h.id).into()),
                ("name", display(&h.name).into()),
                ("sequence_id", h.sequence.map_or(Value::Null, Into::into)),
                ("track_id", h.track.map_or(Value::Null, Into::into)),
                ("start", h.start.map_or(Value::Null, Into::into)),
                ("duration", h.duration.map_or(Value::Null, Into::into)),
            ]));
        }
        Ok(page(revision, items, total, offset, limit, &p.warnings))
    }
}
fn warnings(w: &[String]) -> Value {
    array(w.iter().take(64).map(|s| display(s).into()))
}
fn pagination(a: &Value, revision: &str) -> Result<(usize, usize)> {
    let limit = a.opt("limit").map(Value::u64).transpose()?.unwrap_or(100) as usize;
    if !(1..=100).contains(&limit) {
        return Err(Error::invalid("Page limit must be 1..100"));
    }
    let offset = if let Some(cursor) = a.opt("cursor") {
        let (hash, n) = cursor.string()?.split_once(':').ok_or_else(Error::stale)?;
        if hash != revision {
            return Err(Error::stale());
        }
        n.parse::<usize>().map_err(|_| Error::stale())?
    } else {
        0
    };
    if offset > 100_000 {
        return Err(Error::limit("Cursor offset exceeds collection budget"));
    }
    Ok((offset, limit))
}
fn page(
    revision: &str,
    items: Vec<Value>,
    total: usize,
    offset: usize,
    limit: usize,
    w: &[String],
) -> Value {
    obj([
        ("revision", revision.into()),
        ("items", array(items)),
        ("total", total.into()),
        (
            "cursor",
            if offset.saturating_add(limit) < total {
                format!("{revision}:{}", offset + limit).into()
            } else {
                Value::Null
            },
        ),
        ("warnings", warnings(w)),
    ])
}
fn kind_from_id(id: &str) -> Result<&'static str> {
    match id.split_once('_').map(|(p, _)| p) {
        Some("s") => Ok("sequence"),
        Some("a") => Ok("asset"),
        Some("t") => Ok("track"),
        Some("c") => Ok("clip"),
        Some("ef") => Ok("effect"),
        Some("tr") => Ok("transition"),
        Some("m") => Ok("marker"),
        _ => Err(Error::new("Internal", "Unknown generated identity prefix")),
    }
}
pub fn parse_profile(v: &Value) -> Result<Profile> {
    let n = |k: &str| {
        v.uint(k)
            .and_then(|n| u32::try_from(n).map_err(|_| Error::invalid("Profile number overflow")))
    };
    let p = Profile {
        width: n("width")?,
        height: n("height")?,
        fps: FrameRate::new(n("fps_num")?, n("fps_den")?)?,
        progressive: v.get("progressive")?.boolean()?,
        sample_aspect: (n("sample_aspect_num")?, n("sample_aspect_den")?),
        display_aspect: (n("display_aspect_num")?, n("display_aspect_den")?),
        colorspace: n("colorspace")?,
        audio_channels: n("audio_channels")?,
    };
    p.validate()?;
    Ok(p)
}
fn media_frames(info: &crate::runtime::MediaInfo, fps: FrameRate) -> Result<u64> {
    if info.duration_den == 0 || info.duration_num == 0 {
        return Err(Error::unsupported(
            "Probe did not provide a usable duration",
        ));
    }
    let n = u128::from(info.duration_num) * u128::from(fps.num);
    let d = u128::from(info.duration_den) * u128::from(fps.den);
    let frames = n.div_ceil(d);
    if frames == 0 || frames > u128::from(crate::time::MAX_FRAME) {
        return Err(Error::limit(
            "Probed frame duration outside supported range",
        ));
    }
    Ok(frames as u64)
}
