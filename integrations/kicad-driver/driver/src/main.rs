// SPDX-License-Identifier: GPL-3.0-or-later
//! The actual Semwright Driver SDK client. The Go reference executable is not used here.
use async_trait::async_trait;
use kicad_ipc::Client;
use semwright_driver_sdk::{Capability, Driver, serve};
use semwright_types::{Error, ErrorCode, Result};
use serde_json::Value;
use std::path::Path;

fn mapped(value: Value) -> Error {
    serde_json::from_value::<Error>(value).unwrap_or_else(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Native core returned an invalid error envelope",
        )
        .uncertain()
    })
}
struct KiCadDriver {
    core: Client,
}
#[async_trait]
impl Driver for KiCadDriver {
    fn id(&self) -> &str {
        "kicad"
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        serde_json::from_value(self.core.capabilities().map_err(mapped)?).map_err(|_| {
            Error::new(
                ErrorCode::PluginProtocolError,
                "Native capability catalog is invalid",
            )
        })
    }
    async fn execute(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
    ) -> Result<Value> {
        // The SDK serializes invocations. The native channel has finite socket deadlines
        // and never retries a mutation. Host v1 has no cooperative cancellation contract.
        self.core
            .execute(command, descriptor_sha256, args)
            .map_err(mapped)
    }
    async fn health(&mut self) -> Result<Value> {
        self.core.health().map_err(mapped)
    }
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let result = match Client::open(Path::new(kicad_ipc::DEFAULT_CONFIG)) {
        Ok(core) => serve(KiCadDriver { core }).await,
        Err(error) => Err(mapped(error)),
    };
    if let Err(error) = result {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}
