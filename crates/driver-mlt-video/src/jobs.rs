//! Bounded persistent-driver render jobs. Polling is ordinary Execute v1, not an event protocol.
use crate::{
    Error, Result, adapters,
    fs::{Artifact, PrivateDir, Root},
    hash::{Sha256, random_id, sha256},
    json::{Value, array, obj},
    model::*,
    runtime::{MediaInfo, RenderProfile, Runtime, ServiceCatalog},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::File,
    io::{Read, Write},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
pub const MAX_ACTIVE_JOBS: usize = 2;
pub const MAX_RETAINED_JOBS: usize = 16;
pub const MAX_MEDIA_BYTES: u64 = 256 * 1024 * 1024;
pub const MAX_JOB_INPUT_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Queued,
    Starting,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Unknown,
}
impl State {
    pub fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Unknown => "unknown",
        }
    }
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Unknown
        )
    }
}

/// Optional original Host custody, independent of the native state/error.
#[derive(Clone, Debug)]
pub enum NativeHostCustody {
    Retained(Artifact),
    Unretained(Error),
}
impl NativeHostCustody {
    pub fn json(&self) -> Value {
        match self {
            Self::Retained(file) => obj([
                ("state", "retained".into()),
                ("artifact", file.json()),
                ("error", Value::Null),
            ]),
            Self::Unretained(error) => obj([
                (
                    "state",
                    (if error.outcome_known {
                        "failed_known"
                    } else {
                        "outcome_unknown"
                    })
                    .into(),
                ),
                ("artifact", Value::Null),
                (
                    "error",
                    obj([
                        ("code", error.code.into()),
                        ("outcome_known", error.outcome_known.into()),
                    ]),
                ),
            ]),
        }
    }
}

#[derive(Clone, Debug)]
pub struct JobSnapshot {
    pub id: String,
    pub revision: String,
    pub profile: String,
    pub output: String,
    pub state: State,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub artifact: Option<Artifact>,
    pub media: Option<MediaInfo>,
    pub native_input: Option<Artifact>,
    pub native_input_sources: Vec<Artifact>,
    pub color_observation: Option<Artifact>,
    pub native_host_custody: Option<NativeHostCustody>,
    pub error: Option<Error>,
    pub cancellation_requested: bool,
}
impl JobSnapshot {
    pub fn json(&self) -> Value {
        let mut value = obj([
            ("job", self.id.clone().into()),
            ("project_revision", self.revision.clone().into()),
            ("profile", self.profile.clone().into()),
            ("output", self.output.clone().into()),
            ("state", self.state.name().into()),
            ("progress", Value::Null),
            ("created_at_ms", self.created_at.into()),
            (
                "started_at_ms",
                self.started_at.map_or(Value::Null, Into::into),
            ),
            (
                "completed_at_ms",
                self.completed_at.map_or(Value::Null, Into::into),
            ),
            (
                "artifact",
                self.artifact.as_ref().map_or(Value::Null, Artifact::json),
            ),
            (
                "media",
                self.media.as_ref().map_or(Value::Null, MediaInfo::json),
            ),
            (
                "native_input",
                self.native_input
                    .as_ref()
                    .map_or(Value::Null, Artifact::json),
            ),
            (
                "native_input_sources",
                array(self.native_input_sources.iter().map(Artifact::json)),
            ),
            (
                "color_observation",
                self.color_observation
                    .as_ref()
                    .map_or(Value::Null, Artifact::json),
            ),
            (
                "error",
                self.error.as_ref().map_or(Value::Null, Error::json),
            ),
            ("cancellation_requested", self.cancellation_requested.into()),
        ]);
        if let (Value::Object(fields), Some(custody)) = (&mut value, &self.native_host_custody) {
            fields.insert("native_host_custody".into(), custody.json());
        }
        value
    }
}
struct Job {
    snapshot: Arc<Mutex<JobSnapshot>>,
    cancel: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}
