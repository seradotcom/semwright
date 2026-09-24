//! One shared filesystem Backend, parameterized by an enforcing native factory.
use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
use semwright_platform_api::filesystem::{ScopedFilesystem, ScopedRoot};
use semwright_policy::FilesystemGrant;
use semwright_types::*;
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path};

const MAX_PUBLIC_TEXT_BYTES: usize = 1024 * 1024;
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
                let limit = args["max_bytes"]
                    .as_u64()
                    .unwrap_or(MAX_PUBLIC_TEXT_BYTES as u64);
                if limit == 0 || limit > MAX_PUBLIC_TEXT_BYTES as u64 {
                    return Err(Error::invalid(
                        "filesystem.read exceeds the 1 MiB text budget",
                    ));
                }
                let bytes = root.read(path, limit as usize)?;
                let size = bytes.len();
                let text = String::from_utf8(bytes).map_err(|_| {
                    Error::invalid("File is not UTF-8; binary reads are not exposed")
                })?;
                Ok(json!({"text":text,"bytes":size}))
            }
            "filesystem.write" => {
                let text = arg_str(args, "text")?;
                if text.len() > MAX_PUBLIC_TEXT_BYTES {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "filesystem.write exceeds the 1 MiB text budget",
                    ));
                }
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

#[cfg(test)]
mod tests {
    use super::*;
    use semwright_backend_api::Context;
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    fn context() -> Context {
        Context {
            session: "filesystem-test".into(),
            request_id: "request".into(),
            cancellation: CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn public_text_operations_keep_one_mib_budget() {
        let directory = tempdir().unwrap();
        let grant = FilesystemGrant {
            name: "workspace".into(),
            path: std::fs::canonicalize(directory.path()).unwrap(),
            read: true,
            write: true,
        };
        let backend = Filesystem::new(&[grant]).unwrap();
        let oversized = "x".repeat(MAX_PUBLIC_TEXT_BYTES + 1);
        let write = backend
            .execute(
                &context(),
                "filesystem.write",
                &json!({"root":"workspace","path":"large.txt","text":oversized}),
            )
            .await
            .unwrap_err();
        assert_eq!(write.code, ErrorCode::ResourceExhausted);

        std::fs::write(directory.path().join("small.txt"), b"ok").unwrap();
        let read = backend
            .execute(
                &context(),
                "filesystem.read",
                &json!({
                    "root":"workspace",
                    "path":"small.txt",
                    "max_bytes": MAX_PUBLIC_TEXT_BYTES + 1
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(read.code, ErrorCode::InvalidArgument);
    }
}
