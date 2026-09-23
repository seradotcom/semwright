use semwright_obs_driver::{client::Client, config::Config};
use serde_json::json;
use tokio_util::sync::CancellationToken;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let port = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port > 0)
        .unwrap_or(4455);
    let config = Config {
        port,
        connect_timeout_ms: 2500,
        request_timeout_ms: 2500,
        reconnect_limit: 1,
        event_capacity: 16,
        secret_socket: false,
        allow_stream_start: false,
        ..Config::default()
    };
    let client = match Client::start(config, None) {
        Ok(client) => client,
        Err(_) => std::process::exit(2),
    };
    if client.ready().await.is_err() {
        let _ = client.shutdown().await;
        std::process::exit(3);
    }
    let version = client
        .request(
            "GetVersion",
            json!({}),
            false,
            None,
            CancellationToken::new(),
        )
        .await;
    let scenes = client
        .request(
            "GetSceneList",
            json!({}),
            false,
            None,
            CancellationToken::new(),
        )
        .await;
    let _ = client.shutdown().await;
    match (version, scenes) {
        (Ok(version), Ok(scenes)) => {
            println!(
                "{}",
                serde_json::to_string(&json!({"version":version,"scenes":scenes})).unwrap()
            );
        }
        _ => std::process::exit(4),
    }
}