pub struct Jobs {
    entries: BTreeMap<String, Job>,
    order: VecDeque<String>,
}
impl Default for Jobs {
    fn default() -> Self {
        Self::new()
    }
}
impl Jobs {
    pub fn new() -> Self {
        Self {
            entries: BTreeMap::new(),
            order: VecDeque::new(),
        }
    }
    pub fn get(&self, id: &str) -> Result<JobSnapshot> {
        let j = self.entries.get(id).ok_or_else(|| {
            Error::new(
                "StaleReference",
                "Render job belongs to another driver lifetime or was evicted",
            )
        })?;
        j.snapshot
            .lock()
            .map(|s| s.clone())
            .map_err(|_| Error::new("Internal", "Job state lock poisoned"))
    }
    pub fn cancel(&mut self, id: &str) -> Result<JobSnapshot> {
        let j = self.entries.get(id).ok_or_else(Error::stale)?;
        let mut s = j
            .snapshot
            .lock()
            .map_err(|_| Error::new("Internal", "Job state lock poisoned"))?;
        if !s.state.terminal() {
            s.cancellation_requested = true;
            j.cancel.store(true, Ordering::Release);
        }
        Ok(s.clone())
    }
    fn evict(&mut self) -> Result<()> {
        while self.entries.len() >= MAX_RETAINED_JOBS {
            let index = self
                .order
                .iter()
                .position(|id| {
                    self.entries.get(id).is_some_and(|j| {
                        j.snapshot.lock().is_ok_and(|s| s.state.terminal())
                            && j.worker.as_ref().is_none_or(JoinHandle::is_finished)
                    })
                })
                .ok_or_else(|| Error::limit("Render job retention is full"))?;
            if let Some(id) = self.order.remove(index) {
                if let Some(mut job) = self.entries.remove(&id) {
                    if let Some(worker) = job.worker.take() {
                        let _ = worker.join();
                    }
                }
            }
        }
        Ok(())
    }
    pub fn start(
        &mut self,
        runtime: Arc<Runtime>,
        project: Project,
        sequence: String,
        revision: String,
        profile: RenderProfile,
        output: String,
        roots: Arc<BTreeMap<String, Arc<Root>>>,
    ) -> Result<JobSnapshot> {
        if self
            .entries
            .values()
            .filter(|j| j.snapshot.lock().is_ok_and(|s| !s.state.terminal()))
            .count()
            >= MAX_ACTIVE_JOBS
        {
            return Err(Error::limit("At most two render jobs may be active"));
        }
        let plan = render_plan(
            &project,
            &sequence,
            &revision,
            &profile,
            &output,
            Some(&runtime.catalog),
            &roots,
        )?;
        if !plan.get("runnable")?.boolean()? {
            return Err(Error::new(
                "Unavailable",
                "Render preflight lacks runtime services",
            ));
        }
        self.evict()?;
        let id = format!("render:{}", random_id()?);
        let snapshot = JobSnapshot {
            id: id.clone(),
            revision,
            profile: profile.id.into(),
            output: output.clone(),
            state: State::Queued,
            created_at: now(),
            started_at: None,
            completed_at: None,
            artifact: None,
            media: None,
            native_input: None,
            native_input_sources: vec![],
            color_observation: None,
            native_host_custody: None,
            error: None,
            cancellation_requested: false,
        };
        let shared = Arc::new(Mutex::new(snapshot.clone()));
        let cancel = Arc::new(AtomicBool::new(false));
        let shared_worker = shared.clone();
        let cancel_worker = cancel.clone();
        let worker = thread::spawn(move || {
            if let Ok(mut s) = shared_worker.lock() {
                s.state = State::Starting;
                s.started_at = Some(now());
            }
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                render_worker(
                    &runtime,
                    project,
                    &sequence,
                    &profile,
                    &output,
                    &roots,
                    &cancel_worker,
                    &shared_worker,
                )
            }));
            let result = match result {
                Ok(v) => v,
                Err(_) => Err(Error::new(
                    "Internal",
                    "Render worker panicked; job did not certify an artifact",
                )),
            };
            if let Ok(mut s) = shared_worker.lock() {
                s.completed_at = Some(now());
                match result {
                    Ok(result) => {
                        s.state = State::Succeeded;
                        s.artifact = Some(result.artifact);
                        s.media = Some(result.media);
                        s.native_input = result.native_input;
                        s.native_input_sources = result.native_input_sources;
                        s.color_observation = result.color_observation;
                    }
                    Err(error) => {
                        s.state = if error.code == "Cancelled" {
                            State::Cancelled
                        } else if !error.outcome_known {
                            State::Unknown
                        } else {
                            State::Failed
                        };
                        s.error = Some(error);
                    }
                }
            }
        });
        self.entries.insert(
            id.clone(),
            Job {
                snapshot: shared,
                cancel,
                worker: Some(worker),
            },
        );
        self.order.push_back(id);
        Ok(snapshot)
    }
    pub fn shutdown(&mut self) {
        for j in self.entries.values() {
            j.cancel.store(true, Ordering::Release);
        }
        let deadline = Instant::now() + Duration::from_millis(1500);
        while Instant::now() < deadline
            && self
                .entries
                .values()
                .any(|j| j.worker.as_ref().is_some_and(|w| !w.is_finished()))
        {
            thread::sleep(Duration::from_millis(10));
        }
        for j in self.entries.values_mut() {
            if j.worker.as_ref().is_some_and(JoinHandle::is_finished) {
                if let Some(w) = j.worker.take() {
                    let _ = w.join();
                }
            }
        }
        // On process exit, remaining worker threads end; PDEATHSIG + bubblewrap contain children.
        // No claim of durable jobs or same-process recovery from stuck kernel I/O is made.
    }
}
impl Drop for Jobs {
    fn drop(&mut self) {
        self.shutdown();
    }
}
pub fn required(
    p: &Project,
    s: &Sequence,
) -> Result<(BTreeSet<String>, BTreeMap<String, BTreeSet<String>>)> {
    let mut assets = BTreeSet::new();
    let mut groups: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for t in &s.tracks {
        if t.opaque || t.lanes.len() != 1 {
            return Err(Error::unsupported(
                "Render requires supported single-lane tracks",
            ));
        }
        for l in &t.lanes {
            for c in &l.clips {
                assets.insert(c.asset.clone());
                for e in &c.effects {
                    if e.opaque.is_some() {
                        return Err(Error::unsupported(
                            "Unknown effect cannot enter curated rendering",
                        ));
                    }
                    groups
                        .entry("filters".into())
                        .or_default()
                        .insert(e.service.clone());
                }
            }
        }
        for e in &t.effects {
            if e.opaque.is_some() {
                return Err(Error::unsupported("Opaque track effect"));
            }
            groups
                .entry("filters".into())
                .or_default()
                .insert(e.service.clone());
        }
    }
    for tr in &s.transitions {
        if tr.opaque.is_some() {
            return Err(Error::unsupported(
                "Opaque transition cannot enter curated rendering",
            ));
        }
        groups
            .entry("transitions".into())
            .or_default()
            .insert(if tr.kind == "dissolve" { "luma" } else { "mix" }.into());
    }
    for id in &assets {
        let a = p
            .assets
            .get(id)
            .ok_or_else(|| Error::invalid("Missing asset"))?;
        if a.opaque || matches!(a.resource, Resource::External(_) | Resource::Opaque(_)) {
            return Err(Error::new(
                "PermissionDenied",
                "Unresolved/opaque external media is observed, never opened",
            ));
        }
        groups.entry("producers".into()).or_default().insert(
            if a.service == "colour" {
                "color"
            } else {
                &a.service
            }
            .into(),
        );
    }
    Ok((assets, groups))
}
pub fn validate_staged_asset(
    project: &Project,
    sequence: &Sequence,
    asset_id: &str,
    info: &MediaInfo,
) -> Result<()> {
    let asset = project
        .assets
        .get(asset_id)
        .ok_or_else(|| Error::invalid("Missing staged asset"))?;
    if !info.video && !info.audio {
        return Err(Error::unsupported(
            "Staged file is not recognized audio/video/image media",
        ));
    }
    if asset.kind != "image" {
        let capacity = info
            .frame_capacity(project.profile.fps)?
            .ok_or_else(|| Error::unsupported("Staged media has no measurable duration"))?;
        let required_end = sequence
            .tracks
            .iter()
            .flat_map(|track| &track.lanes)
            .flat_map(|lane| &lane.clips)
            .filter(|clip| clip.asset == asset_id)
            .map(|clip| clip.source.end.0)
            .max()
            .unwrap_or(0);
        if required_end > capacity {
            return Err(Error::new(
                "Conflict",
                "Staged media is shorter than the planned source range",
            ));
        }
    }
    Ok(())
}

