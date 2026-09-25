use semwright_driver_motion_canvas::driver::MotionDriver;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = match MotionDriver::production() {
        Ok(driver) => semwright_driver_sdk::serve(driver).await,
        Err(error) => Err(error),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}
