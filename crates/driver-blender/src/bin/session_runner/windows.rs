//! Windows fixed native session. Host owns token, Job, tool pins and mount grants.
use semwright_driver_blender::runtime_profile::{HostPlatform, RuntimeProfile};
use semwright_types::unique_id;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::windows::{fs::MetadataExt, io::AsRawHandle, process::CommandExt},
    path::{Component, Path, PathBuf, Prefix},
    process::{Child, Command, ExitStatus, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{
        GetTokenInformation, TOKEN_QUERY, TokenAppContainerSid, TokenCapabilities,
        TokenIsAppContainer,
    },
    System::{
        JobObjects::IsProcessInJob,
        Threading::{CREATE_NO_WINDOW, GetCurrentProcess, OpenProcessToken},
    },
};
#[path = "windows_lpac.rs"]
mod lpac;
const MAX_FRAME: usize = 1_048_576;
const MAX_STARTUP: usize = 65_536;
const BRIDGE: &str = include_str!("../../bridge.py");
const AUTHORING: &str = include_str!("../../authoring_native.py");
const SEMANTIC: &str = include_str!("../../semantic.py");
const COMMANDS: &str =
    include_str!("../../../../../adapters/blender/semwright_blender/commands.py");
const COMMANDS_JSON: &str =
    include_str!("../../../../../adapters/blender/semwright_blender/commands.json");
const VALIDATION: &str =
    include_str!("../../../../../adapters/blender/semwright_blender/validation.py");
const EXPORT_SCOPE: &str =
    include_str!("../../../../../adapters/blender/semwright_blender/export_scope.py");
