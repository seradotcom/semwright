use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
use semwright_policy::{FilesystemGrant, Root};
use semwright_types::*;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::Path;
pub struct Filesystem {
    roots: BTreeMap<String, Root>,
}
impl Filesystem {
    pub fn new(grants: &[FilesystemGrant]) -> Result<Self> {
        let mut roots = BTreeMap::new();
        for g in grants {
            roots.insert(g.name.clone(), Root::open(&g.path, g.read, g.write)?);
        }
        Ok(Self { roots })
    }
}
#[async_trait]
impl Backend for Filesystem {
    fn name(&self) -> &'static str {
        "filesystem"
    }
    fn supports(&self, c: &str) -> bool {
        matches!(c, "filesystem.read" | "filesystem.write")
    }
    async fn probe(&self) -> Vec<Feature> {
        vec![feature(
            self.name(),
            "filesystem.scoped",
            !self.roots.is_empty(),
            "Requires explicit root grants and Linux openat2",
            "Configure named filesystem roots in the owner-controlled policy",
        )]
    }
    async fn execute(&self, ctx: &Context, c: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        let root = self
            .roots
            .get(arg_str(args, "root")?)
            .ok_or_else(|| Error::new(ErrorCode::PolicyDenied, "Unknown filesystem root"))?;
        let path = Path::new(arg_str(args, "path")?);
        match c {
            "filesystem.read" => {
                let bytes =
                    root.read(path, args["max_bytes"].as_u64().unwrap_or(1048576) as usize)?;
                let size = bytes.len();
                let text = String::from_utf8(bytes).map_err(|_| {
                    Error::invalid("File is not UTF-8; binary reads are not exposed")
                })?;
                Ok(json!({"text":text,"bytes":size}))
            }
            "filesystem.write" => {
                let text = arg_str(args, "text")?;
                root.write_atomic(path, text.as_bytes())?;
                Ok(json!({"written":true,"bytes":text.len(),"atomic":true}))
            }
            _ => Err(Error::new(
                ErrorCode::Unsupported,
                "Unknown filesystem command",
            )),
        }
    }
}
