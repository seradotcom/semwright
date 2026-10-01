use semwright_driver_sdk::serve;
use semwright_faust_audio::analysis_driver::AudioAnalysisDriver;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = match AudioAnalysisDriver::production() {
        Ok(driver) => serve(driver).await,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}
