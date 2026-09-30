use semwright_ardour_audio::driver::ArdourAudioDriver;
use semwright_driver_sdk::serve;

#[tokio::main]
async fn main() {
    match ArdourAudioDriver::discover() {
        Ok(driver) => {
            if let Err(error) = serve(driver).await {
                eprintln!("ardour audio driver failed: {}", error.message);
                std::process::exit(1);
            }
        }
        Err(error) => {
            eprintln!("ardour audio driver unavailable: {}", error.message);
            std::process::exit(1);
        }
    }
}
