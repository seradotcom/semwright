//! Semwright-owned runtime helper for Host-mediated MLT tools.
//!
//! The Driver Host supplies owner-pinned dependency paths through protocol-v7 typed
//! ToolPath arguments. This helper never searches PATH or accepts shell fragments.

use semwright_mlt_video::{
    hash::reader_hash,
    runtime::{
        NATIVE_DIAGNOSTIC_FILE, ProcessSpec, RenderProfile, ServiceCatalog,
        constrained_environment, native_render_diagnostic, render_argv, run,
    },
};
use serde_json::{Map, Value, json};
use std::{
    ffi::OsString,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

const GROUPS: [&str; 6] = [
    "producers",
    "filters",
    "transitions",
    "consumers",
    "video_codecs",
    "audio_codecs",
];

// These are identities inside the already delegated output grant. They do not
// create a grant or select an arbitrary scratch directory. The original Driver
// freezes the receipt before its first probe/render; the helper joins that same
// attempt by descriptor identity and exact original receipt bytes.
struct OriginalWorkspaceArgs {
    output_root: PathBuf,
    output_path: String,
    receipt_sha256: String,
    probe_slot: Option<usize>,
}

struct OriginalWorkspace {
    output: semwright_mlt_video::fs::Root,
    inputs: semwright_mlt_video::fs::PrivateDir,
    work: semwright_mlt_video::fs::PrivateDir,
    receipt: Value,
}

impl OriginalWorkspaceArgs {
    fn open(&self) -> Result<OriginalWorkspace, String> {
        use semwright_mlt_video::fs::Root;
        let output = Root::open(&self.output_root, true, true)
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        let (inputs, work) = output
            .reopen_original_workspaces(&self.output_path, &self.receipt_sha256)
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        let bytes = output
            .read(
                &format!("{}.raw-original-workspaces.json", self.output_path),
                semwright_mlt_video::color::REPORT_LIMIT,
            )
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        if semwright_mlt_video::hash::sha256(&bytes) != self.receipt_sha256 {
            return Err("Original workspace declaration changed during handoff".into());
        }
        let receipt = serde_json::from_slice(&bytes)
            .map_err(|_| "Original workspace declaration is malformed")?;
        Ok(OriginalWorkspace {
            output,
            inputs,
            work,
            receipt,
        })
    }
}

fn original_probe_role(
    receipt: &Value,
    name: &str,
    count_video_frames: bool,
    slot: usize,
) -> Result<(), String> {
    let inputs = receipt["input_members"]
        .as_array()
        .ok_or("Original input roster is absent")?;
    let assets = inputs
        .iter()
        .filter(|member| {
            !matches!(
                member["basename"].as_str(),
                Some(
                    "project.mlt"
                        | "native-observation-request.json"
                        | "import-request.original.json"
                )
            )
        })
        .collect::<Vec<_>>();
    match receipt["original_kind"].as_str() {
        Some("host-native-render") => {
            if inputs.len() != assets.len() + 2
                || inputs
                    .iter()
                    .filter(|member| {
                        member["basename"] == "project.mlt"
                            && member["maximum_bytes"].as_u64()
                                == Some(semwright_mlt_video::xml::MAX_XML as u64)
                    })
                    .count()
                    != 1
                || inputs
                    .iter()
                    .filter(|member| {
                        member["basename"] == "native-observation-request.json"
                            && member["maximum_bytes"].as_u64()
                                == Some(
                                    semwright_mlt_video::color::NATIVE_OBSERVATION_REQUEST_LIMIT
                                        as u64,
                                )
                    })
                    .count()
                    != 1
            {
                return Err("Original visual input metadata is outside its closed roster".into());
            }
            let expected = if count_video_frames {
                slot == assets.len() && name == "partial.mkv"
            } else {
                assets.get(slot).is_some_and(|member| {
                    member["basename"] == name
                        && member["maximum_bytes"].as_u64()
                            == Some(semwright_mlt_video::jobs::MAX_MEDIA_BYTES)
                })
            };
            if !expected {
                return Err(
                    "Original probe slot differs from the declared visual asset order".into(),
                );
            }
        }
        Some("input-probe") => {
            if count_video_frames
                || slot != 0
                || name != "probe.bin"
                || assets.len() != 1
                || inputs.len() != 2
                || assets[0]["basename"] != name
                || assets[0]["maximum_bytes"].as_u64()
                    != Some(semwright_mlt_video::jobs::MAX_MEDIA_BYTES + 1)
                || inputs
                    .iter()
                    .filter(|member| {
                        member["basename"] == "import-request.original.json"
                            && member["maximum_bytes"].as_u64()
                                == Some(semwright_mlt_video::json::MAX_JSON as u64)
                    })
                    .count()
                    != 1
            {
                return Err("Original import probe differs from its single declared role".into());
            }
        }
        _ => return Err("Original probe workspace kind is not admitted".into()),
    }
    let work = receipt["work_members"]
        .as_array()
        .ok_or("Original work roster is absent")?;
    for (tail, cap) in [
        (
            "media-probe.stdout.original.bin",
            semwright_mlt_video::runtime::FFV1_CUSTODY_STREAM_PREFIX,
        ),
        (
            "media-probe.stderr.original.bin",
            semwright_mlt_video::runtime::FFV1_CUSTODY_STREAM_PREFIX,
        ),
        (
            "media-probe.terminal.original.json",
            semwright_mlt_video::runtime::FFV1_CUSTODY_METADATA,
        ),
        (
            "process-custody.original.json",
            semwright_mlt_video::runtime::FFV1_CUSTODY_METADATA,
        ),
    ] {
        let basename = format!("native-input-probe-{slot:06}-{tail}");
        if work
            .iter()
            .filter(|member| {
                member["basename"] == basename
                    && member["maximum_bytes"].as_u64() == Some(cap as u64)
            })
            .count()
            != 1
        {
            return Err("Original probe is outside the frozen work roster".into());
        }
    }
    Ok(())
}

fn original_workspace_flags(
    first: Option<String>,
    args: &mut impl Iterator<Item = String>,
    with_probe_slot: bool,
) -> Result<Option<OriginalWorkspaceArgs>, String> {
    let Some(first) = first else {
        return Ok(None);
    };
    if first != "--original-output-root" {
        return Err("runtime runner received unexpected original-custody arguments".into());
    }
    let output_root = PathBuf::from(args.next().ok_or("Original output root is absent")?);
    let output_path = required_flag(args, "--original-output-path", "original custody")?;
    let receipt_sha256 = required_flag(args, "--original-workspaces-sha256", "original custody")?;
    let probe_slot = if with_probe_slot {
        let slot = required_flag(args, "--original-probe-slot", "original custody")?;
        let parsed = slot
            .parse::<usize>()
            .map_err(|_| "Original probe slot is invalid")?;
        if parsed >= 1_000_000 || parsed.to_string() != slot {
            return Err("Original probe slot is outside its closed bound".into());
        }
        Some(parsed)
    } else {
        None
    };
    if args.next().is_some() {
        return Err("runtime runner received unexpected arguments".into());
    }
    Ok(Some(OriginalWorkspaceArgs {
        output_root,
        output_path,
        receipt_sha256,
        probe_slot,
    }))
}

fn dependency_path(value: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if !path.is_absolute() || value.contains('\0') {
        return Err("runtime dependency must be an absolute Host-materialized path".into());
    }
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|_| "runtime dependency is unavailable".to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("runtime dependency must be a regular non-symlink file".into());
    }
    Ok(path)
}

