use semwright_godot_driver::{Config, GodotDriver};

#[tokio::main]
async fn main() {
    let mut args = std::env::args_os().skip(1);
    let path: std::path::PathBuf = match (args.next(), args.next(), args.next()) {
        (None, None, None) => "/workspace/godot-config/config.json".into(),
        (Some(flag), Some(path), None) if flag == "--config" => path.into(),
        _ => {
            eprintln!("usage: semwright-godot-driver [--config PATH]");
            std::process::exit(2);
        }
    };
    let result = async {
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
