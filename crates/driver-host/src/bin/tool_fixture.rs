#[cfg(windows)]
fn is_appcontainer() -> bool {
    use windows::Win32::{
        Foundation::{CloseHandle, HANDLE},
        Security::{GetTokenInformation, TOKEN_QUERY, TokenIsAppContainer},
        System::Threading::{GetCurrentProcess, OpenProcessToken},
    };

    let mut token = HANDLE::default();
    // SAFETY: GetCurrentProcess returns the current pseudo-handle and token is a writable out handle.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) }.is_err() {
        return false;
    }
    let mut value = 0u32;
    let mut returned = 0u32;
    // SAFETY: token is live, value is a correctly-sized DWORD buffer, and returned is writable.
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenIsAppContainer,
            Some((&mut value as *mut u32).cast()),
            std::mem::size_of::<u32>() as u32,
            &mut returned,
        )
    };
    // SAFETY: token was opened successfully above and is owned by this function.
    unsafe {
        let _ = CloseHandle(token);
    }
    result.is_ok() && returned == std::mem::size_of::<u32>() as u32 && value != 0
}

fn session_loop() -> std::io::Result<()> {
    use std::io::{ErrorKind, Read, Write};

    const MAX_FRAME: usize = 256 * 1024;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut input = stdin.lock();
    let mut output = stdout.lock();
    loop {
        let mut header = [0u8; 4];
        match input.read_exact(&mut header) {
            Ok(()) => {}
            Err(error) if error.kind() == ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        }
        let length = u32::from_be_bytes(header) as usize;
        if length > MAX_FRAME {
            return Err(std::io::Error::new(
                ErrorKind::InvalidData,
                "session frame exceeds fixture bound",
            ));
        }
        let mut payload = vec![0u8; length];
        input.read_exact(&mut payload)?;
        output.write_all(&header)?;
        output.write_all(&payload)?;
        output.flush()?;
    }
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args.iter().any(|arg| arg == "--session") {
        let lifecycle_marker = args.iter().any(|arg| arg == "--lifecycle-marker");
        if args
            .iter()
            .any(|arg| !matches!(arg.as_str(), "--session" | "--lifecycle-marker"))
            || args.iter().filter(|arg| *arg == "--session").count() != 1
        {
            eprintln!("session mode received an unsupported argument");
            std::process::exit(12);
        }
        if lifecycle_marker && std::fs::write("started.marker", b"started").is_err() {
            std::process::exit(14);
        }
        let result = session_loop();
        if lifecycle_marker
            && result.is_ok()
            && std::fs::write("finished.marker", b"finished").is_err()
        {
            std::process::exit(15);
        }
        if result.is_err() {
            std::process::exit(13);
        }
        return;
    }
    let print_cwd = args.iter().any(|arg| arg == "--print-cwd");
    let lifecycle_marker = args.iter().any(|arg| arg == "--lifecycle-marker");
    if lifecycle_marker && std::fs::write("started.marker", b"started").is_err() {
        eprintln!("failed to write started.marker");
        std::process::exit(3);
    }
    if let Some(index) = args.iter().position(|arg| arg == "--sleep-ms") {
        let sleep_ms = args
            .get(index + 1)
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| (1..=10_000).contains(value))
            .unwrap_or_else(|| {
                eprintln!("invalid --sleep-ms");
                std::process::exit(2);
            });
        std::thread::sleep(std::time::Duration::from_millis(sleep_ms));
    }
    if lifecycle_marker && std::fs::write("finished.marker", b"finished").is_err() {
        eprintln!("failed to write finished.marker");
        std::process::exit(4);
    }
    let probe_path = args
        .iter()
        .position(|arg| arg == "--probe-path")
        .and_then(|index| args.get(index + 1))
        .cloned();
    let dependency_path = args
        .iter()
        .position(|arg| arg == "--run-dependency")
        .and_then(|index| args.get(index + 1))
        .cloned();
    let dependency_probe_path = args
        .iter()
        .position(|arg| arg == "--probe-dependency-path")
        .and_then(|index| args.get(index + 1))
        .cloned();
    let probed = match probe_path {
        Some(path) => {
            let path = std::path::PathBuf::from(path);
            let target = if path.is_dir() {
                path.join("allowed.txt")
            } else {
                path
            };
            match std::fs::read_to_string(target) {
                Ok(value) => Some(value),
                Err(error) => {
                    eprintln!("failed to read typed mount path: {error}");
                    std::process::exit(5);
                }
            }
        }
        None => None,
    };
    let dependency_output = match (dependency_path, dependency_probe_path) {
        (Some(_), Some(_)) => {
            eprintln!("dependency path may be consumed by only one fixture mode");
            std::process::exit(6);
        }
        (Some(path), None) => match std::process::Command::new(path).output() {
            Ok(output) if output.status.success() => match String::from_utf8(output.stdout) {
                Ok(value) => Some(value),
                Err(_) => {
                    eprintln!("dependency output is not UTF-8");
                    std::process::exit(7);
                }
            },
            Ok(output) => {
                eprintln!(
                    "dependency exited unsuccessfully: {:?}",
                    output.status.code()
                );
                std::process::exit(8);
            }
            Err(error) => {
                eprintln!("failed to launch typed dependency: {error}");
                std::process::exit(9);
            }
        },
        (None, Some(path)) => {
            let mut file = match std::fs::File::open(path) {
                Ok(file) => file,
                Err(error) => {
                    eprintln!("failed to open typed dependency path: {error}");
                    std::process::exit(10);
                }
            };
            let mut magic = [0u8; 2];
            if std::io::Read::read_exact(&mut file, &mut magic).is_err() || magic != *b"MZ" {
                eprintln!("typed Windows dependency path is not a readable PE image");
                std::process::exit(11);
            }
            Some("readable-pe".into())
        }
        (None, None) => None,
    };
    #[cfg(windows)]
    {
        print!("tool-ok|appcontainer={}", u8::from(is_appcontainer()));
    }
    #[cfg(not(windows))]
    {
        print!("tool-ok");
    }
    if let Some(value) = probed {
        print!("|path={value}");
    }
    if let Some(value) = dependency_output {
        print!("|dependency={}", value.trim_end());
    }
    if print_cwd {
        match std::env::current_dir() {
            Ok(path) => print!("|cwd={}", path.display()),
            Err(_) => print!("|cwd=<unavailable>"),
        }
    }
}
