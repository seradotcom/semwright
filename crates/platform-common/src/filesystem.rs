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
                let overwrite = match args.get("overwrite") {
                    None => true,
                    Some(Value::Bool(value)) => *value,
                    Some(_) => return Err(Error::invalid("overwrite must be a Boolean")),
                };
                if text.len() > MAX_PUBLIC_TEXT_BYTES {
                    return Err(Error::new(
                        ErrorCode::ResourceExhausted,
                        "filesystem.write exceeds the 1 MiB text budget",
                    ));
                }
                if overwrite {
                    root.write_atomic(path, text.as_bytes())?;
                } else {
                    root.write_new_atomic(path, text.as_bytes())?;
                }
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
            path: crate::fixture_root(directory.path()),
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

    #[cfg(any(target_os = "linux", target_os = "windows"))]
    #[tokio::test]
    async fn new_only_text_write_preserves_existing_and_human_edits() {
        let directory = tempdir().unwrap();
        let grant = FilesystemGrant {
            name: "workspace".into(),
            path: crate::fixture_root(directory.path()),
            read: true,
            write: true,
        };
        let backend = Filesystem::new(&[grant]).unwrap();
        let new_only = json!({
            "root": "workspace", "path": "notes.txt", "text": "first", "overwrite": false
        });
        let created = backend
            .execute(&context(), "filesystem.write", &new_only)
            .await
            .unwrap();
        assert_eq!(created["written"], true);
        assert_eq!(
            std::fs::read(directory.path().join("notes.txt")).unwrap(),
            b"first"
        );

        // An external edit before a second request may never be overwritten.
        std::fs::write(directory.path().join("notes.txt"), b"owner edit").unwrap();
        let conflict = backend
            .execute(&context(), "filesystem.write", &new_only)
            .await
            .unwrap_err();
        assert_eq!(conflict.code, ErrorCode::Conflict);
        assert!(conflict.outcome_known);
        assert_eq!(
            std::fs::read(directory.path().join("notes.txt")).unwrap(),
            b"owner edit"
        );

        let bad_flag = json!({
            "root": "workspace", "path": "notes.txt", "text": "unsafe", "overwrite": "false"
        });
        let bad = backend
            .execute(&context(), "filesystem.write", &bad_flag)
            .await
            .unwrap_err();
        assert_eq!(bad.code, ErrorCode::InvalidArgument);
        assert_eq!(
            std::fs::read(directory.path().join("notes.txt")).unwrap(),
            b"owner edit"
        );

        // Omitted overwrite retains the existing compatibility contract.
        let legacy = json!({"root":"workspace", "path":"notes.txt", "text":"legacy"});
        backend
            .execute(&context(), "filesystem.write", &legacy)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(directory.path().join("notes.txt")).unwrap(),
            b"legacy"
        );

        let traversal = json!({
            "root": "workspace", "path": "../outside.txt", "text": "no", "overwrite": false
        });
        let denied = backend
            .execute(&context(), "filesystem.write", &traversal)
            .await
            .unwrap_err();
        assert!(matches!(
            denied.code,
            ErrorCode::PolicyDenied | ErrorCode::InvalidArgument
        ));
        assert!(!directory.path().join("../outside.txt").exists());
        assert!(!std::fs::read_dir(directory.path()).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".semwright-")
        }));
    }

    // Unsupported native backends must never downgrade new-only to replacement.
    struct ReplacementOnly(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl ScopedRoot for ReplacementOnly {
        fn confinement(&self) -> semwright_platform_api::filesystem::Confinement {
            semwright_platform_api::filesystem::Confinement::PinnedRootSingleChild
        }
        fn read(&self, _: &Path, _: usize) -> Result<Vec<u8>> {
            panic!("No read is expected")
        }
        fn write_atomic(&self, _: &Path, _: &[u8]) -> Result<()> {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn no_replace_unsupported_does_not_call_replacement() {
        let writes = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut roots = BTreeMap::new();
        roots.insert(
            "workspace".into(),
            Box::new(ReplacementOnly(std::sync::Arc::clone(&writes))) as Box<dyn ScopedRoot>,
        );
        let backend = Filesystem { roots };
        let safe = json!({
            "root":"workspace", "path":"existing.txt", "text":"unsafe", "overwrite": false
        });
        let refused = backend
            .execute(&context(), "filesystem.write", &safe)
            .await
            .unwrap_err();
        assert_eq!(refused.code, ErrorCode::Unsupported);
        assert!(refused.outcome_known);
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 0);

        let legacy = json!({"root":"workspace", "path":"existing.txt", "text":"legacy"});
        backend
            .execute(&context(), "filesystem.write", &legacy)
            .await
            .unwrap();
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
