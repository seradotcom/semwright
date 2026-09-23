use semwright_plugin_host::adversarial_fixture::{NAME, VERSION, commands, dispatch};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let commands = commands();
    if semwright_plugin_sdk::serve(NAME, VERSION, &commands, dispatch)
        .await
        .is_err()
    {
        std::process::exit(1);
    }
}
