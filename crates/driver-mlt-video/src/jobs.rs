//! Bounded persistent-driver render jobs. Polling is ordinary Execute v1, not an event protocol.
use crate::{
    Error, Result, adapters,
    fs::{Artifact, PrivateDir, Root},
    hash::{random_id, sha256},
    json::{Value, array, obj},
    model::*,
    runtime::{MediaInfo, RenderProfile, Runtime},
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
    pub error: Option<Error>,
    pub cancellation_requested: bool,
}
impl JobSnapshot {
    pub fn json(&self) -> Value {
        obj([
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
                "error",
                self.error.as_ref().map_or(Value::Null, Error::json),
            ),
            ("cancellation_requested", self.cancellation_requested.into()),
        ])
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
            Some(&runtime),
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
                    Ok((artifact, media)) => {
                        s.state = State::Succeeded;
                        s.artifact = Some(artifact);
                        s.media = Some(media);
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
fn required(
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
    runtime: Option<&Runtime>,
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
            if runtime.is_none_or(|r| !r.catalog.has(group, service)) {
                missing.push(format!("{group}:{service}"));
            }
        }
    }
    let available = runtime.is_some_and(|r| profile.available(&r.catalog));
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
fn render_worker(
    runtime: &Runtime,
    mut p: Project,
    sequence: &str,
    profile: &RenderProfile,
    output: &str,
    roots: &BTreeMap<String, Arc<Root>>,
    cancel: &AtomicBool,
    snapshot: &Arc<Mutex<JobSnapshot>>,
) -> Result<(Artifact, MediaInfo)> {
    check_cancel(cancel)?;
    let seq = p.sequence(sequence)?.clone();
    let (asset_ids, groups) = required(&p, &seq)?;
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
    let inputs = PrivateDir::new(Path::new("/tmp"))?;
    let work = PrivateDir::new(Path::new("/tmp"))?;
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
            destination.write_all(&buf[..n])?;
        }
        destination.sync_all()?;
        drop(destination);
        inputs.seal(&name)?;
        let info = runtime.probe(inputs.path(), work.path(), &name, cancel)?;
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
    p.sequences = vec![seq];
    p.assets.retain(|id, _| asset_ids.contains(id));
    if let Some(width) = profile.width {
        p.profile.width = width;
    }
    if let Some(height) = profile.height {
        p.profile.height = height;
    }
    let xml = crate::xml::serialize(&adapters::write_normal_form(&p, Some(&staged))?)?;
    let mut f = inputs.create("project.mlt")?;
    f.write_all(xml.as_bytes())?;
    f.sync_all()?;
    drop(f);
    inputs.seal("project.mlt")?;
    check_cancel(cancel)?;
    if let Ok(mut s) = snapshot.lock() {
        s.state = State::Running;
    }
    runtime.render(inputs.path(), work.path(), profile, cancel)?;
    check_cancel(cancel)?;
    let media = runtime.validate_output(
        work.path(),
        profile,
        &p.profile,
        p.sequences[0].duration(),
        cancel,
    )?;
    check_cancel(cancel)?;
    let source = Root::open(work.path(), true, false)?.read_file(
        &format!("partial.{}", profile.extension),
        MAX_ARTIFACT_BYTES,
    )?;
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
    let output_root = roots
        .get("output")
        .ok_or_else(|| Error::new("Unavailable", "Output mount disappeared"))?;
    let artifact = match output_root.publish(
        "output",
        output,
        CancelReader {
            file: source,
            cancel,
        },
        MAX_ARTIFACT_BYTES,
    ) {
        Ok(a) => a,
        Err(e) => {
            check_cancel(cancel)?;
            return Err(e);
        }
    };
    // Publication is the success linearization point. A cancellation arriving after it does not
    // delete a validated final artifact or relabel success as cancelled.
    Ok((artifact, media))
}
