use semwright_obs_driver::{Fault, ObsDriver, client::Client, config::Config};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = run().await {
        eprintln!(
            "{{\"component\":\"obs-driver\",\"error\":\"{:?}\"}}",
            error.kind
        );
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Fault> {
    let config = Config::load()?;
    let client = Client::start(config.clone(), None)?;
    let driver = ObsDriver::new(client.clone(), config)?;
    let result = semwright_driver_sdk::serve(driver).await;
    let cleanup = client.shutdown().await;
    result.map_err(|_| Fault::new(semwright_obs_driver::FaultKind::Protocol))?;
    cleanup
}
