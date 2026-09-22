//! Pinned ELF tools and bounded subprocesses. Production tools run through bubblewrap, never a shell.
use crate::{
    Error, Result,
    fs::PrivateDir,
    hash::{reader_hash, valid_digest},
    json::{self, Value, array, obj},
    model::Profile,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};
const RETAIN_LOG: usize = 16_384;
const TOTAL_LOG: usize = 262_144;
#[repr(C)]
struct Limit {
    current: u64,
    maximum: u64,
}
unsafe extern "C" {
    fn prctl(option: i32, ...) -> i32;
    fn getppid() -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
    fn setrlimit(resource: u32, limit: *const Limit) -> i32;
    fn getrlimit(resource: u32, limit: *mut Limit) -> i32;
    fn getuid() -> u32;
}
#[derive(Clone, Debug)]
pub struct ProcessSpec {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    pub cwd: PathBuf,
    pub timeout: Duration,
    pub cpu_seconds: u64,
    pub environment: BTreeMap<String, String>,
}
#[derive(Clone, Debug)]
pub struct ProcessResult {
    pub exit_code: Option<i32>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub cancelled: bool,
    pub timed_out: bool,
    pub output_exceeded: bool,
}
impl ProcessResult {
    pub fn success(&self) -> bool {
        self.exit_code == Some(0) && !self.cancelled && !self.timed_out && !self.output_exceeded
    }
    pub fn checked(self) -> Result<Self> {
        if self.cancelled {
            return Err(Error::new("Cancelled", "Operation cancelled"));
        }
        if self.timed_out {
            return Err(Error::new("Timeout", "Tool wall-clock deadline exceeded"));
        }
        if self.output_exceeded {
            return Err(Error::limit("Tool output exceeded total byte budget"));
        }
        if self.exit_code != Some(0) {
            return Err(Error::new(
                "BackendFailed",
                format!(
                    "Tool failed with exit code {:?}; raw logs are not exposed",
                    self.exit_code
                ),
            ));
        }
        Ok(self)
    }
}
fn capture(
    mut reader: impl Read + Send + 'static,
    overflow: Arc<AtomicBool>,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut retained = Vec::new();
        let mut bytes = 0usize;
        let mut buf = [0; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    bytes = bytes.saturating_add(n);
                    let keep = n.min(RETAIN_LOG.saturating_sub(retained.len()));
                    retained.extend_from_slice(&buf[..keep]);
                    if bytes > TOTAL_LOG {
                        overflow.store(true, Ordering::Release);
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        retained
    })
}
/// Owns the child on every error path, including pipe setup and wait failures.
struct ChildGuard {
    child: std::process::Child,
    group: i32,
    armed: bool,
}
impl std::ops::Deref for ChildGuard {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.child
    }
}
impl std::ops::DerefMut for ChildGuard {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.child
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if self.armed {
            // SAFETY: group belongs to the child created by this supervisor, not a caller-supplied PID.
            unsafe { kill(-self.group, 9) };
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
/// Testable supervisor. Only Runtime builds production specs; no public capability accepts argv.
pub fn run(spec: &ProcessSpec, cancel: &AtomicBool) -> Result<ProcessResult> {
    if !spec.executable.is_absolute()
        || !spec.cwd.is_absolute()
        || spec.timeout.is_zero()
        || spec.timeout > Duration::from_secs(3600)
    {
        return Err(Error::invalid("Invalid process specification"));
    }
    if cancel.load(Ordering::Acquire) {
        return Err(Error::new("Cancelled", "Cancelled before spawn"));
    }
    let mut command = Command::new(&spec.executable);
    command
        .args(&spec.args)
        .current_dir(&spec.cwd)
        .env_clear()
        .envs(&spec.environment)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let parent = std::process::id() as i32;
    let cpu = spec.cpu_seconds.clamp(1, 300);
    // SAFETY: the closure only uses async-signal-safe Linux syscalls and stack values after fork.
    unsafe {
        command.pre_exec(move || {
            if prctl(1, 9, 0, 0, 0) != 0 || getppid() != parent {
                return Err(std::io::Error::last_os_error());
            }
            // Reduce, never raise, inherited hard limits. Limits are per process, not a cgroup.
            for (resource, budget) in [
                (4u32, 0u64),
                (0, cpu),
                (1, 1073741824),
                (7, 128),
                (9, 1073741824),
                // RLIMIT_NPROC is intentionally owned by the outer DriverProvider sandbox.
                // Linux accounts it against the real UID, so imposing a second fixed limit here
                // can reject legitimate child workers when the host UID already has many tasks.
            ] {
                let mut current = Limit {
                    current: 0,
                    maximum: 0,
                };
                if getrlimit(resource, &mut current) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                let maximum = current.maximum.min(budget);
                let limit = Limit {
                    current: current.current.min(maximum),
                    maximum,
                };
                if setrlimit(resource, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    let child = command.spawn()?;
    let pgid = child.id() as i32;
    let mut child = ChildGuard {
        child,
        group: pgid,
        armed: true,
    };
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::new("Internal", "stdout pipe missing"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::new("Internal", "stderr pipe missing"))?;
    let overflow = Arc::new(AtomicBool::new(false));
    let a = capture(stdout, overflow.clone());
    let b = capture(stderr, overflow.clone());
    let start = Instant::now();
    let mut cancelled = false;
    let mut timed_out = false;
    let status;
    loop {
        if let Some(s) = child.try_wait()? {
            status = s;
            break;
        }
        cancelled = cancel.load(Ordering::Acquire);
        timed_out = start.elapsed() >= spec.timeout;
        if cancelled || timed_out || overflow.load(Ordering::Acquire) {
            // SAFETY: this process group was created for this still-owned child; negative PID targets it.
            unsafe { kill(-pgid, 15) };
            let until = Instant::now() + Duration::from_millis(300);
            while Instant::now() < until {
                if child.try_wait()?.is_some() {
                    break;
                }
                thread::sleep(Duration::from_millis(10));
            }
            // SAFETY: kill all remaining members of the same owned process group before reaping.
            unsafe { kill(-pgid, 9) };
            status = child.wait()?;
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    // SAFETY: clean residual same-group descendants even after a successful parent exit.
    // A production bubblewrap PID namespace also contains descendants that create another group.
    unsafe { kill(-pgid, 9) };
    child.armed = false;
    let stdout = a
        .join()
        .map_err(|_| Error::new("Internal", "stdout reader failed"))?;
    let stderr = b
        .join()
        .map_err(|_| Error::new("Internal", "stderr reader failed"))?;
    Ok(ProcessResult {
        exit_code: status.code(),
        stdout,
        stderr,
        cancelled,
        timed_out,
        output_exceeded: overflow.load(Ordering::Acquire),
    })
}
#[derive(Clone, Debug)]
pub struct Tool {
    pub path: PathBuf,
    pub sha256: String,
}
impl Tool {
    fn parse(v: &Value) -> Result<Self> {
        v.strict(&["path", "sha256"], &["path", "sha256"])?;
        let tool = Self {
            path: v.str("path")?.into(),
            sha256: v.str("sha256")?.into(),
        };
        tool.verify()?;
        Ok(tool)
    }
    pub fn verify(&self) -> Result<File> {
        if !self.path.is_absolute()
            || !valid_digest(&self.sha256)
            || std::fs::canonicalize(&self.path)? != self.path
        {
            return Err(Error::new(
                "PermissionDenied",
                "Tool must have a canonical absolute path and lowercase SHA-256 pin",
            ));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(0o400000 | 0o2000000 | 0o4000)
            .open(&self.path)?;
        let m = file.metadata()?;
        // SAFETY: getuid has no arguments or memory preconditions.
        let uid = unsafe { getuid() };
        if m.uid() != 0 && m.uid() != uid {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool owner must be root or the sandbox uid",
            ));
        }
        if !m.is_file() {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool must be a regular file",
            ));
        }
        if m.nlink() != 1 {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool must have exactly one hard link",
            ));
        }
        if m.mode() & 0o022 != 0 {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool must not be group- or other-writable",
            ));
        }
        if m.mode() & 0o111 == 0 {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool must have an executable mode bit",
            ));
        }
        if m.len() > 64 * 1024 * 1024 {
            return Err(Error::new(
                "PermissionDenied",
                "Pinned tool must not exceed 64 MiB",
            ));
        }
        let mut elf = [0; 4];
        file.read_exact(&mut elf)?;
        if &elf != b"\x7fELF" {
            return Err(Error::invalid("Pinned tool is not ELF"));
        }
        use std::io::{Seek, SeekFrom};
        file.seek(SeekFrom::Start(0))?;
        let (actual, _) = reader_hash(&mut file, 64 * 1024 * 1024)?;
        if actual != self.sha256 {
            return Err(Error::new("PermissionDenied", "Tool digest changed"));
        }
        file.seek(SeekFrom::Start(0))?;
        Ok(file)
    }
}
#[derive(Clone, Debug, Default)]
pub struct ServiceCatalog {
    pub version: String,
    pub groups: BTreeMap<String, BTreeSet<String>>,
}
impl ServiceCatalog {
    pub fn has(&self, group: &str, id: &str) -> bool {
        self.groups.get(group).is_some_and(|s| s.contains(id))
    }
    pub fn parse_list(text: &str) -> Result<BTreeSet<String>> {
        if text.len() > RETAIN_LOG {
            return Err(Error::limit("Service catalog text too large"));
        }
        let mut values = BTreeSet::new();
        for line in text.lines() {
            if let Some(token) = line.trim().strip_prefix("- ") {
                let token = token.trim().trim_matches('"').trim_matches('\'');
                if token.len() <= 128
                    && !token.is_empty()
                    && token.bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.' | b':')
                    })
                {
                    values.insert(token.into());
                }
            }
            if values.len() > 2048 {
                return Err(Error::limit("Runtime service catalog budget exceeded"));
            }
        }
        Ok(values)
    }
}
pub struct Runtime {
    pub melt: Tool,
    pub ffprobe: Tool,
    pub bubblewrap: Tool,
    pub timeout: Duration,
    pub catalog: ServiceCatalog,
    tools: PrivateDir,
}
impl Runtime {
    pub fn load(v: &Value) -> Result<Self> {
        v.strict(
            &["schema", "melt", "ffprobe", "bubblewrap", "timeout_seconds"],
            &["schema", "melt", "ffprobe", "bubblewrap", "timeout_seconds"],
        )?;
        if v.uint("schema")? != 1 {
            return Err(Error::unsupported("Runtime configuration schema"));
        }
        let melt = Tool::parse(v.get("melt")?)?;
        let ffprobe = Tool::parse(v.get("ffprobe")?)?;
        let bubblewrap = Tool::parse(v.get("bubblewrap")?)?;
        let timeout = v.uint("timeout_seconds")?;
        if !(1..=3600).contains(&timeout) {
            return Err(Error::invalid("Render timeout must be 1..3600 seconds"));
        }
        let tools = PrivateDir::new(Path::new("/tmp"))?;
        // Stage the media tools so the bytes executed later are exactly the pinned
        // bytes we verified. Bubblewrap is different: Linux/AppArmor installations can
        // grant user-namespace permission specifically to its canonical system path.
        // Keep that root-owned, non-writable path and re-verify its digest before use.
        for (name, tool) in [("melt", &melt), ("ffprobe", &ffprobe)] {
            let mut source = tool.verify()?;
            let mut destination = tools.create(name)?;
            let mut hash = crate::hash::Sha256::new();
            let mut size = 0usize;
            let mut buffer = [0; 65536];
            loop {
                let count = source.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                size += count;
                if size > 64 * 1024 * 1024 {
                    return Err(Error::limit("Tool changed beyond staged byte budget"));
                }
                destination.write_all(&buffer[..count])?;
                hash.update(&buffer[..count]);
            }
            if hash.finish() != tool.sha256 {
                return Err(Error::new(
                    "PermissionDenied",
                    "Tool bytes changed while staging",
                ));
            }
            destination.sync_all()?;
            std::fs::set_permissions(
                tools.path().join(name),
                std::fs::Permissions::from_mode(0o500),
            )?;
        }
        let mut runtime = Self {
            melt,
            ffprobe,
            bubblewrap,
            timeout: Duration::from_secs(timeout),
            catalog: ServiceCatalog::default(),
            tools,
        };
        runtime.discover()?;
        Ok(runtime)
    }
    fn spec(
        &self,
        tool: &str,
        args: Vec<OsString>,
        inputs: &Path,
        work: &Path,
        timeout: Duration,
    ) -> Result<ProcessSpec> {
        self.bubblewrap.verify()?;
        let mut argv: Vec<OsString> = [
            "--die-with-parent",
            "--new-session",
            "--unshare-all",
            "--clearenv",
            "--cap-drop",
            "ALL",
            "--proc",
            "/proc",
            "--dev",
            "/dev",
            "--tmpfs",
            "/tmp",
            "--dir",
            "/home",
            "--dir",
            "/etc",
            "--dir",
            "/tools",
            "--dir",
            "/inputs",
            "--dir",
            "/work",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        for path in ["/usr", "/lib", "/lib64"] {
            if Path::new(path).exists() {
                argv.extend(["--ro-bind".into(), path.into(), path.into()]);
            }
        }
        if Path::new("/etc/ld.so.cache").exists() {
            argv.extend([
                "--ro-bind".into(),
                "/etc/ld.so.cache".into(),
                "/etc/ld.so.cache".into(),
            ]);
        }
        argv.extend([
            "--ro-bind".into(),
            self.tools.path().as_os_str().into(),
            "/tools".into(),
            "--ro-bind".into(),
            inputs.as_os_str().into(),
            "/inputs".into(),
            "--bind".into(),
            work.as_os_str().into(),
            "/work".into(),
        ]);
        for (key, value) in [
            ("LC_ALL", "C"),
            ("HOME", "/home"),
            ("PATH", "/usr/bin:/bin"),
            ("QT_QPA_PLATFORM", "offscreen"),
            ("XDG_CONFIG_HOME", "/tmp/config"),
            ("XDG_CACHE_HOME", "/tmp/cache"),
            ("MLT_NO_VDPAU", "1"),
        ] {
            argv.extend(["--setenv".into(), key.into(), value.into()]);
        }
        argv.extend([
            "--chdir".into(),
            "/work".into(),
            "--".into(),
            format!("/tools/{tool}").into(),
        ]);
        argv.extend(args);
        Ok(ProcessSpec {
            executable: self.bubblewrap.path.clone(),
            args: argv,
            cwd: work.into(),
            timeout,
            cpu_seconds: 120,
            environment: BTreeMap::from([("LC_ALL".into(), "C".into())]),
        })
    }
    fn discover(&mut self) -> Result<()> {
        let inputs = PrivateDir::new(Path::new("/tmp"))?;
        let work = PrivateDir::new(Path::new("/tmp"))?;
        let cancel = AtomicBool::new(false);
        let result = run(
            &self.spec(
                "melt",
                vec!["-version".into()],
                inputs.path(),
                work.path(),
                Duration::from_secs(2),
            )?,
            &cancel,
        )?
        .checked()
        .map_err(|error| {
            Error::new(
                error.code,
                format!("melt -version discovery failed: {}", error.message),
            )
        })?;
        let text = format!(
            "{} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        self.catalog.version = json::display(
            text.lines()
                .find(|l| l.to_ascii_lowercase().contains("melt"))
                .unwrap_or("Version output did not identify melt"),
        );
        for group in [
            "producers",
            "filters",
            "transitions",
            "consumers",
            "video_codecs",
            "audio_codecs",
        ] {
            let result = run(
                &self.spec(
                    "melt",
                    vec!["-query".into(), group.into()],
                    inputs.path(),
                    work.path(),
                    Duration::from_secs(2),
                )?,
                &cancel,
            )?
            .checked()
            .map_err(|error| {
                Error::new(
                    error.code,
                    format!("melt -query {group} discovery failed: {}", error.message),
                )
            })?;
            let text = format!(
                "{}\n{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
            self.catalog
                .groups
                .insert(group.into(), ServiceCatalog::parse_list(&text)?);
        }
        Ok(())
    }
    pub fn probe(
        &self,
        inputs: &Path,
        work: &Path,
        name: &str,
        cancel: &AtomicBool,
    ) -> Result<MediaInfo> {
        crate::fs::validate_relative(name)?;
        if name.contains('/') {
            return Err(Error::invalid("Probe accepts one staged basename"));
        }
        let args = [
            "-v",
            "error",
            "-show_streams",
            "-show_format",
            "-of",
            "json",
        ]
        .into_iter()
        .map(OsString::from)
        .chain(std::iter::once(format!("/inputs/{name}").into()))
        .collect();
        let r = run(
            &self.spec("ffprobe", args, inputs, work, Duration::from_secs(5))?,
            cancel,
        )?
        .checked()?;
        MediaInfo::parse(&r.stdout)
    }
    pub fn render(
        &self,
        inputs: &Path,
        work: &Path,
        profile: &RenderProfile,
        cancel: &AtomicBool,
    ) -> Result<ProcessResult> {
        let mut args: Vec<OsString> = vec![
            "/inputs/project.mlt".into(),
            "-silent".into(),
            "-consumer".into(),
            format!("avformat:/work/partial.{}", profile.extension).into(),
            format!("f={}", profile.container).into(),
            "real_time=-1".into(),
            "threads=2".into(),
        ];
        if let Some(codec) = profile.video_codec {
            args.push(format!("vcodec={codec}").into());
            if codec == "libx264" {
                args.extend([
                    "pix_fmt=yuv420p".into(),
                    "crf=20".into(),
                    "preset=medium".into(),
                ]);
            }
        } else {
            args.push("vn=1".into());
        }
        args.push(format!("acodec={}", profile.audio_codec).into());
        run(
            &self.spec("melt", args, inputs, work, self.timeout)?,
            cancel,
        )?
        .checked()
    }
    pub fn validate_output(
        &self,
        work: &Path,
        profile: &RenderProfile,
        expected: &Profile,
        frames: u64,
        cancel: &AtomicBool,
    ) -> Result<MediaInfo> {
        let info = self.probe(
            work,
            work,
            &format!("partial.{}", profile.extension),
            cancel,
        )?;
        if info.duration_num == 0 || info.duration_den == 0 {
            return Err(Error::new(
                "BackendFailed",
                "Rendered artifact has no measurable duration",
            ));
        }
        let actual = u128::from(info.duration_num) * u128::from(expected.fps.num);
        let wanted =
            u128::from(frames) * u128::from(expected.fps.den) * u128::from(info.duration_den);
        let tolerance = u128::from(expected.fps.den) * u128::from(info.duration_den);
        if actual.abs_diff(wanted) > tolerance {
            return Err(Error::new(
                "BackendFailed",
                "Rendered duration differs by more than one project frame",
            ));
        }
        if profile.video_codec.is_some() {
            if info.width != Some(expected.width) || info.height != Some(expected.height) {
                return Err(Error::new(
                    "BackendFailed",
                    "Rendered dimensions do not match plan",
                ));
            }
            if info.frames.is_some_and(|f| f != frames) {
                return Err(Error::new(
                    "BackendFailed",
                    "Rendered decoded/frame-count metadata differs from plan",
                ));
            }
        }
        if !info.audio {
            return Err(Error::new(
                "BackendFailed",
                "Curated render profile requires an audio stream",
            ));
        }
        Ok(info)
    }
}
#[derive(Clone, Debug)]
pub struct RenderProfile {
    pub id: &'static str,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub video_codec: Option<&'static str>,
    pub audio_codec: &'static str,
    pub container: &'static str,
    pub extension: &'static str,
}
impl RenderProfile {
    pub fn all() -> Vec<Self> {
        vec![
            Self {
                id: "h264-1080p",
                width: Some(1920),
                height: Some(1080),
                video_codec: Some("libx264"),
                audio_codec: "aac",
                container: "mp4",
                extension: "mp4",
            },
            Self {
                id: "h264-720p",
                width: Some(1280),
                height: Some(720),
                video_codec: Some("libx264"),
                audio_codec: "aac",
                container: "mp4",
                extension: "mp4",
            },
            Self {
                id: "lossless",
                width: None,
                height: None,
                video_codec: Some("ffv1"),
                audio_codec: "pcm_s16le",
                container: "matroska",
                extension: "mkv",
            },
            Self {
                id: "audio-wav",
                width: None,
                height: None,
                video_codec: None,
                audio_codec: "pcm_s16le",
                container: "wav",
                extension: "wav",
            },
        ]
    }
    pub fn get(id: &str) -> Result<Self> {
        Self::all()
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| Error::invalid("Unknown render profile"))
    }
    pub fn available(&self, catalog: &ServiceCatalog) -> bool {
        catalog.has("consumers", "avformat")
            && catalog.has("audio_codecs", self.audio_codec)
            && self
                .video_codec
                .is_none_or(|v| catalog.has("video_codecs", v))
    }
    pub fn json(&self, catalog: Option<&ServiceCatalog>) -> Value {
        obj([
            ("id", self.id.into()),
            (
                "available",
                catalog.is_some_and(|c| self.available(c)).into(),
            ),
            (
                "width",
                self.width.map_or(Value::Null, |v| u64::from(v).into()),
            ),
            (
                "height",
                self.height.map_or(Value::Null, |v| u64::from(v).into()),
            ),
            (
                "video_codec",
                self.video_codec.map_or(Value::Null, Into::into),
            ),
            ("audio_codec", self.audio_codec.into()),
            ("container", self.container.into()),
            ("extension", self.extension.into()),
        ])
    }
}
#[derive(Clone, Debug, Default)]
pub struct MediaInfo {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub frames: Option<u64>,
    pub duration_num: u64,
    pub duration_den: u64,
    pub audio: bool,
    pub video: bool,
    pub codecs: Vec<String>,
}
impl MediaInfo {
    /// Capacity expressed in project frames, rounded up once at the last partial frame.
    pub fn frame_capacity(&self, rate: crate::time::FrameRate) -> Result<Option<u64>> {
        rate.validate()?;
        if self.duration_num == 0 || self.duration_den == 0 {
            return Ok(None);
        }
        let numerator = u128::from(self.duration_num) * u128::from(rate.num);
        let denominator = u128::from(self.duration_den) * u128::from(rate.den);
        let frames = numerator.div_ceil(denominator);
        Ok(Some(u64::try_from(frames).map_err(|_| {
            Error::limit("Media duration exceeds frame capacity")
        })?))
    }

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let v = json::parse(bytes)?;
        let streams = v.get("streams")?.as_array()?;
        if streams.is_empty() || streams.len() > 16 {
            return Err(Error::limit("Probe stream count outside budget"));
        }
        let mut info = Self::default();
        for s in streams {
            let kind = s.str("codec_type")?;
            if let Some(codec) = s.opt("codec_name").and_then(|v| v.string().ok()) {
                info.codecs.push(json::display(codec));
            }
            if kind == "video" && !info.video {
                info.video = true;
                info.width = s
                    .opt("width")
                    .map(Value::u64)
                    .transpose()?
                    .and_then(|n| u32::try_from(n).ok());
                info.height = s
                    .opt("height")
                    .map(Value::u64)
                    .transpose()?
                    .and_then(|n| u32::try_from(n).ok());
                info.frames = s
                    .opt("nb_frames")
                    .and_then(|v| v.string().ok())
                    .and_then(|s| s.parse().ok());
            }
            if kind == "audio" {
                info.audio = true;
            }
            if info.duration_num == 0 {
                if let (Some(ts), Some(tb)) = (
                    s.opt("duration_ts").and_then(|v| v.u64().ok()),
                    s.opt("time_base").and_then(|v| v.string().ok()),
                ) {
                    if let Some((a, b)) = tb.split_once('/') {
                        let a = a
                            .parse::<u64>()
                            .map_err(|_| Error::invalid("Probe time base numerator"))?;
                        let b = b
                            .parse::<u64>()
                            .map_err(|_| Error::invalid("Probe time base denominator"))?;
                        if b > 0 {
                            info.duration_num = ts
                                .checked_mul(a)
                                .ok_or_else(|| Error::invalid("Probe duration overflow"))?;
                            info.duration_den = b;
                        }
                    }
                }
            }
        }
        if info.duration_num == 0 {
            if let Some(text) = v
                .opt("format")
                .and_then(|f| f.opt("duration"))
                .and_then(|v| v.string().ok())
            {
                let (a, b) = text.split_once('.').unwrap_or((text, ""));
                if b.len() > 9 || !a.bytes().chain(b.bytes()).all(|c| c.is_ascii_digit()) {
                    return Err(Error::invalid("Invalid decimal probe duration"));
                }
                let den = 10u64.pow(b.len() as u32);
                let n = a
                    .parse::<u64>()
                    .map_err(|_| Error::invalid("Probe duration integer"))?
                    .checked_mul(den)
                    .and_then(|n| n.checked_add(b.parse::<u64>().unwrap_or(0)))
                    .ok_or_else(|| Error::invalid("Probe duration overflow"))?;
                info.duration_num = n;
                info.duration_den = den;
            }
        }
        Ok(info)
    }
    pub fn json(&self) -> Value {
        obj([
            (
                "width",
                self.width.map_or(Value::Null, |n| u64::from(n).into()),
            ),
            (
                "height",
                self.height.map_or(Value::Null, |n| u64::from(n).into()),
            ),
            ("frames", self.frames.map_or(Value::Null, Into::into)),
            ("duration_num", self.duration_num.into()),
            ("duration_den", self.duration_den.into()),
            ("audio", self.audio.into()),
            ("video", self.video.into()),
            ("codecs", array(self.codecs.iter().cloned().map(Into::into))),
        ])
    }
}