fn denied(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: this guard owns the token handle returned by OpenProcessToken.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
fn appcontainer_sid(token: HANDLE) -> io::Result<Vec<u8>> {
    let mut needed = 0;
    // SAFETY: a live query-only token and null buffer request the required size.
    unsafe {
        GetTokenInformation(
            token,
            TokenAppContainerSid,
            std::ptr::null_mut(),
            0,
            &mut needed,
        );
    }
    if !(8..=4096).contains(&needed) {
        return Err(denied("Invalid container identity size"));
    }
    let mut data = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: aligned allocation contains at least the API byte requirement.
    if unsafe {
        GetTokenInformation(
            token,
            TokenAppContainerSid,
            data.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: TOKEN_APPCONTAINER_INFORMATION begins with a PSID; buffer is aligned.
    let sid = unsafe { *(data.as_ptr() as *const *const u8) };
    let base = data.as_ptr() as usize;
    let end = base + needed as usize;
    let address = sid as usize;
    if address < base || address.checked_add(8).is_none_or(|value| value > end) {
        return Err(denied("Container identity is outside the token buffer"));
    }
    // SAFETY: the fixed SID header lies entirely inside the owned allocation.
    let (revision, count) = unsafe { (*sid, *sid.add(1) as usize) };
    let length = 8 + 4 * count;
    if revision != 1 || count > 15 || address.checked_add(length).is_none_or(|value| value > end) {
        return Err(denied("Invalid container SID"));
    }
    // SAFETY: the complete SID span was checked above and remains owned/live.
    Ok(unsafe { std::slice::from_raw_parts(sid, length) }.to_vec())
}
fn verify_process(process: HANDLE) -> io::Result<Vec<u8>> {
    let mut handle = std::ptr::null_mut();
    // SAFETY: supplied live process handle and writable token out-parameter.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut handle) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Token(handle);
    for class in [TokenIsAppContainer] {
        let mut value = 0u32;
        let mut size = 0;
        // SAFETY: the selected classes return DWORD values into this exact-sized buffer.
        if unsafe {
            GetTokenInformation(
                token.0,
                class,
                (&mut value as *mut u32).cast(),
                4,
                &mut size,
            )
        } == 0
            || size != 4
            || value != 1
        {
            return Err(denied("Blender runner requires the Host LPAC token"));
        }
    }
    let mut needed = 0;
    // SAFETY: null/zero buffer requests the size; token remains live.
    unsafe {
        GetTokenInformation(
            token.0,
            TokenCapabilities,
            std::ptr::null_mut(),
            0,
            &mut needed,
        );
    }
    if !(4..=65_536).contains(&needed) {
        return Err(denied("Invalid AppContainer capability size"));
    }
    lpac::verify_lpac(token.0)?;
    let mut capabilities = vec![0u64; (needed as usize).div_ceil(8)];
    // SAFETY: eight-byte-aligned buffer has at least the API byte capacity.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenCapabilities,
            capabilities.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
        || capabilities[0] & 0xffff_ffff != 0
    {
        return Err(denied(
            "Blender runner does not accept ambient AppContainer capabilities",
        ));
    }
    let mut in_job = 0;
    // SAFETY: read-only query on the supplied live process handle.
    if unsafe { IsProcessInJob(process, std::ptr::null_mut(), &mut in_job) } == 0 || in_job == 0 {
        return Err(denied("Blender runner requires a Host Job"));
    }
    appcontainer_sid(token.0)
}
fn local_path(path: &Path) -> io::Result<()> {
    let mut parts = path.components();
    match parts.next() {
        Some(Component::Prefix(p))
            if matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)) => {}
        _ => return Err(denied("Runner paths require a local drive")),
    }
    if parts.next() != Some(Component::RootDir) {
        return Err(denied("Runner paths must be absolute"));
    }
    for part in parts {
        match part {
            Component::Normal(name) => {
                let n = name.to_string_lossy();
                if n.contains(':') || n.ends_with('.') || n.ends_with(' ') {
                    return Err(denied("Ambiguous Windows path component"));
                }
            }
            _ => return Err(denied("Runner path traversal is forbidden")),
        }
    }
    Ok(())
}
fn kind(path: &Path, directory: bool) -> io::Result<()> {
    local_path(path)?;
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_attributes() & 0x400 != 0
        || metadata.is_dir() != directory
        || (!directory && !metadata.is_file())
    {
        return Err(denied("Runner requires regular non-reparse paths"));
    }
    Ok(())
}
struct Args {
    blender: PathBuf,
    workspace: PathBuf,
    runtime: PathBuf,
    scratch: PathBuf,
}
fn parse_args() -> io::Result<Args> {
    let mut args = std::env::args().skip(1);
    fn required(args: &mut impl Iterator<Item = String>, flag: &str) -> io::Result<PathBuf> {
        if args.next().as_deref() != Some(flag) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Unexpected runner argument",
            ));
        }
        let path =
            PathBuf::from(args.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "Missing runner path")
            })?);
        local_path(&path)?;
        Ok(path)
    }
    let result = Args {
        blender: required(&mut args, "--blender")?,
        workspace: required(&mut args, "--workspace")?,
        runtime: required(&mut args, "--runtime")?,
        scratch: required(&mut args, "--scratch")?,
    };
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unexpected runner argument",
        ));
    }
    Ok(result)
}
fn write_new(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(data)?;
    file.sync_all()
}
fn stage(scratch: &Path) -> io::Result<(PathBuf, PathBuf)> {
    kind(scratch, true)?;
    let session = scratch.join(format!("blender-session-{}", unique_id()));
    fs::create_dir(&session)?;
    let package = session.join("semwright_blender_runtime");
    fs::create_dir(&package)?;
    for (name, data) in [
        ("__init__.py", ""),
        ("commands.py", COMMANDS),
        ("commands.json", COMMANDS_JSON),
        ("validation.py", VALIDATION),
        ("semantic.py", SEMANTIC),
        ("authoring_native.py", AUTHORING),
        ("export_scope.py", EXPORT_SCOPE),
    ] {
        write_new(&package.join(name), data.as_bytes())?;
    }
    let bridge = session.join("bridge.py");
    write_new(&bridge, BRIDGE.as_bytes())?;
    fs::create_dir(session.join("user"))?;
    Ok((session, bridge))
}
fn read_frame(input: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut header = [0u8; 4];
    let mut count = 0;
    while count < 4 {
        let got = input.read(&mut header[count..])?;
        if got == 0 {
            if count == 0 {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Truncated native frame",
            ));
        }
        count += got;
    }
    let len = u32::from_be_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Native frame exceeds bounds",
        ));
    }
    let mut body = vec![0; len];
    input.read_exact(&mut body)?;
    Ok(Some(body))
}
fn write_frame(output: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    if payload.is_empty() || payload.len() > MAX_FRAME {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Native frame exceeds bounds",
        ));
    }
    output.write_all(&(payload.len() as u32).to_be_bytes())?;
    output.write_all(payload)?;
    output.flush()
}
fn startup(input: &mut impl Read, marker: &[u8]) -> io::Result<()> {
    let mut prefix = Vec::new();
    let mut byte = [0u8; 1];
    for _ in 0..MAX_STARTUP + marker.len() {
        input.read_exact(&mut byte)?;
        prefix.push(byte[0]);
        if prefix.ends_with(marker) {
            return Ok(());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "Native startup marker is absent or exceeds bounds",
    ))
}
enum Event {
    Ready,
    Frame(Vec<u8>),
    Failure(io::Error),
}
fn reader(mut output: std::process::ChildStdout, marker: Vec<u8>) -> io::Result<Receiver<Event>> {
    let (sender, receiver) = mpsc::sync_channel(1);
    thread::Builder::new()
        .name("blender-native-stdio".into())
        .spawn(move || {
            if let Err(error) = startup(&mut output, &marker) {
                let _ = sender.send(Event::Failure(error));
                return;
            }
            if sender.send(Event::Ready).is_err() {
                return;
            }
            loop {
                match read_frame(&mut output) {
                    Ok(Some(frame)) => {
                        if sender.send(Event::Frame(frame)).is_err() {
                            return;
                        }
                    }
                    Ok(None) => {
                        let _ = sender.send(Event::Failure(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "Native transport closed",
                        )));
                        return;
                    }
                    Err(error) => {
                        let _ = sender.send(Event::Failure(error));
                        return;
                    }
                }
            }
        })?;
    Ok(receiver)
}
fn retain_failure(
    log: &File,
    workspace: &Path,
    status: Option<ExitStatus>,
    phase: &str,
) -> io::Result<()> {
    let dir = workspace.join(".semwright-native-failures");
    match fs::create_dir(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    kind(&dir, true)?;
    let mut retained = 0;
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        if entry.file_name().to_string_lossy().starts_with("blender-")
            && entry.path().extension().is_some_and(|ext| ext == "log")
        {
            retained += 1;
        }
        if retained >= 8 {
            return Err(io::Error::other(
                "Native failure retention capacity reached",
            ));
        }
    }
    let length = log.metadata()?.len().min(8192);
    let mut reader = log.try_clone()?;
    reader.seek(SeekFrom::End(-(length as i64)))?;
    let mut bytes = vec![0; length as usize];
    reader.read_exact(&mut bytes)?;
    let digest = hex::encode(Sha256::digest(&bytes));
    let id = unique_id();
    let receipt = serde_json::json!({"schema":"semwright-blender-native-failure/v1", "phase":phase,
        "retained_bytes":bytes.len(), "sha256":digest, "exit_code":status.and_then(|s| s.code()),
        "source":"windows-isolated-session-runner", "log_scope":"private local only"});
    write_new(&dir.join(format!("blender-{id}.log")), &bytes)?;
    write_new(
        &dir.join(format!("blender-{id}.json")),
        &serde_json::to_vec(&receipt).map_err(io::Error::other)?,
    )
}
struct Native {
    child: Child,
    input: std::process::ChildStdin,
    events: Receiver<Event>,
    session: PathBuf,
    log: File,
    workspace: PathBuf,
}
impl Native {
    fn retain(&mut self, phase: &str) {
        let _ = self.child.kill();
        let status = self.child.wait().ok();
        let _ = retain_failure(&self.log, &self.workspace, status, phase);
    }
}
impl Drop for Native {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.session);
    }
}
fn retain_start_failure(workspace: &Path, phase: &str, error: &io::Error) -> io::Result<()> {
    kind(workspace, true)?;
    let dir = workspace.join(".semwright-native-failures");
    match fs::create_dir(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    kind(&dir, true)?;
    if fs::read_dir(&dir)?.take(24).count() >= 24 {
        return Err(io::Error::other("Native diagnostic retention capacity reached"));
    }
    // Fixed phase identifiers and numeric error codes only: no paths, OS text or environment.
    let receipt = serde_json::json!({
        "schema": "semwright-blender-start-failure/v1",
        "phase": phase,
        "error_kind": format!("{:?}", error.kind()),
        "os_error": error.raw_os_error(),
        "source": "windows-isolated-session-runner",
        "log_scope": "private local only"
    });
    write_new(
        &dir.join(format!("start-{}.json", unique_id())),
        &serde_json::to_vec(&receipt).map_err(io::Error::other)?,
    )
}
fn spawn(args: &Args, session: PathBuf, bridge: PathBuf, parent_sid: &[u8], phase: &mut &'static str) -> io::Result<Native> {
    *phase = "sealed_executable";
    kind(&args.blender, false)?;
    *phase = "mount_roots";
    for root in [&args.workspace, &args.runtime, &args.scratch] {
        kind(root, true)?;
    }
    *phase = "runtime_layout";
    let profile = RuntimeProfile::exact(HostPlatform::Windows, [5, 2, 1])?;
    let version_root = args.runtime.join(profile.resource_directory());
    kind(&version_root, true)?;
    for directory in profile.required_directories() {
        kind(&args.runtime.join(directory), true)?;
    }
    *phase = "python_libraries";
    for library in ["python313.dll", "python3.dll"] {
        kind(&args.runtime.join(library), false)?;
    }
    *phase = "host_system_root";
    let system = PathBuf::from(
        std::env::var_os("SystemRoot").ok_or_else(|| denied("Host did not supply SystemRoot"))?,
    );
    kind(&system, true)?;
    *phase = "host_system32";
    let system32 = system.join("System32");
    kind(&system32, true)?;
    let mut nonce = unique_id();
    nonce.retain(|c| c != '-');
    if nonce.len() != 32
        || !nonce
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return Err(io::Error::other(
            "Invalid generated native session identity",
        ));
    }
    let marker = format!("\0SEMWRIGHT-BLENDER-STDIO-v1:{nonce}\0").into_bytes();
    *phase = "open_native_log";
    let log = OpenOptions::new()
        .create_new(true)
        .write(true)
        .read(true)
        .open(session.join("blender.log"))?;
    let user = session.join("user");
    *phase = "runtime_search_path";
    let path = std::env::join_paths([
        args.runtime.clone(),
        args.runtime.join("blender.shared"),
        args.runtime.join("blender.crt"),
        system32,
    ])
    .map_err(|_| denied("Invalid trusted runtime search path"))?;
    let mut command = Command::new(&args.blender);
    command
        .args([
            "--background",
            "--factory-startup",
            "--disable-autoexec",
            "--offline-mode",
            "-q",
            "--python-exit-code",
            "1",
            "--python",
        ])
        .arg(&bridge)
        .arg("--")
        .arg(format!("stdio:{nonce}"))
        .arg(&args.workspace)
        .arg(&session)
        .current_dir(&args.runtime)
        .env_clear()
        .env("SystemRoot", &system)
        .env("WINDIR", &system)
        .env("PATH", path)
        .env("USERPROFILE", &user)
        .env("APPDATA", &user)
        .env("LOCALAPPDATA", &user)
        .env("TEMP", &session)
        .env("TMP", &session)
        .env("BLENDER_SYSTEM_RESOURCES", &version_root)
        .env("BLENDER_SYSTEM_SCRIPTS", version_root.join("scripts"))
        .env("BLENDER_SYSTEM_EXTENSIONS", version_root.join("extensions"))
        .env("BLENDER_SYSTEM_DATAFILES", version_root.join("datafiles"))
        .env("BLENDER_SYSTEM_PYTHON", version_root.join("python"))
        .env("BLENDER_USER_RESOURCES", &user)
        .env("PYTHONNOUSERSITE", "1")
        .env("PYTHONIOENCODING", "utf-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::from(log.try_clone()?))
        .creation_flags(CREATE_NO_WINDOW);
    // Fixed Host-attested dependency; inherit the restricted token and Job, never break away.
    *phase = "spawn_blender";
    let mut child = command.spawn()?;
    *phase = "verify_child";
    match verify_process(child.as_raw_handle()) {
        Ok(sid) if sid == parent_sid => {}
        _ => {
            let _ = child.kill();
            let status = child.wait().ok();
            let _ = retain_failure(&log, &args.workspace, status, "child_confinement");
            return Err(denied(
                "Native Blender did not retain the Host container identity and Job",
            ));
        }
    }
    *phase = "native_pipes";
    let input = child
        .stdin
        .take()
        .ok_or_else(|| io::Error::other("Native input pipe unavailable"))?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("Native output pipe unavailable"))?;
    *phase = "reader_start";
    let events = match reader(output, marker) {
        Ok(events) => events,
        Err(error) => {
            let _ = child.kill();
            let status = child.wait().ok();
            let _ = retain_failure(&log, &args.workspace, status, "reader_start");
            return Err(error);
        }
    };
    let mut native = Native {
        child,
        input,
        events,
        session,
        log,
        workspace: args.workspace.clone(),
    };
    *phase = "await_native_marker";
    match native.events.recv_timeout(Duration::from_secs(20)) {
        Ok(Event::Ready) => Ok(native),
        Ok(Event::Failure(error)) => {
            native.retain("startup");
            Err(error)
        }
        _ => {
            native.retain("startup");
            Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "Native readiness handshake failed",
            ))
        }
    }
}
fn failed(output: &mut impl Write) -> io::Result<()> {
    write_frame(
        output,
        br#"{"ok":false,"error":{"code":"NativeTransportFailed"}}"#,
    )
}
pub fn run() -> io::Result<()> {
    // SAFETY: GetCurrentProcess returns the process pseudohandle for this query.
    let parent_sid = verify_process(unsafe { GetCurrentProcess() })?;
    if std::env::var_os("SEMWRIGHT_NVIDIA_GPU").is_some() {
        return Err(denied("Windows Blender GPU authority is not implemented"));
    }
    let args = parse_args()?;
    let (session, bridge) = match stage(&args.scratch) {
        Ok(staged) => staged,
        Err(error) => {
            let _ = retain_start_failure(&args.workspace, "stage_fixed_scripts", &error);
            let _ = failed(&mut io::stdout().lock());
            return Err(error);
        }
    };
    let cleanup = session.clone();
    let mut phase = "preflight";
    let mut native = match spawn(&args, session, bridge, &parent_sid, &mut phase) {
        Ok(native) => native,
        Err(error) => {
            let _ = retain_start_failure(&args.workspace, phase, &error);
            let _ = fs::remove_dir_all(cleanup);
            let _ = failed(&mut io::stdout().lock());
            return Err(error);
        }
    };
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    loop {
        let request = match read_frame(&mut input) {
            Ok(Some(request)) => request,
            Ok(None) => break,
            Err(error) => {
                native.retain("host_read");
                let _ = failed(&mut output);
                return Err(error);
            }
        };
        if let Err(error) = write_frame(&mut native.input, &request) {
            native.retain("native_write");
            let _ = failed(&mut output);
            return Err(error);
        }
        let response = match native.events.recv_timeout(Duration::from_secs(300)) {
            Ok(Event::Frame(response)) => response,
            Ok(Event::Failure(error)) => {
                native.retain("native_exchange");
                let _ = failed(&mut output);
                return Err(error);
            }
            _ => {
                native.retain("native_exchange");
                let _ = failed(&mut output);
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Native response was not received",
                ));
            }
        };
        if let Err(error) = write_frame(&mut output, &response) {
            native.retain("host_write");
            return Err(error);
        }
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    #[test]
    fn binary_frames_preserve_bytes_and_reject_bad_lengths() {
        let value = br#"{"command":"driver.blender.scene.list","args":{}}"#;
        let mut bytes = Vec::new();
        write_frame(&mut bytes, value).unwrap();
        assert_eq!(read_frame(&mut Cursor::new(bytes)).unwrap().unwrap(), value);
        for size in [0, MAX_FRAME as u32 + 1] {
            assert!(read_frame(&mut Cursor::new(size.to_be_bytes())).is_err());
        }
        assert!(read_frame(&mut Cursor::new([0u8, 0, 0])).is_err());
        assert!(read_frame(&mut Cursor::new([0u8, 0, 0, 8, 1])).is_err());
    }
    #[test]
    fn startup_consumes_only_bounded_prefix_before_exact_marker() {
        let marker = b"\0SEMWRIGHT-BLENDER-STDIO-v1:0123456789abcdef0123456789abcdef\0";
        let mut bytes = b"Blender native startup log\r\n".to_vec();
        bytes.extend_from_slice(marker);
        bytes.extend_from_slice(b"next");
        let mut input = Cursor::new(bytes);
        startup(&mut input, marker).unwrap();
        let mut tail = String::new();
        input.read_to_string(&mut tail).unwrap();
        assert_eq!(tail, "next");
        assert!(
            startup(
                &mut Cursor::new(vec![b'x'; MAX_STARTUP + marker.len()]),
                marker
            )
            .is_err()
        );
    }
    #[test]
    fn only_local_unambiguous_absolute_paths_are_accepted() {
        for path in [r"C:\runtime\blender.exe", r"\\?\E:\runtime\blender.exe"] {
            local_path(Path::new(path)).unwrap();
        }
        for path in [
            r"relative\file",
            r"C:relative",
            r"\\server\share\file",
            r"\\?\UNC\server\share\file",
            r"\\.\pipe\name",
            r"C:\safe\file:stream",
            r"C:\safe\..\escape",
            r"C:\safe\trailing.",
            r"C:\safe\trailing ",
        ] {
            assert!(local_path(Path::new(path)).is_err(), "{path}");
        }
    }
}
