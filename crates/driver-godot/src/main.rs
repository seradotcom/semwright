use semwright_godot_driver::{Config, GodotDriver};

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
        eprintln!("godot driver stopped: {:?}", error.code);
        std::process::exit(1);
    }
}