fn runtime_entry(runtime_root: &Path, sealed_tool: &Path, name: &str) -> Result<PathBuf, String> {
    if !matches!(name, "melt" | "ffprobe" | "ffmpeg") {
        return Err("runtime entrypoint is not allowlisted".into());
    }
    let root_meta = std::fs::symlink_metadata(runtime_root)
        .map_err(|_| "MLT runtime root is unavailable".to_string())?;
    if !runtime_root.is_absolute() || !root_meta.is_dir() || root_meta.file_type().is_symlink() {
        return Err("MLT runtime root must be an absolute non-symlink directory".into());
    }
    let canonical_root = std::fs::canonicalize(runtime_root)
        .map_err(|_| "MLT runtime root could not be canonicalized".to_string())?;
    let entry = runtime_root.join("bin").join(name);
    let candidate = std::fs::canonicalize(&entry)
        .map_err(|_| format!("{name} runtime entrypoint is unavailable"))?;
    if candidate.strip_prefix(&canonical_root).is_err() {
        return Err(format!(
            "{name} runtime entrypoint escaped the delegated runtime root"
        ));
    }
    let candidate_meta = std::fs::symlink_metadata(&candidate)
        .map_err(|_| format!("{name} runtime entrypoint metadata is unavailable"))?;
    if !candidate_meta.is_file() || candidate_meta.file_type().is_symlink() {
        return Err(format!(
            "Canonical {name} runtime entrypoint must be a regular file"
        ));
    }
    let (sealed_hash, sealed_size) = reader_hash(
        std::fs::File::open(sealed_tool)
            .map_err(|_| format!("sealed {name} dependency is unreadable"))?,
        64 * 1024 * 1024,
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let (runtime_hash, runtime_size) = reader_hash(
        std::fs::File::open(&candidate)
            .map_err(|_| format!("{name} runtime entrypoint is unreadable"))?,
        64 * 1024 * 1024,
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    if sealed_size != runtime_size || sealed_hash != runtime_hash {
        return Err(format!(
            "{name} runtime entrypoint does not match the Host-sealed tool bytes"
        ));
    }
    Ok(candidate)
}

fn execute(
    executable: &Path,
    args: Vec<OsString>,
    cwd: &Path,
    timeout: Duration,
    cpu_seconds: u64,
    address_space_bytes: u64,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    let cancel = AtomicBool::new(false);
    let result = run(
        &ProcessSpec {
            executable: executable.to_path_buf(),
            args,
            cwd: cwd.to_path_buf(),
            timeout,
            cpu_seconds,
            address_space_bytes,
            environment: constrained_environment(),
        },
        &cancel,
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))?
    .checked()
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    Ok((result.stdout, result.stderr))
}

fn direct_component(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err(format!("{label} must be one bounded path component"));
    }
    Ok(())
}

fn scratch_dir(scratch_root: &Path, directory: &str) -> Result<PathBuf, String> {
    direct_component(directory, "scratch directory")?;
    let root_meta = std::fs::symlink_metadata(scratch_root)
        .map_err(|_| "scratch root is unavailable".to_string())?;
    if !scratch_root.is_absolute() || !root_meta.is_dir() || root_meta.file_type().is_symlink() {
        return Err("scratch root must be an absolute non-symlink directory".into());
    }
    let work = scratch_root.join(directory);
    let work_meta = std::fs::symlink_metadata(&work)
        .map_err(|_| "scratch job directory is unavailable".to_string())?;
    if !work_meta.is_dir() || work_meta.file_type().is_symlink() {
        return Err("scratch job directory must be a non-symlink directory".into());
    }
    Ok(work)
}