#[derive(Clone, Debug)]
pub struct PreparedRenderDocument {
    pub xml: String,
    pub expected_profile: Profile,
    pub frames: u64,
}

/// Bound native decoder memory only in the newly staged render graph. Semantic
/// project/save serialization stays unchanged, as do consumer codec settings.
fn bound_staged_decoder_threads(document: &mut crate::xml::Node) {
    for producer in document.elements_mut() {
        if matches!(producer.name.as_str(), "chain" | "producer")
            && matches!(
                producer.property("mlt_service").as_deref(),
                Some("avformat" | "avformat-novalidate")
            )
        {
            producer.set_property("threads", "1");
        }
    }
}

pub fn prepare_render_document(
    mut project: Project,
    sequence_id: &str,
    profile: &RenderProfile,
    staged: &BTreeMap<String, String>,
) -> Result<PreparedRenderDocument> {
    let sequence = project.sequence(sequence_id)?.clone();
    let (asset_ids, _) = required(&project, &sequence)?;
    project.sequences = vec![sequence.clone()];
    project.assets.retain(|id, _| asset_ids.contains(id));
    if let Some(width) = profile.width {
        project.profile.width = width;
    }
    if let Some(height) = profile.height {
        project.profile.height = height;
    }
    let mut document = adapters::write_normal_form(&project, Some(staged))?;
    bound_staged_decoder_threads(&mut document);
    let xml = crate::xml::serialize(&document)?;
    Ok(PreparedRenderDocument {
        xml,
        expected_profile: project.profile,
        frames: sequence.duration(),
    })
}

pub fn media_location(p: &Project, a: &MediaAsset) -> Result<(String, String)> {
    let (root, path) = match &a.resource {
        Resource::Scoped { root, path } => (root.clone(), path.clone()),
        Resource::Relative(path) => {
            let root = p.source_root.clone().ok_or_else(|| {
                Error::new(
                    "PermissionDenied",
                    "Relative media needs an opened project root",
                )
            })?;
            let path = if p.source_dir.is_empty() {
                path.clone()
            } else {
                format!("{}/{}", p.source_dir, path)
            };
            (root, path)
        }
        _ => return Err(Error::new("PermissionDenied", "Resource cannot be opened")),
    };
    if !matches!(root.as_str(), "project" | "media" | "output") {
        return Err(Error::new("PermissionDenied", "Media root is not allowed"));
    }
    crate::fs::validate_relative(&path)?;
    Ok((root, path))
}
pub fn render_plan(
    p: &Project,
    sequence: &str,
    revision: &str,
    profile: &RenderProfile,
    output: &str,
    catalog: Option<&ServiceCatalog>,
    roots: &BTreeMap<String, Arc<Root>>,
) -> Result<Value> {
    p.validate()?;
    if p.profile.audio_channels != 2 {
        return Err(Error::unsupported(
            "Curated render backend currently validates stereo output only",
        ));
    }
    crate::fs::validate_relative(output)?;
    if !p.generated {
        return Err(Error::unsupported(
            "Native/unknown graphs are inspection/preservation-only; rendering requires an owned semantic normal form",
        ));
    }
    let seq = p.sequence(sequence)?;
    if seq.duration() == 0 || seq.opaque || !seq.nested.is_empty() || !seq.subtitles.is_empty() {
        return Err(Error::unsupported(
            "Empty, opaque, nested or subtitle sequence cannot be rendered by the curated backend",
        ));
    }

    let semantic_project = crate::domain::project(p);
    let render_intent =
        semwright_video_domain::render::RenderIntent::full(sequence, profile.semantic());
    let semantic_frames = render_intent.expected_frames(&semantic_project)?;
    if semantic_frames != seq.duration() {
        return Err(Error::new(
            "BackendFailed",
            "Native and semantic render duration disagree",
        ));
    }

    let out = roots
        .get("output")
        .ok_or_else(|| Error::new("Unavailable", "Output mount is absent"))?;
    if out.exists(output)? {
        return Err(Error::new(
            "Conflict",
            "Render output already exists; overwrite is not exposed",
        ));
    }
    let (ids, groups) = required(p, seq)?;
    let mut missing = vec![];
    let mut media = vec![];
    for id in &ids {
        let a = &p.assets[id];
        match &a.resource {
            Resource::Color(c) => {
                if !valid_color(c) {
                    return Err(Error::invalid("Generator color is not curated"));
                }
            }
            _ => {
                let (root, path) = media_location(p, a)?;
                let mount = roots
                    .get(&root)
                    .ok_or_else(|| Error::new("PermissionDenied", "Media mount unavailable"))?;
                if !mount.exists(&path)? {
                    return Err(Error::new("NotFound", "A required media file is missing"));
                }
                media.push(obj([
                    ("asset", id.clone().into()),
                    ("root", root.into()),
                    ("path", path.into()),
                ]));
            }
        }
    }
    for (group, services) in &groups {
        for service in services {
            if catalog.is_none_or(|catalog| !catalog.has(group, service)) {
                missing.push(format!("{group}:{service}"));
            }
        }
    }
    let available = catalog.is_some_and(|catalog| profile.available(catalog));
    Ok(obj([("project_revision",revision.into()),("sequence",sequence.into()),("profile",profile.id.into()),("output_root","output".into()),("output_path",output.into()),("frames",semantic_frames.into()),("runnable",(available&&missing.is_empty()).into()),("missing_services",array(missing.into_iter().map(Into::into))),("media",array(media)),("warnings",array(["No wall-clock duration estimate; progress is state-only".into(),"Media are staged from confined FDs; output is validated before no-replace publication".into()]))]))
}
pub fn valid_color(s: &str) -> bool {
    matches!(s, "red" | "green" | "blue" | "black" | "white" | "yellow")
        || (s.len() == 7 && s.starts_with('#') && s[1..].bytes().all(|b| b.is_ascii_hexdigit()))
        || (s.len() == 10 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit()))
}
fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(Error::new("Cancelled", "Render cancelled"))
    } else {
        Ok(())
    }
}
pub struct RenderedOutput {
    pub artifact: Artifact,
    pub media: MediaInfo,
    pub native_input: Option<Artifact>,
    pub native_input_sources: Vec<Artifact>,
    pub color_observation: Option<Artifact>,
}

