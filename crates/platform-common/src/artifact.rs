//! Broker-mediated binary artifact handoff between explicitly granted filesystem roots.
use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
use semwright_platform_api::filesystem::{MAX_SCOPED_BINARY_BYTES, ScopedFilesystem, ScopedRoot};
use semwright_policy::FilesystemGrant;
use semwright_types::{Error, ErrorCode, Feature, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path};

pub struct ArtifactHandoff {
    roots: BTreeMap<String, Box<dyn ScopedRoot>>,
    available: bool,
}

impl ArtifactHandoff {
    pub fn new(grants: &[FilesystemGrant]) -> Result<Self> {
        Self::with_factory(grants, &semwright_platform_services::filesystem())
    }

    fn with_factory(grants: &[FilesystemGrant], factory: &dyn ScopedFilesystem) -> Result<Self> {
        let mut roots = BTreeMap::new();
        for grant in grants {
            if roots
                .insert(
                    grant.name.clone(),
                    factory.open_root(&grant.path, grant.read, grant.write)?,
                )
                .is_some()
            {
                return Err(Error::invalid("Duplicate artifact filesystem root"));
            }
        }
        Ok(Self {
            roots,
            available: grants.iter().any(|grant| grant.read)
                && grants.iter().any(|grant| grant.write),
        })
    }

    fn root(&self, name: &str) -> Result<&dyn ScopedRoot> {
        self.roots
            .get(name)
            .map(Box::as_ref)
            .ok_or_else(|| Error::new(ErrorCode::PolicyDenied, "Unknown artifact filesystem root"))
    }
}

#[async_trait]
impl Backend for ArtifactHandoff {
    fn name(&self) -> &'static str {
        "artifacts"
    }

    fn supports(&self, command: &str) -> bool {
        command == "artifact.handoff"
    }

    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command)
            .then(|| "artifact.handoff".to_owned())
    }

    async fn probe(&self) -> Vec<Feature> {
        vec![feature(
            self.name(),
            "artifact.handoff",
            self.available,
            "Bounded binary copy across owner-granted roots with digest verification",
            "Configure at least one readable and one writable filesystem grant",
        )]
    }

    async fn execute(&self, ctx: &Context, command: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        if command != "artifact.handoff" {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Unknown artifact handoff command",
            ));
        }
        let overwrite = match args.get("overwrite") {
            None => true,
            Some(Value::Bool(value)) => *value,
            Some(_) => return Err(Error::invalid("overwrite must be a Boolean")),
        };
        let source_root = arg(args, "source_root")?;
        let source_path = arg(args, "source_path")?;
        let destination_root = arg(args, "destination_root")?;
        let destination_path = arg(args, "destination_path")?;
        if source_root == destination_root && source_path == destination_path {
            return Err(Error::invalid(
                "Artifact source and destination must differ",
            ));
        }
        let max_bytes = args["max_bytes"]
            .as_u64()
            .unwrap_or(MAX_SCOPED_BINARY_BYTES as u64);
        if max_bytes == 0 || max_bytes > MAX_SCOPED_BINARY_BYTES as u64 {
            return Err(Error::invalid(
                "Artifact max_bytes exceeds bounded handoff budget",
            ));
        }

        let bytes = self
            .root(source_root)?
            .read(Path::new(source_path), max_bytes as usize)?;
        ctx.check_cancelled()?;
        let sha256 = hex::encode(Sha256::digest(&bytes));
        if let Some(expected) = args["expected_sha256"].as_str()
            && !expected.eq_ignore_ascii_case(&sha256)
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Artifact digest changed before handoff",
            ));
        }

        if overwrite {
            self.root(destination_root)?
                .write_atomic(Path::new(destination_path), &bytes)?;
        } else {
            self.root(destination_root)?
                .write_new_atomic(Path::new(destination_path), &bytes)?;
        }
        Ok(json!({
            "copied": true,
            "bytes": bytes.len(),
            "sha256": sha256,
            "source": {"root": source_root, "path": source_path},
            "destination": {"root": destination_root, "path": destination_path},
            "semantic_type": args.get("semantic_type").cloned().unwrap_or(Value::Null),
            "media_type": args.get("media_type").cloned().unwrap_or(Value::Null),
            "atomic": true
        }))
    }
}

