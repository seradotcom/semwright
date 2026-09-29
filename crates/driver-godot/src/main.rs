use semwright_godot_driver::{Config, GodotDriver};
use std::io::Write;

const STARTUP_DIAGNOSTIC_GATE: &str =
    "/workspace/godot-authoring-state/.enable-startup-diagnostics";
const STARTUP_DIAGNOSTIC_PATH: &str = "/workspace/godot-authoring-state/.driver-startup-error";

fn write_startup_diagnostic(error: &semwright_types::Error) {
    let Ok(gate) = std::fs::symlink_metadata(STARTUP_DIAGNOSTIC_GATE) else {
        return;
    };
    if !gate.file_type().is_file() || gate.len() > 32 {
        return;
    }
    let mut message = error.message.replace(['\r', '\n'], " ");
    message.truncate(2048);
    let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(STARTUP_DIAGNOSTIC_PATH)
    else {
        return;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = file.set_permissions(std::fs::Permissions::from_mode(0o600));
    }
    let _ = writeln!(file, "code={:?}", error.code);
    let _ = writeln!(file, "message={message}");
    let _ = file.sync_all();
}

#[tokio::main]
async fn main() {
    let mut args = std::env::args_os().skip(1);
    let path: semwright_types::Result<std::path::PathBuf> =
        match (args.next(), args.next(), args.next()) {
            (None, None, None) => semwright_driver_sdk::workspace_mount("godot-config")
                .map(|root| root.join("config.json")),
            (Some(flag), Some(path), None) if flag == "--config" => Ok(path.into()),
            _ => {
                eprintln!("usage: semwright-godot-driver [--config PATH]");
                std::process::exit(2);
            }
        };
    let result = async {
        let path = path?;
        let config = Config::load(&path)?;
        let driver = GodotDriver::new(config).await?;
        semwright_driver_sdk::serve(driver).await
    }
    .await;
    if let Err(error) = result {
        write_startup_diagnostic(&error);
        eprintln!("godot driver stopped: {:?}", error.code);
        std::process::exit(1);
    }
}
