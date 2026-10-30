use semwright_backend_api::{Context, Provider};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DRIVER_PROTOCOL_VERSION, DriverInterfaces, DriverMount, DriverResources,
    DriverToolMount, Manifest, Transport,
};
use semwright_policy::FilesystemGrant;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    hex::encode(Sha256::digest(std::fs::read(path).unwrap()))
}

fn configured_tool(variable: &str) -> PathBuf {
    std::fs::canonicalize(
        std::env::var_os(variable)
            .unwrap_or_else(|| panic!("{variable} must point to the real executable")),
    )
    .unwrap()
}

fn audio_live_custody_sdk_error(after_effect: bool) -> semwright_types::Error {
    let error = semwright_types::Error::new(
        semwright_types::ErrorCode::BackendFailed,
        "MLT live original custody is incomplete; the attempt is stopped without replay",
    );
    if after_effect {
        error.uncertain()
    } else {
        error
    }
}

impl AudioLiveOriginals {
    fn begin_call(
        &self,
        context: &Context,
        name: &str,
        descriptor: &Value,
        args: &Value,
    ) -> AudioLiveResult<String> {
        self.before_effect()?;
        if self.terminal_case.is_some() {
            self.terminal_started
                .compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                )
                .map_err(|_| std::io::Error::other("Terminal negative scope already called"))?;
        }
        let index = self
            .calls
            .fetch_update(
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
                |value| {
                    value
                        .checked_add(1)
                        .filter(|next| *next <= AUDIO_LIVE_CALL_LIMIT)
                },
            )
            .map_err(|_| std::io::Error::other("Live provider call record budget exhausted"))?
            + 1;
        let stem = format!("call-{index:05}");
        self.record(&format!("{stem}.request.original.json"), &json!({
            "schema":"semwright.mlt-live-provider-request-original/1",
            "terminal_case":self.terminal_case,
            "context":{"session":context.session,"request_id":context.request_id,
                "cancellation_requested":context.cancellation.is_cancelled()},
            "capability":name,"descriptor":descriptor,"arguments":args,
            "capture_scope":"same-public-Context-descriptor-and-Value-before-single-Provider-execute",
            "wire_bytes_claimed":false,"authority_issued":false,"native_execution_retried":false
        }))?;
        Ok(stem)
    }

    fn finish_call(
        &self,
        stem: &str,
        original: semwright_types::Result<Value>,
    ) -> semwright_types::Result<Value> {
        if self.terminal_case.is_some() {
            self.stop();
        }
        let retained = (|| -> AudioLiveResult<()> {
            self.record(
                &format!("{stem}.result.original.json"),
                &json!({
                    "schema":"semwright.mlt-live-provider-result-original/1",
                    "terminal_case":self.terminal_case,
                    "result":serde_json::to_value(&original)?,
                    "capture_scope":"complete-returned-public-SDK-Result-before-interpretation",
                    "host_stream_eof_attested":false,"native_execution_retried":false
                }),
            )
        })();
        match original {
            Err(original) => {
                if !original.outcome_known {
                    self.stop();
                }
                if let Err(secondary) = retained {
                    self.stop();
                    self.secondary(
                        &format!("{stem}.custody-error.original.json"),
                        secondary.as_ref(),
                    );
                }
                // Never reconstruct the original SDK Error or discard its
                // progress, candidates, context, diagnostic or UNKNOWN state.
                Err(original)
            }
            Ok(value) => match retained {
                Ok(()) => Ok(value),
                Err(secondary) => {
                    self.stop();
                    self.secondary(
                        &format!("{stem}.custody-error.original.json"),
                        secondary.as_ref(),
                    );
                    Err(audio_live_custody_sdk_error(true))
                }
            },
        }
    }
}

async fn call(
    originals: &AudioLiveOriginals,
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    name: &str,
    args: Value,
) -> semwright_types::Result<Value> {
    let context = Context {
        session: originals.terminal_case.map_or_else(
            || "mlt-video-live".into(),
            |case| format!("mlt-video-live-negative-{case}"),
        ),
        request_id: semwright_types::unique_id(),
        cancellation: CancellationToken::new(),
    };
    let capability = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == name)
        .ok_or_else(|| {
            semwright_types::Error::new(
                semwright_types::ErrorCode::Unsupported,
                "Required MLT live capability is absent",
            )
        })?;
    let prepared = (|| -> AudioLiveResult<String> {
        let descriptor = serde_json::to_value(&capability.descriptor)?;
        originals.begin_call(&context, name, &descriptor, &args)
    })();
    let stem = match prepared {
        Ok(stem) => stem,
        Err(secondary) => {
            originals.stop();
            originals.secondary(
                "provider-request-custody-error.original.json",
                secondary.as_ref(),
            );
            return Err(audio_live_custody_sdk_error(false));
        }
    };
    // One existing public dispatcher and one actual Context. Retention never
    // invokes the provider a second time or manufactures a transport envelope.
    let original = Provider::execute(provider, &context, &capability.descriptor, &args).await;
    originals.finish_call(&stem, original)
}

fn audio_live_expect_known_error(
    original: semwright_types::Result<Value>,
) -> AudioLiveResult<semwright_types::Error> {
    match original {
        Err(error) if error.outcome_known => Ok(error),
        Err(error) => Err(Box::new(error)),
        Ok(_) => {
            Err(std::io::Error::other("Expected a known rejection from the live provider").into())
        }
    }
}

fn audio_live_observe_case_files(
    originals: &AudioLiveOriginals,
    case: AudioLiveCase,
) -> AudioLiveResult<Value> {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let name = case.output_name();
    let files = [
        (name.clone(), 40 * 1024 * 1024),
        (
            format!("{name}.audio-derive.source.original.wav"),
            6 * 1024 * 1024,
        ),
        (
            format!("{name}.audio-derive.output.original.wav"),
            40 * 1024 * 1024,
        ),
        (
            format!("{name}.audio-derive.ffmpeg.argv.original.json"),
            AUDIO_LIVE_METADATA_LIMIT,
        ),
        (
            format!("{name}.audio-derive.ffmpeg.stdout.original.bin"),
            AUDIO_LIVE_TOOL_PREFIX_LIMIT,
        ),
        (
            format!("{name}.audio-derive.ffmpeg.stderr.original.bin"),
            AUDIO_LIVE_TOOL_PREFIX_LIMIT,
        ),
        (
            format!("{name}.audio-derive.ffmpeg.terminal.original.json"),
            AUDIO_LIVE_METADATA_LIMIT,
        ),
        (
            format!("{name}.audio-derive-host.stdin.original.bin"),
            AUDIO_LIVE_METADATA_LIMIT,
        ),
        (
            format!("{name}.audio-derive-host.stdout.original.bin"),
            AUDIO_LIVE_TOOL_PREFIX_LIMIT,
        ),
        (
            format!("{name}.audio-derive-host.stderr.original.bin"),
            AUDIO_LIVE_TOOL_PREFIX_LIMIT,
        ),
        (
            format!("{name}.audio-derive-host.terminal.original.json"),
            AUDIO_LIVE_METADATA_LIMIT,
        ),
        (
            format!("{name}.audio-derive.operation.original.json"),
            AUDIO_LIVE_METADATA_LIMIT,
        ),
        (
            format!("{name}.audio-derive-catalog.stdin.original.bin"),
            semwright_mlt_video::runtime::AUDIO_DERIVATIVE_STDIN_LIMIT,
        ),
        (
            format!("{name}.audio-derive-catalog.stdout.original.bin"),
            semwright_mlt_video::runtime::AUDIO_DERIVATIVE_STREAM_LIMIT,
        ),
        (
            format!("{name}.audio-derive-catalog.stderr.original.bin"),
            semwright_mlt_video::runtime::AUDIO_DERIVATIVE_STREAM_LIMIT,
        ),
        (
            format!("{name}.audio-derive-catalog.terminal.original.json"),
            semwright_mlt_video::runtime::AUDIO_DERIVATIVE_METADATA_LIMIT,
        ),
    ];
    let mut rows = Vec::new();
    let mut first_error: Option<Box<dyn std::error::Error + Send + Sync>> = None;
    for (path, maximum) in files {
        let opened = originals.output.read_file(&path, u64::MAX);
        let mut file = match opened {
            Err(error) if error.code == "NotFound" => {
                rows.push(
                    json!({"root":"output","path":path,"state":"NOT_PRESENT_AT_OBSERVATION",
                    "producer_effect_absent_claimed":false}),
                );
                continue;
            }
            Err(error) => {
                rows.push(json!({"root":"output","path":path,"state":"OPEN_FAILED",
                    "original_error":error.to_string(),"bytes":null,"sha256":null}));
                if first_error.is_none() {
                    first_error = Some(Box::new(error));
                }
                continue;
            }
            Ok(file) => file,
        };
        let before = file.metadata();
        let mut bytes = Vec::new();
        let read = file
            .by_ref()
            .take(maximum as u64 + 1)
            .read_to_end(&mut bytes);
        let after = file.metadata();
        let over = bytes.len() > maximum;
        if over {
            bytes.truncate(maximum);
        }
        let stable = match (&before, &after) {
            (Ok(before), Ok(after)) => {
                before.dev() == after.dev()
                    && before.ino() == after.ino()
                    && before.len() == after.len()
                    && after.len() == bytes.len() as u64
                    && before.mtime() == after.mtime()
                    && before.mtime_nsec() == after.mtime_nsec()
                    && before.ctime() == after.ctime()
                    && before.ctime_nsec() == after.ctime_nsec()
            }
            _ => false,
        };
        let complete_file_snapshot = read.is_ok() && !over && stable;
        rows.push(json!({
            "root":"output","path":path,"state":if complete_file_snapshot {"FILE_SNAPSHOT_OBSERVED"} else {"INCOMPLETE_FILE_SNAPSHOT"},
            "maximum_inspected_bytes":maximum,"inspected_bytes":bytes.len(),
            "inspected_sha256":semwright_mlt_video::hash::sha256(&bytes),
            "metadata_bytes_before":before.as_ref().ok().map(std::fs::Metadata::len),
            "metadata_bytes_after":after.as_ref().ok().map(std::fs::Metadata::len),
            "metadata_error_before":before.as_ref().err().map(ToString::to_string),
            "metadata_error_after":after.as_ref().err().map(ToString::to_string),
            "read_error":read.as_ref().err().map(ToString::to_string),
            "over_inspection_limit":over,"same_file_snapshot":stable,
            "producer_eof_attested":false,"descendant_closure_attested":false,
            "original_retention_location":"same-owner-output-root-no-recapture"
        }));
        if first_error.is_none() {
            first_error = match read {
                Err(error) => Some(Box::new(error)),
                Ok(_) if !complete_file_snapshot => Some(Box::new(std::io::Error::other(
                    "Audio derive original file snapshot is incomplete",
                ))),
                Ok(_) => None,
            };
        }
    }
    let observation = json!({
        "schema":"semwright.audio-derive-live-file-observation/1","case":case.id,"files":rows,
        "captured_before_result_unwrap_or_oracle":true,"native_execution_retried":false
    });
    let retained = originals.record(
        &format!("audio-live-{}.backend-files.original.json", case.id),
        &observation,
    );
    if let Some(original) = first_error {
        originals.stop();
        if let Err(secondary) = retained {
            originals.secondary(
                &format!("audio-live-{}.file-custody-error.original.json", case.id),
                secondary.as_ref(),
            );
        }
        return Err(original);
    }
    retained.map(|()| observation)
}

