#[cfg(target_os = "linux")]
#[path = "session_runner/native_failure.rs"]
mod native_failure;

#[cfg(target_os = "linux")]
mod linux {
    use semwright_types::{NativeFailurePhase, unique_id};
    use std::{
        fs::{self, File, OpenOptions},
        io::{self, Read, Write},
        os::fd::{AsRawFd, FromRawFd},
        os::unix::{
            fs::{OpenOptionsExt, PermissionsExt},
            net::UnixStream,
            process::CommandExt,
        },
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        thread,
        time::{Duration, Instant},
    };

    const MAX_FRAME: usize = 1_048_576;
    const NVIDIA_POLICY_FD_ENV: &str = "SEMWRIGHT_INTERNAL_NVIDIA_POLICY_FD";
    const NVIDIA_NATIVE_HELPER: &str = "/plugin/gpu-native-helper";
    use semwright_driver_sdk::NVIDIA_BLENDER_EXECUTABLE as NVIDIA_NATIVE_EXECUTABLE;

    fn take_nvidia_policy_fd(
        grant: Option<&str>,
        marker: Option<&str>,
    ) -> io::Result<Option<File>> {
        match (grant, marker) {
            (None, None) => Ok(None),
            (Some("1"), Some(marker)) => {
                let fd = marker
                    .parse::<i32>()
                    .ok()
                    .filter(|fd| *fd >= 3)
                    .ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "invalid NVIDIA policy descriptor",
                        )
                    })?;
                let required = libc::F_SEAL_WRITE
                    | libc::F_SEAL_GROW
                    | libc::F_SEAL_SHRINK
                    | libc::F_SEAL_SEAL;
                // SAFETY: F_GET_SEALS only reads scalar descriptor state.
                let seals = unsafe { libc::fcntl(fd, libc::F_GET_SEALS) };
                if seals < 0 || seals & required != required {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "NVIDIA policy descriptor must be immutable",
                    ));
                }
                // SAFETY: the trusted initial helper transferred ownership of
                // this descriptor; it is consumed exactly once by this runner.
                let file = unsafe { File::from_raw_fd(fd) };
                let metadata = file.metadata()?;
                if !metadata.is_file() || metadata.len() == 0 || metadata.len() > 65_536 {
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "NVIDIA policy descriptor exceeds bounds",
                    ));
                }
                // Keep the plan private until the intended helper child only.
                // SAFETY: F_SETFD acts on this owned descriptor only.
                if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } != 0 {
                    return Err(io::Error::last_os_error());
                }
                Ok(Some(file))
            }
            _ => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "NVIDIA handoff requires both the compute grant and sealed policy",
            )),
        }
    }
    const BRIDGE_PY: &str = include_str!("../bridge.py");
    const AUTHORING_PY: &str = include_str!("../authoring_native.py");
    const EXPORT_SCOPE_PY: &str =
        include_str!("../../../../adapters/blender/semwright_blender/export_scope.py");
    const SEMANTIC_PY: &str = include_str!("../semantic.py");
    const COMMANDS_PY: &str =
        include_str!("../../../../adapters/blender/semwright_blender/commands.py");
    const COMMANDS_JSON: &str =
        include_str!("../../../../adapters/blender/semwright_blender/commands.json");
    const VALIDATION_PY: &str =
        include_str!("../../../../adapters/blender/semwright_blender/validation.py");

    fn configure_gpu_environment(
        command: &mut Command,
        session: &Path,
        grant: Option<&str>,
    ) -> io::Result<()> {
        match grant {
            None => Ok(()),
            Some("1") => {
                let cache = session.join("gpu-cache");
                fs::create_dir(&cache)?;
                fs::set_permissions(&cache, fs::Permissions::from_mode(0o700))?;
                command
                    .env("SEMWRIGHT_NVIDIA_GPU", "1")
                    .env("XDG_CACHE_HOME", &cache)
                    .env("CUDA_CACHE_PATH", cache.join("cuda"))
                    .env("OPTIX_CACHE_PATH", cache.join("optix"));
                Ok(())
            }
            Some(_) => Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "NVIDIA compute grant marker is invalid",
            )),
        }
    }

    struct Args {
        blender: PathBuf,
        workspace: PathBuf,
        runtime: PathBuf,
        scratch: PathBuf,
        fontconfig: PathBuf,
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

    fn parse_args() -> io::Result<Args> {
        let mut args = std::env::args().skip(1);
        let parsed = Args {
            blender: required(&mut args, "--blender")?,
            workspace: required(&mut args, "--workspace")?,
            runtime: required(&mut args, "--runtime")?,
            scratch: required(&mut args, "--scratch")?,
            fontconfig: required(&mut args, "--fontconfig")?,
        };
        if args.next().is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unexpected session runner argument",
            ));
        }
        Ok(parsed)
    }

    fn ensure_file(path: &Path, label: &str) -> io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{label} must be a regular non-symlink file"),
            ));
        }
        Ok(())
    }

    fn ensure_dir(path: &Path, label: &str) -> io::Result<()> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{label} must be a non-symlink directory"),
            ));
        }
        Ok(())
    }

    fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true).mode(0o600);
        let mut file = options.open(path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o400))?;
        Ok(())
    }

    fn stage_runtime(scratch: &Path) -> io::Result<(PathBuf, PathBuf, PathBuf)> {
        ensure_dir(scratch, "scratch root")?;
        let session = scratch.join(format!("blender-session-{}", unique_id()));
        fs::create_dir(&session)?;
        fs::set_permissions(&session, fs::Permissions::from_mode(0o700))?;
        let package = session.join("semwright_blender_runtime");
        fs::create_dir(&package)?;
        fs::set_permissions(&package, fs::Permissions::from_mode(0o700))?;
        write_private(&package.join("__init__.py"), b"")?;
        write_private(&package.join("commands.py"), COMMANDS_PY.as_bytes())?;
        write_private(&package.join("commands.json"), COMMANDS_JSON.as_bytes())?;
        write_private(&package.join("validation.py"), VALIDATION_PY.as_bytes())?;
        write_private(&package.join("semantic.py"), SEMANTIC_PY.as_bytes())?;
        write_private(
            &package.join("authoring_native.py"),
            AUTHORING_PY.as_bytes(),
        )?;
        write_private(&package.join("export_scope.py"), EXPORT_SCOPE_PY.as_bytes())?;
        let bridge = session.join("bridge.py");
        write_private(&bridge, BRIDGE_PY.as_bytes())?;
        let user = session.join("user");
        fs::create_dir(&user)?;
        fs::set_permissions(&user, fs::Permissions::from_mode(0o700))?;
        Ok((session.clone(), bridge, session.join("bridge.sock")))
    }

    struct BlenderChild {
        child: Child,
        pgid: i32,
        session_dir: PathBuf,
        native_log: File,
        workspace: PathBuf,
    }

    impl BlenderChild {
        fn kill_and_wait(&mut self) {
            // SAFETY: pgid is the process group created for the owned Blender child.
            let _ = unsafe { libc::killpg(self.pgid, libc::SIGKILL) };
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    impl Drop for BlenderChild {
        fn drop(&mut self) {
            self.kill_and_wait();
            let _ = fs::remove_dir_all(&self.session_dir);
        }
    }

    fn spawn_blender(
        args: &Args,
        session: &Path,
        bridge: &Path,
        socket: &Path,
    ) -> io::Result<BlenderChild> {
        ensure_file(&args.blender, "Blender executable")?;
        ensure_dir(&args.workspace, "workspace mount")?;
        ensure_dir(&args.runtime, "Blender runtime mount")?;
        ensure_dir(&args.fontconfig, "font-config mount")?;
        for relative in [
            "lib",
            "4.5/scripts",
            "4.5/extensions",
            "4.5/datafiles",
            "4.5/python",
        ] {
            ensure_dir(&args.runtime.join(relative), "Blender runtime component")?;
        }

        let log_path = session.join("blender.log");
        let log = OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .mode(0o600)
            .open(&log_path)?;
        let log_err = log.try_clone()?;
        let native_log = log.try_clone()?;
        let version_root = args.runtime.join("4.5");
        let user = session.join("user");

        let gpu_grant = std::env::var("SEMWRIGHT_NVIDIA_GPU").ok();
        let policy_marker = std::env::var(NVIDIA_POLICY_FD_ENV).ok();
        let gpu_policy = take_nvidia_policy_fd(gpu_grant.as_deref(), policy_marker.as_deref())?;
        let mut command = if let Some(policy) = &gpu_policy {
            if args.blender != Path::new(NVIDIA_NATIVE_EXECUTABLE) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "NVIDIA native executable must be sealed Blender",
                ));
            }
            ensure_file(
                Path::new(NVIDIA_NATIVE_HELPER),
                "NVIDIA native policy helper",
            )?;
            let mut command = Command::new(NVIDIA_NATIVE_HELPER);
            command
                .args(["--nvidia-native-child", "--policy-fd"])
                .arg(policy.as_raw_fd().to_string())
                .args(["--", NVIDIA_NATIVE_EXECUTABLE]);
            command
        } else {
            Command::new(&args.blender)
        };
        command
            .args([
                "--background",
                "--factory-startup",
                "--disable-autoexec",
                "--offline-mode",
                "-q",
                "--log-file",
            ])
            .arg(&log_path)
            .args(["--python-exit-code", "1", "--python"])
            .arg(bridge)
            .arg("--")
            .arg(socket)
            .arg(&args.workspace)
            .arg(session)
            .current_dir(session)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", "/home")
            .env("LANG", "C.UTF-8")
            .env("MALLOC_CONF", "narenas:4,retain:false")
            .env("LD_LIBRARY_PATH", args.runtime.join("lib"))
            .env("BLENDER_SYSTEM_RESOURCES", &version_root)
            .env("BLENDER_SYSTEM_SCRIPTS", version_root.join("scripts"))
            .env("BLENDER_SYSTEM_EXTENSIONS", version_root.join("extensions"))
            .env("BLENDER_SYSTEM_DATAFILES", version_root.join("datafiles"))
            .env("BLENDER_SYSTEM_PYTHON", version_root.join("python"))
            .env("PYTHONNOUSERSITE", "1")
            .env("BLENDER_USER_RESOURCES", &user)
            .env("TMPDIR", session)
            .env("FONTCONFIG_PATH", &args.fontconfig)
            .env("FONTCONFIG_FILE", args.fontconfig.join("fonts.conf"))
            .stdin(Stdio::null())
            .stdout(Stdio::from(log))
            .stderr(Stdio::from(log_err));
        configure_gpu_environment(&mut command, session, gpu_grant.as_deref())?;
        command.process_group(0);
        let policy_fd = gpu_policy.as_ref().map(AsRawFd::as_raw_fd);

        // Kill Blender if the v8 session runner is terminated by the Host pidfd path.
        // SAFETY: pre_exec executes after fork and before exec; prctl uses scalar arguments only.
        unsafe {
            command.pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(io::Error::last_os_error());
                }
                if let Some(fd) = policy_fd {
                    // Only this fixed native-child helper inherits the immutable
                    // plan. It closes the descriptor before loading Blender.
                    if libc::fcntl(fd, libc::F_SETFD, 0) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }

        let child = command.spawn()?;
        let pid = child.id();
        Ok(BlenderChild {
            child,
            pgid: pid as i32,
            session_dir: session.to_path_buf(),
            native_log,
            workspace: args.workspace.clone(),
        })
    }

    fn send_socket_frame(socket: &Path, payload: &[u8]) -> io::Result<Vec<u8>> {
        if payload.is_empty() || payload.len() > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "session request frame is out of bounds",
            ));
        }
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(Duration::from_secs(300)))?;
        stream.set_write_timeout(Some(Duration::from_secs(30)))?;
        stream.write_all(&(payload.len() as u32).to_be_bytes())?;
        stream.write_all(payload)?;
        stream.flush()?;
        let mut header = [0u8; 4];
        stream.read_exact(&mut header)?;
        let length = u32::from_be_bytes(header) as usize;
        if length == 0 || length > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Blender response frame is out of bounds",
            ));
        }
        let mut body = vec![0u8; length];
        stream.read_exact(&mut body)?;
        Ok(body)
    }

    fn wait_until_ready(child: &mut BlenderChild, socket: &Path) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(20);
        let probe = br#"{"command":"driver.blender.introspect.summary","args":{}}"#;
        loop {
            if let Some(status) = child.child.try_wait()? {
                return Err(io::Error::other(format!(
                    "Blender exited during session startup: {status}"
                )));
            }
            if socket.exists() && send_socket_frame(socket, probe).is_ok() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Blender session startup timed out",
                ));
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn read_host_frame(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
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
                "Host session frame is out of bounds",
            ));
        }
        let mut body = vec![0u8; length];
        input.read_exact(&mut body)?;
        Ok(Some(body))
    }

    fn write_host_frame(output: &mut impl Write, payload: &[u8]) -> io::Result<()> {
        if payload.is_empty() || payload.len() > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Host response frame is out of bounds",
            ));
        }
        output.write_all(&(payload.len() as u32).to_be_bytes())?;
        output.write_all(payload)?;
        output.flush()
    }

    fn failure_response(
        child: &mut BlenderChild,
        phase: NativeFailurePhase,
        error: &io::Error,
    ) -> Vec<u8> {
        let status = child.child.try_wait().ok().flatten();
        let diagnostic = super::native_failure::capture(
            &child.native_log,
            &child.workspace,
            status,
            phase,
            error,
        );
        let value = match diagnostic {
            Ok(diagnostic) => serde_json::json!({"ok":false,"error":{
                "code":"NativeTransportFailed","diagnostic":diagnostic}}),
            Err(_) => serde_json::json!({"ok":false,"error":{"code":"NativeTransportFailed"}}),
        };
        // A fixed small envelope: log text/paths never enter the public channel.
        serde_json::to_vec(&value).expect("native diagnostic serializes")
    }

    pub fn run() -> io::Result<()> {
        let args = parse_args()?;
        let (session, bridge, socket) = stage_runtime(&args.scratch)?;
        let mut blender = spawn_blender(&args, &session, &bridge, &socket)?;
        if let Err(error) = wait_until_ready(&mut blender, &socket) {
            let response = failure_response(&mut blender, NativeFailurePhase::Startup, &error);
            let _ = write_host_frame(&mut io::stdout().lock(), &response);
            return Err(error);
        }

        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut input = stdin.lock();
        let mut output = stdout.lock();
        loop {
            let request = match read_host_frame(&mut input) {
                Ok(Some(request)) => request,
                Ok(None) => break,
                Err(error) => {
                    let response =
                        failure_response(&mut blender, NativeFailurePhase::HostRead, &error);
                    let _ = write_host_frame(&mut output, &response);
                    return Err(error);
                }
            };
            let response = match send_socket_frame(&socket, &request) {
                Ok(response) => response,
                Err(error) => {
                    let response =
                        failure_response(&mut blender, NativeFailurePhase::NativeExchange, &error);
                    let _ = write_host_frame(&mut output, &response);
                    return Err(error);
                }
            };
            if let Err(error) = write_host_frame(&mut output, &response) {
                let _ = failure_response(&mut blender, NativeFailurePhase::HostWrite, &error);
                return Err(error);
            }
        }
        Ok(())
    }
    #[cfg(test)]
    mod diagnostic_tests {
        use super::*;
        use std::os::unix::fs::{MetadataExt, symlink};
        #[test]
        fn nvidia_handoff_requires_both_markers_and_immutable_descriptor() {
            assert!(take_nvidia_policy_fd(None, None).unwrap().is_none());
            for (grant, marker) in [
                (Some("1"), None),
                (None, Some("3")),
                (Some("ambient"), Some("3")),
                (Some("1"), Some("0")),
            ] {
                assert!(take_nvidia_policy_fd(grant, marker).is_err());
            }
            // SAFETY: fixed name and scalar flags create an owned test memfd.
            let fd = unsafe {
                libc::memfd_create(
                    c"semwright-runner-policy-test".as_ptr(),
                    libc::MFD_ALLOW_SEALING,
                )
            };
            assert!(fd >= 3);
            // SAFETY: fd is new and uniquely owned by this file.
            let mut file = unsafe { File::from_raw_fd(fd) };
            file.write_all(b"{}").unwrap();
            let marker = fd.to_string();
            assert!(take_nvidia_policy_fd(Some("1"), Some(&marker)).is_err());
            let seals =
                libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
            // SAFETY: scalar fcntl seals the owned fixture and duplicates it for handoff.
            assert_eq!(unsafe { libc::fcntl(fd, libc::F_ADD_SEALS, seals) }, 0);
            // SAFETY: fd is the live owned fixture; duplication returns a new descriptor.
            let handoff = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 3) };
            assert!(handoff >= 3);
            let marker = handoff.to_string();
            let transferred = take_nvidia_policy_fd(Some("1"), Some(&marker))
                .unwrap()
                .unwrap();
            // SAFETY: F_GETFD reads flags from the live transferred descriptor only.
            let flags = unsafe { libc::fcntl(transferred.as_raw_fd(), libc::F_GETFD) };
            assert_ne!(flags & libc::FD_CLOEXEC, 0);
        }

        #[test]
        fn nvidia_gpu_cache_and_marker_are_confined_to_the_private_session() {
            let session = tempfile::tempdir().unwrap();
            let mut cpu = Command::new("/not-executed");
            cpu.env_clear();
            configure_gpu_environment(&mut cpu, session.path(), None).unwrap();
            assert!(!session.path().join("gpu-cache").exists());
            assert_eq!(cpu.get_envs().count(), 0);
            assert!(configure_gpu_environment(&mut cpu, session.path(), Some("ambient")).is_err());
            let mut gpu = Command::new("/not-executed");
            gpu.env_clear();
            configure_gpu_environment(&mut gpu, session.path(), Some("1")).unwrap();
            let cache = session.path().join("gpu-cache");
            assert_eq!(fs::metadata(&cache).unwrap().mode() & 0o777, 0o700);
            let environment = gpu
                .get_envs()
                .map(|(name, value)| {
                    (
                        name.to_str().unwrap(),
                        value.unwrap().to_string_lossy().into_owned(),
                    )
                })
                .collect::<std::collections::BTreeMap<_, _>>();
            assert_eq!(environment["SEMWRIGHT_NVIDIA_GPU"], "1");
            for name in ["XDG_CACHE_HOME", "CUDA_CACHE_PATH", "OPTIX_CACHE_PATH"] {
                assert!(Path::new(&environment[name]).starts_with(&cache));
            }
        }
        fn fake(workspace: &Path, command: &str) -> BlenderChild {
            let session = workspace.join(format!("fixture-session-{}", unique_id()));
            fs::create_dir(&session).unwrap();
            let log = OpenOptions::new()
                .create_new(true)
                .read(true)
                .write(true)
                .mode(0o600)
                .open(session.join("blender.log"))
                .unwrap();
            let child = Command::new("/bin/sh")
                .args(["-c", command])
                .process_group(0)
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(Stdio::from(log.try_clone().unwrap()))
                .spawn()
                .unwrap();
            BlenderChild {
                pgid: child.id() as i32,
                child,
                session_dir: session,
                native_log: log,
                workspace: workspace.to_path_buf(),
            }
        }
        #[test]
        fn child_failure_retains_bounded_tail_after_drop_and_redacts_public_error() {
            let workspace = tempfile::tempdir().unwrap();
            let mut child = fake(
                workspace.path(),
                "printf diagnostic_fixture_allocation_failed >&2; exit 9",
            );
            let status = child.child.wait().unwrap();
            assert_eq!(status.code(), Some(9));
            child.native_log.write_all(&vec![b'x'; 12000]).unwrap();
            child
                .native_log
                .write_all(b"fixture-secret-not-public")
                .unwrap();
            let session = child.session_dir.clone();
            let reply = failure_response(
                &mut child,
                NativeFailurePhase::NativeExchange,
                &io::Error::from(io::ErrorKind::UnexpectedEof),
            );
            let envelope: serde_json::Value = serde_json::from_slice(&reply).unwrap();
            let diagnostic: semwright_types::NativeDiagnostic =
                serde_json::from_value(envelope["error"]["diagnostic"].clone()).unwrap();
            assert_eq!(diagnostic.exit_code, Some(9));
            let encoded = String::from_utf8(reply).unwrap();
            assert!(!encoded.contains("fixture-secret"));
            assert!(!encoded.contains(&workspace.path().display().to_string()));
            assert_eq!(diagnostic.stderr_tail_bytes, 8192);
            assert!(diagnostic.stderr_tail_truncated);
            drop(child);
            assert!(!session.exists());
            let audit = workspace.path().join(".semwright-native-failures");
            assert_eq!(audit.metadata().unwrap().mode() & 0o777, 0o700);
            let log = fs::read_dir(&audit)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.extension().is_some_and(|s| s == "log"))
                .unwrap();
            let bytes = fs::read(&log).unwrap();
            assert_eq!(bytes.len(), 8192);
            assert!(bytes.ends_with(b"fixture-secret-not-public"));
            use sha2::{Digest, Sha256};
            assert_eq!(
                hex::encode(Sha256::digest(&bytes)),
                diagnostic.stderr_tail_sha256
            );
            assert_eq!(log.metadata().unwrap().mode() & 0o777, 0o600);
        }
        #[test]
        fn actual_native_socket_eof_is_uncertain_and_preserved() {
            let workspace = tempfile::tempdir().unwrap();
            let socket = workspace.path().join("fixture.sock");
            let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
            let server = thread::spawn(move || {
                let (mut client, _) = listener.accept().unwrap();
                let mut head = [0; 4];
                client.read_exact(&mut head).unwrap();
                let mut request = vec![0; u32::from_be_bytes(head) as usize];
                client.read_exact(&mut request).unwrap();
                client.write_all(&[0, 0]).unwrap();
            });
            let mut child = fake(workspace.path(), "sleep 30");
            let error = send_socket_frame(&socket, b"fixture").unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::UnexpectedEof);
            let response = failure_response(&mut child, NativeFailurePhase::NativeExchange, &error);
            let envelope: serde_json::Value = serde_json::from_slice(&response).unwrap();
            let diagnostic: semwright_types::NativeDiagnostic =
                serde_json::from_value(envelope["error"]["diagnostic"].clone()).unwrap();
            assert!(matches!(
                diagnostic.reason,
                semwright_types::NativeFailureReason::UnexpectedEof
            ));
            drop(child);
            server.join().unwrap();
            assert!(workspace.path().join(".semwright-native-failures").is_dir());
        }
        #[test]
        fn private_audit_symlink_is_refused_and_no_public_raw_fallback() {
            let workspace = tempfile::tempdir().unwrap();
            let outside = tempfile::tempdir().unwrap();
            symlink(
                outside.path(),
                workspace.path().join(".semwright-native-failures"),
            )
            .unwrap();
            let mut child = fake(workspace.path(), "sleep 30");
            let response = failure_response(
                &mut child,
                NativeFailurePhase::Startup,
                &io::Error::from(io::ErrorKind::TimedOut),
            );
            let envelope: serde_json::Value = serde_json::from_slice(&response).unwrap();
            assert_eq!(envelope["error"].as_object().unwrap().len(), 1);
            assert_eq!(envelope["error"]["code"], "NativeTransportFailed");
            assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
            drop(child);
        }
    }
}

#[cfg(target_os = "linux")]
fn main() {
    if linux::run().is_err() {
        std::process::exit(2);
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("Blender persistent session runner is currently available on Linux only");
    std::process::exit(2);
}