pub(crate) fn render_sync_pinned(
    runtime: &Runtime,
    project: Project,
    sequence: &str,
    profile: &RenderProfile,
    output: &str,
    roots: &BTreeMap<String, Arc<Root>>,
    cancel: &AtomicBool,
    expected_sha256: &BTreeMap<String, String>,
    artifact_limit: u64,
) -> Result<RenderedOutput> {
    render_impl(
        runtime,
        project,
        sequence,
        profile,
        output,
        roots,
        cancel,
        Some(expected_sha256),
        artifact_limit,
        |_| {},
    )
}

fn render_worker(
    runtime: &Runtime,
    project: Project,
    sequence: &str,
    profile: &RenderProfile,
    output: &str,
    roots: &BTreeMap<String, Arc<Root>>,
    cancel: &AtomicBool,
    snapshot: &Arc<Mutex<JobSnapshot>>,
) -> Result<RenderedOutput> {
    render_impl(
        runtime,
        project,
        sequence,
        profile,
        output,
        roots,
        cancel,
        None,
        MAX_ARTIFACT_BYTES,
        |_| {
            if let Ok(mut state) = snapshot.lock() {
                state.state = State::Running;
            }
        },
    )
}

fn render_impl<F: FnOnce(&Path)>(
    runtime: &Runtime,
    mut p: Project,
    sequence: &str,
    profile: &RenderProfile,
    output: &str,
    roots: &BTreeMap<String, Arc<Root>>,
    cancel: &AtomicBool,
    expected_sha256: Option<&BTreeMap<String, String>>,
    artifact_limit: u64,
    on_running: F,
) -> Result<RenderedOutput> {
    if artifact_limit == 0 || artifact_limit > MAX_ARTIFACT_BYTES {
        return Err(Error::limit("Render artifact limit is outside bounds"));
    }
    check_cancel(cancel)?;
    let seq = p.sequence(sequence)?.clone();
    let (asset_ids, groups) = required(&p, &seq)?;
    let selected = crate::runtime::complete_observation_profile(profile);
    let output_root = roots
        .get("output")
        .ok_or_else(|| Error::new("Unavailable", "Output mount disappeared"))?;
    if selected.is_some() {
        crate::color::preflight_observation_output(
            output_root,
            output,
            profile.width.unwrap_or(p.profile.width),
            profile.height.unwrap_or(p.profile.height),
            seq.duration(),
            true,
        )?;
    }
    if let Some(pins) = expected_sha256 {
        let required_pins = asset_ids
            .iter()
            .filter(|id| !matches!(p.assets[id.as_str()].resource, Resource::Color(_)))
            .cloned()
            .collect::<BTreeSet<_>>();
        let supplied_pins = pins.keys().cloned().collect::<BTreeSet<_>>();
        if supplied_pins != required_pins
            || pins
                .values()
                .any(|digest| !crate::hash::valid_digest(digest))
        {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned AV render must bind every staged media asset and no unrelated asset",
            ));
        }
    }
    if !profile.available(&runtime.catalog)
        || groups
            .iter()
            .any(|(g, ids)| ids.iter().any(|id| !runtime.catalog.has(g, id)))
    {
        return Err(Error::new(
            "Unavailable",
            "Runtime lacks a required MLT service or codec",
        ));
    }
    let input_probe_roster = asset_ids
        .iter()
        .filter(|id| !matches!(p.assets[id.as_str()].resource, Resource::Color(_)))
        .cloned()
        .collect::<Vec<_>>();
    let (mut inputs, mut work) = if selected.is_some() {
        output_root.create_original_workspaces(output)?
    } else {
        (
            PrivateDir::new(Path::new("/tmp"))?,
            PrivateDir::new(Path::new("/tmp"))?,
        )
    };
    let work_root = work.owned_root(true, true)?;
    let input_root = inputs.owned_root(true, false)?;
    let mut raw_originals = if selected.is_some() {
        Some(crate::originals::VideoOriginals::new(
            output_root,
            &work_root,
            output,
            (
                profile.width.unwrap_or(p.profile.width),
                profile.height.unwrap_or(p.profile.height),
            ),
            seq.duration(),
            &format!("partial.{}", profile.extension),
            crate::originals::VideoOriginalKind::NativeRender,
            input_probe_roster.len(),
        )?)
    } else {
        None
    };
    if let Some(raw) = &mut raw_originals {
        let mut input_members = input_probe_roster
            .iter()
            .map(|id| {
                (
                    format!("asset-{}.bin", &sha256(id.as_bytes())[..24]),
                    MAX_MEDIA_BYTES,
                )
            })
            .collect::<Vec<_>>();
        input_members.push(("project.mlt".to_owned(), crate::xml::MAX_XML as u64));
        raw.declare_workspaces(&inputs, &work, &input_members)?;
    }
    if selected.is_some() {
        let budget = crate::color::observation_budget(
            profile.width.unwrap_or(p.profile.width),
            profile.height.unwrap_or(p.profile.height),
            seq.duration(),
        )?;
        let required = raw_originals
            .as_ref()
            .map_or(budget.peak_free_bytes, |raw| raw.required_free_bytes);
        // declare_workspaces includes the same existing two-XML allowance,
        // plus its own bounded pre-effect receipt on this output filesystem.
        if work_root.available_bytes_for("decoded-000000.png")? < required {
            return Err(Error::limit(
                "Native scratch lacks complete observation free-space budget",
            ));
        }
    }
    let mut source_sha256: Option<String> = None;
    let mut process_originals_complete = true;
    let mut process_secondary_errors = Vec::new();
    let original = (|| -> Result<RenderedOutput> {
        let mut native_staged_sources = Vec::new();
        let mut probe_slot = 0usize;
        let mut staged = BTreeMap::new();
        let mut total = 0;
        for id in &asset_ids {
            check_cancel(cancel)?;
            let a = &p.assets[id];
            if matches!(a.resource, Resource::Color(_)) {
                continue;
            }
            let (root, path) = media_location(&p, a)?;
            let mount = roots
                .get(&root)
                .ok_or_else(|| Error::new("PermissionDenied", "Media mount absent"))?;
            let mut source = mount.read_file(&path, MAX_MEDIA_BYTES)?;
            let name = format!("asset-{}.bin", &sha256(id.as_bytes())[..24]);
            let mut destination = inputs.create(&name)?;
            let mut staged_hash = Sha256::new();
            let mut buf = [0; 65536];
            let mut size = 0;
            loop {
                check_cancel(cancel)?;
                let n = source.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                size += n as u64;
                total += n as u64;
                if size > MAX_MEDIA_BYTES || total > MAX_JOB_INPUT_BYTES {
                    return Err(Error::limit("Staged media exceeds job input budget"));
                }
                staged_hash.update(&buf[..n]);
                destination.write_all(&buf[..n])?;
            }
            let staged_sha256 = staged_hash.finish();
            if let Some(expected) = expected_sha256.and_then(|pins| pins.get(id))
                && staged_sha256 != *expected
            {
                return Err(Error::new(
                    "StaleReference",
                    "Staged media bytes changed after the AV plan was prepared",
                ));
            }
            if selected.is_some() {
                native_staged_sources.push((
                    Artifact {
                        root: root.clone(),
                        path: path.clone(),
                        sha256: staged_sha256.clone(),
                        bytes: size,
                    },
                    name.clone(),
                ));
            }
            destination.sync_all()?;
            drop(destination);
            inputs.seal(&name)?;
            if input_probe_roster.get(probe_slot) != Some(id) {
                return Err(Error::new(
                    "Internal",
                    "Actual staged probe differs from its original ordered roster",
                ));
            }
            let probe_custody = if selected.is_some() {
                Some(crate::runtime::Ffv1ProcessCustody::for_input_probe(
                    output_root,
                    output,
                    &staged_sha256,
                    probe_slot,
                    &work_root,
                )?)
            } else {
                None
            };
            let probe = runtime.probe_observed(
                inputs.path(),
                work.path(),
                &name,
                cancel,
                probe_custody.as_ref(),
            );
            let probe = if let Some(custody) = &probe_custody {
                let result = custody.complete(probe);
                process_originals_complete &= custody.original_capture_complete();
                if let Some(report) = custody.secondary_report() {
                    process_secondary_errors.push(report);
                }
                result
            } else {
                probe
            };
            probe_slot += 1;
            let info = probe?;
            if !info.video && !info.audio {
                return Err(Error::unsupported(
                    "Staged file is not recognized audio/video/image media",
                ));
            }
            if a.kind != "image" {
                let capacity = info
                    .frame_capacity(p.profile.fps)?
                    .ok_or_else(|| Error::unsupported("Staged media has no measurable duration"))?;
                let required_end = seq
                    .tracks
                    .iter()
                    .flat_map(|t| &t.lanes)
                    .flat_map(|l| &l.clips)
                    .filter(|c| &c.asset == id)
                    .map(|c| c.source.end.0)
                    .max()
                    .unwrap_or(0);
                if required_end > capacity {
                    return Err(Error::new(
                        "Conflict",
                        "Staged media is shorter than the planned source range",
                    ));
                }
            }
            staged.insert(id.clone(), runtime.input_reference(inputs.path(), &name));
        }
        if probe_slot != input_probe_roster.len() {
            return Err(Error::new(
                "Internal",
                "Original input-probe roster was not completely consumed",
            ));
        }
        let native_sources = native_staged_sources
            .iter()
            .map(|(source, _)| source.clone())
            .collect::<Vec<_>>();
        p.sequences = vec![seq];
        p.assets.retain(|id, _| asset_ids.contains(id));
        if let Some(width) = profile.width {
            p.profile.width = width;
        }
        if let Some(height) = profile.height {
            p.profile.height = height;
        }
        let mut document = adapters::write_normal_form(&p, Some(&staged))?;
        bound_staged_decoder_threads(&mut document);
        let xml = crate::xml::serialize(&document)?;
        let mut f = inputs.create("project.mlt")?;
        f.write_all(xml.as_bytes())?;
        f.sync_all()?;
        drop(f);
        inputs.seal("project.mlt")?;
        let native_before = if selected.is_some() {
            Some(input_root.read("project.mlt", crate::xml::MAX_XML)?)
        } else {
            None
        };
        let native_identity = native_before
            .as_ref()
            .map(|bytes| (sha256(bytes), bytes.len() as u64));
        let verify_native_inputs = || -> Result<()> {
            if let Some((digest, bytes)) = &native_identity {
                crate::color::verify_staged_native_inputs(
                    &input_root,
                    digest,
                    *bytes,
                    &native_staged_sources,
                )?;
            }
            Ok(())
        };
        source_sha256 = native_identity.as_ref().map(|(digest, _)| digest.clone());
        let custody = if let Some(source) = source_sha256.as_deref() {
            Some(crate::runtime::Ffv1ProcessCustody::with_owned_work(
                output_root,
                output,
                source,
                crate::originals::VideoOriginalKind::NativeRender,
                &work_root,
            )?)
        } else {
            None
        };
        let mut native_input = None;
        let original = (|| -> Result<RenderedOutput> {
            check_cancel(cancel)?;
            if let Some(before) = &native_before {
                let retained = output_root.publish_verified(
                    "output",
                    &format!("{output}.native-input.mlt"),
                    before.as_slice(),
                    crate::xml::MAX_XML as u64,
                    &sha256(before),
                    before.len() as u64,
                )?;
                if output_root.digest(&retained.path, crate::xml::MAX_XML as u64)?
                    != retained.sha256
                {
                    return Err(Error::new(
                        "StaleReference",
                        "Actual native XML original readback changed before render",
                    ));
                }
                native_input = Some(retained);
            }
            let deadline = Instant::now() + runtime.timeout;
            on_running(inputs.path());
            verify_native_inputs()?;
            runtime.render_observed(
                inputs.path(),
                work.path(),
                profile,
                cancel,
                deadline,
                custody.as_ref(),
            )?;
            check_cancel(cancel)?;
            verify_native_inputs()?;
            let validated = runtime.validate_output_observed(
                work.path(),
                profile,
                &p.profile,
                p.sequences[0].duration(),
                cancel,
                deadline,
                custody.as_ref(),
            )?;
            let media = validated.media;
            let mut color_observation = None;
            if let Some(observed) = &validated.observation {
                let before = native_before
                    .as_ref()
                    .ok_or_else(|| Error::invalid("Original native input bytes missing"))?;
                let after = input_root.read("project.mlt", crate::xml::MAX_XML)?;
                if after != *before {
                    return Err(Error::new(
                        "StaleReference",
                        "Actual native XML changed during render",
                    ));
                }
                let retained = native_input.as_ref().ok_or_else(|| {
                    Error::invalid("Original native XML was not retained before render")
                })?;
                if retained.sha256 != sha256(&after) || retained.bytes != after.len() as u64 {
                    return Err(Error::new(
                        "StaleReference",
                        "Retained native XML differs from actual rendered input",
                    ));
                }
                if output_root.digest(&retained.path, crate::xml::MAX_XML as u64)?
                    != retained.sha256
                {
                    return Err(Error::new(
                        "StaleReference",
                        "Actual native XML retention changed",
                    ));
                }
                let source_binding = serde_json::json!({
                    "source_kind":"mlt_render_project_bytes","root":"output","path":retained.path,
                    "sha256":retained.sha256,"bytes":retained.bytes,"source_first_frame":0,
                });
                verify_native_inputs()?;
                color_observation = Some(crate::color::publish_observation(
                    observed,
                    &work_root,
                    output_root,
                    output,
                    source_binding,
                    Some(&native_sources),
                    (p.profile.width, p.profile.height),
                    p.profile.fps.num,
                    p.profile.fps.den,
                    cancel,
                )?);
            }
            check_cancel(cancel)?;
            let source =
                work_root.read_file(&format!("partial.{}", profile.extension), artifact_limit)?;
            struct CancelReader<'a> {
                file: File,
                cancel: &'a AtomicBool,
            }
            impl Read for CancelReader<'_> {
                fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
                    if self.cancel.load(Ordering::Acquire) {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::Interrupted,
                            "cancelled",
                        ));
                    }
                    self.file.read(b)
                }
            }
            let reader = CancelReader {
                file: source,
                cancel,
            };
            let publication = if let Some(observed) = &validated.observation {
                output_root.publish_verified(
                    "output",
                    output,
                    reader,
                    artifact_limit,
                    &observed.video_sha256,
                    observed.video_bytes,
                )
            } else {
                output_root.publish("output", output, reader, artifact_limit)
            };
            let artifact = match publication {
                Ok(a) => a,
                Err(e) => {
                    if !e.outcome_known {
                        return Err(e);
                    }
                    check_cancel(cancel)?;
                    return Err(e);
                }
            };
            // Publication is the success linearization point. A cancellation arriving after it does not
            // delete a validated final artifact or relabel success as cancelled.
            if validated.observation.is_some() {
                crate::color::verify_published_video(output_root, &artifact)?;
            }
            Ok(RenderedOutput {
                artifact,
                media,
                native_input,
                native_input_sources: native_sources,
                color_observation,
            })
        })();
        let original = if let Some(custody) = &custody {
            let result = custody.complete(original);
            process_originals_complete &= custody.original_capture_complete();
            if let Some(report) = custody.secondary_report() {
                process_secondary_errors.push(report);
            }
            result
        } else {
            original
        };
        original
    })();
    if let Some(raw) = raw_originals {
        raw.finish(
            original,
            source_sha256.as_deref(),
            process_originals_complete,
            &process_secondary_errors,
            &mut inputs,
            &mut work,
            &input_root,
            &work_root,
        )
    } else {
        original
    }
}