fn created(value: &Value, kind: &str) -> (String, String) {
    let row = value["created_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["kind"].as_str() == Some(kind))
        .unwrap_or_else(|| panic!("missing created {kind} ref"));
    (
        row["id"].as_str().unwrap().to_owned(),
        row["reference"].as_str().unwrap().to_owned(),
    )
}

fn page_ref(value: &Value, name: &str) -> String {
    value["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["name"].as_str() == Some(name))
        .and_then(|row| row["reference"].as_str())
        .unwrap_or_else(|| panic!("missing live ref for {name}"))
        .to_owned()
}

async fn sequence_ref(
    originals: &AudioLiveOriginals,
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    project: &str,
) -> AudioLiveResult<String> {
    let sequences = call(
        originals,
        provider,
        capabilities,
        "driver.mlt-video.sequence.list",
        json!({"project":project,"limit":100}),
    )
    .await?;
    Ok(page_ref(&sequences, "Main"))
}

// This selects a private test destination created by the external custodian.
// It grants no tool, filesystem, provider, source or execution authority.
const AUDIO_LIVE_ORIGINALS_ENV: &str = "SEMWRIGHT_TEST_MLT_VIDEO_EVIDENCE_ROOT";
const AUDIO_LIVE_COMMAND: &str = "driver.mlt-video.audio.derive";
const AUDIO_LIVE_METADATA_LIMIT: usize = 65_536;
const AUDIO_LIVE_CAPABILITIES_LIMIT: usize = 1_048_576;
const AUDIO_LIVE_RECORDS_LIMIT: u64 = 134_217_728;
const AUDIO_LIVE_CALL_LIMIT: usize = 16_384;
const AUDIO_LIVE_TOOL_PREFIX_LIMIT: usize = 262_144;
const AUDIO_LIVE_ROOTS: [&str; 8] = [
    "binaries", "project", "media", "output", "runtime", "scratch", "state", "records",
];
const AUDIO_LIVE_LEVELS: [i16; 4] = [4096, -8192, 12288, 0];
const AUDIO_LIVE_LEVEL_ERROR_LSB: i32 = 1;
type AudioLiveResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn audio_live_require(condition: bool, message: &'static str) -> AudioLiveResult<()> {
    if condition {
        Ok(())
    } else {
        Err(std::io::Error::other(message).into())
    }
}

fn audio_live_selector(value: Option<&str>) -> AudioLiveResult<()> {
    audio_live_require(
        value == Some("1"),
        "Explicit ignored live execution requires SEMWRIGHT_TEST_MLT_VIDEO=1",
    )
}

fn audio_live_excluded_locations() -> AudioLiveResult<Vec<PathBuf>> {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| std::io::Error::other("SDK checkout root unavailable"))?
        .canonicalize()?;
    let workspace = std::env::var_os("GITHUB_WORKSPACE").ok_or_else(|| {
        std::io::Error::other(
            "GITHUB_WORKSPACE is required to exclude the CI source workspace before live effects",
        )
    })?;
    let working = std::env::current_dir()?.canonicalize()?;
    let mut paths = vec![
        checkout.clone(),
        checkout.join("target"),
        working.clone(),
        PathBuf::from(workspace),
    ];
    for name in [
        "CARGO_TARGET_DIR",
        "CARGO_HOME",
        "RUSTUP_HOME",
        "RUNNER_TOOL_CACHE",
        "XDG_CACHE_HOME",
    ] {
        if let Some(value) = std::env::var_os(name) {
            paths.push(PathBuf::from(value));
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home_path = PathBuf::from(home);
        paths.extend([
            home_path.join(".cargo"),
            home_path.join(".rustup"),
            home_path.join(".cache"),
        ]);
    }
    let mut locations = Vec::new();
    for path in paths {
        let path = if path.is_absolute() {
            path
        } else {
            working.join(path)
        };
        locations.push(path.clone());
        match path.canonicalize() {
            Ok(observed) => locations.push(observed),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    locations.sort();
    locations.dedup();
    Ok(locations)
}

#[derive(Clone)]
struct RetainedLiveDirectory {
    path: PathBuf,
    handle: std::sync::Arc<std::fs::File>,
    device: u64,
    inode: u64,
    uid: u32,
    mode: u32,
}

impl RetainedLiveDirectory {
    fn path(&self) -> &Path {
        &self.path
    }

    fn from_file(path: PathBuf, handle: std::fs::File) -> AudioLiveResult<Self> {
        use std::os::unix::fs::MetadataExt;
        let metadata = handle.metadata()?;
        audio_live_require(metadata.is_dir(), "Live evidence handle is not a directory")?;
        Ok(Self {
            path,
            handle: std::sync::Arc::new(handle),
            device: metadata.dev(),
            inode: metadata.ino(),
            uid: metadata.uid(),
            mode: metadata.mode(),
        })
    }

    fn check(&self) -> AudioLiveResult<()> {
        use std::os::unix::fs::MetadataExt;
        let retained = self.handle.metadata()?;
        let named = std::fs::symlink_metadata(&self.path)?;
        for metadata in [retained, named] {
            audio_live_require(
                metadata.is_dir()
                    && !metadata.file_type().is_symlink()
                    && metadata.dev() == self.device
                    && metadata.ino() == self.inode
                    && metadata.uid() == self.uid
                    && metadata.mode() == self.mode,
                "Live evidence directory binding changed",
            )?;
        }
        Ok(())
    }

    fn children(&self) -> AudioLiveResult<Vec<std::ffi::OsString>> {
        use std::os::fd::AsRawFd;
        let mut names = std::fs::read_dir(format!("/proc/self/fd/{}", self.handle.as_raw_fd()))?
            .map(|entry| entry.map(|entry| entry.file_name()))
            .collect::<std::io::Result<Vec<_>>>()?;
        names.sort();
        Ok(names)
    }

    fn observation(&self) -> Value {
        json!({"path":self.path,"device":self.device,"inode":self.inode,
            "uid":self.uid,"mode":self.mode,"handle_retained":true})
    }
}

// Open every component without following symlinks and retain each directory
// descriptor. This is destination integrity, not a replacement for SDK grants.
fn audio_live_open_chain(path: &Path) -> AudioLiveResult<Vec<RetainedLiveDirectory>> {
    use std::os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    };
    audio_live_require(path.is_absolute(), "Live evidence root must be absolute")?;
    let handle = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open("/")?;
    let mut chain = vec![RetainedLiveDirectory::from_file(
        PathBuf::from("/"),
        handle,
    )?];
    let mut named = PathBuf::from("/");
    for component in path.components() {
        let name = match component {
            std::path::Component::RootDir => continue,
            std::path::Component::Normal(name) => name,
            _ => return Err(std::io::Error::other("Live evidence root has a path alias").into()),
        };
        let name_c = std::ffi::CString::new(name.as_bytes())?;
        let parent = chain
            .last()
            .ok_or_else(|| std::io::Error::other("Missing evidence ancestor"))?;
        // SAFETY: parent FD and NUL-terminated component remain alive; success
        // returns one owned descriptor. O_DIRECTORY cannot open a FIFO.
        let descriptor = unsafe {
            libc::openat(
                parent.handle.as_raw_fd(),
                name_c.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: descriptor is newly returned by the successful openat above.
        let handle = unsafe { std::fs::File::from_raw_fd(descriptor) };
        named.push(name);
        chain.push(RetainedLiveDirectory::from_file(named.clone(), handle)?);
    }
    for directory in &chain {
        directory.check()?;
    }
    Ok(chain)
}

struct AudioLiveOriginals {
    media_path: RetainedLiveDirectory,
    output_path: RetainedLiveDirectory,
    media: semwright_mlt_video::fs::Root,
    output: semwright_mlt_video::fs::Root,
    records: semwright_mlt_video::fs::Root,
    directories: Vec<RetainedLiveDirectory>,
    ancestors: Vec<RetainedLiveDirectory>,
    excluded_locations: Vec<PathBuf>,
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    record_bytes: std::sync::Arc<std::sync::atomic::AtomicU64>,
    custody_failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
    terminal_case: Option<&'static str>,
    terminal_started: std::sync::atomic::AtomicBool,
    blocked: std::sync::atomic::AtomicBool,
}

impl AudioLiveOriginals {
    fn open(selected: Option<PathBuf>) -> AudioLiveResult<Self> {
        let path = selected.ok_or_else(|| {
            std::io::Error::other(
                "SEMWRIGHT_TEST_MLT_VIDEO_EVIDENCE_ROOT is required for explicit live execution",
            )
        })?;
        let excluded = audio_live_excluded_locations()?;
        Self::open_with_exclusions(path, &excluded)
    }

    fn open_with_exclusions(path: PathBuf, excluded: &[PathBuf]) -> AudioLiveResult<Self> {
        Self::open_scope(path, excluded, None)
    }

    fn open_scope(
        path: PathBuf,
        excluded: &[PathBuf],
        terminal: Option<(&'static str, &Self)>,
    ) -> AudioLiveResult<Self> {
        if let Some((_, positive)) = terminal {
            positive.before_effect()?;
        }
        use std::os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::MetadataExt,
        };
        audio_live_require(
            path.is_absolute() && path != Path::new("/"),
            "Audio live evidence root must be an absolute non-root directory",
        )?;
        audio_live_require(
            !excluded.is_empty() && excluded.iter().all(|value| value.is_absolute()),
            "Audio live checkout/cache exclusions must be explicit absolute locations",
        )?;
        audio_live_require(
            !excluded.iter().any(|value| path.starts_with(value)),
            "Audio live evidence must be outside checkouts and caches",
        )?;
        let ancestors = audio_live_open_chain(&path)?;
        let root = ancestors
            .last()
            .ok_or_else(|| std::io::Error::other("Missing evidence root"))?;
        // SAFETY: geteuid only reads the current process's effective identity.
        let owner = unsafe { libc::geteuid() };
        let root_metadata = root.handle.metadata()?;
        audio_live_require(
            root.uid == owner
                && root_metadata.mode() & 0o7777 == 0o700
                && path.canonicalize()? == path,
            "Audio live evidence root is not a private canonical owner directory",
        )?;
        audio_live_require(
            root.children()?.is_empty(),
            "Audio live evidence root must initially be empty",
        )?;
        // Every rejection above precedes any directory creation, file copy,
        // fixture command or provider connection. Partial setup is retained.
        let mut directories = Vec::new();
        for name in AUDIO_LIVE_ROOTS {
            root.check()?;
            let name_c = std::ffi::CString::new(name)?;
            // SAFETY: root is retained and each name is a fixed direct basename.
            if unsafe { libc::mkdirat(root.handle.as_raw_fd(), name_c.as_ptr(), 0o700) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: successful openat returns a uniquely owned directory FD.
            let descriptor = unsafe {
                libc::openat(
                    root.handle.as_raw_fd(),
                    name_c.as_ptr(),
                    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                )
            };
            if descriptor < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: descriptor belongs to the successful openat immediately above.
            let handle = unsafe { std::fs::File::from_raw_fd(descriptor) };
            let directory = RetainedLiveDirectory::from_file(path.join(name), handle)?;
            audio_live_require(
                directory.uid == owner && directory.mode & 0o7777 == 0o700,
                "Audio live subroot is not private to its owner",
            )?;
            directory.check()?;
            directories.push(directory);
        }
        let value = Self {
            media_path: directories[2].clone(),
            output_path: directories[3].clone(),
            media: semwright_mlt_video::fs::Root::open(&path.join("media"), true, true)?,
            output: semwright_mlt_video::fs::Root::open(&path.join("output"), true, true)?,
            records: semwright_mlt_video::fs::Root::open(&path.join("records"), true, true)?,
            directories,
            ancestors,
            excluded_locations: excluded.to_vec(),
            calls: terminal.map_or_else(
                || std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0)),
                |(_, positive)| positive.calls.clone(),
            ),
            record_bytes: terminal.map_or_else(
                || std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                |(_, positive)| positive.record_bytes.clone(),
            ),
            custody_failed: terminal.map_or_else(
                || std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                |(_, positive)| positive.custody_failed.clone(),
            ),
            terminal_case: terminal.map(|(case, _)| case),
            terminal_started: std::sync::atomic::AtomicBool::new(false),
            blocked: std::sync::atomic::AtomicBool::new(false),
        };
        value.check_directories()?;
        value.record("audio-live-start.original.json", &json!({
            "schema":"semwright.audio-derive-live-owner-observation/2",
            "selector":AUDIO_LIVE_ORIGINALS_ENV,"selector_is_authority":false,
            "root":value.ancestors.last().map(RetainedLiveDirectory::observation),
            "subroots":value.directories.iter().map(RetainedLiveDirectory::observation).collect::<Vec<_>>(),
            "excluded_locations_observed":value.excluded_locations,
            "initially_empty":true,"root_created_by_test":false,
            "external_source_run_binary_binding_required":true,
            "external_custody_admitted":false,"external_deadline_attested":false,
            "external_filesystem_budget_attested":false,
            "scope":"existing-MLT-live-plus-three-synthetic-PCM16-derivatives-and-one-six-frame-Motion-Canvas-source",
            "formal_acceptance":false,"native_execution_retried":false
        }))?;
        Ok(value)
    }

    fn directory(&self, name: &str) -> AudioLiveResult<RetainedLiveDirectory> {
        let index = AUDIO_LIVE_ROOTS
            .iter()
            .position(|candidate| *candidate == name)
            .ok_or_else(|| std::io::Error::other("Undeclared live subroot"))?;
        Ok(self.directories[index].clone())
    }

    fn check_directories(&self) -> AudioLiveResult<()> {
        for directory in self.ancestors.iter().chain(&self.directories) {
            directory.check()?;
        }
        let root = self
            .ancestors
            .last()
            .ok_or_else(|| std::io::Error::other("Missing evidence root"))?;
        let mut expected = AUDIO_LIVE_ROOTS.map(std::ffi::OsString::from);
        expected.sort();
        audio_live_require(
            root.children()? == expected,
            "Audio live top-level root roster changed",
        )
    }

    fn before_effect(&self) -> AudioLiveResult<()> {
        audio_live_require(
            !self.blocked.load(std::sync::atomic::Ordering::SeqCst)
                && !self
                    .custody_failed
                    .load(std::sync::atomic::Ordering::SeqCst)
                && self.calls.load(std::sync::atomic::Ordering::SeqCst) < AUDIO_LIVE_CALL_LIMIT
                && self.record_bytes.load(std::sync::atomic::Ordering::SeqCst)
                    < AUDIO_LIVE_RECORDS_LIMIT,
            "Audio live attempt is stopped after uncertainty, custody failure or budget exhaustion",
        )?;
        self.check_directories()
    }

    fn stop(&self) {
        self.blocked
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    fn new_record_file(&self, name: &str) -> AudioLiveResult<std::fs::File> {
        use std::os::{
            fd::{AsRawFd, FromRawFd},
            unix::fs::MetadataExt,
        };
        self.check_directories()?;
        audio_live_require(
            !name.is_empty()
                && name.len() <= 180
                && name
                    .bytes()
                    .all(|value| value.is_ascii_alphanumeric() || b"-_.".contains(&value))
                && name != "."
                && name != "..",
            "Live original record name is not a closed basename",
        )?;
        let name = std::ffi::CString::new(name)?;
        let directory = &self.directories[7];
        // SAFETY: records FD and CString remain live. O_EXCL never replaces an
        // original; errors leave any already written prefix in the owner tree.
        let descriptor = unsafe {
            libc::openat(
                directory.handle.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: descriptor is uniquely returned by successful openat above.
        let file = unsafe { std::fs::File::from_raw_fd(descriptor) };
        let metadata = file.metadata()?;
        audio_live_require(
            metadata.is_file()
                && metadata.nlink() == 1
                && metadata.uid() == directory.uid
                && metadata.mode() & 0o7777 == 0o600,
            "Live original is not a private regular single-link file",
        )?;
        Ok(file)
    }

    fn record_bytes(&self, name: &str, bytes: &[u8], maximum: usize) -> AudioLiveResult<()> {
        use std::io::Write;
        let result = (|| -> AudioLiveResult<()> {
            audio_live_require(
                bytes.len() <= maximum,
                "Audio live record exceeds its per-file bound",
            )?;
            self.record_bytes
                .fetch_update(
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                    |current| {
                        current
                            .checked_add(bytes.len() as u64)
                            .filter(|next| *next <= AUDIO_LIVE_RECORDS_LIMIT)
                    },
                )
                .map_err(|_| std::io::Error::other("Audio live record byte budget exhausted"))?;
            let mut file = self.new_record_file(name)?;
            // No temporary-file destructor removes prefixes on failure.
            file.write_all(bytes)?;
            file.flush()?;
            file.sync_all()?;
            self.directories[7].handle.sync_all()?;
            self.check_directories()?;
            audio_live_require(
                self.records.read(name, maximum)? == bytes,
                "Audio live record readback differs",
            )?;
            Ok(())
        })();
        if result.is_err() {
            self.stop();
            self.custody_failed
                .store(true, std::sync::atomic::Ordering::SeqCst);
        }
        result
    }

    fn record_limited(&self, name: &str, value: &Value, maximum: usize) -> AudioLiveResult<()> {
        let bytes = serde_json::to_vec(value)?;
        self.record_bytes(name, &bytes, maximum)
    }

    fn record(&self, name: &str, value: &Value) -> AudioLiveResult<()> {
        self.record_limited(name, value, AUDIO_LIVE_METADATA_LIMIT)
    }

    fn secondary(&self, name: &str, error: &(dyn std::error::Error + Send + Sync)) {
        let result = self.record(
            name,
            &json!({
                "schema":"semwright.audio-derive-live-secondary-error/1",
                "message":error.to_string(),"primary_error_replaced":false,
                "native_execution_retried":false
            }),
        );
        if result.is_err() {
            use std::io::Write;
            let _ = writeln!(
                std::io::stderr(),
                "audio derive live secondary custody failed"
            );
        }
    }
}

fn audio_live_copy(
    originals: &AudioLiveOriginals,
    label: &'static str,
    source: &Path,
    destination: &Path,
) -> AudioLiveResult<()> {
    originals.before_effect()?;
    originals.record(
        &format!("{label}.copy-request.original.json"),
        &json!({
            "schema":"semwright.mlt-live-copy-request/1","source":source,"destination":destination,
            "copy_count":1,"authority_issued":false
        }),
    )?;
    let original = std::fs::copy(source, destination);
    let retained = originals.record(
        &format!("{label}.copy-result.original.json"),
        &json!({
            "schema":"semwright.mlt-live-copy-result/1",
            "returned_bytes":original.as_ref().ok(),
            "original_error":original.as_ref().err().map(ToString::to_string),
            "destination_preserved_on_failure":true,"copy_retried":false
        }),
    );
    match original {
        Err(original) => {
            originals.stop();
            if let Err(secondary) = retained {
                originals.secondary(
                    &format!("{label}.copy-custody-error.original.json"),
                    secondary.as_ref(),
                );
            }
            Err(Box::new(original))
        }
        Ok(_) => retained,
    }
}

fn audio_live_tool_stream(
    originals: &AudioLiveOriginals,
    name: &str,
    retained: &std::fs::File,
) -> (
    Option<Vec<u8>>,
    Value,
    Option<Box<dyn std::error::Error + Send + Sync>>,
) {
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let opened = originals.records.read_file(name, u64::MAX);
    let mut file = match opened {
        Ok(file) => file,
        Err(error) => {
            return (
                None,
                json!({"path":name,"state":"OPEN_FAILED",
            "original_error":error.to_string(),"bytes":null,"sha256":null,
            "stream_eof_attested":false}),
                Some(Box::new(error)),
            );
        }
    };
    let retained_metadata = retained.metadata();
    let before = file.metadata();
    let mut bytes = Vec::new();
    let read = file
        .by_ref()
        .take(AUDIO_LIVE_TOOL_PREFIX_LIMIT as u64 + 1)
        .read_to_end(&mut bytes);
    let after = file.metadata();
    let over = bytes.len() > AUDIO_LIVE_TOOL_PREFIX_LIMIT;
    if over {
        bytes.truncate(AUDIO_LIVE_TOOL_PREFIX_LIMIT);
    }
    let same = match (&retained_metadata, &before, &after) {
        (Ok(retained), Ok(before), Ok(after)) => {
            retained.dev() == before.dev()
                && retained.ino() == before.ino()
                && before.dev() == after.dev()
                && before.ino() == after.ino()
                && before.len() == after.len()
                && after.len() == bytes.len() as u64
                && before.mtime() == after.mtime()
                && before.mtime_nsec() == after.mtime_nsec()
                && before.ctime() == after.ctime()
                && before.ctime_nsec() == after.ctime_nsec()
        }
        _ => false,
    };
    let observed = json!({
        "path":name,"state":if read.is_ok() && !over && same {"FILE_SNAPSHOT_OBSERVED"} else {"INCOMPLETE_FILE_SNAPSHOT"},
        "inspected_bytes":bytes.len(),"inspected_sha256":semwright_mlt_video::hash::sha256(&bytes),
        "inspected_prefix_limit":AUDIO_LIVE_TOOL_PREFIX_LIMIT,"hard_capture_limit":null,
        "file_bytes_before":before.as_ref().ok().map(std::fs::Metadata::len),
        "file_bytes_after":after.as_ref().ok().map(std::fs::Metadata::len),
        "over_inspection_limit":over,"retained_descriptor_matches":same,
        "read_error":read.as_ref().err().map(ToString::to_string),
        "metadata_error":retained_metadata.as_ref().err().or(before.as_ref().err())
            .or(after.as_ref().err()).map(ToString::to_string),
        "stream_eof_attested":false,"descendant_closure_attested":false,
        "raw_file_preserved":true,"external_file_budget_required":true
    });
    let error: Option<Box<dyn std::error::Error + Send + Sync>> = match read {
        Err(error) => Some(Box::new(error)),
        Ok(_) if over || !same => Some(Box::new(std::io::Error::other(
            "Live tool original is partial, changed or exceeds the inspection budget",
        ))),
        Ok(_) => None,
    };
    (Some(bytes), observed, error)
}

// Instrument the SIX existing fixture/oracle Command objects. Their process
// is spawned once, with original bytes written directly to exclusive files.
// This does not add a runtime executor, sandbox, decoder, timeout or retry.
// File snapshots do not assert pipe EOF or descendant closure. The existing
// outer CI owner must supply and observe the finite deadline/filesystem budget.
fn audio_live_command(
    originals: &AudioLiveOriginals,
    label: &'static str,
    command: &mut Command,
) -> AudioLiveResult<std::process::Output> {
    use std::os::unix::{ffi::OsStrExt, process::ExitStatusExt};
    audio_live_require(
        [
            "fixture-h264",
            "fixture-sync-markers",
            "fixture-motion-frames",
            "fixture-caption-overlay",
            "fixture-caption-audio",
            "oracle-video-only-ffprobe",
        ]
        .contains(&label),
        "Undeclared live fixture/oracle command",
    )?;
    originals.before_effect()?;
    let stdout_name = format!("{label}.stdout.original.bin");
    let stderr_name = format!("{label}.stderr.original.bin");
    let stdout = originals.new_record_file(&stdout_name)?;
    let stderr = originals.new_record_file(&stderr_name)?;
    let cwd = match command.get_current_dir() {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir()?,
    };
    let argv = json!({
        "schema":"semwright.mlt-live-command-original/1","label":label,
        "program_os_bytes":command.get_program().as_bytes(),
        "argv_os_bytes":command.get_args().map(|value| value.as_bytes().to_vec()).collect::<Vec<_>>(),
        "cwd_os_bytes":cwd.as_os_str().as_bytes(),
        "explicit_environment_os_bytes":command.get_envs().map(|(name,value)|
            json!({"name":name.as_bytes(),"value":value.map(|value| value.as_bytes())})).collect::<Vec<_>>(),
        "inherited_environment_claimed_captured":false,
        "stdout_original":stdout_name,"stderr_original":stderr_name,
        "stream_destination":"exclusive-retained-owner-files",
        "external_deadline_and_file_budget_required":true,"native_execution_retried":false
    });
    originals.record(&format!("{label}.argv.original.json"), &argv)?;
    command.stdout(std::process::Stdio::from(stdout.try_clone()?));
    command.stderr(std::process::Stdio::from(stderr.try_clone()?));
    originals.before_effect()?;
    let spawned = command.spawn();
    let mut child = match spawned {
        Err(original) => {
            let retained = originals.record(
                &format!("{label}.terminal.original.json"),
                &json!({
                    "schema":"semwright.mlt-live-command-terminal/1","spawn_attempted":true,
                    "pid":null,"terminal_observed":false,"exit_code":null,"term_signal":null,
                    "spawn_error":original.to_string(),"wait_error":null,
                    "stream_eof_attested":false,"descendant_closure_attested":false,
                    "native_execution_retried":false
                }),
            );
            originals.stop();
            if let Err(secondary) = retained {
                originals.secondary(
                    &format!("{label}.terminal-custody-error.original.json"),
                    secondary.as_ref(),
                );
            }
            return Err(Box::new(original));
        }
        Ok(child) => child,
    };
    let pid = child.id();
    // A PID-record failure must not return with an unobserved live child.
    let mut custody_error = originals
        .record(
            &format!("{label}.spawn.original.json"),
            &json!({
                "schema":"semwright.mlt-live-command-spawn/1","pid":pid,"spawn_attempted":true,
                "terminal_observed":false,"native_execution_retried":false
            }),
        )
        .err();
    let waited = child.wait();
    // Store the actual wait result before any sync/read/interpretation.
    let observed_wait = json!({
        "exit_code":waited.as_ref().ok().and_then(std::process::ExitStatus::code),
        "term_signal":waited.as_ref().ok().and_then(ExitStatusExt::signal),
        "wait_error":waited.as_ref().err().map(ToString::to_string),
        "terminal_observed":waited.is_ok()
    });
    let cleanup = if waited.is_err() {
        // One cleanup request on this same Child; try_wait is nonblocking.
        // A missing terminal remains UNKNOWN for the external owner.
        let killed = child.kill();
        let reaped = child.try_wait();
        json!({"kill_attempted":true,"kill_error":killed.as_ref().err().map(ToString::to_string),
            "wait_error":reaped.as_ref().err().map(ToString::to_string),
            "terminal_observed":matches!(&reaped, Ok(Some(_))),
            "exit_code":reaped.as_ref().ok().and_then(Option::as_ref).and_then(std::process::ExitStatus::code),
            "term_signal":reaped.as_ref().ok().and_then(Option::as_ref).and_then(ExitStatusExt::signal),
            "descendant_closure_attested":false})
    } else {
        Value::Null
    };
    for file in [&stdout, &stderr] {
        if let Err(error) = file.sync_all()
            && custody_error.is_none()
        {
            custody_error = Some(Box::new(error));
        }
    }
    let (stdout_bytes, stdout_observation, stdout_error) =
        audio_live_tool_stream(originals, &stdout_name, &stdout);
    let (stderr_bytes, stderr_observation, stderr_error) =
        audio_live_tool_stream(originals, &stderr_name, &stderr);
    if custody_error.is_none() {
        custody_error = stdout_error;
    }
    if custody_error.is_none() {
        custody_error = stderr_error;
    }
    let recorded = originals.record(&format!("{label}.terminal.original.json"), &json!({
        "schema":"semwright.mlt-live-command-terminal/1","pid":pid,"spawn_attempted":true,
        "wait":observed_wait,"cleanup":cleanup,"stdout":stdout_observation,"stderr":stderr_observation,
        "custody_error":custody_error.as_ref().map(ToString::to_string),
        "stream_eof_attested":false,"descendant_closure_attested":false,
        "external_deadline_attested":false,"external_file_budget_attested":false,
        "native_execution_retried":false
    }));
    if custody_error.is_none() {
        custody_error = recorded.err();
    }
    let status = match waited {
        Err(original) => {
            originals.stop();
            if let Some(secondary) = custody_error {
                originals.secondary(
                    &format!("{label}.custody-error.original.json"),
                    secondary.as_ref(),
                );
            }
            return Err(Box::new(original));
        }
        Ok(status) if !status.success() => {
            originals.stop();
            if let Some(secondary) = custody_error {
                originals.secondary(
                    &format!("{label}.custody-error.original.json"),
                    secondary.as_ref(),
                );
            }
            return Err(std::io::Error::other(format!(
                "Original live command failed: {label}: {status}"
            ))
            .into());
        }
        Ok(status) => status,
    };
    if let Some(original) = custody_error {
        originals.stop();
        return Err(original);
    }
    let stdout = stdout_bytes
        .ok_or_else(|| std::io::Error::other("Live tool stdout snapshot unavailable"))?;
    let stderr = stderr_bytes
        .ok_or_else(|| std::io::Error::other("Live tool stderr snapshot unavailable"))?;
    Ok(std::process::Output {
        status,
        stdout,
        stderr,
    })
}

#[derive(Clone, Copy)]
struct AudioLiveCase {
    id: &'static str,
    profile: &'static str,
    source_rate: u32,
    source_channels: u16,
    source_frames: u64,
    target_rate: u32,
    target_channels: u16,
    target_frames: u64,
    antiphase: bool,
}

const AUDIO_LIVE_CASES: [AudioLiveCase; 3] = [
    AudioLiveCase {
        id: "down-antiphase",
        profile: "pcm16-stereo48-to-mono16-v1",
        source_rate: 48_000,
        source_channels: 2,
        source_frames: 384_000,
        target_rate: 16_000,
        target_channels: 1,
        target_frames: 128_000,
        antiphase: true,
    },
    AudioLiveCase {
        id: "down-control",
        profile: "pcm16-stereo48-to-mono16-v1",
        source_rate: 48_000,
        source_channels: 2,
        source_frames: 384_000,
        target_rate: 16_000,
        target_channels: 1,
        target_frames: 128_000,
        antiphase: false,
    },
    AudioLiveCase {
        id: "up-steps",
        profile: "pcm16-mono16-to-dualmono48-v1",
        source_rate: 16_000,
        source_channels: 1,
        source_frames: 2_880_000,
        target_rate: 48_000,
        target_channels: 2,
        target_frames: 8_640_000,
        antiphase: false,
    },
];

impl AudioLiveCase {
    fn source_name(self) -> String {
        format!("audio-derive-{}.source.wav", self.id)
    }
    fn output_name(self) -> String {
        format!("audio-derive-{}.wav", self.id)
    }
}

fn audio_live_level(frame: u64, frames: u64) -> i16 {
    AUDIO_LIVE_LEVELS[((frame * 4) / frames) as usize]
}

// Technical fixtures only. This constructs RIFF PCM16 bytes; all reading uses
// the existing public WaveReader and every native conversion uses the Host.
fn audio_live_fixture(case: AudioLiveCase) -> AudioLiveResult<Vec<u8>> {
    let payload = case
        .source_frames
        .checked_mul(u64::from(case.source_channels))
        .and_then(|value| value.checked_mul(2))
        .ok_or_else(|| std::io::Error::other("Audio fixture size overflow"))?;
    audio_live_require(
        payload + 44 <= 6 * 1024 * 1024,
        "Audio fixture exceeds source bound",
    )?;
    let payload = u32::try_from(payload)?;
    let mut bytes = Vec::with_capacity(payload as usize + 44);
    bytes.extend_from_slice(b"RIFF");
    bytes.extend_from_slice(&(payload + 36).to_le_bytes());
    bytes.extend_from_slice(b"WAVEfmt ");
    bytes.extend_from_slice(&16u32.to_le_bytes());
    bytes.extend_from_slice(&1u16.to_le_bytes());
    bytes.extend_from_slice(&case.source_channels.to_le_bytes());
    bytes.extend_from_slice(&case.source_rate.to_le_bytes());
    let block = case.source_channels * 2;
    bytes.extend_from_slice(&(case.source_rate * u32::from(block)).to_le_bytes());
    bytes.extend_from_slice(&block.to_le_bytes());
    bytes.extend_from_slice(&16u16.to_le_bytes());
    bytes.extend_from_slice(b"data");
    bytes.extend_from_slice(&payload.to_le_bytes());
    for frame in 0..case.source_frames {
        let level = audio_live_level(frame, case.source_frames);
        for channel in 0..case.source_channels {
            let sample = if case.antiphase && channel == 1 {
                -level
            } else {
                level
            };
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
    }
    Ok(bytes)
}

fn audio_live_pcm_observation(
    case: AudioLiveCase,
    bytes: &[u8],
    source: bool,
) -> AudioLiveResult<Value> {
    use semwright_audio_domain::wav::WaveReader;
    let (rate, channels, frames, maximum) = if source {
        (
            case.source_rate,
            case.source_channels,
            case.source_frames,
            6 * 1024 * 1024,
        )
    } else {
        (
            case.target_rate,
            case.target_channels,
            case.target_frames,
            40 * 1024 * 1024,
        )
    };
    audio_live_require(bytes.len() <= maximum, "Audio live WAV exceeds bound")?;
    let mut reader = WaveReader::open(std::io::Cursor::new(bytes), maximum as u64)?;
    let info = reader.info().clone();
    audio_live_require(
        info.sample_rate.0 == rate
            && info.channels == channels
            && info.frames == frames
            && info.container_bits == 16
            && info.valid_bits == 16
            && !info.ieee_float
            && info
                .channel_mask
                .is_none_or(|mask| mask == if channels == 1 { 4 } else { 3 }),
        "Audio live WAV layout or exact clock differs",
    )?;
    audio_live_require(
        frames > 0 && frames % 4 == 0,
        "Audio live fixture clock is not quartered",
    )?;
    let mut observed = 0u64;
    let mut interior_samples = 0u64;
    let mut max_level_error = 0i32;
    let mut nonzero = 0u64;
    loop {
        let samples = reader.read_frames(16_384)?;
        if samples.is_empty() {
            break;
        }
        audio_live_require(
            samples.len() % usize::from(channels) == 0,
            "Audio live decoded frame is incomplete",
        )?;
        for values in samples.chunks_exact(usize::from(channels)) {
            audio_live_require(observed < frames, "Audio live decoded frame count overflow")?;
            let expected = i32::from(audio_live_level(observed, frames));
            let quarter = frames / 4;
            let within = observed % quarter;
            let interior = within >= quarter / 4 && within < quarter * 3 / 4;
            for (channel, value) in values.iter().enumerate() {
                let scaled = *value * 32768.0;
                audio_live_require(
                    scaled.is_finite()
                        && scaled.fract() == 0.0
                        && (-32768.0..=32767.0).contains(&scaled),
                    "Audio live sample is not PCM16",
                )?;
                let sample = scaled as i32;
                nonzero += u64::from(sample != 0);
                if source {
                    let expected = if case.antiphase && channel == 1 {
                        -expected
                    } else {
                        expected
                    };
                    audio_live_require(
                        sample == expected,
                        "Audio live original source bytes differ from the fixed fixture",
                    )?;
                } else if case.antiphase {
                    audio_live_require(
                        sample == 0,
                        "Audio live antiphase average is not exactly zero",
                    )?;
                } else if interior {
                    let error = (sample - expected).abs();
                    max_level_error = max_level_error.max(error);
                    interior_samples += 1;
                    audio_live_require(
                        error <= AUDIO_LIVE_LEVEL_ERROR_LSB,
                        "Audio live interior level exceeds the fixed one-LSB bound",
                    )?;
                }
            }
            if !source && channels == 2 {
                audio_live_require(
                    values[0] == values[1],
                    "Audio live dual mono channels differ",
                )?;
            }
            observed += 1;
        }
    }
    audio_live_require(
        observed == frames,
        "Audio live decoded sample count differs",
    )?;
    if source || !case.antiphase {
        audio_live_require(nonzero > 0, "Audio live non-silent control was silenced")?;
    }
    if !source && !case.antiphase {
        audio_live_require(
            interior_samples > 0,
            "Audio live level observation is empty",
        )?;
    }
    Ok(json!({
        "sample_rate":rate,"channels":channels,"sample_frames":observed,
        "channel_mask":info.channel_mask,"container_bits":info.container_bits,"valid_bits":info.valid_bits,
        "all_payload_frames_read":true,"nonzero_samples":nonzero,
        "interior_samples":interior_samples,"max_interior_level_error_lsb":max_level_error,
        "interior_level_limit_lsb":AUDIO_LIVE_LEVEL_ERROR_LSB,
        "transient_waveform_fidelity_assessed":false,"formal_acceptance":false
    }))
}

fn audio_live_stage_sources(originals: &AudioLiveOriginals) -> AudioLiveResult<Vec<Value>> {
    let mut sources = Vec::new();
    for case in AUDIO_LIVE_CASES {
        let bytes = audio_live_fixture(case)?;
        let artifact = originals
            .media
            .write_new("media", &case.source_name(), &bytes)?;
        audio_live_require(
            originals.media.read(&case.source_name(), 6 * 1024 * 1024)? == bytes,
            "Audio live source publication readback differs",
        )?;
        let observation = audio_live_pcm_observation(case, &bytes, true)?;
        sources.push(json!({
            "id":case.id,"profile":case.profile,"root":"media","path":artifact.path,
            "sha256":artifact.sha256,"bytes":artifact.bytes,"pcm":observation,
            "origin":"synthetic-technical-fixture","voice_identity":"NOT_APPLICABLE",
            "manual_review":false,"levels":AUDIO_LIVE_LEVELS,"antiphase":case.antiphase
        }));
    }
    originals.record(
        "audio-live-sources.original.json",
        &json!({
            "schema":"semwright.audio-derive-live-sources/1","sources":sources,
            "profile_bounds_changed":false,"native_execution_retried":false
        }),
    )?;
    Ok(sources)
}

fn audio_live_json(root: &semwright_mlt_video::fs::Root, path: &str) -> AudioLiveResult<Value> {
    let bytes = root.read(path, AUDIO_LIVE_METADATA_LIMIT)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn audio_live_artifact(
    root: &semwright_mlt_video::fs::Root,
    value: &Value,
    path: &str,
    maximum: usize,
) -> AudioLiveResult<Vec<u8>> {
    audio_live_require(
        value["root"] == "output" && value["path"] == path,
        "Audio live Artifact destination differs",
    )?;
    let bytes = root.read(path, maximum)?;
    audio_live_require(
        value["bytes"].as_u64() == Some(bytes.len() as u64)
            && value["sha256"] == semwright_mlt_video::hash::sha256(&bytes),
        "Audio live Artifact bytes differ",
    )?;
    Ok(bytes)
}

fn audio_live_verify_return(
    originals: &AudioLiveOriginals,
    case: AudioLiveCase,
    source: &Value,
    args: &Value,
    result: &Value,
    descriptor: &str,
    ffmpeg_sha256: &str,
) -> AudioLiveResult<Value> {
    originals.check_directories()?;
    let name = case.output_name();
    let original_source = originals.media.read(&case.source_name(), 6 * 1024 * 1024)?;
    audio_live_require(
        source["sha256"] == semwright_mlt_video::hash::sha256(&original_source)
            && source["bytes"].as_u64() == Some(original_source.len() as u64),
        "Audio live source changed during the operation",
    )?;
    audio_live_pcm_observation(case, &original_source, true)?;
    audio_live_require(
        result["schema"] == "semwright.mlt-audio-derivative/1"
            && result["profile"] == case.profile
            && result["source_sha256"] == source["sha256"]
            && result["runtime_sha256"] == ffmpeg_sha256
            && result["process_capture_complete"] == true,
        "Audio live result identity differs",
    )?;
    let bytes = audio_live_artifact(
        &originals.output,
        &result["artifact"],
        &name,
        40 * 1024 * 1024,
    )?;
    let retained_source = audio_live_artifact(
        &originals.output,
        &result["original_source"],
        &format!("{name}.audio-derive.source.original.wav"),
        6 * 1024 * 1024,
    )?;
    let retained_candidate = audio_live_artifact(
        &originals.output,
        &result["original_output"],
        &format!("{name}.audio-derive.output.original.wav"),
        40 * 1024 * 1024,
    )?;
    audio_live_require(
        retained_source == original_source && retained_candidate == bytes,
        "Audio live original source or native candidate differs from the published artifact",
    )?;
    let observed = audio_live_pcm_observation(case, &bytes, false)?;
    audio_live_require(
        result["source"]["sample_frames"] == case.source_frames
            && result["target"]["sample_frames"] == case.target_frames
            && u128::from(case.source_frames) * u128::from(case.target_rate)
                == u128::from(case.target_frames) * u128::from(case.source_rate),
        "Audio live source/result rational duration differs",
    )?;
    let process_originals = result["process_originals"]
        .as_array()
        .ok_or_else(|| std::io::Error::other("Audio live process original roster is missing"))?;
    audio_live_require(
        process_originals.len() == 4,
        "Audio live process original roster differs",
    )?;
    for (index, (suffix, maximum)) in [
        ("argv.original.json", AUDIO_LIVE_METADATA_LIMIT),
        ("stdout.original.bin", 262_144),
        ("stderr.original.bin", 262_144),
        ("terminal.original.json", AUDIO_LIVE_METADATA_LIMIT),
    ]
    .into_iter()
    .enumerate()
    {
        audio_live_artifact(
            &originals.output,
            &process_originals[index],
            &format!("{name}.audio-derive.ffmpeg.{suffix}"),
            maximum,
        )?;
    }
    let process = audio_live_json(
        &originals.output,
        &format!("{name}.audio-derive.ffmpeg.terminal.original.json"),
    )?;
    audio_live_require(
        process["schema"] == "semwright.audio-derivative-process-original/1"
            && process["process_result_received"] == true
            && process.get("custody_error").is_some_and(Value::is_null)
            && process["profile"] == case.profile
            && process["source_sha256"] == source["sha256"]
            && process["ffmpeg_sha256"] == ffmpeg_sha256
            && process["recipe_sha256"] == result["recipe_sha256"]
            && process["process"]["pid"]
                .as_u64()
                .is_some_and(|pid| pid > 0)
            && process["process"]["spawn_attempted"] == true
            && process["process"]["terminal_observed"] == true
            && process["process"]["exit_code"] == 0
            && process["process"]
                .get("term_signal")
                .is_some_and(Value::is_null)
            && process["process"]["cancelled"] == false
            && process["process"]["timed_out"] == false
            && process["process"]["output_exceeded"] == false
            && process["process"]
                .get("supervisor_error")
                .is_some_and(Value::is_null)
            && process["process"]
                .get("drain_stop_reason")
                .is_some_and(Value::is_null)
            && process["process"]["capture_complete"] == true,
        "Audio live native terminal/capture is incomplete",
    )?;
    for stream in ["stdout", "stderr"] {
        let raw = originals.output.read(
            &format!("{name}.audio-derive.ffmpeg.{stream}.original.bin"),
            262_144,
        )?;
        let metadata = &process["process"][stream];
        audio_live_require(
            metadata["complete"] == true
                && metadata["eof_observed"] == true
                && metadata.get("read_error").is_some_and(Value::is_null)
                && metadata["reader_panicked"] == false
                && metadata["drain_stopped"] == false
                && metadata["truncated"] == false
                && metadata["observed_bytes"].as_u64() == Some(raw.len() as u64)
                && metadata["retained_bytes"].as_u64() == Some(raw.len() as u64)
                && metadata["retained_sha256"] == semwright_mlt_video::hash::sha256(&raw),
            "Audio live original stream metadata differs from its bytes",
        )?;
    }
    let argv = originals.output.read(
        &format!("{name}.audio-derive.ffmpeg.argv.original.json"),
        AUDIO_LIVE_METADATA_LIMIT,
    )?;
    audio_live_require(!argv.is_empty(), "Audio live original argv is absent")?;
    let host_input = audio_live_json(
        &originals.output,
        &format!("{name}.audio-derive-host.stdin.original.bin"),
    )?;
    audio_live_require(
        host_input["command"] == AUDIO_LIVE_COMMAND
            && host_input["descriptor_sha256"] == descriptor
            && host_input["args"] == *args,
        "Audio live Host original request differs from its only caller",
    )?;
    let host_stdout = originals.output.read(
        &format!("{name}.audio-derive-host.stdout.original.bin"),
        262_144,
    )?;
    audio_live_require(
        serde_json::from_slice::<Value>(&host_stdout)? == *result,
        "Audio live Host original output differs from the returned Value",
    )?;
    let host_stderr = originals.output.read(
        &format!("{name}.audio-derive-host.stderr.original.bin"),
        262_144,
    )?;
    let host = audio_live_json(
        &originals.output,
        &format!("{name}.audio-derive-host.terminal.original.json"),
    )?;
    audio_live_require(
        host["schema"] == "semwright.audio-derivative-host-original/1"
            && host["host_stage"] == "host"
            && host["state"] == "completed"
            && host["host_exit_code"] == 0
            && host["stdin_delivery_confirmed"] == true
            && host["stream_eof_attested"] == false
            && host.get("original_error").is_some_and(Value::is_null)
            && host["native_execution_retried"] == false
            && host["runtime_job_id"]
                .as_str()
                .is_some_and(|id| !id.is_empty()),
        "Audio live Host terminal does not bind its original completed job",
    )?;
    for (stream, raw) in [("stdout", host_stdout), ("stderr", host_stderr)] {
        audio_live_require(
            host[stream]["retained"] == true
                && host[stream]["diagnostic_prefix_truncated"] == false
                && host[stream]["bytes"].as_u64() == Some(raw.len() as u64)
                && host[stream]["sha256"] == semwright_mlt_video::hash::sha256(&raw),
            "Audio live Host original stream was not retained completely as delivered",
        )?;
    }
    let operation = audio_live_json(
        &originals.output,
        &format!("{name}.audio-derive.operation.original.json"),
    )?;
    audio_live_require(
        operation["schema"] == "semwright.audio-derivative-operation-original/1"
            && operation["profile"] == case.profile
            && operation["output_path"] == name
            && operation["source_sha256"] == source["sha256"]
            && operation["operation_succeeded"] == true
            && operation["phase"] == "validated-and-published"
            && operation.get("original_error").is_some_and(Value::is_null)
            && operation["native_execution_retried"] == false,
        "Audio live operation terminal differs",
    )?;
    Ok(json!({
        "id":case.id,"profile":case.profile,"pcm":observed,"artifact":result["artifact"],
        "host_job_id":host["runtime_job_id"],"native_pid":process["process"]["pid"],
        "source_sha256":source["sha256"],"source_bytes":source["bytes"],
        "recipe_sha256":result["recipe_sha256"],"runtime_sha256":result["runtime_sha256"],
        "native_execution_retried":false,"formal_acceptance":false
    }))
}

fn audio_live_case_call_identity(
    originals: &AudioLiveOriginals,
    index: usize,
    args: &Value,
    descriptor: &Value,
) -> AudioLiveResult<Value> {
    audio_live_require(
        index > 0
            && index <= AUDIO_LIVE_CALL_LIMIT
            && originals.calls.load(std::sync::atomic::Ordering::SeqCst) == index,
        "Audio catalog observation does not identify its single retained caller",
    )?;
    let path = format!("call-{index:05}.request.original.json");
    let bytes = originals.records.read(&path, AUDIO_LIVE_METADATA_LIMIT)?;
    let request: Value = serde_json::from_slice(&bytes)?;
    let context = &request["context"];
    audio_live_require(
        request["schema"] == "semwright.mlt-live-provider-request-original/1"
            && request["capability"] == AUDIO_LIVE_COMMAND
            && request["arguments"] == *args
            && request["descriptor"] == *descriptor
            && context["session"] == "mlt-video-live"
            && context["cancellation_requested"] == false
            && context["request_id"].as_str().is_some_and(|id| {
                !id.is_empty() && id.len() <= 256 && !id.chars().any(char::is_control)
            }),
        "Audio catalog original caller identity, descriptor or arguments differ",
    )?;
    Ok(json!({
        "session":context["session"],"request_id":context["request_id"],
        "capability":AUDIO_LIVE_COMMAND,"record_index":index,
        "request_original":{"root":"records","path":path,"bytes":bytes.len(),
            "sha256":semwright_mlt_video::hash::sha256(&bytes)},
        "capture_scope":"original-public-provider-call-not-a-reconstructed-Host-envelope"
    }))
}

fn audio_live_catalog_keys(value: &Value, keys: &[&str]) -> AudioLiveResult<()> {
    audio_live_require(
        value.as_object().is_some_and(|object| {
            object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
        }),
        "Audio catalog original metadata shape differs",
    )
}

// This observes the four originals only if the SAME existing catalog gate
// emitted them for this call. It never runs discover or invents cache history.
fn audio_live_check_catalog(
    output_name: &str,
    files: [Option<&[u8]>; 4],
) -> AudioLiveResult<Value> {
    let present = files.iter().filter(|file| file.is_some()).count();
    if present == 0 {
        return Ok(json!({
            "state":"ABSENT_AT_OBSERVATION","original_count":0,
            "absence_scope":"four-fixed-catalog-paths-in-this-call-output-namespace",
            "gate_source":"ensure_host_catalog returns early when host_catalog_verified or Host tools are absent",
            "actual_gate_reason":"NOT_EXPOSED_BY_EXISTING_DRIVER",
            "previous_catalog_observation_inherited":false,
            "catalog_dispatch_absent_claimed":false,"catalog_acceptance_claimed":false,
            "native_execution_retried":false
        }));
    }
    audio_live_require(present == 4, "Audio catalog original roster is partial")?;
    let [
        Some(stdin),
        Some(stdout),
        Some(stderr),
        Some(terminal_bytes),
    ] = files
    else {
        return Err(std::io::Error::other("Audio catalog originals are unavailable").into());
    };
    audio_live_require(
        stdin.is_empty()
            && stdout.len() <= semwright_mlt_video::runtime::AUDIO_DERIVATIVE_STREAM_LIMIT
            && stderr.len() <= semwright_mlt_video::runtime::AUDIO_DERIVATIVE_STREAM_LIMIT
            && terminal_bytes.len()
                <= semwright_mlt_video::runtime::AUDIO_DERIVATIVE_METADATA_LIMIT,
        "Audio catalog original exceeds the existing SDK custody contract",
    )?;
    let terminal: Value = serde_json::from_slice(terminal_bytes)?;
    audio_live_catalog_keys(
        &terminal,
        &[
            "schema",
            "host_stage",
            "state",
            "output_path",
            "runtime_job_id",
            "runtime_job_id_exposed",
            "host_exit_code",
            "stdin",
            "stdin_scope",
            "stdin_delivery_confirmed",
            "stdout",
            "stderr",
            "original_error",
            "stream_eof_attested",
            "native_execution_retried",
            "physical_acceptance",
            "capture_scope",
        ],
    )?;
    audio_live_require(
        terminal["schema"] == "semwright.audio-derivative-host-original/1"
            && terminal["host_stage"] == "catalog"
            && terminal["state"] == "completed"
            && terminal["output_path"] == output_name
            && terminal["runtime_job_id"].is_null()
            && terminal["runtime_job_id_exposed"] == false
            && terminal["host_exit_code"] == 0
            && terminal["stdin_scope"] == "same-empty-Vec-supplied-to-existing-discover-call"
            && terminal["stdin_delivery_confirmed"] == true
            && terminal["original_error"].is_null()
            && terminal["stream_eof_attested"] == false
            && terminal["native_execution_retried"] == false
            && terminal["physical_acceptance"] == false
            && terminal["capture_scope"]
                == "same-existing-Host-return-before-parse-and-public-redaction",
        "Audio catalog original terminal does not identify a complete returned Host observation",
    )?;
    audio_live_catalog_keys(&terminal["stdin"], &["root", "path", "sha256", "bytes"])?;
    audio_live_require(
        terminal["stdin"]["root"] == "output"
            && terminal["stdin"]["path"]
                == format!("{output_name}.audio-derive-catalog.stdin.original.bin")
            && terminal["stdin"]["bytes"] == 0
            && terminal["stdin"]["sha256"] == semwright_mlt_video::hash::sha256(stdin),
        "Audio catalog original empty stdin differs",
    )?;
    for (label, bytes) in [("stdout", stdout), ("stderr", stderr)] {
        let metadata = &terminal[label];
        audio_live_catalog_keys(
            metadata,
            &[
                "root",
                "path",
                "sha256",
                "bytes",
                "delivered_buffer_bytes",
                "delivered_buffer_sha256",
                "diagnostic_prefix_truncated",
                "retained",
                "stream_eof_attested",
            ],
        )?;
        let sha256 = semwright_mlt_video::hash::sha256(bytes);
        audio_live_require(
            metadata["root"] == "output"
                && metadata["path"]
                    == format!("{output_name}.audio-derive-catalog.{label}.original.bin")
                && metadata["bytes"].as_u64() == Some(bytes.len() as u64)
                && metadata["sha256"] == sha256
                && metadata["delivered_buffer_sha256"] == sha256
                && metadata["delivered_buffer_bytes"].as_u64() == Some(bytes.len() as u64)
                && metadata["diagnostic_prefix_truncated"] == false
                && metadata["retained"] == true
                && metadata["stream_eof_attested"] == false,
            "Audio catalog buffer was truncated, changed or not retained as delivered",
        )?;
    }
    // Discovery is the MLT ServiceCatalog envelope, not the much larger
    // CommandDescriptor catalog. The Driver remains its semantic validator.
    let catalog: Value = serde_json::from_slice(stdout)?;
    audio_live_catalog_keys(&catalog, &["schema", "operation", "version", "groups"])?;
    audio_live_require(
        catalog["schema"] == 1
            && catalog["operation"] == "discover"
            && catalog["version"]
                .as_str()
                .is_some_and(|value| !value.is_empty() && value.len() <= 512),
        "Audio catalog original is not the existing MLT discovery envelope",
    )?;
    audio_live_catalog_keys(
        &catalog["groups"],
        &[
            "producers",
            "filters",
            "transitions",
            "consumers",
            "video_codecs",
            "audio_codecs",
        ],
    )?;
    Ok(json!({
        "state":"FULL_RETURNED_BUFFERS_OBSERVED","original_count":4,
        "stdout_bytes":stdout.len(),"stdout_sha256":semwright_mlt_video::hash::sha256(stdout),
        "stderr_bytes":stderr.len(),"stderr_sha256":semwright_mlt_video::hash::sha256(stderr),
        "terminal_sha256":semwright_mlt_video::hash::sha256(terminal_bytes),
        "stream_eof_attested":false,"descendant_closure_attested":false,
        "previous_catalog_observation_inherited":false,
        "scope":"same-existing-Host-catalog-return-no-new-discovery",
        "formal_acceptance":false,"native_execution_retried":false
    }))
}

fn audio_live_catalog_originals(
    originals: &AudioLiveOriginals,
    case: AudioLiveCase,
    call_identity: &Value,
    file_observation: &Value,
) -> AudioLiveResult<Value> {
    let name = case.output_name();
    let observed = (|| -> AudioLiveResult<Value> {
        audio_live_require(
            file_observation["schema"] == "semwright.audio-derive-live-file-observation/1"
                && file_observation["case"] == case.id
                && file_observation["captured_before_result_unwrap_or_oracle"] == true,
            "Audio catalog has no original file observation for this call",
        )?;
        let rows = file_observation["files"]
            .as_array()
            .ok_or_else(|| std::io::Error::other("Audio catalog file observation has no roster"))?;
        audio_live_require(
            rows.len() == 16,
            "Audio catalog file observation roster differs",
        )?;
        let mut files = Vec::new();
        for (suffix, maximum) in [
            (
                "stdin.original.bin",
                semwright_mlt_video::runtime::AUDIO_DERIVATIVE_STDIN_LIMIT,
            ),
            (
                "stdout.original.bin",
                semwright_mlt_video::runtime::AUDIO_DERIVATIVE_STREAM_LIMIT,
            ),
            (
                "stderr.original.bin",
                semwright_mlt_video::runtime::AUDIO_DERIVATIVE_STREAM_LIMIT,
            ),
            (
                "terminal.original.json",
                semwright_mlt_video::runtime::AUDIO_DERIVATIVE_METADATA_LIMIT,
            ),
        ] {
            let path = format!("{name}.audio-derive-catalog.{suffix}");
            let original = match originals.output.read(&path, maximum) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.code == "NotFound" => None,
                Err(error) => return Err(Box::new(error)),
            };
            let mut matching = rows
                .iter()
                .filter(|row| row["root"] == "output" && row["path"] == path);
            let previous = matching.next().ok_or_else(|| {
                std::io::Error::other("Audio catalog path is absent from its original observation")
            })?;
            audio_live_require(
                matching.next().is_none(),
                "Audio catalog path is duplicated in its original observation",
            )?;
            match &original {
                None => audio_live_require(
                    previous["state"] == "NOT_PRESENT_AT_OBSERVATION",
                    "Audio catalog original disappeared after its first observation",
                )?,
                Some(bytes) => audio_live_require(
                    previous["state"] == "FILE_SNAPSHOT_OBSERVED"
                        && previous["same_file_snapshot"] == true
                        && previous["over_inspection_limit"] == false
                        && previous["inspected_bytes"].as_u64() == Some(bytes.len() as u64)
                        && previous["inspected_sha256"] == semwright_mlt_video::hash::sha256(bytes),
                    "Audio catalog original changed after its first observation",
                )?,
            }
            files.push(original);
        }
        let value = audio_live_check_catalog(
            &name,
            [
                files[0].as_deref(),
                files[1].as_deref(),
                files[2].as_deref(),
                files[3].as_deref(),
            ],
        )?;
        Ok(json!({
            "schema":"semwright.audio-derive-live-catalog-observation/1",
            "case":case.id,"output_path":name,"caller":call_identity,"observation":value,
            "first_call_assumed_to_discover":false,"native_execution_retried":false
        }))
    })();
    let retained = originals.record(
        &format!("audio-live-{}.catalog-observation.original.json", case.id),
        &json!({
            "observed":observed.as_ref().ok(),
            "original_error":observed.as_ref().err().map(ToString::to_string),
            "caller":call_identity,"native_execution_retried":false
        }),
    );
    match observed {
        Err(original) => {
            originals.stop();
            if let Err(secondary) = retained {
                originals.secondary(
                    &format!("audio-live-{}.catalog-custody-error.original.json", case.id),
                    secondary.as_ref(),
                );
            }
            Err(original)
        }
        Ok(value) => retained.map(|()| value),
    }
}

fn audio_live_finish_observed<T, U>(
    original: semwright_types::Result<T>,
    observed: AudioLiveResult<U>,
    retained: AudioLiveResult<()>,
) -> AudioLiveResult<(T, U)> {
    let value = match original {
        Err(original) => return Err(Box::new(original)),
        Ok(value) => value,
    };
    let observation = observed?;
    retained?;
    Ok((value, observation))
}

fn audio_live_finish_native<T>(
    original: semwright_types::Result<T>,
    retained: AudioLiveResult<()>,
) -> AudioLiveResult<T> {
    match original {
        Err(original) => Err(Box::new(original)),
        Ok(value) => retained.map(|()| value),
    }
}

async fn audio_live_cases(
    originals: &AudioLiveOriginals,
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    sources: &[Value],
    ffmpeg_sha256: &str,
) -> AudioLiveResult<()> {
    let capability = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == AUDIO_LIVE_COMMAND)
        .ok_or_else(|| std::io::Error::other("The composed driver does not expose audio.derive"))?;
    let canonical = semwright_mlt_video::catalog::capabilities()?
        .into_iter()
        .find(|capability| capability.name == AUDIO_LIVE_COMMAND)
        .ok_or_else(|| {
            std::io::Error::other("The composed canonical catalog lacks audio.derive")
        })?;
    let observed_descriptor = serde_json::to_value(&capability.descriptor)?;
    let expected_descriptor: Value = serde_json::from_str(&canonical.descriptor.encode())?;
    originals.record("audio-live-descriptor.original.json", &json!({
        "descriptor":observed_descriptor,"expected_descriptor":expected_descriptor,
        "canonical_descriptor_sha256":canonical.digest,"source":"same-compiled-catalog-public-API"
    }))?;
    audio_live_require(
        observed_descriptor == expected_descriptor,
        "Audio live descriptor differs from the composed canonical catalog",
    )?;
    audio_live_require(
        sources.len() == AUDIO_LIVE_CASES.len(),
        "Audio live source roster differs",
    )?;
    let mut observations = Vec::new();
    for (case, source) in AUDIO_LIVE_CASES.into_iter().zip(sources) {
        originals.check_directories()?;
        let bytes = originals.media.read(&case.source_name(), 6 * 1024 * 1024)?;
        audio_live_require(
            source["id"] == case.id
                && source["sha256"] == semwright_mlt_video::hash::sha256(&bytes)
                && source["bytes"].as_u64() == Some(bytes.len() as u64),
            "Audio live prepared source changed before dispatch",
        )?;
        let args = json!({
            "profile":case.profile,
            "source":{"root":"media","path":case.source_name(),"sha256":source["sha256"],
                "size_bytes":source["bytes"],"sample_rate":case.source_rate,
                "channels":case.source_channels,"sample_frames":case.source_frames},
            "output_path":case.output_name()
        });
        originals.record(&format!("audio-live-{}.prepared-request.original.json", case.id), &json!({
            "command":AUDIO_LIVE_COMMAND,"descriptor_sha256":canonical.digest,"args":args,
            "capture_scope":"same-caller-Value-before-Provider-execute-not-a-wire-reconstruction"
        }))?;
        // The existing helper below owns the only Provider::execute. No new
        // context factory, dispatcher, Host, native tool or retry is introduced.
        let call_index = originals
            .calls
            .load(std::sync::atomic::Ordering::SeqCst)
            .checked_add(1)
            .ok_or_else(|| std::io::Error::other("Audio live caller index overflow"))?;
        let original = call(
            originals,
            provider,
            capabilities,
            AUDIO_LIVE_COMMAND,
            args.clone(),
        )
        .await;
        // Observe all sixteen fixed paths even on an Error before interpreting
        // the Result. The four catalog paths are conditional on the SAME
        // existing gate; no second discover and no invented first-call rule.
        let catalog_observed = audio_live_observe_case_files(originals, case).and_then(|files| {
            let identity =
                audio_live_case_call_identity(originals, call_index, &args, &observed_descriptor)?;
            audio_live_catalog_originals(originals, case, &identity, &files)
        });
        let retained = (|| -> AudioLiveResult<()> {
            let value = serde_json::to_value(&original)?;
            originals.record(&format!("audio-live-{}.driver-result.original.json", case.id), &json!({
                "schema":"semwright.audio-derive-live-returned-result/1","result":value,
                "capture_scope":"original-returned-SDK-Result-raw-Host-pipes-retained-separately"
            }))
        })();
        if (original.is_err() || catalog_observed.is_err())
            && let Err(secondary) = &retained
        {
            originals.secondary(
                &format!("audio-live-{}.result-custody-error.original.json", case.id),
                secondary.as_ref(),
            );
        }
        if let Err(secondary) = &catalog_observed {
            originals.secondary(
                &format!(
                    "audio-live-{}.file-observation-error.original.json",
                    case.id
                ),
                secondary.as_ref(),
            );
        }
        // The original SDK Error is primary. After SDK success, an earlier
        // original-observation error precedes a later Result-custody error.
        let (result, catalog_observed) =
            audio_live_finish_observed(original, catalog_observed, retained)?;
        let observed = audio_live_verify_return(
            originals,
            case,
            source,
            &args,
            &result,
            &canonical.digest,
            ffmpeg_sha256,
        )
        .map(|mut value| {
            value["catalog_original_observation"] = catalog_observed;
            value
        });
        let retained = match &observed {
            Ok(value) => originals.record(
                &format!("audio-live-{}.observation.original.json", case.id),
                value,
            ),
            Err(error) => originals.record(
                &format!("audio-live-{}.validation-error.original.json", case.id),
                &json!({"error":error.to_string(),"native_execution_retried":false}),
            ),
        };
        match observed {
            Err(original) => {
                if let Err(secondary) = retained {
                    originals.secondary(
                        &format!(
                            "audio-live-{}.validation-custody-error.original.json",
                            case.id
                        ),
                        secondary.as_ref(),
                    );
                }
                return Err(original);
            }
            Ok(value) => {
                retained?;
                observations.push(value);
            }
        }
    }
    originals.record(
        "audio-live-observation.original.json",
        &json!({
            "schema":"semwright.audio-derive-live-observation/1","cases":observations,
            "scope":"exact-sample-clock-and-declared-PCM-properties",
            "calls":3,"native_execution_retried":false,
            "voice_identity":"NOT_APPLICABLE","human_manual_review":false,"formal_acceptance":false
        }),
    )?;
    Ok(())
}

async fn audio_live_shutdown_recorded<T>(
    originals: &AudioLiveOriginals,
    provider: &DriverProvider,
    primary: AudioLiveResult<T>,
    motion: bool,
) -> AudioLiveResult<T> {
    let prefix = if motion { "motion-live" } else { "audio-live" };
    let schema = if motion {
        "semwright.mlt-live-motion-provider-close/1"
    } else {
        "semwright.audio-derive-live-provider-close/1"
    };
    let closed = Provider::shutdown(provider).await;
    let retained = (|| -> AudioLiveResult<()> {
        originals.record(&format!("{prefix}-provider-shutdown.original.json"), &json!({
            "schema":schema,
            "result":serde_json::to_value(&closed)?,"original_operation_failed":primary.is_err(),
            "external_process_cleanup_attested":false
        }))
    })();
    let value = match primary {
        Err(original) => {
            if let Err(secondary) = closed {
                originals.secondary(
                    &format!("{prefix}-secondary-shutdown.original.json"),
                    &secondary,
                );
            }
            if let Err(secondary) = retained {
                originals.secondary(
                    &format!("{prefix}-shutdown-custody-error.original.json"),
                    secondary.as_ref(),
                );
            }
            return Err(original);
        }
        Ok(value) => value,
    };
    if closed.is_err()
        && let Err(secondary) = &retained
    {
        originals.secondary(
            &format!("{prefix}-shutdown-custody-error.original.json"),
            secondary.as_ref(),
        );
    }
    audio_live_finish_native(closed, retained).map(|()| value)
}

async fn audio_live_shutdown(
    originals: &AudioLiveOriginals,
    provider: &DriverProvider,
    primary: AudioLiveResult<()>,
) -> AudioLiveResult<()> {
    audio_live_shutdown_recorded(originals, provider, primary, false).await
}

// This fixture's pixels are authored before the existing Motion Canvas exporter.
// A Canvas observation is never constructed here and no exported PNG is edited.
fn motion_live_project() -> AudioLiveResult<semwright_driver_motion_canvas::model::Project> {
    use semwright_driver_motion_canvas::model::{
        AnimatedProperty, AnimatedValue, Animation, Easing, Node, NodeKind, Project, Properties,
        Scene, TimeAnchor,
    };
    let mut project = Project::empty("mlt-live-six-frames".into());
    // A deterministic synthetic source identity, not a runtime observation.
    project.generation = "1197a71a33c1149ba1f86c1c11e0e3ef".into();
    project.settings.width = 160;
    project.settings.height = 90;
    project.settings.fps = 30;
    project.settings.fps_denominator = 1;
    project.settings.color_space = semwright_driver_motion_canvas::model::ColorSpace::Srgb;
    project.settings.background = Some("#0000ff".into());
    let animations = [("caption-on", 80, 0.0, 1.0), ("caption-off", 120, 1.0, 0.0)]
        .into_iter()
        .map(|(id, offset_ms, from, to)| Animation {
            id: id.into(),
            target: "caption-box".into(),
            property: AnimatedProperty::Opacity,
            from: Some(AnimatedValue::Number(from)),
            to: AnimatedValue::Number(to),
            at: TimeAnchor {
                cue: None,
                offset_ms,
            },
            duration_ms: 1,
            duration_cue: None,
            easing: Easing::Linear,
        })
        .collect();
    project.scenes.push(Scene {
        id: "mlt-caption-source".into(),
        name: "Original six-frame MLT caption source".into(),
        duration_ms: 200,
        nodes: vec![Node {
            id: "caption-box".into(),
            name: "Frame three original caption box".into(),
            kind: NodeKind::Rect,
            parent: None,
            properties: Properties {
                // Canvas origin (80,45) + Rect center (-42.5,23) gives
                // the original pixel box [20,55) x [58,78).
                position: Some([-42.5, 23.0]),
                scale: Some([1.0, 1.0]),
                rotation: Some(0.0),
                width: Some(35.0.into()),
                height: Some(20.0.into()),
                fill: Some("#ffff00".into()),
                stroke_width: Some(0.0),
                radius: Some(0.0.into()),
                opacity: Some(0.0),
                ..Properties::default()
            },
        }],
        // Both core's integer sample and the runtime's interior representative
        // place frame 3 after 81 ms and before 120 ms, frames 2/4 outside it.
        animations,
        cues: vec![],
        transition: None,
    });
    semwright_driver_motion_canvas::validate::project_valid(&project)?;
    Ok(project)
}

fn motion_live_profile() -> semwright_driver_motion_canvas::model::RenderProfile {
    semwright_driver_motion_canvas::model::RenderProfile {
        first_frame: 0,
        end_frame_exclusive: 6,
        scale: semwright_driver_motion_canvas::model::RenderScale::Full,
        transparent: false,
        timeout_ms: 120_000,
    }
}

fn motion_live_expected_pixel_sha256(index: u64) -> String {
    // Independent, predeclared pixel oracle for this axis-aligned opaque source.
    // These bytes are hashed only; they never become renderer input or evidence.
    let mut hasher = Sha256::new();
    for y in 0..90 {
        for x in 0..160 {
            hasher.update(
                if index == 3 && (20..55).contains(&x) && (58..78).contains(&y) {
                    [255, 255, 0, 255]
                } else {
                    [0, 0, 255, 255]
                },
            );
        }
    }
    hex::encode(hasher.finalize())
}

fn motion_live_manifest(executable: PathBuf, node_sha256: String) -> Manifest {
    Manifest {
        manifest_version: 1,
        protocol: DRIVER_PROTOCOL_VERSION,
        id: "motion-canvas".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: None,
            process_names: vec!["node".into()],
            supported_versions: vec!["3.17.2".into(), "Node 22.22.0".into()],
        },
        transport: Transport::StdioV1,
        mounts: vec![
            DriverMount {
                root: "project".into(),
                read_only: false,
                execute: false,
            },
            DriverMount {
                root: "output".into(),
                read_only: false,
                execute: false,
            },
            DriverMount {
                root: "runtime".into(),
                read_only: true,
                execute: true,
            },
            DriverMount {
                root: "fontconfig".into(),
                read_only: true,
                execute: false,
            },
        ],
        system_config: vec![],
        secrets: vec![],
        tools: vec![DriverToolMount {
            root: "motion-node-tool".into(),
            name: "motion-node".into(),
            sha256: node_sha256,
            mounts: vec![
                "project".into(),
                "output".into(),
                "runtime".into(),
                "fontconfig".into(),
            ],
            system_config: vec![],
            dependencies: vec![],
            nvidia_gpu: false,
            resources: None,
        }],
        network: false,
        nvidia_gpu: false,
        loopback_port: None,
        // Exactly the existing Motion Canvas live producer's limits.
        resources: DriverResources {
            open_files: 512,
            processes: 256,
            cpu_seconds: 300,
            operation_cpu_seconds: 0,
            address_space_bytes: 4_294_967_296,
            file_size_bytes: 1_073_741_824,
        },
        request_timeout_ms: 300_000,
        interfaces: DriverInterfaces {
            cooperative_cancellation: true,
            progress: true,
            artifacts: true,
            health: true,
            host_tools: true,
            ..DriverInterfaces::default()
        },
    }
}

fn motion_live_original_artifact(
    originals: &AudioLiveOriginals,
    terminal: &Value,
) -> AudioLiveResult<(String, String)> {
    use semwright_driver_motion_canvas::{security, validate};
    originals.before_effect()?;
    audio_live_require(
        terminal["state"] == "succeeded",
        "Motion fixture did not succeed",
    )?;
    let artifact = &terminal["artifact"];
    let directory = artifact["directory"]
        .as_str()
        .ok_or_else(|| std::io::Error::other("Motion fixture artifact directory is missing"))?;
    audio_live_require(
        directory.strip_prefix("render-").is_some_and(|id| {
            id.len() == 32
                && id
                    .bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        }),
        "Motion fixture artifact is not one original render directory",
    )?;
    let manifest_path = format!("{directory}/artifact-manifest.json");
    audio_live_require(
        artifact["manifest"] == manifest_path
            && artifact["frame_count"] == 6
            && artifact["first_png"] == format!("{directory}/frames/000000.png")
            && artifact["last_png"] == format!("{directory}/frames/000005.png"),
        "Motion fixture artifact summary changed its six-frame contract",
    )?;
    let bytes = originals.media.read(&manifest_path, 8 * 1024 * 1024)?;
    let manifest_sha256 = security::sha256(&bytes);
    audio_live_require(
        artifact["manifest_sha256"] == manifest_sha256
            && artifact["manifest_bytes"].as_u64() == Some(bytes.len() as u64),
        "Motion fixture original manifest does not match its Provider result",
    )?;
    let manifest: Value = serde_json::from_slice(&bytes)?;
    let plan = validate::render_plan(&motion_live_project()?, &motion_live_profile())?;
    audio_live_require(
        manifest["renderer"] == "motion-canvas-core-renderer-v3.17.2"
            && manifest["plan"] == serde_json::to_value(plan)?
            && manifest["pixel_validation"] == json!({"mode":"all","sample_indices":[0,1,2,3,4,5]}),
        "Motion fixture original manifest does not cover the exact source plan",
    )?;
    audio_live_require(
        manifest["canvas_color_observation"]["path"] == "canvas-color-observations.json",
        "Motion fixture lacks its original Canvas receipt",
    )?;
    let color_path = format!("{directory}/canvas-color-observations.json");
    let color_bytes = originals.media.read(&color_path, 16 * 1024 * 1024)?;
    let color_sha256 = security::sha256(&color_bytes);
    audio_live_require(
        manifest["canvas_color_observation"]["sha256"] == color_sha256
            && manifest["canvas_color_observation"]["bytes"].as_u64()
                == Some(color_bytes.len() as u64),
        "Motion fixture original Canvas receipt bytes changed",
    )?;
    let colors: Value = serde_json::from_slice(&color_bytes)?;
    audio_live_require(
        colors.as_object().is_some_and(|object| object.len() == 3)
            && colors["schema"] == "semwright.motion-canvas-context-color/1"
            && colors["render_input_digest"]
                .as_str()
                .is_some_and(security::digest),
        "Motion fixture original Canvas receipt has no render input binding",
    )?;
    let frames = manifest["frames"]
        .as_array()
        .filter(|rows| rows.len() == 6)
        .ok_or_else(|| {
            std::io::Error::other("Motion fixture must retain exactly six original PNGs")
        })?;
    let color_frames = colors["frames"]
        .as_array()
        .filter(|rows| rows.len() == 6)
        .ok_or_else(|| {
            std::io::Error::other("Motion fixture must retain six actual Canvas observations")
        })?;
    let mut observations = Vec::new();
    for (index, (frame, canvas)) in frames.iter().zip(color_frames).enumerate() {
        let relative = format!("frames/{index:06}.png");
        audio_live_require(
            frame["index"].as_u64() == Some(index as u64) && frame["file"] == relative,
            "Motion fixture original frame order or path changed",
        )?;
        let png = originals
            .media
            .read(&format!("{directory}/{relative}"), 16 * 1024 * 1024)?;
        let png_sha256 = security::sha256(&png);
        audio_live_require(
            frame["sha256"] == png_sha256
                && frame["bytes"].as_u64() == Some(png.len() as u64)
                && canvas.as_object().is_some_and(|object| object.len() == 4)
                && canvas["index"].as_u64() == Some(index as u64)
                && canvas["frame"].as_u64() == Some(index as u64)
                && canvas["png_sha256"] == png_sha256
                && canvas["canvas"] == frame["canvas_color"],
            "Motion fixture Canvas observation is not bound to the unmodified PNG",
        )?;
        security::validate_canvas_color_evidence(&canvas["canvas"], 160, 90, "srgb")?;
        let pixels = security::inspect_png(&png)?;
        audio_live_require(
            pixels.width == 160
                && pixels.height == 90
                && pixels.min_alpha == 255
                && pixels.max_alpha == 255
                && frame["pixel_sha256"] == pixels.pixel_sha256
                && frame["min_alpha"] == pixels.min_alpha
                && frame["max_alpha"] == pixels.max_alpha
                && pixels.pixel_sha256 == motion_live_expected_pixel_sha256(index as u64),
            "Motion fixture pixels differ from the predeclared frame-three box geometry",
        )?;
        observations.push(json!({"index":index,"png_sha256":png_sha256,"pixels":pixels}));
    }
    originals.record("motion-live-artifact-observation.original.json", &json!({
        "schema":"semwright.mlt-live-motion-fixture-observation/1",
        "manifest_path":manifest_path,"manifest_sha256":manifest_sha256,
        "canvas_receipt_path":color_path,"canvas_receipt_sha256":color_sha256,
        "render_input_digest":colors["render_input_digest"],"frames":observations,
        "canvas_origin":"fixed-exporter-getContextAttributes-from-the-same-PNG-producing-canvas",
        "source_pixel_oracle":"six-160x90-opaque-srgb-blue-frames-yellow-box-20-58-35-20-only-frame-3",
        "files_modified_by_observer":false,"wire_bytes_claimed":false,
        "host_stream_eof_attested":false,"descendant_closure_attested":false,
        "authority_issued":false,"formal_acceptance":false,"native_execution_retried":false
    }))?;
    Ok((manifest_path, manifest_sha256))
}

async fn motion_live_fixture(
    originals: &AudioLiveOriginals,
    helper: &Path,
) -> AudioLiveResult<(String, String)> {
    use std::io::Write;
    use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
    originals.before_effect()?;
    let project = originals.directory("project")?;
    let binary = originals.directory("binaries")?;
    let state = originals.directory("state")?;
    let cargo_executable = configured_tool("SEMWRIGHT_TEST_MOTION_DRIVER");
    let runtime = configured_tool("SEMWRIGHT_TEST_MOTION_RUNTIME");
    let node = configured_tool("SEMWRIGHT_TEST_MOTION_NODE");
    let executable = binary.path().join("semwright-motion-canvas-driver");
    audio_live_copy(
        originals,
        "motion-driver-binary",
        &cargo_executable,
        &executable,
    )?;
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
    audio_live_require(
        digest(&executable) == digest(&cargo_executable),
        "Motion driver copy differs from its source",
    )?;
    let source = serde_json::to_vec_pretty(&motion_live_project()?)?;
    originals.record_bytes(
        "motion-live-project.original.json",
        &source,
        AUDIO_LIVE_METADATA_LIMIT,
    )?;
    let source_path = project.path().join("semwright-motion.json");
    let mut source_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&source_path)?;
    source_file.write_all(&source)?;
    source_file.sync_all()?;
    project.handle.sync_all()?;
    let motion_state = state.path().join("motion-canvas");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&motion_state)?;
    let state_chain = audio_live_open_chain(&motion_state)?;
    let manifest = motion_live_manifest(executable.clone(), digest(&node));
    let grants = [
        ("project", project.path().to_path_buf(), true),
        ("output", originals.media_path.path().to_path_buf(), true),
        ("runtime", runtime.clone(), false),
        ("fontconfig", PathBuf::from("/etc/fonts"), false),
        ("motion-node-tool", node.clone(), false),
    ]
    .into_iter()
    .map(|(name, path, write)| {
        Ok(FilesystemGrant {
            name: name.into(),
            path: path.canonicalize()?,
            read: true,
            write,
        })
    })
    .collect::<std::io::Result<Vec<_>>>()?;
    originals.record(
        "motion-live-tool-source-pins.original.json",
        &json!({
            "schema":"semwright.mlt-live-motion-fixture-inputs/1",
            "project_sha256":semwright_mlt_video::hash::sha256(&source),
            "cargo_driver_path":cargo_executable,"cargo_driver_sha256":digest(&cargo_executable),
            "driver_path":executable,"driver_sha256":digest(&executable),
            "node_path":node,"node_sha256":digest(&node),
            "runtime_path":runtime,"runtime_lock_sha256":digest(&runtime.join("package-lock.json")),
            "runtime_render_helper_sha256":digest(&runtime.join("render.mjs")),
            "sandbox_helper_path":helper,"sandbox_helper_sha256":digest(helper),
            "source_scope":"same-six-frame-source-before-any-Canvas-effect",
            "authority_issued":false,"native_execution_retried":false
        }),
    )?;
    originals.before_effect()?;
    for directory in &state_chain {
        directory.check()?;
    }
    originals.record(
        "motion-live-provider-connect-request.original.json",
        &json!({
            "schema":"semwright.mlt-live-provider-connect-request/1",
            "manifest":serde_json::to_value(&manifest)?,"state":motion_state,"helper":helper,
            "filesystem_grants":grants.iter().map(|grant| json!({
                "name":grant.name,"path":grant.path,"read":grant.read,"write":grant.write
            })).collect::<Vec<_>>(),"dry_run":false,"authority_issued_by_evidence_root":false
        }),
    )?;
    let connected = DriverProvider::connect(manifest, &motion_state, helper, &grants, false).await;
    let retained = (|| -> AudioLiveResult<()> {
        originals.record("motion-live-provider-connect.original.json", &json!({
            "schema":"semwright.mlt-live-motion-provider-connect/1","returned_ok":connected.is_ok(),
            "original_error":serde_json::to_value(connected.as_ref().err())?,
            "capture_scope":"same-original-connection-return-no-provider-serialization",
            "native_execution_retried":false
        }))
    })();
    let provider = match connected {
        Err(original) => {
            originals.stop();
            if let Err(secondary) = retained {
                originals.secondary(
                    "motion-live-connect-custody-error.original.json",
                    secondary.as_ref(),
                );
            }
            return Err(Box::new(original));
        }
        Ok(provider) => {
            if let Err(original) = retained {
                originals.stop();
                return audio_live_shutdown_recorded(
                    originals,
                    provider.as_ref(),
                    Err(original),
                    true,
                )
                .await;
            }
            provider
        }
    };
    let operations: AudioLiveResult<Value> = async {
        originals.before_effect()?;
        let original = Provider::capabilities(provider.as_ref()).await;
        let retained = (|| -> AudioLiveResult<()> {
            let value = match &original {
                Ok(capabilities) => json!({"Ok":capabilities.iter().map(|capability| json!({
                    "descriptor":capability.descriptor,"aliases":capability.aliases,
                    "tags":capability.tags,"object_types":capability.object_types
                })).collect::<Vec<_>>()}),
                Err(error) => json!({"Err":serde_json::to_value(error)?}),
            };
            originals.record_limited(
                "motion-live-capabilities-return.original.json",
                &json!({
                    "schema":"semwright.mlt-live-motion-capabilities-return/1","result":value,
                    "capture_scope":"all-original-ProvidedCapability-fields-not-wire-bytes",
                    "native_execution_retried":false
                }),
                AUDIO_LIVE_CAPABILITIES_LIMIT,
            )
        })();
        if original.is_err()
            && let Err(secondary) = &retained
        {
            originals.secondary(
                "motion-live-capabilities-custody-error.original.json",
                secondary.as_ref(),
            );
        }
        let capabilities = audio_live_finish_native(original, retained)?;
        let inspected = call(
            originals,
            provider.as_ref(),
            &capabilities,
            "driver.motion-canvas.project.inspect",
            json!({}),
        )
        .await?;
        let fingerprint = inspected["fingerprint"]
            .as_str()
            .filter(|value| semwright_driver_motion_canvas::security::digest(value))
            .ok_or_else(|| {
                std::io::Error::other("Motion fixture lacks its inspected fingerprint")
            })?;
        audio_live_require(
            fingerprint == semwright_mlt_video::hash::sha256(&source),
            "Motion inspected fingerprint differs from the retained original source",
        )?;
        let terminal = call(
            originals,
            provider.as_ref(),
            &capabilities,
            "driver.motion-canvas.render.execute",
            json!({
                "expected_fingerprint":fingerprint,"profile":motion_live_profile()
            }),
        )
        .await?;
        audio_live_require(
            terminal["state"] == "succeeded",
            "Original Motion fixture render did not succeed",
        )?;
        Ok(terminal)
    }
    .await;
    if operations.is_err() {
        originals.stop();
    }
    // The real public shutdown Result is retained. It does not attest stream EOF
    // or descendant closure, and cannot turn an original UNKNOWN into known.
    let terminal =
        audio_live_shutdown_recorded(originals, provider.as_ref(), operations, true).await?;
    for directory in &state_chain {
        directory.check()?;
    }
    originals.before_effect()?;
    let project_root = semwright_mlt_video::fs::Root::open(project.path(), true, false)?;
    audio_live_require(
        project_root.read("semwright-motion.json", AUDIO_LIVE_METADATA_LIMIT)? == source,
        "Original Motion project changed during preparation",
    )?;
    let observed = motion_live_original_artifact(originals, &terminal);
    if let Err(error) = &observed {
        originals.stop();
        let retained = originals.record(
            "motion-live-artifact-validation-error.original.json",
            &json!({
                "error":error.to_string(),"native_execution_retried":false,
                "host_stream_eof_attested":false,"descendant_closure_attested":false
            }),
        );
        if let Err(secondary) = retained {
            originals.secondary(
                "motion-live-validation-custody-error.original.json",
                secondary.as_ref(),
            );
        }
    }
    observed
}

#[derive(Clone, Copy)]
struct AudioLiveNegativeCase {
    id: &'static str,
    capability: &'static str,
    expected: semwright_types::ErrorCode,
    absent_outputs: &'static [&'static str],
}

const AUDIO_LIVE_NEGATIVES: [AudioLiveNegativeCase; 6] = [
    AudioLiveNegativeCase {
        id: "stale_frames",
        capability: "driver.mlt-video.frames.encode",
        expected: semwright_types::ErrorCode::StaleReference,
        absent_outputs: &["stale-mezzanine.mkv"],
    },
    AudioLiveNegativeCase {
        id: "stale",
        capability: "driver.mlt-video.av.caption_pixels.verify",
        expected: semwright_types::ErrorCode::StaleReference,
        absent_outputs: &[],
    },
    AudioLiveNegativeCase {
        id: "denied",
        capability: "driver.mlt-video.av.caption_pixels.verify",
        expected: semwright_types::ErrorCode::BackendFailed,
        absent_outputs: &[],
    },
    AudioLiveNegativeCase {
        id: "invalid",
        capability: "driver.mlt-video.av.caption_pixels.verify",
        expected: semwright_types::ErrorCode::InvalidArgument,
        absent_outputs: &[],
    },
    AudioLiveNegativeCase {
        id: "stale_sync",
        capability: "driver.mlt-video.sync.probe",
        expected: semwright_types::ErrorCode::StaleReference,
        absent_outputs: &[],
    },
    AudioLiveNegativeCase {
        id: "stale_mux",
        capability: "driver.mlt-video.av.mux",
        expected: semwright_types::ErrorCode::StaleReference,
        absent_outputs: &["stale-av-master.mp4", "stale-av-master.decoded.wav"],
    },
];
const AUDIO_LIVE_NEGATIVE_INPUT_LIMIT: usize = 16 * 1024 * 1024;
const AUDIO_LIVE_NEGATIVE_TOTAL_INPUT_LIMIT: usize = 32 * 1024 * 1024;

struct AudioLiveNegativePlan {
    case: AudioLiveNegativeCase,
    args: Value,
    inputs: Vec<(&'static str, String)>,
}

struct AudioLiveNegativeInput {
    root: &'static str,
    path: String,
    bytes: Vec<u8>,
    sha256: String,
}

fn audio_live_negative_root<'a>(
    originals: &'a AudioLiveOriginals,
    root: &str,
) -> AudioLiveResult<&'a semwright_mlt_video::fs::Root> {
    match root {
        "media" => Ok(&originals.media),
        "output" => Ok(&originals.output),
        _ => Err(std::io::Error::other("Undeclared negative input root").into()),
    }
}

fn audio_live_negative_grants(
    originals: &AudioLiveOriginals,
    shared: &[FilesystemGrant],
) -> AudioLiveResult<Vec<FilesystemGrant>> {
    shared
        .iter()
        .map(|grant| {
            let path = match grant.name.as_str() {
                "project" | "media" | "output" | "scratch" => {
                    originals.directory(&grant.name)?.path
                }
                _ => {
                    audio_live_require(!grant.write, "A shared negative input grant is writable")?;
                    grant.path.clone()
                }
            };
            Ok(FilesystemGrant {
                name: grant.name.clone(),
                path,
                read: grant.read,
                write: grant.write,
            })
        })
        .collect()
}

fn audio_live_prepare_negative_inputs(
    positive: &AudioLiveOriginals,
    scopes: &[AudioLiveOriginals],
    plans: &[AudioLiveNegativePlan],
) -> AudioLiveResult<Vec<Vec<AudioLiveNegativeInput>>> {
    audio_live_require(
        scopes.len() == 6 && plans.len() == 6,
        "Six terminal negatives are required",
    )?;
    let mut total = 0usize;
    let mut count = 0usize;
    let mut prepared = Vec::new();
    for ((scope, plan), declared) in scopes.iter().zip(plans).zip(AUDIO_LIVE_NEGATIVES) {
        scope.before_effect()?;
        audio_live_require(
            scope.terminal_case == Some(declared.id) && plan.case.id == declared.id,
            "Terminal case order or scope binding differs",
        )?;
        let mut inputs = Vec::new();
        for (root, path) in &plan.inputs {
            audio_live_require(
                !path.is_empty()
                    && Path::new(path)
                        .components()
                        .all(|part| matches!(part, std::path::Component::Normal(_))),
                "Negative input is not a relative closed path",
            )?;
            let bytes = audio_live_negative_root(positive, root)?
                .read(path, AUDIO_LIVE_NEGATIVE_INPUT_LIMIT)?;
            total = total
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("Negative input size overflow"))?;
            count += 1;
            audio_live_require(
                total <= AUDIO_LIVE_NEGATIVE_TOTAL_INPUT_LIMIT && count <= 10,
                "Negative preparation exceeds its aggregate ten-file / 32 MiB bound",
            )?;
            inputs.push(AudioLiveNegativeInput {
                root,
                path: path.clone(),
                sha256: semwright_mlt_video::hash::sha256(&bytes),
                bytes,
            });
        }
        // Capture every selected original and the exact request before copies or
        // any negative Provider connection. No runtime, binary or long PCM copy.
        scope.record(
            "negative-plan.original.json",
            &json!({
                "case":plan.case.id,"capability":plan.case.capability,"arguments":plan.args,
                "expected_code":plan.case.expected,"expected_outcome_known":false,
                "inputs":inputs.iter().map(|input| json!({"root":input.root,"path":input.path,
                    "bytes":input.bytes.len(),"sha256":input.sha256})).collect::<Vec<_>>(),
                "absent_outputs":plan.case.absent_outputs,"native_execution_retried":false
            }),
        )?;
        prepared.push(inputs);
    }
    positive.record("negative-preparation-budget.original.json", &json!({
        "cases":AUDIO_LIVE_NEGATIVES.map(|case| case.id),"input_files":count,"input_bytes":total,
        "maximum_files":10,"maximum_input_bytes":AUDIO_LIVE_NEGATIVE_TOTAL_INPUT_LIMIT,
        "records_limit_shared":AUDIO_LIVE_RECORDS_LIMIT,"calls_limit_shared":AUDIO_LIVE_CALL_LIMIT,
        "runtime_and_binaries_copied":false,"all_positives_and_shutdown_completed":true
    }))?;
    for (scope, inputs) in scopes.iter().zip(&prepared) {
        for input in inputs {
            scope.before_effect()?;
            let directory = scope.directory(input.root)?;
            if let Some(parent) = Path::new(&input.path).parent() {
                // Every path component was checked above; this newly opened,
                // private scope has not yet been granted to any provider.
                std::fs::create_dir_all(directory.path().join(parent))?;
            }
            audio_live_negative_root(scope, input.root)?.write_new(
                input.root,
                &input.path,
                &input.bytes,
            )?;
            let retained = audio_live_negative_root(scope, input.root)?
                .read(&input.path, AUDIO_LIVE_NEGATIVE_INPUT_LIMIT)?;
            audio_live_require(
                retained == input.bytes,
                "Negative input copy differs from its original",
            )?;
        }
    }
    Ok(prepared)
}

fn audio_live_observe_negative(
    positive: &AudioLiveOriginals,
    scope: &AudioLiveOriginals,
    plan: &AudioLiveNegativePlan,
    inputs: &[AudioLiveNegativeInput],
) -> AudioLiveResult<Value> {
    positive.check_directories()?;
    scope.check_directories()?;
    let mut observed = Vec::new();
    for input in inputs {
        for (label, originals) in [("positive", positive), ("negative", scope)] {
            let bytes = audio_live_negative_root(originals, input.root)?
                .read(&input.path, AUDIO_LIVE_NEGATIVE_INPUT_LIMIT)?;
            let actual = semwright_mlt_video::hash::sha256(&bytes);
            audio_live_require(
                actual == input.sha256 && bytes.len() == input.bytes.len(),
                "Terminal negative changed a retained source",
            )?;
            observed.push(json!({"scope":label,"root":input.root,"path":input.path,
                "bytes":bytes.len(),"sha256":actual}));
        }
    }
    for path in plan.case.absent_outputs {
        match scope.output.read_file(path, 1) {
            Err(error) if error.code == "NotFound" => {}
            Err(error) => return Err(Box::new(error)),
            Ok(_) => {
                return Err(std::io::Error::other("Terminal negative published an output").into());
            }
        }
    }
    Ok(
        json!({"case":plan.case.id,"sources":observed,"absent_outputs":plan.case.absent_outputs,
        "observation_scope":"named-files-only","all_effects_absent_claimed":false,
        "host_stream_eof_attested":false,"descendant_closure_attested":false}),
    )
}

fn audio_live_evaluate_terminal_negative(
    scope: &AudioLiveOriginals,
    plan: &AudioLiveNegativePlan,
    index: usize,
    original: semwright_types::Result<Value>,
    observations: AudioLiveResult<Value>,
) -> AudioLiveResult<()> {
    let checked = (|| -> AudioLiveResult<()> {
        audio_live_require(
            scope.terminal_case == Some(plan.case.id)
                && scope
                    .terminal_started
                    .load(std::sync::atomic::Ordering::SeqCst)
                && scope.blocked.load(std::sync::atomic::Ordering::SeqCst)
                && !scope
                    .custody_failed
                    .load(std::sync::atomic::Ordering::SeqCst)
                && scope.calls.load(std::sync::atomic::Ordering::SeqCst) == index,
            "Terminal negative scope or custody is incomplete",
        )?;
        let error = original
            .as_ref()
            .err()
            .ok_or_else(|| std::io::Error::other("Terminal negative unexpectedly succeeded"))?;
        audio_live_require(
            error.code == plan.case.expected && !error.outcome_known,
            "Terminal negative returned a different code or unexpected known certainty",
        )?;
        if plan.case.id == "stale" {
            audio_live_require(
                !error.message.contains("caption-oracle-master.mp4"),
                "Caption rejection disclosed the private master name",
            )?;
        }
        let stem = format!("call-{index:05}");
        let declared: Value = serde_json::from_slice(
            &scope
                .records
                .read("negative-plan.original.json", AUDIO_LIVE_METADATA_LIMIT)?,
        )?;
        let request: Value = serde_json::from_slice(&scope.records.read(
            &format!("{stem}.request.original.json"),
            AUDIO_LIVE_METADATA_LIMIT,
        )?)?;
        let result: Value = serde_json::from_slice(&scope.records.read(
            &format!("{stem}.result.original.json"),
            AUDIO_LIVE_METADATA_LIMIT,
        )?)?;
        audio_live_require(
            declared["case"] == plan.case.id
                && declared["capability"] == plan.case.capability
                && declared["arguments"] == plan.args
                && declared["expected_code"] == serde_json::to_value(plan.case.expected)?
                && declared["expected_outcome_known"] == false
                && request["terminal_case"] == plan.case.id
                && request["capability"] == plan.case.capability
                && request["arguments"] == plan.args
                && request["context"]["session"]
                    == format!("mlt-video-live-negative-{}", plan.case.id)
                && result["terminal_case"] == plan.case.id
                && result["result"] == serde_json::to_value(&original)?,
            "Terminal evaluation is not bound to the complete original request and return",
        )?;
        let observed = observations
            .as_ref()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        audio_live_require(
            observed["case"] == plan.case.id,
            "Terminal file observation belongs to another case",
        )?;
        scope.record("negative-files-observation.original.json", observed)?;
        Ok(())
    })();
    let retained = scope.record(
        "negative-evaluation.json",
        &json!({
            "case":plan.case.id,"call_index":index,"expected_code":plan.case.expected,
            "assessment":if checked.is_ok() {"EXPECTED_TERMINAL_REJECTION"} else {"FAILED"},
            "assessment_error":checked.as_ref().err().map(ToString::to_string),
            "original_result":format!("call-{index:05}.result.original.json"),
            "original_outcome_known":original.as_ref().err().map(|error| error.outcome_known),
            "outcome_reclassified":false,"another_execute_permitted":false,
            "formal_acceptance":false,"native_execution_retried":false
        }),
    );
    match (checked, retained) {
        (Ok(()), Ok(())) => Ok(()),
        (checked, retained) => {
            let secondary = checked
                .err()
                .or_else(|| retained.err())
                .ok_or_else(|| std::io::Error::other("Missing terminal evaluation failure"))?;
            scope.secondary(
                "negative-evaluation-error.original.json",
                secondary.as_ref(),
            );
            // A custody/evaluation failure never replaces a native primary error.
            match original {
                Err(error) => Err(Box::new(error)),
                Ok(_) => Err(secondary),
            }
        }
    }
}

async fn audio_live_terminal_negatives(
    positive: &AudioLiveOriginals,
    scopes: &[AudioLiveOriginals],
    plans: &[AudioLiveNegativePlan],
    manifest: &Value,
    shared_grants: &[FilesystemGrant],
    helper: &Path,
) -> AudioLiveResult<()> {
    let prepared = audio_live_prepare_negative_inputs(positive, scopes, plans)?;
    for ((scope, plan), inputs) in scopes.iter().zip(plans).zip(&prepared) {
        scope.before_effect()?;
        let state = scope.directory("state")?;
        let grants = audio_live_negative_grants(scope, shared_grants)?;
        scope.record(
            "negative-connect-request.original.json",
            &json!({
                "case":plan.case.id,"manifest":manifest,"state":state.path(),"helper":helper,
                "filesystem_grants":grants.iter().map(|grant| json!({"name":grant.name,
                    "path":grant.path,"read":grant.read,"write":grant.write})).collect::<Vec<_>>()
            }),
        )?;
        let connected = DriverProvider::connect(
            serde_json::from_value(manifest.clone())?,
            state.path(),
            helper,
            &grants,
            false,
        )
        .await;
        let retained = scope.record("negative-connect-return.original.json", &json!({
            "returned_ok":connected.is_ok(),"original_error":serde_json::to_value(connected.as_ref().err())?
        }));
        let provider = match connected {
            Err(original) => {
                if let Err(secondary) = retained {
                    scope.secondary(
                        "negative-connect-custody-error.original.json",
                        secondary.as_ref(),
                    );
                }
                return Err(Box::new(original));
            }
            Ok(provider) => {
                if let Err(error) = retained {
                    return audio_live_shutdown(scope, provider.as_ref(), Err(error)).await;
                }
                provider
            }
        };
        let evaluated: AudioLiveResult<()> = async {
            let original_capabilities = Provider::capabilities(provider.as_ref()).await;
            let retained = scope.record_limited(
                "negative-capabilities.original.json",
                &match &original_capabilities {
                    Ok(capabilities) => json!({"Ok":capabilities.iter().map(|capability| json!({
                    "descriptor":capability.descriptor,"aliases":capability.aliases,
                    "tags":capability.tags,"object_types":capability.object_types
                })).collect::<Vec<_>>()}),
                    Err(error) => json!({"Err":serde_json::to_value(error)?}),
                },
                AUDIO_LIVE_CAPABILITIES_LIMIT,
            );
            let capabilities = audio_live_finish_native(original_capabilities, retained)?;
            let index = scope
                .calls
                .load(std::sync::atomic::Ordering::SeqCst)
                .checked_add(1)
                .ok_or_else(|| std::io::Error::other("Negative call index overflow"))?;
            // Exactly one call in this predeclared scope; finish_call permanently
            // fences it even for an unexpected Ok or known Error.
            let original = call(
                scope,
                provider.as_ref(),
                &capabilities,
                plan.case.capability,
                plan.args.clone(),
            )
            .await;
            let observations = audio_live_observe_negative(positive, scope, plan, inputs);
            audio_live_evaluate_terminal_negative(scope, plan, index, original, observations)
        }
        .await;
        audio_live_shutdown(scope, provider.as_ref(), evaluated).await?;
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires bubblewrap, real melt and ffprobe on a Linux host"]
async fn real_mlt_video_driver_runs_inside_sandbox() -> AudioLiveResult<()> {
    let selector = std::env::var("SEMWRIGHT_TEST_MLT_VIDEO").ok();
    audio_live_selector(selector.as_deref())?;
    let evidence_root = std::env::var_os(AUDIO_LIVE_ORIGINALS_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| std::io::Error::other("External live evidence root is required"))?;
    let audio_originals = AudioLiveOriginals::open(Some(evidence_root.join("positive")))?;
    // The workflow creates these empty scopes before the attempt. Every scope
    // shares the original aggregate counters; only its terminal fence is local.
    let negative_originals = AUDIO_LIVE_NEGATIVES
        .iter()
        .map(|case| {
            AudioLiveOriginals::open_scope(
                evidence_root.join(case.id),
                &audio_originals.excluded_locations,
                Some((case.id, &audio_originals)),
            )
        })
        .collect::<AudioLiveResult<Vec<_>>>()?;
    let mut negative_plans = Vec::new();

    let cargo_executable = PathBuf::from(env!("CARGO_BIN_EXE_semwright-mlt-video-driver"));
    let cargo_runtime_runner = PathBuf::from(env!("CARGO_BIN_EXE_semwright-mlt-runtime-runner"));
    let binary_dir = audio_originals.directory("binaries")?;
    std::fs::set_permissions(binary_dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let executable = binary_dir.path().join("semwright-mlt-video-driver");
    let runtime_runner = binary_dir.path().join("semwright-mlt-runtime-runner");
    audio_live_copy(
        &audio_originals,
        "driver-binary",
        &cargo_executable,
        &executable,
    )?;
    audio_live_copy(
        &audio_originals,
        "runtime-runner-binary",
        &cargo_runtime_runner,
        &runtime_runner,
    )?;
    for binary in [&executable, &runtime_runner] {
        std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("SEMWRIGHT_TEST_SANDBOX_HELPER must point to semwright-sandbox"),
    );
    let melt = configured_tool("SEMWRIGHT_TEST_MELT");
    let ffprobe = configured_tool("SEMWRIGHT_TEST_FFPROBE");
    let ffmpeg = configured_tool("SEMWRIGHT_TEST_FFMPEG");
    let bwrap = configured_tool("SEMWRIGHT_TEST_BWRAP");
    let mlt_runtime_root = melt
        .parent()
        .and_then(Path::parent)
        .expect("melt must live below a runtime bin directory")
        .canonicalize()
        .expect("canonical MLT runtime root");

    let project = audio_originals.directory("project")?;
    let media = audio_originals.media_path.clone();
    let output = audio_originals.output_path.clone();
    let audio_sources = audio_live_stage_sources(&audio_originals)?;
    let runtime = audio_originals.directory("runtime")?;
    let scratch = audio_originals.directory("scratch")?;
    let state = audio_originals.directory("state")?;
    for directory in [
        project.path(),
        media.path(),
        output.path(),
        runtime.path(),
        scratch.path(),
        state.path(),
    ] {
        std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    }

    // /usr is mounted read-only by DriverProvider. Pin the canonical host tools directly:
    // copying them into a user-owned bind mount can change ownership presentation across
    // Bubblewrap user namespaces even though the bytes and mode are unchanged.
    let runtime_json = json!({
        "schema": 1,
        "melt": {
            "path": melt,
            "sha256": digest(&melt)
        },
        "ffprobe": {
            "path": ffprobe,
            "sha256": digest(&ffprobe)
        },
        "ffmpeg": {
            "path": ffmpeg,
            "sha256": digest(&ffmpeg)
        },
        "bubblewrap": {
            "path": bwrap.to_string_lossy(),
            "sha256": digest(&bwrap)
        },
        "timeout_seconds": 120
    });
    let runtime_file = runtime.path().join("runtime.json");
    let runtime_bytes = serde_json::to_vec_pretty(&runtime_json)?;
    audio_originals.record_bytes(
        "audio-live-runtime-config.original.json",
        &runtime_bytes,
        AUDIO_LIVE_METADATA_LIMIT,
    )?;
    std::fs::write(&runtime_file, &runtime_bytes)?;
    let audio_ffmpeg_sha256 = digest(&ffmpeg);
    audio_live_require(
        runtime_json["ffmpeg"]["sha256"] == audio_ffmpeg_sha256,
        "Pinned FFmpeg changed during live preparation",
    )?;
    audio_originals.record("audio-live-tool-source-pins.original.json", &json!({
        "schema":"semwright.audio-derive-live-tool-source-observation/1",
        "driver_sha256":digest(&executable),"runner_sha256":digest(&runtime_runner),
        "cargo_driver_path":cargo_executable,"cargo_driver_sha256":digest(&cargo_executable),
        "cargo_runner_path":cargo_runtime_runner,"cargo_runner_sha256":digest(&cargo_runtime_runner),
        "sandbox_helper_path":helper,"sandbox_helper_sha256":digest(&helper),
        "melt_sha256":digest(&melt),"ffprobe_sha256":digest(&ffprobe),
        "ffmpeg_sha256":audio_ffmpeg_sha256,"bubblewrap_sha256":digest(&bwrap),
        "catalog_sha256":semwright_mlt_video::catalog::digest(&semwright_mlt_video::catalog::capabilities()?),
        "compiled_test_source_sha256":{
            "app.rs":semwright_mlt_video::hash::sha256(include_bytes!("../src/app.rs")),
            "runtime.rs":semwright_mlt_video::hash::sha256(include_bytes!("../src/runtime.rs")),
            "main.rs":semwright_mlt_video::hash::sha256(include_bytes!("../src/main.rs")),
            "runtime_runner.rs":semwright_mlt_video::hash::sha256(include_bytes!("../src/bin/runtime_runner.rs")),
            "catalog.json":semwright_mlt_video::hash::sha256(include_bytes!("../src/catalog.json")),
            "Cargo.toml":semwright_mlt_video::hash::sha256(include_bytes!("../Cargo.toml")),
            "Cargo.lock":semwright_mlt_video::hash::sha256(include_bytes!("../../../Cargo.lock")),
            "tests/live.rs":semwright_mlt_video::hash::sha256(include_bytes!("live.rs")),
            "fixtures/expected/capabilities.json":semwright_mlt_video::hash::sha256(include_bytes!("../fixtures/expected/capabilities.json")),
            "fixtures/expected/catalog-wire.json":semwright_mlt_video::hash::sha256(include_bytes!("../fixtures/expected/catalog-wire.json")),
            "fixtures/expected/digests.json":semwright_mlt_video::hash::sha256(include_bytes!("../fixtures/expected/digests.json")),
            "fixtures/SHA256SUMS.json":semwright_mlt_video::hash::sha256(include_bytes!("../fixtures/SHA256SUMS.json"))
        },
        "external_source_binding_required":true,"external_source_binding_attested_here":false,
        "this_record_issues_authority":false,"formal_acceptance":false
    }))?;
    std::fs::set_permissions(&runtime_file, std::fs::Permissions::from_mode(0o600)).unwrap();

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/media");
    audio_live_copy(
        &audio_originals,
        "fixture-red.mkv",
        &fixtures.join("red.mkv"),
        &media.path().join("red.mkv"),
    )?;
    audio_live_copy(
        &audio_originals,
        "fixture-sine.wav",
        &fixtures.join("sine.wav"),
        &media.path().join("sine.wav"),
    )?;
    let h264_source = media.path().join("h264-source.mp4");
    let status = audio_live_command(
        &audio_originals,
        "fixture-h264",
        Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "color=c=red:s=1920x1080:r=30:d=2",
                "-frames:v",
                "60",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "12",
                "-pix_fmt",
                "yuv420p",
                "-an",
            ])
            .arg(&h264_source),
    )?
    .status;
    assert!(
        status.success(),
        "failed to create 1080p H.264 input fixture"
    );

    // Harness-only oracle media: one white video flash and one single-sample audio
    // impulse share the 1.000000 s presentation timestamp. The product under test
    // only receives the finished file plus its digest and expected cue window.
    let sync_source = media.path().join("sync-markers.mkv");
    let status = audio_live_command(&audio_originals, "fixture-sync-markers", Command::new(&ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=320x180:r=30:d=2,drawbox=x=0:y=0:w=iw:h=ih:color=white:t=fill:enable='between(t,1,1.034)'",
            "-f",
            "lavfi",
            "-i",
            r"aevalsrc=if(eq(n\,48000)\,1\,0):s=48000:d=2",
            "-shortest",
            "-c:v",
            "ffv1",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "pcm_s16le",
        ])
        .arg(&sync_source)
    )?.status;
    assert!(status.success(), "failed to create AV sync marker fixture");

    // One real Motion Canvas production through the existing DriverProvider.
    // Its manifest, Canvas receipt and PNGs remain original under retained media.
    let (motion_manifest_path, motion_manifest_digest) =
        motion_live_fixture(&audio_originals, &helper).await?;

    let caption_wav = media.path().join("caption-audio.wav");
    let status = audio_live_command(
        &audio_originals,
        "fixture-caption-audio",
        Command::new(&ffmpeg)
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "anullsrc=r=48000:cl=stereo",
                "-t",
                "0.2",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&caption_wav),
    )?
    .status;
    assert!(
        status.success(),
        "failed to produce bounded synthetic stereo WAV"
    );

    let manifest = Manifest {
        manifest_version: 1,
        protocol: DRIVER_PROTOCOL_VERSION,
        id: "mlt-video".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: Some("org.mltframework.melt".into()),
            process_names: vec!["melt".into()],
            supported_versions: vec![],
        },
        transport: Transport::StdioV1,
        mounts: vec![
            DriverMount {
                root: "project".into(),
                read_only: true,
                execute: false,
            },
            DriverMount {
                root: "media".into(),
                read_only: true,
                execute: false,
            },
            DriverMount {
                root: "output".into(),
                read_only: false,
                execute: false,
            },
            DriverMount {
                root: "runtime".into(),
                read_only: true,
                execute: false,
            },
            DriverMount {
                root: "mlt-runtime".into(),
                read_only: true,
                execute: true,
            },
            DriverMount {
                root: "scratch".into(),
                read_only: false,
                execute: false,
            },
        ],
        system_config: vec![],
        secrets: vec![],
        tools: vec![
            DriverToolMount {
                root: "mlt-runner-root".into(),
                name: "mlt-runner".into(),
                sha256: digest(&runtime_runner),
                mounts: vec![
                    "mlt-runtime".into(),
                    "scratch".into(),
                    "project".into(),
                    "media".into(),
                    "output".into(),
                ],
                system_config: vec![],
                dependencies: vec!["melt".into(), "ffprobe".into(), "ffmpeg".into()],
                nvidia_gpu: false,
                resources: None,
            },
            DriverToolMount {
                root: "melt-root".into(),
                name: "melt".into(),
                sha256: digest(&melt),
                mounts: vec![],
                system_config: vec![],
                dependencies: vec![],
                nvidia_gpu: false,
                resources: None,
            },
            DriverToolMount {
                root: "ffprobe-root".into(),
                name: "ffprobe".into(),
                sha256: digest(&ffprobe),
                mounts: vec![],
                system_config: vec![],
                dependencies: vec![],
                nvidia_gpu: false,
                resources: None,
            },
            DriverToolMount {
                root: "ffmpeg-root".into(),
                name: "ffmpeg".into(),
                sha256: digest(&ffmpeg),
                mounts: vec![],
                system_config: vec![],
                dependencies: vec![],
                nvidia_gpu: false,
                resources: None,
            },
        ],
        network: false,
        nvidia_gpu: false,
        loopback_port: None,
        // Match the launch-film production sandbox budget. These are ceilings,
        // not reservations: H.264/MLT can require materially more virtual address
        // space and worker headroom than the small lossless fixture.
        resources: DriverResources {
            address_space_bytes: 4_294_967_296,
            cpu_seconds: 300,
            operation_cpu_seconds: 0,
            file_size_bytes: 1_073_741_824,
            processes: 256,
            open_files: 512,
        },
        request_timeout_ms: 300_000,
        interfaces: DriverInterfaces {
            health: true,
            host_tools: true,
            ..DriverInterfaces::default()
        },
    };
    let grants = vec![
        FilesystemGrant {
            name: "project".into(),
            path: project.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "media".into(),
            path: media.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "output".into(),
            path: output.path().canonicalize().unwrap(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "runtime".into(),
            path: runtime.path().canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "mlt-runtime".into(),
            path: mlt_runtime_root,
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "scratch".into(),
            path: scratch.path().canonicalize().unwrap(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "mlt-runner-root".into(),
            path: runtime_runner.canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "melt-root".into(),
            path: melt.clone(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "ffprobe-root".into(),
            path: ffprobe.clone(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "ffmpeg-root".into(),
            path: ffmpeg.clone(),
            read: true,
            write: false,
        },
    ];

    audio_originals.before_effect()?;
    audio_originals.record(
        "audio-live-provider-connect-request.original.json",
        &json!({
            "schema":"semwright.mlt-live-provider-connect-request/1",
            "manifest":serde_json::to_value(&manifest)?,"state":state.path(),"helper":helper,
            "filesystem_grants":grants.iter().map(|grant| json!({
                "name":grant.name,"path":grant.path,"read":grant.read,"write":grant.write
            })).collect::<Vec<_>>(),"dry_run":false,"authority_issued_by_evidence_root":false
        }),
    )?;
    let negative_manifest = serde_json::to_value(&manifest)?;
    let connected = DriverProvider::connect(manifest, state.path(), &helper, &grants, false).await;
    let retained = (|| -> AudioLiveResult<()> {
        audio_originals.record(
            "audio-live-provider-connect.original.json",
            &json!({
                "schema":"semwright.audio-derive-live-provider-connect/1",
                "returned_ok":connected.is_ok(),
                "original_error":serde_json::to_value(connected.as_ref().err())?,
                "capture_scope":"same-original-connection-return-no-provider-serialization",
                "native_execution_retried":false
            }),
        )
    })();
    let provider = match connected {
        Err(original) => {
            if let Err(secondary) = retained {
                audio_originals.secondary(
                    "audio-live-connect-custody-error.original.json",
                    secondary.as_ref(),
                );
            }
            return Err(Box::new(original) as Box<dyn std::error::Error + Send + Sync>);
        }
        Ok(provider) => {
            if let Err(original) = retained {
                return audio_live_shutdown(&audio_originals, provider.as_ref(), Err(original))
                    .await;
            }
            provider
        }
    };
    let operations: AudioLiveResult<()> = async {
        let original_capabilities = Provider::capabilities(provider.as_ref()).await;
        let retained = (|| -> AudioLiveResult<()> {
            let value = match &original_capabilities {
                Ok(capabilities) => json!({"Ok":capabilities.iter().map(|capability| json!({
                "descriptor":capability.descriptor,"aliases":capability.aliases,
                "tags":capability.tags,"object_types":capability.object_types
            })).collect::<Vec<_>>()}),
                Err(error) => json!({"Err":serde_json::to_value(error)?}),
            };
            audio_originals.record_limited(
                "audio-live-capabilities-return.original.json",
                &json!({
                    "schema":"semwright.audio-derive-live-capabilities-return/2","result":value,
                    "capture_scope":"all-original-ProvidedCapability-fields-not-wire-bytes",
                    "native_execution_retried":false
                }),
                AUDIO_LIVE_CAPABILITIES_LIMIT,
            )
        })();
        if original_capabilities.is_err()
            && let Err(secondary) = &retained
        {
            audio_originals.secondary(
                "audio-live-capabilities-custody-error.original.json",
                secondary.as_ref(),
            );
        }
        let capabilities = audio_live_finish_native(original_capabilities, retained)?;
        audio_live_cases(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            &audio_sources,
            &audio_ffmpeg_sha256,
        )
        .await?;
        assert!(
            capabilities
                .iter()
                .all(|capability| capability.descriptor.name.starts_with("driver.mlt-video."))
        );
        for required in [
            "driver.mlt-video.frames.encode",
            "driver.mlt-video.sync.probe",
        ] {
            assert!(
                capabilities
                    .iter()
                    .any(|capability| capability.descriptor.name == required),
                "missing required delivery capability: {required}"
            );
        }

        let doctor = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.doctor",
            json!({}),
        )
        .await?;
        assert_eq!(
            doctor["capabilities"].as_u64().unwrap(),
            capabilities.len() as u64
        );
        assert_eq!(doctor["network"], false);
        assert_eq!(doctor["render_available"], true, "MLT doctor: {doctor}");
        assert!(
            doctor["mlt_version"]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );

        let mezzanine = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.frames.encode",
            json!({
                "root":"media",
                "manifest_path":motion_manifest_path,
                "expected_manifest_sha256":motion_manifest_digest,
                "output_path":"motion-mezzanine.mkv",
                "max_bytes":67108864
            }),
        )
        .await?;
        assert_eq!(mezzanine["codec"], "ffv1");
        assert_eq!(mezzanine["container"], "matroska");
        assert_eq!(mezzanine["frame_count"], 6);
        assert_eq!(mezzanine["width"], 160);
        assert_eq!(mezzanine["height"], 90);
        assert_eq!(mezzanine["media"]["video"], true);
        assert_eq!(mezzanine["media"]["audio"], false);
        assert!(
            output.path().join("motion-mezzanine.mkv").is_file(),
            "frames.encode did not publish its verified mezzanine"
        );
        negative_plans.push(AudioLiveNegativePlan {
            case: AUDIO_LIVE_NEGATIVES[0],
            args: json!({"root":"media","manifest_path":motion_manifest_path,
                "expected_manifest_sha256":"0".repeat(64),
                "output_path":"stale-mezzanine.mkv","max_bytes":67108864}),
            inputs: vec![("media", motion_manifest_path.clone())],
        });

        // The same original six source PNGs now flow through the *real* pinned
        // FFV1 driver + MLT H264/AAC mux, followed by the brand-new read-only
        // sealed Host observer. No fake observations, OCR or simulated frame
        // claims; the observer sees actual original owner-root files.
        let original_ffv1 = output.path().join("motion-mezzanine.mkv");
        let original_sha = digest(&original_ffv1);
        let caption_master = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.av.mux",
            json!({
                "video_root":"output",
                "video_path":"motion-mezzanine.mkv",
                "video_sha256":original_sha,
                "audio_root":"media",
                "audio_path":"caption-audio.wav",
                "audio_sha256":digest(&caption_wav),
                "width":160,"height":90,"fps_num":30,"fps_den":1,
                "frame_count":6,"sample_rate":48000,"channels":2,
                "profile":"h264-aac-mp4",
                "output_path":"caption-oracle-master.mp4",
                "max_bytes":67108864
            }),
        )
        .await?;
        assert_eq!(caption_master["profile"], "h264-aac-mp4");
        assert_eq!(caption_master["frame_count"], 6);
        let master_mp4 = output.path().join("caption-oracle-master.mp4");
        let original_master_sha = digest(&master_mp4);
        let owner_args = json!({
            "source_path":"motion-mezzanine.mkv",
            "source_sha256":original_sha,
            "source_bytes":std::fs::metadata(&original_ffv1).unwrap().len(),
            "master_path":"caption-oracle-master.mp4",
            "master_sha256":original_master_sha,
            "master_bytes":std::fs::metadata(&master_mp4).unwrap().len(),
            "width":160,"height":90,"fps_num":30,"fps_den":1,
            "frame_count":6,
            "cues":[{"start_frame":3,"end_frame_exclusive":4}]
        });
        let observed = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.av.caption_pixels.verify",
            owner_args.clone(),
        )
        .await?;
        assert_eq!(observed["status"], "PASS");
        assert_eq!(observed["frame_count"], 6);
        assert_eq!(observed["source_sha256"], original_sha);
        assert_eq!(observed["master_sha256"], original_master_sha);
        assert_eq!(observed["human_readability"], "NOT_RUN");
        assert_eq!(observed["speech_identity"], "NOT_VERIFIED");
        assert_eq!(observed["publication_authorized"], false);
        assert!(
            observed["sampled_frames"][0]["glyph_mask_pixels"]
                .as_u64()
                .unwrap()
                >= 50
        );
        assert!(!observed.to_string().contains("caption-audio.wav"));
        assert!(!observed.to_string().contains("caption-oracle-master.mp4"));
        let _source_len = std::fs::metadata(&original_ffv1).unwrap().len();
        for (case, key, value) in [
            (
                AUDIO_LIVE_NEGATIVES[1],
                "source_sha256",
                json!("0".repeat(64)),
            ),
            (
                AUDIO_LIVE_NEGATIVES[2],
                "source_path",
                json!("../private/master.mkv"),
            ),
            (
                AUDIO_LIVE_NEGATIVES[3],
                "cues",
                json!([{"start_frame":0,"end_frame_exclusive":1}]),
            ),
        ] {
            let mut args = owner_args.clone();
            args[key] = value;
            negative_plans.push(AudioLiveNegativePlan {
                case,
                args,
                inputs: vec![
                    ("output", "motion-mezzanine.mkv".into()),
                    ("output", "caption-oracle-master.mp4".into()),
                ],
            });
        }
        assert_eq!(digest(&master_mp4), original_master_sha);

        let sync_digest = digest(&sync_source);
        let sync = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.sync.probe",
            json!({
                "root":"media",
                "path":"sync-markers.mkv",
                "expected_sha256":sync_digest,
                "window_us":100000,
                "full_scan":true,
                "cues":[{"id":"technical","expected_us":1000000}]
            }),
        )
        .await?;
        assert_eq!(sync["artifact_sha256"], sync_digest);
        assert_eq!(sync["coverage"], "full_scan");
        assert_eq!(sync["exhaustive_video"], true);
        assert_eq!(sync["exhaustive_audio"], true);
        assert_eq!(sync["missing_video"], json!([]));
        assert_eq!(sync["missing_audio"], json!([]));
        let flash = &sync["flashes"][0];
        let impulse = &sync["impulses"][0];
        assert!(flash["confidence"].as_u64().unwrap() >= 9000, "{sync:#}");
        assert!(impulse["confidence"].as_u64().unwrap() >= 9000, "{sync:#}");
        let flash_us = flash["presentation_time_us"].as_u64().unwrap();
        let impulse_us = impulse["presentation_time_us"].as_u64().unwrap();
        assert!(flash_us.abs_diff(1_000_000) <= 35_000, "{sync:#}");
        assert!(impulse_us.abs_diff(1_000_000) <= 10_000, "{sync:#}");
        assert!(flash_us.abs_diff(impulse_us) <= 35_000, "{sync:#}");

        negative_plans.push(AudioLiveNegativePlan {
            case: AUDIO_LIVE_NEGATIVES[4],
            args: json!({"root":"media","path":"sync-markers.mkv",
                "expected_sha256":"0".repeat(64),"window_us":100000,
                "cues":[{"id":"technical","expected_us":1000000}]}),
            inputs: vec![("media", "sync-markers.mkv".into())],
        });

        let muxed = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.av.mux",
            json!({
                "video_root":"media",
                "video_path":"red.mkv",
                "video_sha256":digest(&media.path().join("red.mkv")),
                "audio_root":"media",
                "audio_path":"sine.wav",
                "audio_sha256":digest(&media.path().join("sine.wav")),
                "width":160,
                "height":90,
                "fps_num":25,
                "fps_den":1,
                "frame_count":50,
                "sample_rate":48000,
                "channels":2,
                "profile":"h264-aac-mp4",
                "output_path":"av-master.mp4",
                "max_bytes":67108864
            }),
        )
        .await?;
        assert_eq!(muxed["profile"], "h264-aac-mp4");
        assert_eq!(muxed["frame_count"], 50);
        assert_eq!(muxed["sample_rate"], 48000);
        assert_eq!(muxed["channels"], 2);
        assert_eq!(muxed["media"]["video"], true);
        assert_eq!(muxed["media"]["audio"], true);
        assert_eq!(muxed["artifact"]["root"], "output");
        assert_eq!(muxed["artifact"]["path"], "av-master.mp4");
        assert_eq!(
            muxed["artifact"]["sha256"],
            digest(&output.path().join("av-master.mp4"))
        );
        assert!(muxed["artifact"]["bytes"].as_u64().unwrap() > 0);
        assert_eq!(muxed["decoded_audio"]["root"], "output");
        assert_eq!(muxed["decoded_audio"]["path"], "av-master.decoded.wav");
        assert_eq!(
            muxed["decoded_audio"]["sha256"],
            digest(&output.path().join("av-master.decoded.wav"))
        );
        assert!(muxed["decoded_audio"]["bytes"].as_u64().unwrap() > 0);
        assert_eq!(muxed["decoded_audio_media"]["audio"], true);
        assert_eq!(muxed["decoded_audio_media"]["video"], false);
        assert!(
            muxed["decoded_audio_sample_frames"]
                .as_u64()
                .is_some_and(|frames| frames > 0)
        );

        negative_plans.push(AudioLiveNegativePlan {
            case: AUDIO_LIVE_NEGATIVES[5],
            args: json!({"video_root":"media","video_path":"red.mkv",
                "video_sha256":"0".repeat(64),"audio_root":"media","audio_path":"sine.wav",
                "audio_sha256":digest(&media.path().join("sine.wav")),
                "width":160,"height":90,"fps_num":25,"fps_den":1,"frame_count":50,
                "sample_rate":48000,"channels":2,"profile":"h264-aac-mp4",
                "output_path":"stale-av-master.mp4","max_bytes":67108864}),
            inputs: vec![("media", "red.mkv".into()), ("media", "sine.wav".into())],
        });

        let created_project = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.project.create",
            json!({
                "profile": {
                    "width": 160,
                    "height": 90,
                    "fps_num": 25,
                    "fps_den": 1,
                    "progressive": true,
                    "sample_aspect_num": 1,
                    "sample_aspect_den": 1,
                    "display_aspect_num": 16,
                    "display_aspect_den": 9,
                    "colorspace": 709,
                    "audio_channels": 2
                }
            }),
        )
        .await?;
        let mut project_ref = created_project["project"].as_str().unwrap().to_owned();
        let mut revision = created_project["revision"].as_str().unwrap().to_owned();

        let sequence = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.sequence.create",
            json!({"project":project_ref,"expected_revision":revision,"name":"Main"}),
        )
        .await?;
        let (_, initial_sequence_ref) = created(&sequence, "sequence");
        assert!(initial_sequence_ref.starts_with("video:"));
        project_ref = sequence["project"].as_str().unwrap().to_owned();
        revision = sequence["resulting_revision"].as_str().unwrap().to_owned();

        for (name, path) in [("Video", "red.mkv"), ("Audio", "sine.wav")] {
            let value = call(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                "driver.mlt-video.asset.import",
                json!({
                    "project":project_ref,
                    "expected_revision":revision,
                    "kind":"file",
                    "name":name,
                    "root":"media",
                    "path":path
                }),
            )
            .await?;
            project_ref = value["project"].as_str().unwrap().to_owned();
            revision = value["resulting_revision"].as_str().unwrap().to_owned();
        }

        for (name, kind) in [("Video Track", "video"), ("Audio Track", "audio")] {
            let current_sequence = sequence_ref(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                &project_ref,
            )
            .await?;
            let value = call(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                "driver.mlt-video.track.create",
                json!({
                    "project":project_ref,
                    "expected_revision":revision,
                    "sequence":current_sequence,
                    "name":name,
                    "kind":kind
                }),
            )
            .await?;
            project_ref = value["project"].as_str().unwrap().to_owned();
            revision = value["resulting_revision"].as_str().unwrap().to_owned();
        }

        let assets = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.asset.list",
            json!({"project":project_ref,"limit":100}),
        )
        .await?;
        let current_sequence = sequence_ref(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            &project_ref,
        )
        .await?;
        let tracks = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.track.list",
            json!({"project":project_ref,"sequence":current_sequence,"limit":100}),
        )
        .await?;
        let video_asset = page_ref(&assets, "Video");
        let video_track = page_ref(&tracks, "Video Track");

        let video_clip = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.clip.insert",
            json!({
                "project":project_ref,
                "expected_revision":revision,
                "sequence":current_sequence,
                "track":video_track,
                "asset":video_asset,
                "start":0,
                "source_in":0,
                "source_out":50,
                "name":"Video Clip"
            }),
        )
        .await?;
        project_ref = video_clip["project"].as_str().unwrap().to_owned();
        revision = video_clip["resulting_revision"]
            .as_str()
            .unwrap()
            .to_owned();

        let assets = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.asset.list",
            json!({"project":project_ref,"limit":100}),
        )
        .await?;
        let current_sequence = sequence_ref(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            &project_ref,
        )
        .await?;
        let tracks = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.track.list",
            json!({"project":project_ref,"sequence":current_sequence,"limit":100}),
        )
        .await?;
        let audio_asset = page_ref(&assets, "Audio");
        let audio_track = page_ref(&tracks, "Audio Track");

        let audio_clip = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.clip.insert",
            json!({
                "project":project_ref,
                "expected_revision":revision,
                "sequence":current_sequence,
                "track":audio_track,
                "asset":audio_asset,
                "start":0,
                "source_in":0,
                "source_out":50,
                "name":"Audio Clip"
            }),
        )
        .await?;
        project_ref = audio_clip["project"].as_str().unwrap().to_owned();
        revision = audio_clip["resulting_revision"]
            .as_str()
            .unwrap()
            .to_owned();

        let profiles = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.profiles",
            json!({}),
        )
        .await?;
        for id in ["lossless", "lossless-video-only", "h264-1080p"] {
            assert!(
                profiles["profiles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|profile| { profile["id"] == id && profile["available"] == true }),
                "missing live render profile {id}"
            );
        }

        let current_sequence = sequence_ref(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            &project_ref,
        )
        .await?;
        let plan = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.plan",
            json!({
                "project":project_ref,
                "sequence":current_sequence,
                "profile":"lossless",
                "output":"real-runtime.mkv"
            }),
        )
        .await?;
        assert_eq!(plan["runnable"], true);
        assert_eq!(plan["frames"], 50);

        let started = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.start",
            json!({
                "project":project_ref,
                "expected_revision":revision,
                "sequence":current_sequence,
                "profile":"lossless",
                "output":"real-runtime.mkv"
            }),
        )
        .await?;
        let job = started["job"].as_str().unwrap().to_owned();

        let terminal = loop {
            let status = call(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                "driver.mlt-video.render.status",
                json!({"job":job}),
            )
            .await?;
            match status["state"].as_str().unwrap() {
                "succeeded" | "failed" | "cancelled" | "unknown" => break status,
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        };
        assert_eq!(terminal["state"], "succeeded", "{terminal:#}");

        let result = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.result",
            json!({"job":job}),
        )
        .await?;
        assert_eq!(result["state"], "succeeded");
        assert_eq!(result["artifact"]["root"], "output");
        assert_eq!(result["artifact"]["path"], "real-runtime.mkv");
        assert!(result["artifact"]["bytes"].as_u64().unwrap() > 100);
        assert_eq!(result["media"]["video"], true);
        assert_eq!(result["media"]["audio"], true);
        assert_eq!(result["media"]["width"], 160);
        assert_eq!(result["media"]["height"], 90);

        let artifact = output.path().join("real-runtime.mkv");
        assert!(artifact.is_file());
        assert!(std::fs::metadata(&artifact).unwrap().len() > 100);

        // Exercise the new opt-in video-only *semantic* MLT render on exactly the
        // same audio-bearing editorial project. It must strip audio in the pinned
        // consumer rather than publish silence, and independently count every
        // real decoded FFV1 frame before the Broker releases the artifact.
        let video_only_sequence = sequence_ref(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            &project_ref,
        )
        .await?;
        let video_only_plan = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.plan",
            json!({
                "project":project_ref, "sequence":video_only_sequence,
                "profile":"lossless-video-only", "output":"video-only-runtime.mkv"
            }),
        )
        .await?;
        assert_eq!(video_only_plan["runnable"], true);
        assert_eq!(video_only_plan["frames"], 50);
        let video_only_started = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.start",
            json!({
                "project":project_ref, "expected_revision":revision,
                "sequence":video_only_sequence, "profile":"lossless-video-only",
                "output":"video-only-runtime.mkv"
            }),
        )
        .await?;
        let video_only_job = video_only_started["job"].as_str().unwrap();
        let video_only_terminal = loop {
            let status = call(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                "driver.mlt-video.render.status",
                json!({"job":video_only_job}),
            )
            .await?;
            match status["state"].as_str().unwrap() {
                "succeeded" | "failed" | "cancelled" | "unknown" => break status,
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        };
        assert_eq!(
            video_only_terminal["state"], "succeeded",
            "{video_only_terminal:#}"
        );
        let video_only_result = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.result",
            json!({"job":video_only_job}),
        )
        .await?;
        assert_eq!(video_only_result["profile"], "lossless-video-only");
        assert_eq!(video_only_result["state"], "succeeded");
        assert_eq!(video_only_result["project_revision"], revision);
        assert_eq!(
            video_only_result["artifact"]["path"],
            "video-only-runtime.mkv"
        );
        assert_eq!(video_only_result["media"]["video"], true);
        assert_eq!(video_only_result["media"]["audio"], false);
        assert_eq!(video_only_result["media"]["frames"], 50);
        assert_eq!(video_only_result["media"]["codecs"], json!(["ffv1"]));
        let video_only_artifact = output.path().join("video-only-runtime.mkv");
        assert!(video_only_artifact.is_file());
        assert!(std::fs::metadata(&video_only_artifact).unwrap().len() > 100);
        assert_eq!(
            video_only_result["artifact"]["sha256"],
            digest(&video_only_artifact)
        );

        // Keep an independently invoked *real* ffprobe check in this
        // owner-controlled CI test; a declared vendor receipt is not enough.
        let frame_probe = audio_live_command(
            &audio_originals,
            "oracle-video-only-ffprobe",
            Command::new(&ffprobe)
                .args([
                    "-v",
                    "error",
                    "-count_frames",
                    "-show_streams",
                    "-of",
                    "json",
                ])
                .arg(&video_only_artifact),
        )?;
        assert!(
            frame_probe.status.success(),
            "Independent FFprobe decode failed"
        );
        let checked_streams: Value = serde_json::from_slice(&frame_probe.stdout).unwrap();
        let checked_streams = checked_streams["streams"].as_array().unwrap();
        assert_eq!(checked_streams.len(), 1, "Audio must be absent, not silent");
        assert_eq!(checked_streams[0]["codec_type"], "video");
        assert_eq!(checked_streams[0]["codec_name"], "ffv1");
        assert_eq!(checked_streams[0]["nb_read_frames"], "50");

        // Exercise the same curated H.264 consumer used by the launch film in a
        // separate project born at 1920x1080/30. Reprofiling the earlier 160x90/25
        // lossless fixture after clips already exist changes the timebase underneath
        // those clips and does not represent the production launch-film route.
        let h264_project = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.project.create",
            json!({
                "profile": {
                    "width":1920,
                    "height":1080,
                    "fps_num":30,
                    "fps_den":1,
                    "progressive":true,
                    "sample_aspect_num":1,
                    "sample_aspect_den":1,
                    "display_aspect_num":16,
                    "display_aspect_den":9,
                    "colorspace":709,
                    "audio_channels":2
                }
            }),
        )
        .await?;
        project_ref = h264_project["project"].as_str().unwrap().to_owned();
        revision = h264_project["revision"].as_str().unwrap().to_owned();

        let h264_sequence = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.sequence.create",
            json!({"project":project_ref,"expected_revision":revision,"name":"H264 Main"}),
        )
        .await?;
        project_ref = h264_sequence["project"].as_str().unwrap().to_owned();
        revision = h264_sequence["resulting_revision"]
            .as_str()
            .unwrap()
            .to_owned();

        for (name, path) in [
            ("H264 Video", "h264-source.mp4"),
            ("H264 Audio", "sine.wav"),
        ] {
            let value = call(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                "driver.mlt-video.asset.import",
                json!({
                    "project":project_ref,
                    "expected_revision":revision,
                    "kind":"file",
                    "name":name,
                    "root":"media",
                    "path":path
                }),
            )
            .await?;
            project_ref = value["project"].as_str().unwrap().to_owned();
            revision = value["resulting_revision"].as_str().unwrap().to_owned();
        }

        for (name, kind) in [("H264 Video Track", "video"), ("H264 Audio Track", "audio")] {
            let sequence = sequence_ref(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                &project_ref,
            )
            .await?;
            let value = call(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                "driver.mlt-video.track.create",
                json!({
                    "project":project_ref,
                    "expected_revision":revision,
                    "sequence":sequence,
                    "name":name,
                    "kind":kind
                }),
            )
            .await?;
            project_ref = value["project"].as_str().unwrap().to_owned();
            revision = value["resulting_revision"].as_str().unwrap().to_owned();
        }

        for (track_name, asset_name, clip_name) in [
            ("H264 Video Track", "H264 Video", "H264 Video Clip"),
            ("H264 Audio Track", "H264 Audio", "H264 Audio Clip"),
        ] {
            let assets = call(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                "driver.mlt-video.asset.list",
                json!({"project":project_ref,"limit":100}),
            )
            .await?;
            let asset = page_ref(&assets, asset_name);
            let sequence = sequence_ref(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                &project_ref,
            )
            .await?;
            let tracks = call(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                "driver.mlt-video.track.list",
                json!({"project":project_ref,"sequence":sequence,"limit":100}),
            )
            .await?;
            let track = page_ref(&tracks, track_name);
            let value = call(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                "driver.mlt-video.clip.insert",
                json!({
                    "project":project_ref,
                    "expected_revision":revision,
                    "sequence":sequence,
                    "track":track,
                    "asset":asset,
                    "start":0,
                    "source_in":0,
                    "source_out":5,
                    "name":clip_name
                }),
            )
            .await?;
            project_ref = value["project"].as_str().unwrap().to_owned();
            revision = value["resulting_revision"].as_str().unwrap().to_owned();
        }

        let current_sequence = sequence_ref(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            &project_ref,
        )
        .await?;
        let h264_plan = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.plan",
            json!({
                "project":project_ref,
                "sequence":current_sequence,
                "profile":"h264-1080p",
                "output":"real-runtime-h264.mp4"
            }),
        )
        .await?;
        assert_eq!(h264_plan["runnable"], true);
        assert_eq!(h264_plan["frames"], 5);

        let h264_started = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.start",
            json!({
                "project":project_ref,
                "expected_revision":revision,
                "sequence":current_sequence,
                "profile":"h264-1080p",
                "output":"real-runtime-h264.mp4"
            }),
        )
        .await?;
        let h264_job = h264_started["job"].as_str().unwrap().to_owned();
        let h264_terminal = loop {
            let status = call(
                &audio_originals,
                provider.as_ref(),
                &capabilities,
                "driver.mlt-video.render.status",
                json!({"job":h264_job}),
            )
            .await?;
            match status["state"].as_str().unwrap() {
                "succeeded" | "failed" | "cancelled" | "unknown" => break status,
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        };
        assert_eq!(
            h264_terminal["state"], "succeeded",
            "H.264 live render: {h264_terminal:#}"
        );
        let h264_result = call(
            &audio_originals,
            provider.as_ref(),
            &capabilities,
            "driver.mlt-video.render.result",
            json!({"job":h264_job}),
        )
        .await?;
        assert_eq!(h264_result["state"], "succeeded");
        assert_eq!(h264_result["media"]["video"], true);
        assert_eq!(h264_result["media"]["audio"], true);
        assert_eq!(h264_result["media"]["width"], 1920);
        assert_eq!(h264_result["media"]["height"], 1080);
        assert_eq!(h264_result["media"]["frames"], 5);
        let h264_artifact = output.path().join("real-runtime-h264.mp4");
        assert!(h264_artifact.is_file());
        assert!(std::fs::metadata(&h264_artifact).unwrap().len() > 100);

        Ok(())
    }
    .await;

    let body_retained = audio_originals.record(
        "audio-live-body-return.original.json",
        &json!({
            "schema":"semwright.mlt-live-body-return/1","returned_ok":operations.is_ok(),
            "original_error":operations.as_ref().err().map(ToString::to_string),
            "provider_call_records":audio_originals.calls.load(std::sync::atomic::Ordering::SeqCst),
            "external_custody_admitted":false,"external_deadline_attested":false,
            "external_filesystem_budget_attested":false,"descendant_closure_attested":false,
            "formal_acceptance":false,"native_execution_retried":false
        }),
    );
    let operations = match operations {
        Err(original) => {
            if let Err(secondary) = body_retained {
                audio_originals.secondary(
                    "audio-live-body-custody-error.original.json",
                    secondary.as_ref(),
                );
            }
            Err(original)
        }
        Ok(()) => body_retained,
    };
    audio_live_shutdown(&audio_originals, provider.as_ref(), operations).await?;
    audio_live_terminal_negatives(
        &audio_originals,
        &negative_originals,
        &negative_plans,
        &negative_manifest,
        &grants,
        &helper,
    )
    .await
}