fn arg<'a>(args: &'a Value, name: &str) -> Result<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("{name} required")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use semwright_backend_api::Context;
    use tempfile::tempdir;
    use tokio_util::sync::CancellationToken;

    fn context() -> Context {
        Context {
            session: "artifact-test".into(),
            request_id: "request".into(),
            cancellation: CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn copies_binary_between_explicit_grants_and_checks_digest() {
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        let payload = vec![0x5a; 2 * 1024 * 1024];
        std::fs::write(source.path().join("mesh.blend"), &payload).unwrap();
        let grants = vec![
            FilesystemGrant {
                name: "blender-output".into(),
                path: std::fs::canonicalize(source.path()).unwrap(),
                read: true,
                write: false,
            },
            FilesystemGrant {
                name: "godot-project".into(),
                path: std::fs::canonicalize(destination.path()).unwrap(),
                read: true,
                write: true,
            },
        ];
        let backend = ArtifactHandoff::new(&grants).unwrap();
        let expected = hex::encode(Sha256::digest(&payload));
        let result = backend
            .execute(
                &context(),
                "artifact.handoff",
                &json!({
                    "source_root":"blender-output",
                    "source_path":"mesh.blend",
                    "destination_root":"godot-project",
                    "destination_path":"mesh.blend",
                    "expected_sha256":expected,
                    "semantic_type":"model/3d",
                    "media_type":"application/x-blender"
                }),
            )
            .await
            .unwrap();
        assert_eq!(result["sha256"], expected);
        assert_eq!(
            std::fs::read(destination.path().join("mesh.blend")).unwrap(),
            payload
        );
    }

    #[tokio::test]
    async fn digest_mismatch_fails_before_destination_write() {
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        std::fs::write(source.path().join("asset.bin"), b"actual").unwrap();
        let grants = vec![
            FilesystemGrant {
                name: "source".into(),
                path: std::fs::canonicalize(source.path()).unwrap(),
                read: true,
                write: false,
            },
            FilesystemGrant {
                name: "destination".into(),
                path: std::fs::canonicalize(destination.path()).unwrap(),
                read: true,
                write: true,
            },
        ];
        let backend = ArtifactHandoff::new(&grants).unwrap();
        let error = backend
            .execute(
                &context(),
                "artifact.handoff",
                &json!({
                    "source_root":"source",
                    "source_path":"asset.bin",
                    "destination_root":"destination",
                    "destination_path":"asset.bin",
                    "expected_sha256":"0000000000000000000000000000000000000000000000000000000000000000"
                }),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
        assert!(!destination.path().join("asset.bin").exists());
    }
    type CallLog = std::sync::Arc<std::sync::Mutex<Vec<&'static str>>>;

    #[derive(Clone)]
    struct TestRoot {
        calls: CallLog,
        mode: u8,
    }
    impl ScopedRoot for TestRoot {
        fn confinement(&self) -> semwright_platform_api::filesystem::Confinement {
            semwright_platform_api::filesystem::Confinement::PinnedRootSingleChild
        }
        fn read(&self, _: &Path, _: usize) -> Result<Vec<u8>> {
            self.calls.lock().unwrap().push("read");
            Ok(b"owned".to_vec())
        }
        fn write_atomic(&self, _: &Path, _: &[u8]) -> Result<()> {
            self.calls.lock().unwrap().push("replace");
            Ok(())
        }
        fn write_new_atomic(&self, _: &Path, _: &[u8]) -> Result<()> {
            match self.mode {
                0 => {
                    self.calls.lock().unwrap().push("new");
                    Ok(())
                }
                1 => Err(Error::new(
                    ErrorCode::Unsupported,
                    "Test backend lacks no-replace",
                )),
                _ => {
                    self.calls.lock().unwrap().push("published");
                    Err(
                        Error::new(ErrorCode::BackendFailed, "Test post-publication error")
                            .uncertain(),
                    )
                }
            }
        }
    }
    fn synthetic_handoff(mode: u8) -> (ArtifactHandoff, CallLog) {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(vec![]));
        let roots = ["source", "destination"]
            .into_iter()
            .map(|name| {
                (
                    name.to_owned(),
                    Box::new(TestRoot {
                        calls: calls.clone(),
                        mode,
                    }) as Box<dyn ScopedRoot>,
                )
            })
            .collect();
        (
            ArtifactHandoff {
                roots,
                available: true,
            },
            calls,
        )
    }
    fn copy_arguments() -> Value {
        json!({"source_root":"source","source_path":"input.bin",
            "destination_root":"destination","destination_path":"output.bin"})
    }
    #[tokio::test]
    async fn no_replace_selects_new_primitive_and_preserves_legacy_default() {
        let (backend, calls) = synthetic_handoff(0);
        for flag in [None, Some(true), Some(false)] {
            calls.lock().unwrap().clear();
            let mut args = copy_arguments();
            if let Some(flag) = flag {
                args["overwrite"] = json!(flag);
            }
            let result = backend
                .execute(&context(), "artifact.handoff", &args)
                .await
                .unwrap();
            assert_eq!(result["copied"], true);
            assert_eq!(result["atomic"], true);
            assert_eq!(result["sha256"], format!("{:x}", Sha256::digest(b"owned")));
            assert_eq!(
                *calls.lock().unwrap(),
                vec![
                    "read",
                    if flag == Some(false) {
                        "new"
                    } else {
                        "replace"
                    }
                ]
            );
        }
    }
    #[tokio::test]
    async fn no_replace_invalid_flag_is_rejected_before_io() {
        let (backend, calls) = synthetic_handoff(0);
        for flag in [Value::Null, json!("false"), json!(0)] {
            let mut args = copy_arguments();
            args["overwrite"] = flag;
            assert_eq!(
                backend
                    .execute(&context(), "artifact.handoff", &args)
                    .await
                    .unwrap_err()
                    .code,
                ErrorCode::InvalidArgument
            );
            assert!(calls.lock().unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn no_replace_unsupported_or_post_publication_error_never_replaces() {
        for mode in [1, 2] {
            let (backend, calls) = synthetic_handoff(mode);
            let mut args = copy_arguments();
            args["overwrite"] = json!(false);
            let error = backend
                .execute(&context(), "artifact.handoff", &args)
                .await
                .unwrap_err();
            assert_eq!(
                error.code,
                if mode == 1 {
                    ErrorCode::Unsupported
                } else {
                    ErrorCode::BackendFailed
                }
            );
            assert_eq!(error.outcome_known, mode == 1);
            assert!(!calls.lock().unwrap().contains(&"replace"));
            assert_eq!(calls.lock().unwrap().contains(&"published"), mode == 2);
        }
    }
    #[tokio::test]
    async fn no_replace_digest_mismatch_and_cancellation_prevent_destination_work() {
        let (backend, calls) = synthetic_handoff(0);
        let mut args = copy_arguments();
        args["overwrite"] = json!(false);
        args["expected_sha256"] = json!("0".repeat(64));
        assert_eq!(
            backend
                .execute(&context(), "artifact.handoff", &args)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(*calls.lock().unwrap(), vec!["read"]);
        calls.lock().unwrap().clear();
        let cancelled = context();
        cancelled.cancellation.cancel();
        assert_eq!(
            backend
                .execute(&cancelled, "artifact.handoff", &args)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        assert!(calls.lock().unwrap().is_empty());
    }

    #[cfg(any(target_os = "macos", target_os = "windows"))]
    #[tokio::test]
    async fn no_replace_owned_handoff_refuses_without_modifying_destination() {
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        std::fs::write(source.path().join("input.bin"), b"new").unwrap();
        std::fs::write(destination.path().join("output.bin"), b"preserved").unwrap();
        let grants = vec![
            FilesystemGrant {
                name: "source".into(),
                path: source.path().canonicalize().unwrap(),
                read: true,
                write: false,
            },
            FilesystemGrant {
                name: "destination".into(),
                path: destination.path().canonicalize().unwrap(),
                read: true,
                write: true,
            },
        ];
        let backend = ArtifactHandoff::new(&grants).unwrap();
        let mut args = copy_arguments();
        args["overwrite"] = json!(false);
        for name in ["output.bin", "absent.bin"] {
            args["destination_path"] = json!(name);
            let error = backend
                .execute(&context(), "artifact.handoff", &args)
                .await
                .unwrap_err();
            assert_eq!(error.code, ErrorCode::Unsupported);
            assert!(error.outcome_known);
        }
        assert_eq!(
            std::fs::read(destination.path().join("output.bin")).unwrap(),
            b"preserved"
        );
        assert!(!destination.path().join("absent.bin").exists());
        assert_eq!(std::fs::read_dir(destination.path()).unwrap().count(), 1);
        args["destination_path"] = json!("output.bin");
        args.as_object_mut().unwrap().remove("overwrite");
        backend
            .execute(&context(), "artifact.handoff", &args)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(destination.path().join("output.bin")).unwrap(),
            b"new"
        );
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn no_replace_owned_handoff_preserves_winner_and_legacy_replacement() {
        let source = tempdir().unwrap();
        let destination = tempdir().unwrap();
        std::fs::write(source.path().join("input.bin"), b"first").unwrap();
        let grants = vec![
            FilesystemGrant {
                name: "source".into(),
                path: source.path().canonicalize().unwrap(),
                read: true,
                write: false,
            },
            FilesystemGrant {
                name: "destination".into(),
                path: destination.path().canonicalize().unwrap(),
                read: true,
                write: true,
            },
        ];
        let backend = ArtifactHandoff::new(&grants).unwrap();
        let mut args = copy_arguments();
        args["overwrite"] = json!(false);
        let first = backend
            .execute(&context(), "artifact.handoff", &args)
            .await
            .unwrap();
        assert_eq!(first["sha256"], format!("{:x}", Sha256::digest(b"first")));
        assert_eq!(first["bytes"], 5);
        std::fs::write(source.path().join("input.bin"), b"second").unwrap();
        assert_eq!(
            backend
                .execute(&context(), "artifact.handoff", &args)
                .await
                .unwrap_err()
                .code,
            ErrorCode::Conflict
        );
        assert_eq!(
            std::fs::read(destination.path().join("output.bin")).unwrap(),
            b"first"
        );
        args.as_object_mut().unwrap().remove("overwrite");
        backend
            .execute(&context(), "artifact.handoff", &args)
            .await
            .unwrap();
        assert_eq!(
            std::fs::read(destination.path().join("output.bin")).unwrap(),
            b"second"
        );
    }
}