#[cfg(test)]
mod native_memory_tests {
    use super::*;
    use crate::model::{Clip, Profile, Resource, Timeline, Track};
    use crate::time::FrameRange;
    use crate::xml::{self, Node};

    fn producer(tag: &str, id: &str, service: &str) -> Node {
        let mut node = Node::new(tag).attr("id", id).attr("out", 1436);
        node.set_property("mlt_service", service);
        node.set_property("resource", "asset-0.mkv");
        node
    }

    fn project() -> Project {
        let mut project = Project::new(Profile::default()).unwrap();
        project.assets.insert(
            "a".into(),
            MediaAsset {
                id: "a".into(),
                name: "source".into(),
                kind: "video".into(),
                resource: Resource::Scoped {
                    root: "media".into(),
                    path: "fixture.mkv".into(),
                },
                frames: Some(1437),
                service: "avformat".into(),
                original: None,
                proxy: None,
                opaque: false,
            },
        );
        let clip = |id: &str, start: u64, end: u64| Clip {
            id: id.into(),
            name: id.into(),
            asset: "a".into(),
            start,
            source: FrameRange::new(start, end).unwrap(),
            effects: vec![],
            binding: None,
            speed: (1, 1),
        };
        project.sequences[0].tracks.push(Track {
            id: "video".into(),
            name: "video".into(),
            kind: "av".into(),
            muted: false,
            hidden: false,
            lanes: vec![Timeline {
                id: "lane_video".into(),
                clips: vec![clip("first", 0, 345), clip("second", 345, 1437)],
            }],
            effects: vec![],
            opaque: false,
        });
        project.validate().unwrap();
        project
    }