fn scratch_file(
    scratch_root: &Path,
    directory: &str,
    name: &str,
) -> Result<(PathBuf, PathBuf), String> {
    direct_component(name, "scratch filename")?;
    let work = scratch_dir(scratch_root, directory)?;
    let target = work.join(name);
    let target_meta = std::fs::symlink_metadata(&target)
        .map_err(|_| "scratch media file is unavailable".to_string())?;
    if !target_meta.is_file() || target_meta.file_type().is_symlink() {
        return Err("scratch media file must be a regular non-symlink file".into());
    }
    Ok((work, target))
}

fn probe(
    ffprobe: &Path,
    scratch_root: &Path,
    directory: &str,
    name: &str,
    count_video_frames: bool,
    originals: Option<&OriginalWorkspaceArgs>,
) -> Result<Value, String> {
    let owned = originals.map(OriginalWorkspaceArgs::open).transpose()?;
    let (work, target) = if let (Some(binding), Some(owned)) = (originals, owned.as_ref()) {
        original_probe_role(
            &owned.receipt,
            name,
            count_video_frames,
            binding.probe_slot.ok_or("Original probe slot is absent")?,
        )?;
        if owned.receipt["original_kind"] == "input-probe" {
            let input_root = owned
                .inputs
                .owned_root(true, false)
                .map_err(|e| format!("{}: {}", e.code, e.message))?;
            let original_args = input_root
                .read(
                    "import-request.original.json",
                    semwright_mlt_video::json::MAX_JSON,
                )
                .map_err(|e| format!("{}: {}", e.code, e.message))?;
            let args: Value = serde_json::from_slice(&original_args)
                .map_err(|_| "Original import arguments are malformed")?;
            if binding.output_path.starts_with("asset-import-probe-") {
                let framed = serde_json::to_vec(&("driver.mlt-video.asset.import", &args))
                    .map_err(|_| "Original import identity encoding failed")?;
                if binding.output_path
                    != format!(
                        "asset-import-probe-{}",
                        semwright_mlt_video::hash::sha256(&framed)
                    )
                    || args.get("project").and_then(Value::as_str).is_none()
                    || args
                        .get("expected_revision")
                        .and_then(Value::as_str)
                        .is_none()
                {
                    return Err(
                        "Original import arguments differ from the pre-dispatch journal identity"
                            .into(),
                    );
                }
            } else if !binding
                .output_path
                .strip_prefix("asset-import-invocation-")
                .is_some_and(semwright_mlt_video::hash::valid_digest)
            {
                return Err("Original import namespace is not admitted".into());
            }
        }
        let expected_directory = format!(
            "{}.raw-original-{}",
            binding.output_path,
            if count_video_frames { "work" } else { "inputs" }
        );
        if scratch_root != binding.output_root.as_path()
            || directory != expected_directory
            || (count_video_frames && name != "partial.mkv")
            || (!count_video_frames
                && name != "probe.bin"
                && !(name.len() == 34
                    && name.starts_with("asset-")
                    && name.ends_with(".bin")
                    && name.as_bytes()[6..30].iter().all(u8::is_ascii_hexdigit)))
        {
            return Err("Original probe differs from the derived workspace role".into());
        }
        let source = if count_video_frames {
            &owned.work
        } else {
            &owned.inputs
        };
        (source.path().to_path_buf(), source.path().join(name))
    } else {
        scratch_file(scratch_root, directory, name)?
    };
    let mut args = vec![OsString::from("-v"), OsString::from("error")];
    if count_video_frames {
        // Exact opt-in: independently decode *all* video frames while
        // retaining the entire stream list, so unexpected audio remains
        // visible to the Host and can never be hidden by select_streams.
        args.push(OsString::from("-count_frames"));
    }
    if count_video_frames {
        // Keep a small, closed observation document. Importantly do NOT
        // select only video streams: any unexpected audio remains visible.
        args.extend([
            "-show_streams", "-show_format",
            "-show_entries",
            "stream=codec_name,codec_type,width,height,nb_frames,nb_read_frames,channels,sample_rate:format=duration",
            "-of", "json",
        ].map(OsString::from));
    } else {
        args.extend(["-show_streams", "-show_format", "-of", "json"].map(OsString::from));
    }
    args.push(target.into_os_string());
    if let (Some(binding), Some(owned)) = (originals, owned.as_ref()) {
        use semwright_mlt_video::{
            Error,
            runtime::{
                Ffv1ProcessCustody, ProcessObservation, checked_ffv1_process_observation,
                run_with_file_limit_observed,
            },
        };
        let source = if count_video_frames {
            &owned.work
        } else {
            &owned.inputs
        };
        let source_root = source
            .owned_root(true, false)
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        let work_root = owned
            .work
            .owned_root(true, true)
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        let cap = if count_video_frames {
            semwright_mlt_video::jobs::MAX_ARTIFACT_BYTES
        } else {
            semwright_mlt_video::jobs::MAX_MEDIA_BYTES
        };
        let (source_sha, _) = reader_hash(
            source_root
                .read_file(name, cap)
                .map_err(|e| format!("{}: {}", e.code, e.message))?,
            cap,
        )
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
        let custody = Ffv1ProcessCustody::for_input_probe(
            &owned.output,
            &binding.output_path,
            &source_sha,
            binding.probe_slot.ok_or("Original probe slot is absent")?,
            &work_root,
        )
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
        let spec = ProcessSpec {
            executable: ffprobe.to_path_buf(),
            args,
            cwd: work,
            timeout: Duration::from_secs(if count_video_frames { 120 } else { 5 }),
            cpu_seconds: if count_video_frames { 180 } else { 30 },
            address_space_bytes: 1_073_741_824,
            environment: constrained_environment(),
        };
        let mut observed = ProcessObservation::default();
        let result = run_with_file_limit_observed(
            &spec,
            &AtomicBool::new(false),
            16_384,
            262_144,
            semwright_mlt_video::jobs::MAX_ARTIFACT_BYTES,
            Some(&mut observed),
        );
        custody.record_observed(
            "media-probe",
            &spec,
            &result,
            16_384,
            262_144,
            Some(&observed),
            Some(semwright_mlt_video::jobs::MAX_ARTIFACT_BYTES),
        );
        let original = checked_ffv1_process_observation(result, &observed).and_then(|result| {
            let media: Value = serde_json::from_slice(&result.stdout)
                .map_err(|_| Error::invalid("ffprobe returned malformed JSON"))?;
            if !media.is_object() {
                return Err(Error::invalid("ffprobe result must be a JSON object"));
            }
            Ok(json!({"schema":1,"operation":"probe","media":media}))
        });
        return custody
            .complete(original)
            .map_err(|e| format!("{}: {}", e.code, e.message));
    }
    let (stdout, _stderr) = execute(
        ffprobe,
        args,
        &work,
        if count_video_frames {
            Duration::from_secs(120)
        } else {
            Duration::from_secs(5)
        },
        if count_video_frames { 180 } else { 30 },
        1_073_741_824,
    )?;
    let media: Value = serde_json::from_slice(&stdout)
        .map_err(|_| "ffprobe returned malformed JSON".to_string())?;
    if !media.is_object() {
        return Err("ffprobe result must be a JSON object".into());
    }
    Ok(json!({
        "schema": 1,
        "operation": "probe",
        "media": media,
    }))
}

