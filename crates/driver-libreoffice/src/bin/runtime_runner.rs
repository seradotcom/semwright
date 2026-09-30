//! Host-owned persistent LibreOffice runtime session.
//! The driver sends bounded typed JSON frames; this runner owns soffice + pyuno.

#[cfg(target_os = "linux")]
mod linux {
    use semwright_types::unique_id;
    use serde_json::{Map, Value, json};
    use std::{
        fs::{self, OpenOptions},
        io::{self, Read, Write},
        os::unix::{
            fs::{OpenOptionsExt, PermissionsExt},
            process::CommandExt,
        },
        path::{Component, Path, PathBuf},
        process::{Child, Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    const MAX_FRAME: usize = 256 * 1024;
    const MAX_TOOL_BYTES: u64 = 64 * 1024 * 1024;
    const STARTUP_TIMEOUT: Duration = Duration::from_secs(20);
    const STARTUP_DELAY: Duration = Duration::from_millis(50);
    const UNO_SCRIPT: &str = include_str!("../uno_bridge.py");

    #[derive(Debug)]
    struct Cli {
        workspace: PathBuf,
        runtime: PathBuf,
        soffice_sealed: PathBuf,
        python: PathBuf,
    }

    fn clean_message(value: impl ToString) -> String {
        value
            .to_string()
            .chars()
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .take(512)
            .collect()
    }

    fn error_frame(code: &str, message: impl ToString) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "ok": false,
            "code": code,
            "message": clean_message(message),
        }))
        .unwrap_or_else(|_| br#"{"ok":false,"code":"BackendFailed"}"#.to_vec())
    }

    fn required(args: &mut impl Iterator<Item = String>, flag: &str) -> io::Result<PathBuf> {
        let seen = args.next().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, format!("missing {flag}"))
        })?;
        if seen != flag {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("expected {flag}"),
            ));
        }
        let value = args.next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("missing value for {flag}"),
            )
        })?;
        let path = PathBuf::from(value);
        if !path.is_absolute() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{flag} must be absolute"),
            ));
        }
        Ok(path)
    }

    fn parse_cli() -> io::Result<Cli> {
        let mut args = std::env::args().skip(1);
        let parsed = Cli {
            workspace: required(&mut args, "--workspace")?,
            runtime: required(&mut args, "--runtime")?,
            soffice_sealed: required(&mut args, "--soffice-sealed")?,
            python: required(&mut args, "--python")?,
        };
        if args.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unexpected LibreOffice runner argument",
            ));
        }
        Ok(parsed)
    }

    fn canonical_dir(path: &Path, label: &str) -> io::Result<PathBuf> {
        let canonical = fs::canonicalize(path)?;
        let metadata = fs::symlink_metadata(&canonical)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{label} must be a real directory"),
            ));
        }
        Ok(canonical)
    }

    fn canonical_file(path: &Path, label: &str) -> io::Result<PathBuf> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{label} must be a regular non-symlink file"),
            ));
        }
        fs::canonicalize(path)
    }

    fn same_bounded_bytes(left: &Path, right: &Path) -> io::Result<bool> {
        let left_meta = fs::metadata(left)?;
        let right_meta = fs::metadata(right)?;
        if left_meta.len() == 0
            || left_meta.len() != right_meta.len()
            || left_meta.len() > MAX_TOOL_BYTES
        {
            return Ok(false);
        }
        let mut left_file = fs::File::open(left)?;
        let mut right_file = fs::File::open(right)?;
        let mut left_buf = [0u8; 64 * 1024];
        let mut right_buf = [0u8; 64 * 1024];
        loop {
            let l = left_file.read(&mut left_buf)?;
            let r = right_file.read(&mut right_buf)?;
            if l != r || left_buf[..l] != right_buf[..r] {
                return Ok(false);
            }
            if l == 0 {
                return Ok(true);
            }
        }
    }

    fn runtime_soffice(runtime: &Path, sealed: &Path) -> io::Result<PathBuf> {
        let runtime = canonical_dir(runtime, "LibreOffice runtime")?;
        let candidate = fs::canonicalize(runtime.join("program/soffice.bin"))?;
        if candidate.strip_prefix(&runtime).is_err() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "soffice.bin escaped the delegated runtime root",
            ));
        }
        let meta = fs::symlink_metadata(&candidate)?;
        if !meta.is_file() || meta.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "canonical soffice.bin must be a regular file",
            ));
        }
        if !same_bounded_bytes(&candidate, sealed)? {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "runtime soffice.bin does not match the Host-sealed executable",
            ));
        }
        Ok(candidate)
    }

    fn relative_parts(raw: &str) -> io::Result<Vec<&str>> {
        let path = Path::new(raw);
        if raw.is_empty()
            || raw.len() > 240
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "document path is not a canonical relative path",
            ));
        }
        path.iter()
            .map(|part| {
                part.to_str()
                    .filter(|value| {
                        !value.is_empty()
                            && value.len() <= 128
                            && !value.chars().any(char::is_control)
                    })
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "document path must be bounded UTF-8",
                        )
                    })
            })
            .collect()
    }

    fn workspace_path(root: &Path, raw: &str) -> io::Result<String> {
        let root = canonical_dir(root, "workspace")?;
        let parts = relative_parts(raw)?;
        let mut candidate = root.clone();
        for part in parts {
            candidate.push(part);
        }
        let parent = candidate.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "document path has no parent")
        })?;
        let parent = fs::canonicalize(parent)?;
        if parent.strip_prefix(&root).is_err() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "document path escaped the delegated workspace",
            ));
        }
        if candidate.exists() {
            let meta = fs::symlink_metadata(&candidate)?;
            if meta.file_type().is_symlink() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "document path may not be a symlink",
                ));
            }
            let canonical = fs::canonicalize(&candidate)?;
            if canonical.strip_prefix(&root).is_err() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "document path escaped the delegated workspace",
                ));
            }
        }
        candidate.to_str().map(ToOwned::to_owned).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "document path is not UTF-8")
        })
    }

    fn map_request(workspace: &Path, payload: &[u8]) -> io::Result<(String, Value)> {
        let value: Value = serde_json::from_slice(payload)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid session JSON"))?;
        let object = value.as_object().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "session request must be an object",
            )
        })?;
        if object.len() != 2 || !object.contains_key("operation") || !object.contains_key("args") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session request envelope is invalid",
            ));
        }
        let operation = object
            .get("operation")
            .and_then(Value::as_str)
            .filter(|operation| {
                matches!(
                    *operation,
                    "status"
                        | "writer_create"
                        | "writer_read"
                        | "calc_create"
                        | "calc_get"
                        | "calc_set"
                        | "export_pdf"
                )
            })
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "unsupported UNO operation")
            })?
            .to_owned();
        let args = object
            .get("args")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "UNO args must be an object")
            })?;
        let mut mapped: Map<String, Value> = args.clone();
        for key in ["path", "output"] {
            if let Some(raw) = mapped.get(key).and_then(Value::as_str) {
                mapped.insert(key.into(), Value::String(workspace_path(workspace, raw)?));
            }
        }
        Ok((operation, Value::Object(mapped)))
    }

    struct OfficeChild {
        child: Child,
        pgid: i32,
        session: PathBuf,
    }

    impl OfficeChild {
        fn kill_and_wait(&mut self) {
            // SAFETY: pgid belongs to the process group created for this owned soffice child.
            let _ = unsafe { libc::killpg(self.pgid, libc::SIGKILL) };
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    impl Drop for OfficeChild {
        fn drop(&mut self) {
            self.kill_and_wait();
            let _ = fs::remove_dir_all(&self.session);
        }
    }

    fn create_private_dir(path: &Path) -> io::Result<()> {
        fs::create_dir(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
    }

    fn session_environment(command: &mut Command, session: &Path) {
        command
            .env("HOME", session.join("home"))
            .env("XDG_CACHE_HOME", session.join("cache"))
            .env("XDG_CONFIG_HOME", session.join("config"))
            .env("XDG_DATA_HOME", session.join("data"))
            .env("XDG_RUNTIME_DIR", session.join("run"))
            .env("TMPDIR", session);
    }

    fn spawn_office(runtime: &Path, soffice: &Path) -> io::Result<(OfficeChild, String)> {
        if !Path::new("/etc/libreoffice").is_dir() || !Path::new("/etc/fonts").is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "required per-tool LibreOffice system config is absent",
            ));
        }
        let session = PathBuf::from("/tmp").join(format!("semwright-lo-{}", unique_id()));
        create_private_dir(&session)?;
        for relative in ["home", "cache", "config", "data", "run", "profile"] {
            create_private_dir(&session.join(relative))?;
        }
        let profile = session.join("profile");
        let log = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(session.join("soffice.log"))?;
        let log_err = log.try_clone()?;
        let token = unique_id().replace('-', "_");
        let accept = format!("--accept=pipe,name={token};urp;StarOffice.ComponentContext");
        let profile_arg = format!("-env:UserInstallation=file://{}", profile.display());
        let program = runtime.join("program");

        let mut command = Command::new(soffice);
        command
            .args([
                "--headless",
                "--nologo",
                "--nodefault",
                "--nofirststartwizard",
                "--norestore",
                &profile_arg,
                &accept,
            ])
            .current_dir(&program)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .env("LD_LIBRARY_PATH", &program)
            .env(
                "URE_BOOTSTRAP",
                format!(
                    "vnd.sun.star.pathname:{}/fundamentalrc",
                    program.to_string_lossy()
                ),
            )
            .env("UNO_PATH", &program)
            .env("SAL_USE_VCLPLUGIN", "svp")
            .env("FONTCONFIG_PATH", "/etc/fonts")
            .env("FONTCONFIG_FILE", "/etc/fonts/fonts.conf")
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));
        session_environment(&mut command, &session);
        command.process_group(0);
        // SAFETY: pre_exec runs after fork; prctl uses scalar arguments only.
        unsafe {
            command.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        let pid = child.id();
        Ok((
            OfficeChild {
                child,
                pgid: pid as i32,
                session,
            },
            token,
        ))
    }

    fn python_env(command: &mut Command, runtime: &Path, session: &Path) {
        let program = runtime.join("program");
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .env("PYTHONNOUSERSITE", "1")
            .env("LD_LIBRARY_PATH", &program)
            .env(
                "URE_BOOTSTRAP",
                format!(
                    "vnd.sun.star.pathname:{}/fundamentalrc",
                    program.to_string_lossy()
                ),
            )
            .env("UNO_PATH", &program)
            .env("SEMWRIGHT_LIBREOFFICE_RUNTIME", runtime)
            .env("FONTCONFIG_PATH", "/etc/fonts")
            .env("FONTCONFIG_FILE", "/etc/fonts/fonts.conf");
        session_environment(command, session);
    }

    fn run_python(
        python: &Path,
        runtime: &Path,
        session: &Path,
        pipe: &str,
        operation: &str,
        args: &Value,
    ) -> io::Result<Vec<u8>> {
        let payload = serde_json::to_string(args)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "UNO args are not JSON"))?;
        if payload.len() > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "UNO args exceed the session frame bound",
            ));
        }
        let mut command = Command::new(python);
        command
            .arg("-c")
            .arg(UNO_SCRIPT)
            .arg(pipe)
            .arg(operation)
            .arg(payload)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        python_env(&mut command, runtime, session);
        // Kill an in-flight pyuno helper if the Host kills the persistent runner.
        // SAFETY: pre_exec runs after fork; prctl uses scalar arguments only.
        unsafe {
            command.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let output = command.output()?;
        if !output.status.success() || output.stdout.is_empty() || output.stdout.len() > MAX_FRAME {
            return Ok(error_frame(
                "BackendFailed",
                "LibreOffice UNO helper failed",
            ));
        }
        let value: Value = serde_json::from_slice(&output.stdout).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "UNO helper returned invalid JSON",
            )
        })?;
        if !value.is_object() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "UNO helper response must be an object",
            ));
        }
        Ok(output.stdout)
    }

    fn startup_log(session: &Path) -> String {
        let Ok(bytes) = fs::read(session.join("soffice.log")) else {
            return String::new();
        };
        let start = bytes.len().saturating_sub(4096);
        String::from_utf8_lossy(&bytes[start..])
            .chars()
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .take(4096)
            .collect()
    }

    fn wait_until_ready(
        office: &mut OfficeChild,
        python: &Path,
        runtime: &Path,
        pipe: &str,
    ) -> io::Result<()> {
        let deadline = Instant::now() + STARTUP_TIMEOUT;
        loop {
            if let Some(status) = office.child.try_wait()? {
                let log = startup_log(&office.session);
                return Err(io::Error::other(if log.is_empty() {
                    format!("LibreOffice exited during session startup: {status}")
                } else {
                    format!("LibreOffice exited during session startup: {status}; log={log}")
                }));
            }
            if let Ok(response) =
                run_python(python, runtime, &office.session, pipe, "status", &json!({}))
                && serde_json::from_slice::<Value>(&response)
                    .ok()
                    .and_then(|value| value.get("ok").and_then(Value::as_bool))
                    == Some(true)
            {
                return Ok(());
            }
            if Instant::now() >= deadline {
                let log = startup_log(&office.session);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    if log.is_empty() {
                        "LibreOffice session startup timed out".to_owned()
                    } else {
                        format!("LibreOffice session startup timed out; log={log}")
                    },
                ));
            }
            thread::sleep(STARTUP_DELAY);
        }
    }

    fn read_frame(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
        let mut header = [0u8; 4];
        let mut read = 0usize;
        while read < header.len() {
            let count = input.read(&mut header[read..])?;
            if count == 0 {
                if read == 0 {
                    return Ok(None);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "truncated Host session frame",
                ));
            }
            read += count;
        }
        let length = u32::from_be_bytes(header) as usize;
        if length == 0 || length > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Host session frame exceeds bound",
            ));
        }
        let mut body = vec![0u8; length];
        input.read_exact(&mut body)?;
        Ok(Some(body))
    }

    fn write_frame(output: &mut impl Write, payload: &[u8]) -> io::Result<()> {
        if payload.is_empty() || payload.len() > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Host session response exceeds bound",
            ));
        }
        output.write_all(&(payload.len() as u32).to_be_bytes())?;
        output.write_all(payload)?;
        output.flush()
    }

    pub fn run() -> io::Result<()> {
        let cli = parse_cli()?;
        let workspace = canonical_dir(&cli.workspace, "workspace")?;
        let runtime = canonical_dir(&cli.runtime, "LibreOffice runtime")?;
        let python = canonical_file(&cli.python, "Python dependency")?;
        let sealed = canonical_file(&cli.soffice_sealed, "sealed soffice dependency")?;
        let soffice = runtime_soffice(&runtime, &sealed)?;

        let (mut office, pipe) = spawn_office(&runtime, &soffice)?;
        wait_until_ready(&mut office, &python, &runtime, &pipe)?;

        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut input = stdin.lock();
        let mut output = stdout.lock();
        while let Some(frame) = read_frame(&mut input)? {
            let response = match map_request(&workspace, &frame) {
                Ok((operation, args)) => {
                    run_python(&python, &runtime, &office.session, &pipe, &operation, &args)
                        .unwrap_or_else(|error| error_frame("BackendFailed", error))
                }
                Err(error) => error_frame("InvalidArgument", error),
            };
            write_frame(&mut output, &response)?;
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn main() {
    if let Err(error) = linux::run() {
        let message: String = error
            .to_string()
            .chars()
            .map(|ch| if ch.is_control() { ' ' } else { ch })
            .take(4096)
            .collect();
        eprintln!("LibreOffice runtime runner failed: {message}");
        std::process::exit(2);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("LibreOffice Host runtime runner is currently available on Linux only");
    std::process::exit(2);
}