    #[test]
    fn every_base_and_clone_avformat_decoder_is_single_threaded() {
        let mut document = Node::new("mlt");
        for (tag, id, service) in [
            ("chain", "base", "avformat"),
            ("chain", "source_first", "avformat"),
            ("chain", "source_second", "avformat-novalidate"),
            ("producer", "audio", "avformat"),
        ] {
            document.push(producer(tag, id, service));
        }
        bound_staged_decoder_threads(&mut document);
        assert_eq!(document.elements().count(), 4);
        for node in document.elements() {
            assert_eq!(node.property("threads").as_deref(), Some("1"));
        }
    }

    #[test]
    fn generators_images_filters_and_consumer_settings_stay_exact() {
        let mut document = Node::new("mlt");
        for (tag, id, service) in [
            ("producer", "color", "color"),
            ("producer", "image", "pixbuf"),
            ("chain", "unknown", "avformat-custom"),
            ("filter", "filter", "avformat"),
            ("consumer", "consumer", "avformat"),
        ] {
            let mut node = producer(tag, id, service);
            node.set_property("threads", "2");
            document.push(node);
        }
        let before = document.clone();
        bound_staged_decoder_threads(&mut document);
        assert_eq!(document, before);
    }

    #[test]
    fn explicit_bound_replaces_auto_once_and_is_idempotent() {
        let mut document = Node::new("mlt");
        let mut node = producer("chain", "source", "avformat");
        node.set_property("threads", "0");
        document.push(node);
        bound_staged_decoder_threads(&mut document);
        let once = document.clone();
        bound_staged_decoder_threads(&mut document);
        assert_eq!(document, once);
        let node = document.elements().next().unwrap();
        assert_eq!(node.property("threads").as_deref(), Some("1"));
        assert_eq!(
            node.elements()
                .filter(|n| n.name == "property" && n.a("name") == Some("threads"))
                .count(),
            1
        );
        assert_eq!(node.a("out"), Some("1436"));
        assert_eq!(node.property("resource").as_deref(), Some("asset-0.mkv"));
    }