fn require_render_deadline(deadline: Instant) -> Result<(), String> {
    if Instant::now() >= deadline {
        Err("Timeout: shared native render and observation deadline expired".into())
    } else {
        Ok(())
    }
}

fn render(
    melt: &Path,
    scratch_root: &Path,
    directory: &str,
    profile_id: &str,
    observation_runtime: Option<&semwright_mlt_video::runtime::Runtime>,
    originals: Option<&OriginalWorkspaceArgs>,
) -> Result<Value, String> {
    if let Some(binding) = originals {
        if scratch_root != binding.output_root.as_path()
            || directory != format!("{}.raw-original-work", binding.output_path)
            || profile_id != "lossless-video-only"
        {
            return Err("Original render differs from the closed visual workspace".into());
        }
        return render_original(
            melt,
            observation_runtime.ok_or("Original render requires complete observation")?,
            binding,
        );
    }
    let work = scratch_dir(scratch_root, directory)?;
    let project = work.join("project.mlt");
    let project_meta = std::fs::symlink_metadata(&project)
        .map_err(|_| "render project.mlt is unavailable".to_string())?;
    if !project_meta.is_file() || project_meta.file_type().is_symlink() {
        return Err("render project.mlt must be a regular non-symlink file".into());
    }
    let profile = RenderProfile::get(profile_id)
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let output_name = format!("partial.{}", profile.extension);
    direct_component(&output_name, "render output")?;
    let output = work.join(&output_name);
    if output.exists() {
        return Err("render output already exists".into());
    }
    let args = render_argv(
        OsString::from("project.mlt"),
        OsString::from(output_name.clone()),
        &profile,
    );
    // Preserve the actual native result before checked() can discard failed-tool logs.
    // This uses exactly the existing render argv, environment, deadlines and limits.
    let project_root = semwright_mlt_video::fs::Root::open(&work, true, true)
        .map_err(|_| "native render scratch receipt is unavailable".to_string())?;
    let project_bytes = project_root
        .read("project.mlt", semwright_mlt_video::xml::MAX_XML)
        .map_err(|_| "native render XML diagnostic binding is unavailable".to_string())?;
    let complete_request = if observation_runtime.is_some() {
        if profile.id != "lossless-video-only" {
            return Err(
                "Complete Host observation is restricted to the existing visual profile".into(),
            );
        }
        let bytes = project_root
            .read(
                semwright_mlt_video::color::NATIVE_OBSERVATION_REQUEST_FILE,
                semwright_mlt_video::color::NATIVE_OBSERVATION_REQUEST_LIMIT,
            )
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        let request = semwright_mlt_video::color::NativeObservationRequest::parse(&bytes)
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        request
            .verify_staged(&project_root)
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        Some((bytes, request))
    } else {
        None
    };
    // Retain the actual original 120-second render ceiling. The complete
    // observation shares that deadline; the enclosing Host ceiling stays300s.
    let deadline = Instant::now() + Duration::from_secs(120);
    let result = run(
        &ProcessSpec {
            executable: melt.to_path_buf(),
            args: args.clone(),
            cwd: work.clone(),
            timeout: Duration::from_secs(120),
            cpu_seconds: 300,
            address_space_bytes: 4_294_967_296,
            environment: constrained_environment(),
        },
        &AtomicBool::new(false),
    )
    .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let diagnostic = native_render_diagnostic(&project_bytes, &args, &result)
        .map_err(|_| "native render diagnostic exceeds its contract".to_string())?;
    project_root
        .write_new("scratch", NATIVE_DIAGNOSTIC_FILE, &diagnostic)
        .map_err(|_| "native render diagnostic exclusive save failed".to_string())?;
    result
        .checked()
        .map_err(|error| format!("{}: {}", error.code, error.message))?;
    let metadata = std::fs::symlink_metadata(&output)
        .map_err(|_| "render output is unavailable".to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err("render output must be a regular non-symlink file".into());
    }
    let mut result = json!({
        "schema": 1,
        "operation": "render",
        "output": output_name,
    });
    if let (Some(runtime), Some((request_bytes, request))) = (observation_runtime, complete_request)
    {
        request
            .verify_staged(&project_root)
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        require_render_deadline(deadline)?;
        let observed = runtime
            .observe_video_file(
                &work,
                &output_name,
                semwright_mlt_video::color::ObservationProfile::MltVisualFfv1,
                (request.width, request.height),
                request.frames,
                request.fps_num,
                request.fps_den,
                deadline,
                &AtomicBool::new(false),
            )
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        require_render_deadline(deadline)?;
        let sources = request.sources();
        let report = semwright_mlt_video::color::build_observation_report(
            &observed,
            &request.output_path,
            request.source_binding(),
            Some(&sources),
            request.width,
            request.height,
            request.fps_num,
            request.fps_den,
        )
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
        let parsed = semwright_mlt_video::color::parse_complete_observation(&report)
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        request
            .verify_report(&parsed)
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        request
            .verify_staged(&project_root)
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        require_render_deadline(deadline)?;
        let descriptor = project_root
            .write_new(
                "scratch",
                semwright_mlt_video::color::NATIVE_OBSERVATION_FILE,
                &report,
            )
            .map_err(|e| format!("{}: {}", e.code, e.message))?;
        result["observation"] = serde_json::from_str(&descriptor.json().encode())
            .map_err(|_| "Native observation descriptor serialization")?;
        result["request_sha256"] = json!(semwright_mlt_video::hash::sha256(&request_bytes));
    }
    Ok(result)
}

