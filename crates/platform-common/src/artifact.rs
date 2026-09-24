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
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        if let Some(expected) = args["expected_sha256"].as_str()
            && !expected.eq_ignore_ascii_case(&sha256)
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Artifact digest changed before handoff",
            ));
        }

        self.root(destination_root)?
            .write_atomic(Path::new(destination_path), &bytes)?;
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
        let expected = format!("{:x}", Sha256::digest(&payload));
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
}