    #[test]
    fn native_stage_preserves_saved_project_all_entries_and1437_clock() {
        let project = project();
        let saved = adapters::save(&project).unwrap();
        let staged = BTreeMap::from([("a".into(), "asset-0.mkv".into())]);
        let original = adapters::write_normal_form(&project, Some(&staged)).unwrap();
        let prepared = prepare_render_document(
            project.clone(),
            "sequence0",
            &RenderProfile::get("h264-1080p").unwrap(),
            &staged,
        )
        .unwrap();
        assert_eq!(prepared.frames, 1437);
        assert_eq!(
            (
                prepared.expected_profile.width,
                prepared.expected_profile.height
            ),
            (1920, 1080)
        );
        assert_eq!(prepared.expected_profile.fps, project.profile.fps);
        assert_eq!(adapters::save(&project).unwrap(), saved);
        assert!(!saved.contains("name=\"threads\""));
        let mut actual = xml::parse(prepared.xml.as_bytes()).unwrap();
        let mut count = 0;
        for node in actual
            .elements_mut()
            .filter(|n| matches!(n.name.as_str(), "chain" | "producer"))
        {
            if node.property("mlt_service").as_deref() == Some("avformat") {
                assert_eq!(node.property("threads").as_deref(), Some("1"));
                node.children.retain(|c| !matches!(c, xml::Child::Element(n) if n.name == "property" && n.a("name") == Some("threads")));
                count += 1;
            }
        }
        assert_eq!(count, 3); // one base producer and both independent clip clones
        let mut expected = original;
        let profile = expected
            .elements_mut()
            .find(|n| n.name == "profile")
            .unwrap();
        profile.set("width", 1920);
        profile.set("height", 1080);
        assert_eq!(actual, expected);
    }