fn render_original(
    melt: &Path,
    runtime: &semwright_mlt_video::runtime::Runtime,
    binding: &OriginalWorkspaceArgs,
) -> Result<Value, String> {
    use semwright_mlt_video::{
        Error, color,
        originals::VideoOriginalKind,
        runtime::{
            Ffv1ProcessCustody, ProcessObservation, checked_ffv1_process_observation,
            run_with_file_limit_observed,
        },
    };
    let owned = binding.open()?;
    if owned.receipt["original_kind"] != "host-native-render" {
        return Err("Original render workspace kind is not admitted".into());
    }
    let inputs = owned
        .inputs
        .owned_root(true, false)
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let work = owned
        .work
        .owned_root(true, true)
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let request_bytes = inputs
        .read(
            color::NATIVE_OBSERVATION_REQUEST_FILE,
            color::NATIVE_OBSERVATION_REQUEST_LIMIT,
        )
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let request = color::NativeObservationRequest::parse(&request_bytes)
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    if request.output_path != binding.output_path {
        return Err("Original output binding differs".into());
    }
    request
        .verify_staged(&inputs)
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let project = inputs
        .read("project.mlt", semwright_mlt_video::xml::MAX_XML)
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let source_sha = semwright_mlt_video::hash::sha256(&project);
    let custody = Ffv1ProcessCustody::with_owned_work(
        &owned.output,
        &binding.output_path,
        &source_sha,
        VideoOriginalKind::HostNativeRender,
        &work,
    )
    .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let deadline_check = |deadline| {
        require_render_deadline(deadline).map_err(|message| {
            let mut error = Error::new("Timeout", message);
            error.outcome_known = false;
            error
        })
    };
    let original = (|| -> semwright_mlt_video::Result<Value> {
        let profile = RenderProfile::get("lossless-video-only")?;
        if work.exists("partial.mkv")? {
            return Err(Error::new("Conflict", "Render output already exists"));
        }
        let args = render_argv(
            "project.mlt",
            owned.work.path().join("partial.mkv").into_os_string(),
            &profile,
        );
        // Same first render and same 120-second shared observation deadline;
        // only its owned input/work placement and capture are joined here.
        let deadline = Instant::now() + Duration::from_secs(120);
        let spec = ProcessSpec {
            executable: melt.to_path_buf(),
            args: args.clone(),
            cwd: owned.inputs.path().to_path_buf(),
            timeout: Duration::from_secs(120),
            cpu_seconds: 300,
            address_space_bytes: 4_294_967_296,
            environment: constrained_environment(),
        };
        let mut observed = ProcessObservation::default();
        let process = run_with_file_limit_observed(
            &spec,
            &AtomicBool::new(false),
            16_384,
            262_144,
            semwright_mlt_video::jobs::MAX_ARTIFACT_BYTES,
            Some(&mut observed),
        );
        custody.record_observed(
            "render",
            &spec,
            &process,
            16_384,
            262_144,
            Some(&observed),
            Some(semwright_mlt_video::jobs::MAX_ARTIFACT_BYTES),
        );
        let diagnostic = match &process {
            Ok(result) => native_render_diagnostic(&project, &args, result).and_then(|bytes| {
                work.write_new("owned-work", NATIVE_DIAGNOSTIC_FILE, &bytes)
                    .map(|_| ())
            }),
            Err(_) => Ok(()),
        };
        if let Err(error) = &diagnostic {
            custody.failed_error("native-render-diagnostic", error);
        }
        checked_ffv1_process_observation(process, &observed)?;
        diagnostic?;
        request.verify_staged(&inputs)?;
        deadline_check(deadline)?;
        let observed = runtime.observe_video_file_observed(
            owned.work.path(),
            "partial.mkv",
            color::ObservationProfile::MltVisualFfv1,
            (request.width, request.height),
            request.frames,
            request.fps_num,
            request.fps_den,
            deadline,
            &AtomicBool::new(false),
            Some(&custody),
        )?;
        deadline_check(deadline)?;
        let sources = request.sources();
        let report = color::build_observation_report(
            &observed,
            &request.output_path,
            request.source_binding(),
            Some(&sources),
            request.width,
            request.height,
            request.fps_num,
            request.fps_den,
        )?;
        let parsed = color::parse_complete_observation(&report)?;
        request.verify_report(&parsed)?;
        request.verify_staged(&inputs)?;
        deadline_check(deadline)?;
        let descriptor = work.write_new("scratch", color::NATIVE_OBSERVATION_FILE, &report)?;
        let descriptor: Value = serde_json::from_str(&descriptor.json().encode())
            .map_err(|_| Error::invalid("Native observation descriptor serialization"))?;
        Ok(
            json!({"schema":1,"operation":"render","output":"partial.mkv",
            "observation":descriptor,"request_sha256":semwright_mlt_video::hash::sha256(&request_bytes)}),
        )
    })();
    custody
        .complete(original)
        .map_err(|e| format!("{}: {}", e.code, e.message))
}

