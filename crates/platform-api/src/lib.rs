//! Platform contracts contain semantics and budgets, never native handles.
pub mod filesystem;
pub mod launch;
pub mod model;
use semwright_backend_api::Backend;
use semwright_types::Result;
use serde_json::Value;
use std::{any::Any, path::PathBuf, sync::Arc};

/// Native connection lifetimes stay inside the host, not the authorization engine.
pub struct DesktopHost {
    pub backends: Vec<Arc<dyn Backend>>,
    pub environment: Value,
    pub keepalive: Box<dyn Any + Send + Sync>,
}
#[derive(Clone, Debug)]
pub struct PlatformPaths {
    pub runtime: PathBuf,
    pub state: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
}
pub trait PathService: Send + Sync {
    fn paths(&self) -> Result<PlatformPaths>;
}
/// An availability observation never grants policy authority.
pub trait PermissionProbe: Send + Sync {
    fn probe(&self) -> Result<Vec<model::Permission>>;
}
