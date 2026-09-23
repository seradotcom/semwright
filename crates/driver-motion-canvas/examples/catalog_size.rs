use semwright_driver_motion_canvas::driver::MotionDriver;
use semwright_driver_sdk::{Driver, Response};
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut driver = MotionDriver::production().unwrap();
    let capabilities = driver.capabilities().await.unwrap();
    let digest = semwright_driver_sdk::capabilities_digest(&capabilities).unwrap();
    let response = Response::Capabilities {
        id: "probe".into(),
        capabilities,
        digest,
    };
    let bytes = serde_json::to_vec(&response).unwrap();
    println!("{} {}", bytes.len(), semwright_types::MAX_FRAME);
}