fn discover(melt: &Path) -> Result<Value, String> {
    let (stdout, stderr) = execute(
        melt,
        vec!["-version".into()],
        Path::new("/tmp"),
        Duration::from_secs(3),
        30,
        4_294_967_296,
    )?;
    let version_text = format!(
        "{} {}",
        String::from_utf8_lossy(&stdout),
        String::from_utf8_lossy(&stderr)
    );
    let version = semwright_mlt_video::json::display(
        version_text
            .lines()
            .find(|line| line.to_ascii_lowercase().contains("melt"))
            .unwrap_or("Version output did not identify melt"),
    );

    let mut groups = Map::new();
    for group in GROUPS {
        let (stdout, stderr) = execute(
            melt,
            vec![OsString::from("-query"), OsString::from(group)],
            Path::new("/tmp"),
            Duration::from_secs(3),
            30,
            4_294_967_296,
        )?;
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&stdout),
            String::from_utf8_lossy(&stderr)
        );
        let values = ServiceCatalog::parse_list(&text)
            .map_err(|error| format!("{}: {}", error.code, error.message))?;
        groups.insert(
            group.into(),
            Value::Array(values.into_iter().map(Value::String).collect()),
        );
    }

    Ok(json!({
        "schema": 1,
        "operation": "discover",
        "version": version,
        "groups": groups,
    }))
}

fn required_flag(
    args: &mut impl Iterator<Item = String>,
    expected: &str,
    description: &str,
) -> Result<String, String> {
    if args.next().as_deref() != Some(expected) {
        return Err(format!("{description} requires {expected} <value>"));
    }
    args.next()
        .ok_or_else(|| format!("{description} requires a value for {expected}"))
}

/// Reuse the same Host-sealed dependency binding for render and AV operations.
fn host_runtime(
    runtime_root: &Path,
    sealed: [&Path; 3],
) -> Result<semwright_mlt_video::runtime::Runtime, String> {
    use semwright_mlt_video::runtime::{Runtime, Tool};
    let mut tools = Vec::new();
    for (name, sealed) in ["melt", "ffprobe", "ffmpeg"].into_iter().zip(sealed) {
        // Compare the delegated runtime entry with the Host-sealed dependency,
        // then execute the sealed bytes. The read-only runtime alias can expose
        // an unmapped owner UID inside the namespace; it is not the executable
        // identity delivered by ToolPath. Do not broaden Tool's owner policy.
        runtime_entry(runtime_root, sealed, name)?;
        let path = sealed.to_path_buf();
        let (sha256, _) = reader_hash(
            std::fs::File::open(&path).map_err(|_| "AV dependency unreadable")?,
            64 * 1024 * 1024,
        )
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
        tools.push(Tool { path, sha256 });
    }
    Runtime::from_host_tools(tools[0].clone(), tools[1].clone(), tools[2].clone())
        .map_err(|e| format!("{}: {}", e.code, e.message))
}

fn allowed_av_operation(command: &str) -> bool {
    matches!(
        command,
        "driver.mlt-video.frames.encode"
            | "driver.mlt-video.av.mux"
            | "driver.mlt-video.av.caption_pixels.verify"
            | "driver.mlt-video.sync.probe"
            | semwright_mlt_video::runtime::AUDIO_DERIVATIVE_COMMAND
    )
}

fn av_operation(
    runtime_root: &Path,
    sealed: [&Path; 3],
    roots: [&Path; 3],
) -> Result<Value, String> {
    use semwright_mlt_video::{app::App, fs::Root};
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(65_537)
        .read_to_end(&mut bytes)
        .map_err(|_| "AV request unreadable")?;
    if bytes.len() > 65_536 {
        return Err("AV request exceeds Host stdin budget".into());
    }
    let request: Value = serde_json::from_slice(&bytes).map_err(|_| "AV request must be JSON")?;
    let object = request.as_object().ok_or("AV request must be an object")?;
    if object.len() != 3 || !object.contains_key("args") {
        return Err("AV request has unexpected fields".into());
    }
    let command = object
        .get("command")
        .and_then(Value::as_str)
        .ok_or("AV command missing")?;
    if !allowed_av_operation(command) {
        return Err("AV operation is not allowlisted".into());
    }
    let descriptor = object
        .get("descriptor_sha256")
        .and_then(Value::as_str)
        .ok_or("AV descriptor missing")?;
    let mut delegated = std::collections::BTreeMap::new();
    for ((name, writable), path) in [("project", false), ("media", false), ("output", true)]
        .into_iter()
        .zip(roots)
    {
        if !path.is_absolute() {
            return Err("Delegated AV root must be absolute".into());
        }
        delegated.insert(
            name.to_owned(),
            Arc::new(
                Root::open(path, true, writable)
                    .map_err(|e| format!("{}: {}", e.code, e.message))?,
            ),
        );
    }
    let runtime = host_runtime(runtime_root, sealed)?;
    let mut app = App::new(delegated, Some(Arc::new(runtime)))
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let args = semwright_mlt_video::json::parse(object["args"].to_string().as_bytes())
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    let result = app
        .execute(command, descriptor, args)
        .map_err(|e| format!("{}: {}", e.code, e.message))?;
    serde_json::from_str(&result.encode()).map_err(|_| "AV result serialization failed".into())
}