#[cfg(test)]
mod audio_live_source_tests {
    use super::*;

    fn owner_root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        root
    }

    #[test]
    fn motion_live_source_compiles_the_original_geometry_and_six_frame_clock() {
        use semwright_driver_motion_canvas::{compiler, validate};
        let project = motion_live_project().unwrap();
        let plan = validate::render_plan(&project, &motion_live_profile()).unwrap();
        assert_eq!(
            (plan.width, plan.height, plan.fps, plan.fps_denominator),
            (160, 90, 30, 1)
        );
        assert_eq!(
            (plan.first_frame, plan.end_frame_exclusive, plan.frame_count),
            (0, 6, 6)
        );
        assert!(!plan.alpha);
        assert_eq!(project.settings.background.as_deref(), Some("#0000ff"));
        let scene = &project.scenes[0];
        assert_eq!(
            validate::animation_times(scene, &scene.animations[0]).unwrap(),
            (80, 81)
        );
        assert_eq!(
            validate::animation_times(scene, &scene.animations[1]).unwrap(),
            (120, 121)
        );
        // Check both the core integer instant and render.mjs's interior frame
        // representative, before observing any renderer output.
        for frame in 0..6 {
            for seconds in [
                frame as f64 / 30.0,
                if frame == 0 {
                    0.0
                } else {
                    (frame as f64 - 0.25) / 30.0
                },
            ] {
                assert_eq!((0.081..0.120).contains(&seconds), frame == 3);
            }
        }
        let generated = compiler::compile(&project).unwrap();
        let source =
            std::str::from_utf8(&generated.files["src/scenes/mlt-caption-source.tsx"]).unwrap();
        for fragment in [
            "<Rect ref={n0}",
            "position={[-42.5,23.0]}",
            "width={35.0}",
            "height={20.0}",
            "fill={\"#ffff00\"}",
            "radius={0.0}",
            "lineWidth={0.0}",
            "opacity={0.0}",
            "delay(0.080",
            "delay(0.120",
            "n0().opacity(1.0,0.001,linear)",
            "n0().opacity(0.0,0.001,linear)",
        ] {
            assert!(
                source.contains(fragment),
                "missing compiled fixture fragment: {fragment}"
            );
        }
        let exporter = std::str::from_utf8(&generated.files["src/semwright-exporter.ts"]).unwrap();
        assert!(exporter.contains("getContextAttributes"));
        assert!(exporter.contains("canvas_color:observeCanvasColor(canvas)"));
    }

    #[test]
    fn motion_live_legacy_manifest_cannot_substitute_for_original_canvas_receipt() {
        // The original failing fixture declared the correct renderer/plan but
        // supplied no Canvas producer receipt. This is a source-only rejection;
        // no synthetic JSON here is treated as an admitted Native result.
        let root = owner_root();
        let originals = AudioLiveOriginals::open_with_exclusions(
            root.path().to_path_buf(),
            &[PathBuf::from("/never-a-checkout")],
        )
        .unwrap();
        let directory = "render-1197a71a33c1149ba1f86c1c11e0e3ef";
        std::fs::create_dir(originals.media_path.path().join(directory)).unwrap();
        let manifest = json!({
            "renderer":"motion-canvas-core-renderer-v3.17.2",
            "plan":semwright_driver_motion_canvas::validate::render_plan(&motion_live_project().unwrap(), &motion_live_profile()).unwrap(),
            "pixel_validation":{"mode":"all","sample_indices":[0,1,2,3,4,5]},
            "frames":[]
        });
        let bytes = serde_json::to_vec(&manifest).unwrap();
        let path = format!("{directory}/artifact-manifest.json");
        originals.media.write_new("media", &path, &bytes).unwrap();
        let terminal = json!({"state":"succeeded","artifact":{
            "directory":directory,"manifest":path,"frame_count":6,
            "first_png":format!("{directory}/frames/000000.png"),
            "last_png":format!("{directory}/frames/000005.png"),
            "manifest_sha256":semwright_mlt_video::hash::sha256(&bytes),"manifest_bytes":bytes.len()
        }});
        let error = motion_live_original_artifact(&originals, &terminal).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Motion fixture lacks its original Canvas receipt"
        );
        assert_eq!(originals.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(
            !root
                .path()
                .join("records/motion-live-artifact-observation.original.json")
                .exists()
        );
        assert_eq!(
            originals
                .media
                .read(&path, AUDIO_LIVE_METADATA_LIMIT)
                .unwrap(),
            bytes
        );
    }

    #[test]
    fn audio_live_sources_have_the_original_pcm_clocks_before_any_provider() {
        let mut digests = std::collections::BTreeSet::new();
        for case in AUDIO_LIVE_CASES {
            let bytes = audio_live_fixture(case).unwrap();
            let observed = audio_live_pcm_observation(case, &bytes, true).unwrap();
            assert_eq!(observed["sample_frames"], case.source_frames);
            assert_eq!(observed["sample_rate"], case.source_rate);
            assert_eq!(observed["all_payload_frames_read"], true);
            assert!(observed["nonzero_samples"].as_u64().unwrap() > 0);
            assert!(digests.insert(semwright_mlt_video::hash::sha256(&bytes)));
        }
        assert_eq!(digests.len(), 3);
    }

    #[test]
    fn audio_live_oracles_reject_silenced_control_and_unequal_dual_mono() {
        // These are parser/oracle controls made from RIFF fixture bytes. They
        // do not execute or substitute for an observed native derivative.
        let case = AudioLiveCase {
            source_frames: 16_000,
            target_frames: 48_000,
            ..AUDIO_LIVE_CASES[2]
        };
        let target_shape = AudioLiveCase {
            source_rate: case.target_rate,
            source_channels: case.target_channels,
            source_frames: case.target_frames,
            ..case
        };
        let bytes = audio_live_fixture(target_shape).unwrap();
        audio_live_pcm_observation(case, &bytes, false).unwrap();
        let mut silent = bytes.clone();
        silent[44..].fill(0);
        assert!(audio_live_pcm_observation(case, &silent, false).is_err());
        let mut unequal = bytes;
        let second_channel = 44 + (case.target_frames as usize / 2) * 4 + 2;
        let sample = i16::from_le_bytes(
            unequal[second_channel..second_channel + 2]
                .try_into()
                .unwrap(),
        );
        unequal[second_channel..second_channel + 2].copy_from_slice(&(sample + 1).to_le_bytes());
        assert!(audio_live_pcm_observation(case, &unequal, false).is_err());

        let antiphase = AudioLiveCase {
            source_frames: 48_000,
            target_frames: 16_000,
            ..AUDIO_LIVE_CASES[0]
        };
        let target_shape = AudioLiveCase {
            source_rate: antiphase.target_rate,
            source_channels: antiphase.target_channels,
            source_frames: antiphase.target_frames,
            antiphase: false,
            ..antiphase
        };
        let mut cancellation = audio_live_fixture(target_shape).unwrap();
        cancellation[44..].fill(0);
        audio_live_pcm_observation(antiphase, &cancellation, false).unwrap();
        cancellation[44..46].copy_from_slice(&1i16.to_le_bytes());
        assert!(audio_live_pcm_observation(antiphase, &cancellation, false).is_err());
    }

    #[test]
    fn audio_live_root_requires_fresh_private_owner_paths_outside_the_checkout() {
        assert!(AudioLiveOriginals::open(None).is_err());
        let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let error = AudioLiveOriginals::open_with_exclusions(
            checkout.clone(),
            std::slice::from_ref(&checkout),
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("outside checkouts and caches"));
        let root = owner_root();
        let path = root.path().canonicalize().unwrap();
        let originals =
            AudioLiveOriginals::open_with_exclusions(path.clone(), std::slice::from_ref(&checkout))
                .unwrap();
        let start = originals
            .records
            .read("audio-live-start.original.json", AUDIO_LIVE_METADATA_LIMIT)
            .unwrap();
        assert!(
            AudioLiveOriginals::open_with_exclusions(path.clone(), std::slice::from_ref(&checkout))
                .is_err()
        );
        assert_eq!(
            originals
                .records
                .read("audio-live-start.original.json", AUDIO_LIVE_METADATA_LIMIT)
                .unwrap(),
            start
        );
        let alias_owner = tempfile::tempdir().unwrap();
        let alias = alias_owner.path().join("alias");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(
            AudioLiveOriginals::open_with_exclusions(alias, std::slice::from_ref(&checkout))
                .is_err()
        );
        let wide = owner_root();
        std::fs::set_permissions(wide.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        let error = AudioLiveOriginals::open_with_exclusions(
            wide.path().canonicalize().unwrap(),
            &[checkout],
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("private canonical owner"));
    }

    #[test]
    fn audio_live_custody_failure_preserves_the_same_complete_unknown_error() {
        let mut original = semwright_types::Error::new(
            semwright_types::ErrorCode::BackendFailed,
            "original interrupted call",
        )
        .uncertain()
        .recipe_progress(2, "audio-derive");
        original.candidates = vec!["original-bounded-candidate".into()];
        let before = serde_json::to_value(&original).unwrap();
        let message = original.message.as_ptr();
        let custody: AudioLiveResult<()> =
            Err(
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "custody refusal").into(),
            );
        let returned = audio_live_finish_native::<()>(Err(original), custody).unwrap_err();
        let returned = returned.downcast::<semwright_types::Error>().unwrap();
        assert_eq!(returned.message.as_ptr(), message);
        assert_eq!(serde_json::to_value(&returned).unwrap(), before);
        assert!(!returned.outcome_known);
        let custody: AudioLiveResult<()> =
            Err(
                std::io::Error::new(std::io::ErrorKind::PermissionDenied, "custody refusal").into(),
            );
        let failed = audio_live_finish_native(Ok(42), custody).unwrap_err();
        assert_eq!(
            failed.downcast_ref::<std::io::Error>().unwrap().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
    fn open_owner(root: &tempfile::TempDir) -> AudioLiveOriginals {
        let excluded = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        AudioLiveOriginals::open_with_exclusions(root.path().canonicalize().unwrap(), &[excluded])
            .unwrap()
    }

    fn context(id: &str) -> Context {
        Context {
            session: "mlt-video-live".into(),
            request_id: id.into(),
            cancellation: CancellationToken::new(),
        }
    }

    #[test]
    fn audio_live_missing_selector_and_nonempty_root_fail_before_setup() {
        let owner = owner_root();
        assert!(audio_live_selector(None).is_err());
        assert!(audio_live_selector(Some("0")).is_err());
        assert!(std::fs::read_dir(owner.path()).unwrap().next().is_none());
        audio_live_selector(Some("1")).unwrap();
        std::fs::write(owner.path().join("original"), b"keep").unwrap();
        let error = AudioLiveOriginals::open_with_exclusions(
            owner.path().canonicalize().unwrap(),
            &[PathBuf::from("/closed-unrelated-checkout")],
        )
        .err()
        .unwrap();
        assert!(error.to_string().contains("initially be empty"));
        assert_eq!(
            std::fs::read(owner.path().join("original")).unwrap(),
            b"keep"
        );
        assert!(!owner.path().join("records").exists());
    }

    #[test]
    fn audio_live_retains_eight_subroots_and_detects_substitution() {
        let owner = owner_root();
        let originals = open_owner(&owner);
        let named = owner.path().canonicalize().unwrap();
        for name in AUDIO_LIVE_ROOTS {
            assert!(named.join(name).is_dir());
            assert_eq!(originals.directory(name).unwrap().path(), named.join(name));
        }
        let source = originals
            .records
            .read("audio-live-start.original.json", AUDIO_LIVE_METADATA_LIMIT)
            .unwrap();
        let displaced = owner.path().join("displaced-output");
        std::fs::rename(owner.path().join("output"), &displaced).unwrap();
        std::fs::create_dir(owner.path().join("output")).unwrap();
        std::fs::set_permissions(
            owner.path().join("output"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        assert!(originals.before_effect().is_err());
        drop(originals);
        assert!(displaced.is_dir());
        assert!(named.join("binaries").is_dir());
        assert!(named.join("state").is_dir());
        assert_eq!(
            std::fs::read(named.join("records/audio-live-start.original.json")).unwrap(),
            source
        );
    }

    #[test]
    fn audio_live_request_and_complete_error_are_originals_before_interpretation() {
        let owner = owner_root();
        let originals = open_owner(&owner);
        let context = context("actual-source-test-request");
        let descriptor = json!({"name":AUDIO_LIVE_COMMAND,"source_control_only":true});
        let args = json!({"closed":"synthetic-unit-Value"});
        let stem = originals
            .begin_call(&context, AUDIO_LIVE_COMMAND, &descriptor, &args)
            .unwrap();
        let request: Value = serde_json::from_slice(
            &originals
                .records
                .read(
                    &format!("{stem}.request.original.json"),
                    AUDIO_LIVE_METADATA_LIMIT,
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(request["context"]["request_id"], context.request_id);
        assert_eq!(request["context"]["session"], context.session);
        assert_eq!(request["descriptor"], descriptor);
        assert_eq!(request["arguments"], args);
        let mut original = semwright_types::Error::new(
            semwright_types::ErrorCode::BackendFailed,
            "original unknown native return",
        )
        .uncertain()
        .recipe_progress(2, "audio-derive");
        original.candidates = vec!["original-candidate".into()];
        let expected = serde_json::to_value(&original).unwrap();
        let pointer = original.message.as_ptr();
        let returned = originals.finish_call(&stem, Err(original)).unwrap_err();
        assert_eq!(returned.message.as_ptr(), pointer);
        let result: Value = serde_json::from_slice(
            &originals
                .records
                .read(
                    &format!("{stem}.result.original.json"),
                    AUDIO_LIVE_METADATA_LIMIT,
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(result["result"]["Err"], expected);
        assert_eq!(serde_json::to_value(&returned).unwrap(), expected);
        assert!(originals.before_effect().is_err());
        assert!(
            originals
                .begin_call(&context, AUDIO_LIVE_COMMAND, &descriptor, &args)
                .is_err()
        );
        assert!(
            !owner
                .path()
                .join("records/call-00002.request.original.json")
                .exists()
        );
    }

    #[test]
    fn audio_live_result_collision_retains_prefix_and_never_replaces_unknown() {
        use std::io::Write;
        let owner = owner_root();
        let originals = open_owner(&owner);
        let context = context("retained-error");
        let stem = originals
            .begin_call(&context, AUDIO_LIVE_COMMAND, &json!({}), &json!({}))
            .unwrap();
        let path = format!("{stem}.result.original.json");
        let mut retained = originals.new_record_file(&path).unwrap();
        retained.write_all(b"original partial prefix").unwrap();
        retained.sync_all().unwrap();
        let original = semwright_types::Error::new(
            semwright_types::ErrorCode::BackendFailed,
            "native wait interrupted",
        )
        .uncertain();
        let expected = serde_json::to_value(&original).unwrap();
        let pointer = original.message.as_ptr();
        let returned = originals.finish_call(&stem, Err(original)).unwrap_err();
        assert_eq!(returned.message.as_ptr(), pointer);
        assert_eq!(serde_json::to_value(&returned).unwrap(), expected);
        assert_eq!(
            originals
                .records
                .read(&path, AUDIO_LIVE_METADATA_LIMIT)
                .unwrap(),
            b"original partial prefix"
        );
        assert!(originals.before_effect().is_err());
        let owner2 = owner_root();
        let originals2 = open_owner(&owner2);
        let stem2 = originals2
            .begin_call(&context, AUDIO_LIVE_COMMAND, &json!({}), &json!({}))
            .unwrap();
        originals2
            .record(
                &format!("{stem2}.result.original.json"),
                &json!({"kept":"first"}),
            )
            .unwrap();
        let failed = originals2
            .finish_call(&stem2, Ok(json!({"native_effect":"returned"})))
            .unwrap_err();
        assert!(!failed.outcome_known);
        assert!(originals2.before_effect().is_err());
    }

    #[test]
    fn audio_live_expected_rejection_cannot_consume_unknown_as_a_known_negative() {
        let original = semwright_types::Error::new(
            semwright_types::ErrorCode::BackendFailed,
            "unknown before a negative assertion",
        )
        .uncertain();
        let expected = serde_json::to_value(&original).unwrap();
        let pointer = original.message.as_ptr();
        let error = audio_live_expect_known_error(Err(original)).unwrap_err();
        let error = error.downcast::<semwright_types::Error>().unwrap();
        assert_eq!(error.message.as_ptr(), pointer);
        assert_eq!(serde_json::to_value(error).unwrap(), expected);
        let known = semwright_types::Error::new(
            semwright_types::ErrorCode::StaleReference,
            "known stale input",
        );
        assert!(
            audio_live_expect_known_error(Err(known))
                .unwrap()
                .outcome_known
        );
        assert!(audio_live_expect_known_error(Ok(json!({"unexpected":"success"}))).is_err());
    }

    fn terminal_plan(case: AudioLiveNegativeCase) -> AudioLiveNegativePlan {
        AudioLiveNegativePlan {
            case,
            args: json!({"source_control_only":true}),
            inputs: vec![],
        }
    }

    fn terminal_scope(
        root: &tempfile::TempDir,
        positive: &AudioLiveOriginals,
        plan: &AudioLiveNegativePlan,
    ) -> AudioLiveOriginals {
        let scope = AudioLiveOriginals::open_scope(
            root.path().canonicalize().unwrap(),
            &positive.excluded_locations,
            Some((plan.case.id, positive)),
        )
        .unwrap();
        scope
            .record(
                "negative-plan.original.json",
                &json!({"case":plan.case.id,
            "capability":plan.case.capability,"arguments":plan.args,
            "expected_code":plan.case.expected,"expected_outcome_known":false}),
            )
            .unwrap();
        scope
    }

    fn terminal_request(
        scope: &AudioLiveOriginals,
        plan: &AudioLiveNegativePlan,
    ) -> (usize, String) {
        let ctx = Context {
            session: format!("mlt-video-live-negative-{}", plan.case.id),
            request_id: "source-only-terminal-control".into(),
            cancellation: CancellationToken::new(),
        };
        let stem = scope
            .begin_call(
                &ctx,
                plan.case.capability,
                &json!({"name":plan.case.capability}),
                &plan.args,
            )
            .unwrap();
        (scope.calls.load(std::sync::atomic::Ordering::SeqCst), stem)
    }

    #[test]
    fn audio_live_terminal_negative_preserves_unknown_and_fences_only_its_declared_scope() {
        // These exercise the actual capture/evaluation/fence implementation with
        // a source-only Error; they do not stand in for the ignored Native live.
        let owner = owner_root();
        let first = owner_root();
        let second = owner_root();
        let positive = open_owner(&owner);
        let plan = terminal_plan(AUDIO_LIVE_NEGATIVES[0]);
        let next_plan = terminal_plan(AUDIO_LIVE_NEGATIVES[1]);
        let scope = terminal_scope(&first, &positive, &plan);
        let next = terminal_scope(&second, &positive, &next_plan);
        let (index, stem) = terminal_request(&scope, &plan);
        let mut original = semwright_types::Error::new(plan.case.expected, "complete original")
            .uncertain()
            .recipe_progress(2, "source-only-negative-control");
        original.candidates = vec!["original-candidate".into()];
        let expected = serde_json::to_value(&original).unwrap();
        let pointer = original.message.as_ptr();
        let returned = scope.finish_call(&stem, Err(original)).unwrap_err();
        assert_eq!(returned.message.as_ptr(), pointer);
        assert_eq!(serde_json::to_value(&returned).unwrap(), expected);
        let observed = audio_live_observe_negative(&positive, &scope, &plan, &[]);
        audio_live_evaluate_terminal_negative(&scope, &plan, index, Err(returned), observed)
            .unwrap();
        let retained: Value = serde_json::from_slice(
            &scope
                .records
                .read(
                    &format!("{stem}.result.original.json"),
                    AUDIO_LIVE_METADATA_LIMIT,
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(retained["result"]["Err"], expected);
        assert!(
            scope
                .begin_call(
                    &context("second-call"),
                    plan.case.capability,
                    &json!({}),
                    &plan.args
                )
                .is_err()
        );
        assert_eq!(
            positive.calls.load(std::sync::atomic::Ordering::SeqCst),
            index
        );
        assert!(
            !first
                .path()
                .join(format!(
                    "records/call-{:05}.request.original.json",
                    index + 1
                ))
                .exists()
        );
        assert!(next.before_effect().is_ok());
        let (next_index, _) = terminal_request(&next, &next_plan);
        assert_eq!(next_index, index + 1);
        assert!(std::sync::Arc::ptr_eq(&positive.calls, &next.calls));
        assert!(std::sync::Arc::ptr_eq(
            &scope.record_bytes,
            &next.record_bytes
        ));
    }

    #[test]
    fn audio_live_terminal_negative_rejects_ok_other_code_and_known_certainty() {
        for original in [
            Ok(json!({"unexpected":"success"})),
            Err(semwright_types::Error::new(
                semwright_types::ErrorCode::BackendFailed,
                "different code",
            )
            .uncertain()),
            Err(semwright_types::Error::new(
                semwright_types::ErrorCode::StaleReference,
                "unexpected known",
            )),
        ] {
            let owner = owner_root();
            let negative = owner_root();
            let positive = open_owner(&owner);
            let plan = terminal_plan(AUDIO_LIVE_NEGATIVES[0]);
            let scope = terminal_scope(&negative, &positive, &plan);
            let expected = serde_json::to_value(&original).unwrap();
            let (index, stem) = terminal_request(&scope, &plan);
            let returned = scope.finish_call(&stem, original);
            let observed = audio_live_observe_negative(&positive, &scope, &plan, &[]);
            assert!(
                audio_live_evaluate_terminal_negative(&scope, &plan, index, returned, observed)
                    .is_err()
            );
            assert!(scope.before_effect().is_err());
            let retained: Value = serde_json::from_slice(
                &scope
                    .records
                    .read(
                        &format!("{stem}.result.original.json"),
                        AUDIO_LIVE_METADATA_LIMIT,
                    )
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(retained["result"], expected);
            let assessment: Value = serde_json::from_slice(
                &scope
                    .records
                    .read("negative-evaluation.json", AUDIO_LIVE_METADATA_LIMIT)
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(assessment["assessment"], "FAILED");
        }
    }

    #[test]
    fn audio_live_terminal_collision_preserves_primary_and_stops_all_later_effects() {
        use std::io::Write;
        let owner = owner_root();
        let negative = owner_root();
        let other = owner_root();
        let positive = open_owner(&owner);
        let plan = terminal_plan(AUDIO_LIVE_NEGATIVES[0]);
        let scope = terminal_scope(&negative, &positive, &plan);
        let next = terminal_scope(&other, &positive, &terminal_plan(AUDIO_LIVE_NEGATIVES[1]));
        let (index, stem) = terminal_request(&scope, &plan);
        let result_path = format!("{stem}.result.original.json");
        let mut file = scope.new_record_file(&result_path).unwrap();
        file.write_all(b"original partial prefix").unwrap();
        file.sync_all().unwrap();
        let original =
            semwright_types::Error::new(plan.case.expected, "primary unknown").uncertain();
        let pointer = original.message.as_ptr();
        let expected = serde_json::to_value(&original).unwrap();
        let returned = scope.finish_call(&stem, Err(original));
        let observed = audio_live_observe_negative(&positive, &scope, &plan, &[]);
        let error = audio_live_evaluate_terminal_negative(&scope, &plan, index, returned, observed)
            .unwrap_err()
            .downcast::<semwright_types::Error>()
            .unwrap();
        assert_eq!(error.message.as_ptr(), pointer);
        assert_eq!(serde_json::to_value(&error).unwrap(), expected);
        assert_eq!(
            scope
                .records
                .read(&result_path, AUDIO_LIVE_METADATA_LIMIT)
                .unwrap(),
            b"original partial prefix"
        );
        assert!(next.before_effect().is_err());
        assert!(
            next.begin_call(
                &context("later"),
                plan.case.capability,
                &json!({}),
                &json!({})
            )
            .is_err()
        );
        assert_eq!(
            positive.calls.load(std::sync::atomic::Ordering::SeqCst),
            index
        );
    }

    #[test]
    fn audio_live_terminal_scopes_share_remaining_budgets_without_refunds() {
        use std::sync::atomic::Ordering::SeqCst;
        let owner = owner_root();
        let first = owner_root();
        let second = owner_root();
        let positive = open_owner(&owner);
        let plan = terminal_plan(AUDIO_LIVE_NEGATIVES[0]);
        let scope = terminal_scope(&first, &positive, &plan);
        let next = terminal_scope(&second, &positive, &terminal_plan(AUDIO_LIVE_NEGATIVES[1]));
        positive.calls.store(AUDIO_LIVE_CALL_LIMIT - 1, SeqCst);
        let (index, _) = terminal_request(&scope, &plan);
        assert_eq!(index, AUDIO_LIVE_CALL_LIMIT);
        assert!(
            next.begin_call(
                &context("over-limit"),
                plan.case.capability,
                &json!({}),
                &json!({})
            )
            .is_err()
        );
        assert_eq!(positive.calls.load(SeqCst), AUDIO_LIVE_CALL_LIMIT);
        assert!(
            !second
                .path()
                .join("records/call-16385.request.original.json")
                .exists()
        );

        let owner = owner_root();
        let first = owner_root();
        let second = owner_root();
        let positive = open_owner(&owner);
        let scope = terminal_scope(&first, &positive, &plan);
        let next = terminal_scope(&second, &positive, &terminal_plan(AUDIO_LIVE_NEGATIVES[1]));
        positive
            .record_bytes
            .store(AUDIO_LIVE_RECORDS_LIMIT - 3, SeqCst);
        scope
            .record_bytes("last-two.original.bin", b"ab", 2)
            .unwrap();
        assert_eq!(next.record_bytes.load(SeqCst), AUDIO_LIVE_RECORDS_LIMIT - 1);
        assert!(
            next.record_bytes("over-limit.original.bin", b"cd", 2)
                .is_err()
        );
        assert_eq!(
            positive.record_bytes.load(SeqCst),
            AUDIO_LIVE_RECORDS_LIMIT - 1
        );
        assert!(
            !second
                .path()
                .join("records/over-limit.original.bin")
                .exists()
        );
        assert!(scope.before_effect().is_err());
        assert_eq!(
            scope.records.read("last-two.original.bin", 2).unwrap(),
            b"ab"
        );
    }

    #[test]
    fn audio_live_terminal_observer_keeps_sources_and_rejects_published_negative_outputs() {
        let owner = owner_root();
        let negative = owner_root();
        let positive = open_owner(&owner);
        let plan = terminal_plan(AUDIO_LIVE_NEGATIVES[0]);
        let scope = terminal_scope(&negative, &positive, &plan);
        let input = AudioLiveNegativeInput {
            root: "media",
            path: "source.json".into(),
            bytes: b"synthetic original manifest".to_vec(),
            sha256: semwright_mlt_video::hash::sha256(b"synthetic original manifest"),
        };
        for originals in [&positive, &scope] {
            originals
                .media
                .write_new("media", &input.path, &input.bytes)
                .unwrap();
        }
        assert!(
            audio_live_observe_negative(&positive, &scope, &plan, std::slice::from_ref(&input))
                .is_ok()
        );
        scope
            .output
            .write_new("output", "stale-mezzanine.mkv", b"unexpected output")
            .unwrap();
        assert!(
            audio_live_observe_negative(&positive, &scope, &plan, std::slice::from_ref(&input))
                .is_err()
        );
        assert_eq!(positive.media.read(&input.path, 100).unwrap(), input.bytes);
    }

    #[test]
    fn audio_live_tool_file_prefix_never_claims_eof_or_truncates_original() {
        use std::io::Write;
        let owner = owner_root();
        let originals = open_owner(&owner);
        let name = "fixture-h264.stdout.original.bin";
        let mut raw = originals.new_record_file(name).unwrap();
        raw.write_all(b"original\0binary\xff").unwrap();
        raw.sync_all().unwrap();
        let (bytes, observation, error) = audio_live_tool_stream(&originals, name, &raw);
        assert!(error.is_none());
        assert_eq!(bytes.unwrap(), b"original\0binary\xff");
        assert_eq!(observation["state"], "FILE_SNAPSHOT_OBSERVED");
        assert_eq!(observation["stream_eof_attested"], false);
        assert_eq!(observation["descendant_closure_attested"], false);
        assert!(observation["hard_capture_limit"].is_null());
        raw.write_all(&vec![b'x'; AUDIO_LIVE_TOOL_PREFIX_LIMIT + 1])
            .unwrap();
        raw.sync_all().unwrap();
        let actual_len = raw.metadata().unwrap().len();
        let (bytes, observation, error) = audio_live_tool_stream(&originals, name, &raw);
        assert!(error.is_some());
        assert_eq!(bytes.unwrap().len(), AUDIO_LIVE_TOOL_PREFIX_LIMIT);
        assert_eq!(observation["over_inspection_limit"], true);
        assert_eq!(raw.metadata().unwrap().len(), actual_len);
        assert_eq!(observation["stream_eof_attested"], false);
        let (missing, observation, error) =
            audio_live_tool_stream(&originals, "fixture-h264.stderr.original.bin", &raw);
        assert!(missing.is_none());
        assert!(error.is_some());
        assert!(observation["bytes"].is_null());
    }

    #[test]
    fn audio_live_candidate_original_is_observed_even_when_no_success_value_exists() {
        let owner = owner_root();
        let originals = open_owner(&owner);
        let case = AUDIO_LIVE_CASES[0];
        let candidate = format!("{}.audio-derive.output.original.wav", case.output_name());
        originals
            .output
            .write_new("output", &candidate, b"partial WAV original")
            .unwrap();
        audio_live_observe_case_files(&originals, case).unwrap();
        let record: Value = serde_json::from_slice(
            &originals
                .records
                .read(
                    &format!("audio-live-{}.backend-files.original.json", case.id),
                    AUDIO_LIVE_METADATA_LIMIT,
                )
                .unwrap(),
        )
        .unwrap();
        let rows = record["files"].as_array().unwrap();
        assert_eq!(rows.len(), 16);
        let observed = rows.iter().find(|row| row["path"] == candidate).unwrap();
        assert_eq!(observed["state"], "FILE_SNAPSHOT_OBSERVED");
        assert_eq!(
            observed["inspected_sha256"],
            semwright_mlt_video::hash::sha256(b"partial WAV original")
        );
        assert_eq!(observed["producer_eof_attested"], false);
        let missing_final = rows
            .iter()
            .find(|row| row["path"] == case.output_name())
            .unwrap();
        assert_eq!(missing_final["state"], "NOT_PRESENT_AT_OBSERVATION");
        assert_eq!(missing_final["producer_effect_absent_claimed"], false);
        assert_eq!(
            originals.output.read(&candidate, 128).unwrap(),
            b"partial WAV original"
        );
    }

    // Synthetic metadata below tests this reader only. It is never a
    // substitute for the existing Driver, Host return or native live run.
    fn catalog_fixture(name: &str) -> [Vec<u8>; 4] {
        let stdout = serde_json::to_vec(&json!({
            "schema":1,"operation":"discover","version":"synthetic-reader-control",
            "groups":{"producers":[],"filters":[],"transitions":[],
                "consumers":[],"video_codecs":[],"audio_codecs":[]}
        }))
        .unwrap();
        catalog_fixture_with_stdout(name, stdout)
    }

    fn catalog_fixture_with_stdout(name: &str, stdout: Vec<u8>) -> [Vec<u8>; 4] {
        let stdin = Vec::new();
        let stderr = Vec::new();
        let stream = |label: &str, bytes: &[u8]| {
            json!({
                "root":"output","path":format!("{name}.audio-derive-catalog.{label}.original.bin"),
                "sha256":semwright_mlt_video::hash::sha256(bytes),"bytes":bytes.len(),
                "delivered_buffer_bytes":bytes.len(),
                "delivered_buffer_sha256":semwright_mlt_video::hash::sha256(bytes),
                "diagnostic_prefix_truncated":false,"retained":true,"stream_eof_attested":false
            })
        };
        let terminal = serde_json::to_vec(&json!({
            "schema":"semwright.audio-derivative-host-original/1",
            "host_stage":"catalog","state":"completed","output_path":name,
            "runtime_job_id":null,"runtime_job_id_exposed":false,"host_exit_code":0,
            "stdin":{"root":"output","path":format!("{name}.audio-derive-catalog.stdin.original.bin"),
                "sha256":semwright_mlt_video::hash::sha256(&stdin),"bytes":0},
            "stdin_scope":"same-empty-Vec-supplied-to-existing-discover-call",
            "stdin_delivery_confirmed":true,"stdout":stream("stdout",&stdout),
            "stderr":stream("stderr",&stderr),"original_error":null,
            "stream_eof_attested":false,"native_execution_retried":false,
            "physical_acceptance":false,
            "capture_scope":"same-existing-Host-return-before-parse-and-public-redaction"
        })).unwrap();
        [stdin, stdout, stderr, terminal]
    }

    fn check_catalog_fixture(name: &str, files: &[Vec<u8>; 4]) -> AudioLiveResult<Value> {
        audio_live_check_catalog(
            name,
            [
                Some(&files[0]),
                Some(&files[1]),
                Some(&files[2]),
                Some(&files[3]),
            ],
        )
    }

    fn write_catalog_fixture(
        originals: &AudioLiveOriginals,
        case: AudioLiveCase,
        files: &[Vec<u8>; 4],
    ) {
        for (suffix, bytes) in [
            "stdin.original.bin",
            "stdout.original.bin",
            "stderr.original.bin",
            "terminal.original.json",
        ]
        .into_iter()
        .zip(files)
        {
            originals
                .output
                .write_new(
                    "output",
                    &format!("{}.audio-derive-catalog.{suffix}", case.output_name()),
                    bytes,
                )
                .unwrap();
        }
    }

    #[test]
    fn audio_live_catalog_absence_is_bound_to_the_original_call_without_inventing_cache_history() {
        let owner = owner_root();
        let originals = open_owner(&owner);
        let case = AUDIO_LIVE_CASES[0];
        let args = json!({"synthetic_reader_case":case.id});
        let descriptor = json!({"name":AUDIO_LIVE_COMMAND});
        originals
            .begin_call(
                &context("catalog-absence-original"),
                AUDIO_LIVE_COMMAND,
                &descriptor,
                &args,
            )
            .unwrap();
        let identity = audio_live_case_call_identity(&originals, 1, &args, &descriptor).unwrap();
        let files = audio_live_observe_case_files(&originals, case).unwrap();
        let observed = audio_live_catalog_originals(&originals, case, &identity, &files).unwrap();
        assert_eq!(observed["caller"]["request_id"], "catalog-absence-original");
        assert_eq!(observed["caller"]["record_index"], 1);
        assert_eq!(observed["observation"]["state"], "ABSENT_AT_OBSERVATION");
        assert_eq!(observed["observation"]["original_count"], 0);
        assert_eq!(
            observed["observation"]["actual_gate_reason"],
            "NOT_EXPOSED_BY_EXISTING_DRIVER"
        );
        assert_eq!(
            observed["observation"]["catalog_dispatch_absent_claimed"],
            false
        );
        assert_eq!(observed["observation"]["catalog_acceptance_claimed"], false);
        assert_eq!(observed["first_call_assumed_to_discover"], false);
        let record: Value = serde_json::from_slice(
            &originals
                .records
                .read(
                    &format!("audio-live-{}.catalog-observation.original.json", case.id),
                    AUDIO_LIVE_METADATA_LIMIT,
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(record["observed"], observed);
        assert_eq!(originals.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn audio_live_catalog_complete_originals_bind_the_same_output_and_returned_buffers() {
        let name = AUDIO_LIVE_CASES[0].output_name();
        let files = catalog_fixture(&name);
        let before = files.clone();
        let observed = check_catalog_fixture(&name, &files).unwrap();
        assert_eq!(observed["state"], "FULL_RETURNED_BUFFERS_OBSERVED");
        assert_eq!(observed["original_count"], 4);
        assert_eq!(
            observed["stdout_sha256"],
            semwright_mlt_video::hash::sha256(&files[1])
        );
        assert_eq!(observed["stderr_bytes"], 0);
        assert_eq!(observed["stream_eof_attested"], false);
        assert_eq!(observed["descendant_closure_attested"], false);
        assert_eq!(files, before);
        assert!(check_catalog_fixture(&AUDIO_LIVE_CASES[1].output_name(), &files).is_err());
    }

    #[test]
    fn audio_live_catalog_partial_roster_and_missing_empty_stdin_are_rejected() {
        let name = AUDIO_LIVE_CASES[0].output_name();
        let files = catalog_fixture(&name);
        assert!(files[0].is_empty() && files[2].is_empty());
        check_catalog_fixture(&name, &files).unwrap();
        for mask in 1u8..15 {
            let selected = std::array::from_fn(|index| {
                ((mask & (1 << index)) != 0).then_some(files[index].as_slice())
            });
            assert!(
                audio_live_check_catalog(&name, selected).is_err(),
                "mask {mask}"
            );
        }
        let no_stdout = catalog_fixture_with_stdout(&name, Vec::new());
        assert!(check_catalog_fixture(&name, &no_stdout).is_err());
    }

    #[test]
    fn audio_live_catalog_valid_json_prefix_does_not_hide_truncation_or_delivery_mismatch() {
        let name = AUDIO_LIVE_CASES[0].output_name();
        let files = catalog_fixture(&name);
        let original: Value = serde_json::from_slice(&files[3]).unwrap();
        for (key, replacement) in [
            ("diagnostic_prefix_truncated", json!(true)),
            ("delivered_buffer_bytes", json!(files[1].len() + 1)),
            ("delivered_buffer_sha256", json!("0".repeat(64))),
            ("retained", json!(false)),
            ("stream_eof_attested", json!(true)),
        ] {
            let mut terminal = original.clone();
            terminal["stdout"][key] = replacement;
            let mut changed = files.clone();
            changed[3] = serde_json::to_vec(&terminal).unwrap();
            assert!(check_catalog_fixture(&name, &changed).is_err(), "{key}");
            // The prefix itself remains valid complete JSON; that does not
            // make it the complete buffer delivered by the Host.
            assert!(serde_json::from_slice::<Value>(&changed[1]).is_ok());
        }
    }

    #[test]
    fn audio_live_catalog_caps_use_existing_sdk_limits_without_truncating_originals() {
        let limit = semwright_mlt_video::runtime::AUDIO_DERIVATIVE_STREAM_LIMIT;
        assert_eq!(limit, 262_144);
        let case = AUDIO_LIVE_CASES[0];
        let name = case.output_name();
        let mut stdout = catalog_fixture(&name)[1].clone();
        stdout.resize(limit, b' ');
        let at_limit = catalog_fixture_with_stdout(&name, stdout.clone());
        check_catalog_fixture(&name, &at_limit).unwrap();
        stdout.push(b' ');
        let over_limit = catalog_fixture_with_stdout(&name, stdout.clone());
        assert!(check_catalog_fixture(&name, &over_limit).is_err());
        let owner = owner_root();
        let originals = open_owner(&owner);
        write_catalog_fixture(&originals, case, &over_limit);
        assert!(audio_live_observe_case_files(&originals, case).is_err());
        let path = format!("{name}.audio-derive-catalog.stdout.original.bin");
        assert_eq!(originals.output.read(&path, limit + 1).unwrap(), stdout);
        let observation: Value = serde_json::from_slice(
            &originals
                .records
                .read(
                    &format!("audio-live-{}.backend-files.original.json", case.id),
                    AUDIO_LIVE_METADATA_LIMIT,
                )
                .unwrap(),
        )
        .unwrap();
        let row = observation["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["path"] == path)
            .unwrap();
        assert_eq!(row["state"], "INCOMPLETE_FILE_SNAPSHOT");
        assert_eq!(row["over_inspection_limit"], true);
        assert_eq!(row["producer_eof_attested"], false);
        assert!(originals.before_effect().is_err());
    }

    #[test]
    fn audio_live_catalog_terminal_cannot_promote_nonzero_unknown_or_foreign_scope() {
        let name = AUDIO_LIVE_CASES[0].output_name();
        let files = catalog_fixture(&name);
        let original: Value = serde_json::from_slice(&files[3]).unwrap();
        for (key, replacement) in [
            ("host_exit_code", json!(1)),
            ("host_exit_code", Value::Null),
            ("runtime_job_id", json!("invented-detached-job")),
            ("runtime_job_id_exposed", json!(true)),
            ("state", json!("host-returned-error")),
            ("stdin_delivery_confirmed", Value::Null),
            ("original_error", json!({"outcome_known":false})),
            ("host_stage", json!("host")),
            ("output_path", json!("another-output.wav")),
            ("extra", json!("undeclared-field")),
        ] {
            let mut terminal = original.clone();
            terminal[key] = replacement;
            let mut changed = files.clone();
            changed[3] = serde_json::to_vec(&terminal).unwrap();
            assert!(check_catalog_fixture(&name, &changed).is_err(), "{key}");
        }
        let mut changed = files.clone();
        changed[0].push(b'x');
        assert!(check_catalog_fixture(&name, &changed).is_err());
        let mut wrong_envelope: Value = serde_json::from_slice(&files[1]).unwrap();
        wrong_envelope["operation"] = json!("av-operation");
        assert!(
            check_catalog_fixture(
                &name,
                &catalog_fixture_with_stdout(&name, serde_json::to_vec(&wrong_envelope).unwrap())
            )
            .is_err()
        );
    }

    #[test]
    fn audio_live_catalog_call_binding_rejects_foreign_request_and_descriptor() {
        let owner = owner_root();
        let originals = open_owner(&owner);
        let args = json!({"source":{"sha256":"a".repeat(64)}});
        let descriptor = json!({"name":AUDIO_LIVE_COMMAND,"synthetic_reader_only":true});
        originals
            .begin_call(
                &context("same-original-request"),
                AUDIO_LIVE_COMMAND,
                &descriptor,
                &args,
            )
            .unwrap();
        let matched = audio_live_case_call_identity(&originals, 1, &args, &descriptor).unwrap();
        assert_eq!(matched["request_id"], "same-original-request");
        assert!(
            audio_live_case_call_identity(&originals, 1, &json!({"other":"args"}), &descriptor)
                .is_err()
        );
        assert!(
            audio_live_case_call_identity(&originals, 1, &args, &json!({"name":"other"})).is_err()
        );
        assert!(audio_live_case_call_identity(&originals, 2, &args, &descriptor).is_err());
        let mut foreign = context("other-session-request");
        foreign.session = "another-owner-session".into();
        originals
            .begin_call(&foreign, AUDIO_LIVE_COMMAND, &descriptor, &args)
            .unwrap();
        assert!(audio_live_case_call_identity(&originals, 2, &args, &descriptor).is_err());
        assert!(audio_live_case_call_identity(&originals, 1, &args, &descriptor).is_err());
    }

    #[test]
    fn audio_live_catalog_changed_observation_and_custody_failure_preserve_native_unknown() {
        let owner = owner_root();
        let originals = open_owner(&owner);
        let case = AUDIO_LIVE_CASES[0];
        let args = json!({"synthetic_reader_case":case.id});
        let descriptor = json!({"name":AUDIO_LIVE_COMMAND});
        originals
            .begin_call(
                &context("changed-catalog-original"),
                AUDIO_LIVE_COMMAND,
                &descriptor,
                &args,
            )
            .unwrap();
        let identity = audio_live_case_call_identity(&originals, 1, &args, &descriptor).unwrap();
        let files = audio_live_observe_case_files(&originals, case).unwrap();
        write_catalog_fixture(&originals, case, &catalog_fixture(&case.output_name()));
        let record = format!("audio-live-{}.catalog-observation.original.json", case.id);
        originals
            .records
            .write_new("records", &record, b"retained-prior-prefix")
            .unwrap();
        let observed = audio_live_catalog_originals(&originals, case, &identity, &files);
        assert!(
            observed
                .as_ref()
                .unwrap_err()
                .to_string()
                .contains("changed after its first observation")
        );
        assert_eq!(
            originals
                .records
                .read(&record, AUDIO_LIVE_METADATA_LIMIT)
                .unwrap(),
            b"retained-prior-prefix"
        );
        let mut primary = semwright_types::Error::new(
            semwright_types::ErrorCode::BackendFailed,
            "same original catalog operation error",
        )
        .uncertain()
        .recipe_progress(1, "catalog");
        primary.candidates = vec!["same original candidate".into()];
        let pointer = primary.message.as_ptr();
        let expected = serde_json::to_value(&primary).unwrap();
        let writer_error: AudioLiveResult<()> = Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "later result custody collision",
        )
        .into());
        let returned =
            audio_live_finish_observed::<Value, Value>(Err(primary), observed, writer_error)
                .unwrap_err();
        let returned = returned.downcast::<semwright_types::Error>().unwrap();
        assert_eq!(returned.message.as_ptr(), pointer);
        assert_eq!(serde_json::to_value(&returned).unwrap(), expected);
        assert!(!returned.outcome_known);
        assert_eq!(originals.calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert!(originals.before_effect().is_err());

        let earlier = Box::new(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "original catalog observation failed",
        ));
        let earlier_pointer = earlier.as_ref() as *const std::io::Error;
        let observed: AudioLiveResult<Value> = Err(earlier);
        let writer_error: AudioLiveResult<()> = Err(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "later result custody collision",
        )
        .into());
        let returned = audio_live_finish_observed(
            Ok(json!({"original_native_success":true})),
            observed,
            writer_error,
        )
        .unwrap_err();
        let returned = returned.downcast_ref::<std::io::Error>().unwrap();
        assert_eq!(returned as *const std::io::Error, earlier_pointer);
        assert_eq!(returned.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(returned.to_string(), "original catalog observation failed");

        let other_owner = owner_root();
        let other = open_owner(&other_owner);
        let complete = catalog_fixture(&case.output_name());
        write_catalog_fixture(&other, case, &complete);
        let before = audio_live_observe_case_files(&other, case).unwrap();
        std::fs::remove_file(other.output_path.path.join(format!(
            "{}.audio-derive-catalog.stdout.original.bin",
            case.output_name()
        )))
        .unwrap();
        let error = audio_live_catalog_originals(&other, case, &identity, &before).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("disappeared after its first observation")
        );
    }
}
