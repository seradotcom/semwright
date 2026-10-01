#[cfg(target_os = "linux")]
mod linux {
    use semwright_types::unique_id;
    use std::{
        fs::{self, OpenOptions},
        io::{self, Read, Write},
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
    const BRIDGE_PY: &str = include_str!("../bridge.py");
    const SEMANTIC_PY: &str = include_str!("../semantic.py");
    const COMMANDS_PY: &str =
        include_str!("../../../../adapters/blender/semwright_blender/commands.py");
    const COMMANDS_JSON: &str =
        include_str!("../../../../adapters/blender/semwright_blender/commands.json");
    const VALIDATION_PY: &str =
        include_str!("../../../../adapters/blender/semwright_blender/validation.py");

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
            .write(true)
            .mode(0o600)
            .open(&log_path)?;
        let log_err = log.try_clone()?;
        let version_root = args.runtime.join("4.5");
        let user = session.join("user");

        let mut command = Command::new(&args.blender);
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
        command.process_group(0);

        // Kill Blender if the v8 session runner is terminated by the Host pidfd path.
        // SAFETY: pre_exec executes after fork and before exec; prctl uses scalar arguments only.
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
        Ok(BlenderChild {
            child,
            pgid: pid as i32,
            session_dir: session.to_path_buf(),
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

    pub fn run() -> io::Result<()> {
        let args = parse_args()?;
        let (session, bridge, socket) = stage_runtime(&args.scratch)?;
        let mut blender = spawn_blender(&args, &session, &bridge, &socket)?;
        wait_until_ready(&mut blender, &socket)?;

        let stdin = io::stdin();
        let stdout = io::stdout();
        let mut input = stdin.lock();
        let mut output = stdout.lock();
        while let Some(request) = read_host_frame(&mut input)? {
            let response = send_socket_frame(&socket, &request)?;
            write_host_frame(&mut output, &response)?;
        }
        Ok(())
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