fn parse() -> Result<Value, String> {
    let mut args = std::env::args().skip(1);
    let operation = args
        .next()
        .ok_or_else(|| "runtime operation is required".to_string())?;
    match operation.as_str() {
        "av-operation" => {
            let runtime_root =
                PathBuf::from(required_flag(&mut args, "--runtime-root", "AV operation")?);
            let melt =
                dependency_path(&required_flag(&mut args, "--melt-sealed", "AV operation")?)?;
            let ffprobe = dependency_path(&required_flag(
                &mut args,
                "--ffprobe-sealed",
                "AV operation",
            )?)?;
            let ffmpeg = dependency_path(&required_flag(
                &mut args,
                "--ffmpeg-sealed",
                "AV operation",
            )?)?;
            let project =
                PathBuf::from(required_flag(&mut args, "--project-root", "AV operation")?);
            let media = PathBuf::from(required_flag(&mut args, "--media-root", "AV operation")?);
            let output = PathBuf::from(required_flag(&mut args, "--output-root", "AV operation")?);
            if args.next().is_some() {
                return Err("AV operation has unexpected arguments".into());
            }
            av_operation(
                &runtime_root,
                [&melt, &ffprobe, &ffmpeg],
                [&project, &media, &output],
            )
        }
        "discover" => {
            let runtime_root =
                PathBuf::from(required_flag(&mut args, "--runtime-root", "discover")?);
            let sealed_melt =
                dependency_path(&required_flag(&mut args, "--melt-sealed", "discover")?)?;
            if args.next().is_some() {
                return Err("runtime runner received unexpected arguments".into());
            }
            let melt = runtime_entry(&runtime_root, &sealed_melt, "melt")?;
            discover(&melt)
        }
        "probe" => {
            let runtime_root = PathBuf::from(required_flag(&mut args, "--runtime-root", "probe")?);
            let sealed_ffprobe =
                dependency_path(&required_flag(&mut args, "--ffprobe-sealed", "probe")?)?;
            let scratch_root = PathBuf::from(required_flag(&mut args, "--scratch-root", "probe")?);
            let directory = required_flag(&mut args, "--directory", "probe")?;
            let name = required_flag(&mut args, "--name", "probe")?;
            let next = args.next();
            let count_video_frames = next.as_deref() == Some("--count-video-frames");
            let first_original = if count_video_frames {
                args.next()
            } else {
                next
            };
            let originals = original_workspace_flags(first_original, &mut args, true)?;
            let ffprobe = runtime_entry(&runtime_root, &sealed_ffprobe, "ffprobe")?;
            probe(
                &ffprobe,
                &scratch_root,
                &directory,
                &name,
                count_video_frames,
                originals.as_ref(),
            )
        }
        "render" => {
            let runtime_root = PathBuf::from(required_flag(&mut args, "--runtime-root", "render")?);
            let sealed_melt =
                dependency_path(&required_flag(&mut args, "--melt-sealed", "render")?)?;
            let scratch_root = PathBuf::from(required_flag(&mut args, "--scratch-root", "render")?);
            let directory = required_flag(&mut args, "--directory", "render")?;
            let profile = required_flag(&mut args, "--profile", "render")?;
            let mut originals = None;
            let observation_runtime = match args.next().as_deref() {
                None => None,
                Some("--complete-observation") => {
                    if profile != "lossless-video-only" {
                        return Err("Complete observation profile is not selected".into());
                    }
                    let ffprobe = dependency_path(&required_flag(
                        &mut args,
                        "--ffprobe-sealed",
                        "render observation",
                    )?)?;
                    let ffmpeg = dependency_path(&required_flag(
                        &mut args,
                        "--ffmpeg-sealed",
                        "render observation",
                    )?)?;
                    originals = original_workspace_flags(args.next(), &mut args, false)?;
                    Some(host_runtime(
                        &runtime_root,
                        [&sealed_melt, &ffprobe, &ffmpeg],
                    )?)
                }
                Some(_) => return Err("runtime runner received unexpected arguments".into()),
            };
            let melt = if let Some(runtime) = &observation_runtime {
                // Execute exactly the sealed ToolPath identity checked above.
                runtime.melt.path.clone()
            } else {
                runtime_entry(&runtime_root, &sealed_melt, "melt")?
            };
            render(
                &melt,
                &scratch_root,
                &directory,
                &profile,
                observation_runtime.as_ref(),
                originals.as_ref(),
            )
        }
        _ => Err("runtime operation is unsupported".into()),
    }
}

