use semwright_driver_sdk::serve;
use semwright_faust_audio::driver::FaustAudioDriver;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = match FaustAudioDriver::production() {
        Ok(driver) => serve(driver).await,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}
