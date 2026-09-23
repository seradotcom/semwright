//! One shared filesystem Backend, parameterized by an enforcing native factory.
use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
use semwright_platform_api::filesystem::{ScopedFilesystem, ScopedRoot};
use semwright_policy::FilesystemGrant;
use semwright_types::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};
pub struct Filesystem {
    roots: BTreeMap<String, Box<dyn ScopedRoot>>,
}
impl Filesystem {
    pub fn new(grants: &[FilesystemGrant]) -> Result<Self> {
        Self::with_factory(grants, &semwright_platform_services::filesystem())
    }
    pub fn with_factory(
        grants: &[FilesystemGrant],
        factory: &dyn ScopedFilesystem,
    ) -> Result<Self> {
        let mut roots = BTreeMap::new();
        for grant in grants {
            if roots
                .insert(
                    grant.name.clone(),
                    factory.open_root(&grant.path, grant.read, grant.write)?,
                )
                .is_some()
            {
                return Err(Error::invalid("Duplicate filesystem root"));
            }
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
    fn operation_feature(&self, c: &str) -> Option<String> {
        self.supports(c).then(|| "filesystem.scoped".to_owned())
    }
    async fn probe(&self) -> Vec<Feature> {
        let strength = self
            .roots
            .values()
            .next()
            .map(|r| format!("{:?}", r.confinement()))
            .unwrap_or_else(|| "no configured roots".into());
        vec![feature(
            "filesystem",
            "filesystem.scoped",
            !self.roots.is_empty(),
            &format!("Explicit root grants; confinement: {strength}"),
            "Use named owner grants. Mac roots accept only an immediate child, not nested paths.",
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