fn main() {
    let value = match parse() {
        Ok(value) => value,
        Err(error) => {
            let message: String = error
                .chars()
                .filter(|ch| !ch.is_control())
                .take(1024)
                .collect();
            let value = json!({
                "schema": 1,
                "operation": "error",
                "error": message
            });
            match serde_json::to_string(&value) {
                Ok(encoded) if encoded.len() <= 4096 => print!("{encoded}"),
                _ => {
                    print!(r#"{{"schema":1,"operation":"error","error":"runtime runner failed"}}"#)
                }
            }
            std::process::exit(2);
        }
    };
    match serde_json::to_string(&value) {
        Ok(encoded) if encoded.len() <= 256 * 1024 => print!("{encoded}"),
        _ => {
            print!(
                r#"{{"schema":1,"operation":"error","error":"runtime runner output exceeded its bound"}}"#
            );
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod owned_original_probe_tests {
    use super::*;

    fn work(slots: usize) -> Vec<Value> {
        (0..slots)
            .flat_map(|slot| {
                [
            ("media-probe.stdout.original.bin", 16_384),
            ("media-probe.stderr.original.bin", 16_384),
            ("media-probe.terminal.original.json", 65_536),
            ("process-custody.original.json", 65_536),
        ].into_iter().map(move |(tail, cap)| json!({
            "basename":format!("native-input-probe-{slot:06}-{tail}"),"maximum_bytes":cap
        }))
            })
            .collect()
    }

    #[test]
    fn visual_probe_slots_bind_actual_declared_order_and_final_full_frame_probe() {
        let names = (0..3)
            .map(|slot| format!("asset-{}.bin", slot.to_string().repeat(24)))
            .collect::<Vec<_>>();
        let mut inputs = names
            .iter()
            .map(|name| json!({"basename":name,"maximum_bytes":268_435_456}))
            .collect::<Vec<_>>();
        inputs.extend([
            json!({"basename":"project.mlt","maximum_bytes":8_388_608}),
            json!({"basename":"native-observation-request.json","maximum_bytes":1_048_576}),
        ]);
        let mut receipt = json!({"original_kind":"host-native-render","input_members":inputs,"work_members":work(4)});
        assert!(original_probe_role(&receipt, &names[0], false, 0).is_ok());
        assert!(original_probe_role(&receipt, &names[0], false, 1).is_err());
        assert!(original_probe_role(&receipt, "partial.mkv", true, 3).is_ok());
        assert!(original_probe_role(&receipt, "partial.mkv", true, 2).is_err());
        receipt["work_members"].as_array_mut().unwrap().pop();
        assert!(original_probe_role(&receipt, "partial.mkv", true, 3).is_err());
    }

    #[test]
    fn import_probe_keeps_max_plus_one_input_and_is_not_a_visual_final_probe() {
        let mut receipt = json!({"original_kind":"input-probe","input_members":[
            {"basename":"probe.bin","maximum_bytes":268_435_457},
            {"basename":"import-request.original.json","maximum_bytes":1_048_576}],
            "work_members":work(1)});
        assert!(original_probe_role(&receipt, "probe.bin", false, 0).is_ok());
        assert!(original_probe_role(&receipt, "probe.bin", false, 1).is_err());
        assert!(original_probe_role(&receipt, "probe.bin", true, 0).is_err());
        receipt["input_members"][0]["maximum_bytes"] = json!(268_435_456);
        assert!(original_probe_role(&receipt, "probe.bin", false, 0).is_err());
    }

    #[test]
    fn original_cli_probe_slot_is_canonical_and_has_no_trailing_authority_arguments() {
        let argv = |slot: &str| {
            vec![
                "/workspace/output".to_owned(),
                "--original-output-path".into(),
                "original.mkv".into(),
                "--original-workspaces-sha256".into(),
                "a".repeat(64),
                "--original-probe-slot".into(),
                slot.to_owned(),
            ]
        };
        let mut valid = argv("3").into_iter();
        assert_eq!(
            original_workspace_flags(Some("--original-output-root".into()), &mut valid, true)
                .unwrap()
                .unwrap()
                .probe_slot,
            Some(3)
        );
        let mut noncanonical = argv("03").into_iter();
        assert!(
            original_workspace_flags(
                Some("--original-output-root".into()),
                &mut noncanonical,
                true
            )
            .is_err()
        );
        let mut extra = argv("3");
        extra.extend(["--project-root".into(), "/elsewhere".into()]);
        assert!(
            original_workspace_flags(
                Some("--original-output-root".into()),
                &mut extra.into_iter(),
                true
            )
            .is_err()
        );
    }
}

#[cfg(test)]
mod host_observation_deadline_tests {
    use super::*;
    #[test]
    fn complete_observation_does_not_reset_the_native_deadline() {
        assert!(require_render_deadline(Instant::now() - Duration::from_secs(1)).is_err());
        assert!(require_render_deadline(Instant::now() + Duration::from_secs(60)).is_ok());
    }
}

#[cfg(test)]
mod audio_derivative_runner_tests {
    use super::*;

    #[test]
    fn audio_derivative_uses_the_existing_closed_av_operation_selection() {
        assert!(allowed_av_operation("driver.mlt-video.audio.derive"));
        for command in [
            "driver.mlt-video.frames.encode",
            "driver.mlt-video.av.mux",
            "driver.mlt-video.av.caption_pixels.verify",
            "driver.mlt-video.sync.probe",
        ] {
            assert!(allowed_av_operation(command));
        }
    }

    #[test]
    fn audio_selection_never_accepts_a_shell_alias_or_caller_named_operation() {
        for command in [
            "audio.derive",
            "driver.mlt-video.audio.convert",
            "driver.mlt-video.audio.derive --filter x",
            "driver.mlt-video.audio.derive\n",
            "driver.audio-analysis.voice.align",
            "system.exec",
        ] {
            assert!(!allowed_av_operation(command), "{command:?}");
        }
    }
}