    #[test]
    fn decoder_bound_keeps_audio_and720_profiles_and_source_clock() {
        let project = project();
        let staged = BTreeMap::from([("a".into(), "asset-0.mkv".into())]);
        for id in ["h264-720p", "audio-wav", "lossless"] {
            let profile = RenderProfile::get(id).unwrap();
            let prepared =
                prepare_render_document(project.clone(), "sequence0", &profile, &staged).unwrap();
            assert_eq!(prepared.frames, 1437);
            assert_eq!(prepared.expected_profile.fps, project.profile.fps);
            assert_eq!(prepared.expected_profile.audio_channels, 2);
            assert_eq!(
                prepared.expected_profile.width,
                profile.width.unwrap_or(project.profile.width)
            );
            assert_eq!(
                prepared.expected_profile.height,
                profile.height.unwrap_or(project.profile.height)
            );
            let root = xml::parse(prepared.xml.as_bytes()).unwrap();
            for node in root
                .elements()
                .filter(|n| n.property("mlt_service").as_deref() == Some("avformat"))
            {
                assert_eq!(node.property("threads").as_deref(), Some("1"));
            }
        }
    }

    #[test]
    fn missing_staged_asset_and_sequence_still_fail_closed() {
        let project = project();
        let profile = RenderProfile::get("h264-1080p").unwrap();
        assert!(
            prepare_render_document(project.clone(), "sequence0", &profile, &BTreeMap::new())
                .is_err()
        );
        assert!(prepare_render_document(project, "missing", &profile, &BTreeMap::new()).is_err());
    }
}

#[cfg(all(test, feature = "test-tools"))]
mod native_staging_closure_tests {
    use super::*;
    use crate::model::{Clip, Profile, Resource, Timeline, Track};
    use crate::time::FrameRange;
    use std::cell::Cell;

    const ORIGINAL: &[u8] = b"original-native-media";

    fn staged_render(tamper: impl FnOnce(&Path)) -> Error {
        let (runtime, _tools) = crate::runtime::native_closure_test_tools::runtime();
        let media = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        std::fs::write(media.path().join("fixture.mkv"), ORIGINAL).unwrap();
        let roots = BTreeMap::from([
            (
                "media".into(),
                Arc::new(Root::open(media.path(), true, false).unwrap()),
            ),
            (
                "output".into(),
                Arc::new(Root::open(output.path(), true, true).unwrap()),
            ),
        ]);
        let mut project = Project::new(Profile {
            width: 160,
            height: 90,
            ..Profile::default()
        })
        .unwrap();
        project.assets.insert(
            "a".into(),
            MediaAsset {
                id: "a".into(),
                name: "source".into(),
                kind: "video".into(),
                resource: Resource::Scoped {
                    root: "media".into(),
                    path: "fixture.mkv".into(),
                },
                frames: Some(50),
                service: "avformat".into(),
                original: None,
                proxy: None,
                opaque: false,
            },
        );
        project.sequences[0].tracks.push(Track {
            id: "video".into(),
            name: "video".into(),
            kind: "av".into(),
            muted: false,
            hidden: false,
            lanes: vec![Timeline {
                id: "lane_video".into(),
                clips: vec![Clip {
                    id: "first".into(),
                    name: "first".into(),
                    asset: "a".into(),
                    start: 0,
                    source: FrameRange::new(0, 50).unwrap(),
                    effects: vec![],
                    binding: None,
                    speed: (1, 1),
                }],
            }],
            effects: vec![],
            opaque: false,
        });
        project.validate().unwrap();
        let entered = Cell::new(false);
        let result = render_impl(
            &runtime,
            project,
            "sequence0",
            &RenderProfile::get("lossless-video-only").unwrap(),
            "result.mkv",
            &roots,
            &AtomicBool::new(false),
            Some(&BTreeMap::from([("a".into(), sha256(ORIGINAL))])),
            MAX_ARTIFACT_BYTES,
            |inputs| {
                entered.set(true);
                let xml = std::fs::read(inputs.join("project.mlt")).unwrap();
                let staged = inputs.join(format!("asset-{}.bin", &sha256(b"a")[..24]));
                assert_eq!(std::fs::read(&staged).unwrap(), ORIGINAL);
                tamper(&staged);
                assert_eq!(std::fs::read(inputs.join("project.mlt")).unwrap(), xml);
            },
        );
        assert!(
            entered.get(),
            "fixture must reach the actual staged render boundary"
        );
        assert_eq!(std::fs::read_dir(output.path()).unwrap().count(), 0);
        result
            .err()
            .expect("fixture does not produce an accepted native render")
    }

    #[test]
    fn unchanged_staging_reaches_the_native_process() {
        // fake-melt rejects the absolute Host render argv. This control proves
        // staging completed, without pretending the fixture is a video encoder.
        assert_eq!(staged_render(|_| {}).code, "BackendFailed");
    }

    #[test]
    fn replaced_staged_media_is_rejected_with_identical_xml() {
        let error = staged_render(|path| {
            let replacement = b"tampered-native-media";
            assert_eq!(replacement.len(), ORIGINAL.len());
            std::fs::remove_file(path).unwrap();
            std::fs::write(path, replacement).unwrap();
        });
        assert_eq!(error.code, "StaleReference");
    }
}
