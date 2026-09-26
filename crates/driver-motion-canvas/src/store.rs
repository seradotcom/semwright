//! Atomic managed-project persistence. `semwright-motion.json` is authoritative.
use crate::{
    Error, ErrorCode, Result,
    compiler::{self, Generated},
    model::{MAX_ASSET_BYTES, MAX_PROJECT_BYTES, Project},
    security, validate,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub const SEMANTIC_FILE: &str = "semwright-motion.json";
pub const ISLAND_DIR: &str = ".semwright/motion";
pub const ISLAND_BRIDGE: &str = "managed-scenes.ts";

#[derive(Clone, Debug)]
pub struct ProjectStore {
    root: PathBuf,
    publish_island_bridge: bool,
}

#[derive(Debug)]
pub struct Snapshot {
    pub project: Project,
    pub source_sha256: String,
    pub generated: Generated,
    pub generated_dir: Option<PathBuf>,
}

impl ProjectStore {
    pub fn create_island_root(project_root: &Path) -> Result<PathBuf> {
        if !project_root.is_absolute() || !project_root.is_dir() {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Project grant root must be an existing absolute directory",
            ));
        }
        let root = fs::canonicalize(project_root)?;
        if root != project_root {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Project grant root must be canonical",
            ));
        }
        let mut current = root.clone();
        for name in [".semwright", "motion"] {
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(metadata) => {
                    if !metadata.is_dir() || metadata.file_type().is_symlink() {
                        return Err(Error::new(
                            ErrorCode::PermissionDenied,
                            "Managed island path component is not a real directory",
                        ));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&current)?;
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::PermissionsExt;
                        fs::set_permissions(&current, fs::Permissions::from_mode(0o700))?;
                    }
                }
                Err(error) => return Err(error.into()),
            }
            let canonical = fs::canonicalize(&current)?;
            if canonical != current || !canonical.starts_with(&root) {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Managed island path escapes the project grant",
                ));
            }
        }
        Ok(current)
    }

    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if !root.is_absolute() || !root.is_dir() {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Managed project root must be an existing absolute directory",
            ));
        }
        let canonical = fs::canonicalize(&root)?;
        if canonical != root {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Managed project root must be canonical",
            ));
        }
        Ok(Self {
            root,
            publish_island_bridge: false,
        })
    }
    pub fn open_island(root: impl Into<PathBuf>) -> Result<Self> {
        let mut store = Self::open(root)?;
        store.publish_island_bridge = true;
        Ok(store)
    }
    pub fn is_island(&self) -> bool {
        self.publish_island_bridge
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn semantic_path(&self) -> PathBuf {
        self.root.join(SEMANTIC_FILE)
    }

    fn read_bounded(&self, relative: &str, limit: usize) -> Result<Vec<u8>> {
        read_granted_file(&self.root, relative, limit)
    }

    pub fn load(&self) -> Result<Snapshot> {
        let bytes = self.read_bounded(SEMANTIC_FILE, MAX_PROJECT_BYTES)?;
        let project = validate::parse(&bytes)?;
        let source_sha256 = security::sha256(&bytes);
        let generated = compiler::compile(&project)?;
        Ok(Snapshot {
            project,
            source_sha256,
            generated,
            generated_dir: None,
        })
    }

    pub fn create(&self, project: &Project) -> Result<Snapshot> {
        if self.semantic_path().exists() {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Managed project already exists",
            ));
        }
        validate::project_valid(project)?;
        self.commit_inner(None, project)
    }

    pub fn commit(&self, expected_source_sha256: &str, project: &Project) -> Result<Snapshot> {
        if !security::digest(expected_source_sha256) {
            return Err(Error::invalid("Expected source fingerprint is malformed"));
        }
        self.commit_inner(Some(expected_source_sha256), project)
    }

    fn validate_island_bridge_target(&self) -> Result<()> {
        if !self.publish_island_bridge {
            return Ok(());
        }
        let final_path = self.root.join(ISLAND_BRIDGE);
        match fs::symlink_metadata(&final_path) {
            Ok(metadata) => {
                if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
                    return Err(Error::new(
                        ErrorCode::PermissionDenied,
                        "Managed island bridge path is not a regular owned file",
                    ));
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }

    fn commit_inner(&self, expected: Option<&str>, project: &Project) -> Result<Snapshot> {
        validate::project_valid(project)?;
        let bytes = serde_json::to_vec_pretty(project)?;
        if bytes.len() > MAX_PROJECT_BYTES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Managed project exceeds byte budget",
            ));
        }
        let generated = compiler::compile(project)?;
        if let Some(expected) = expected {
            let current = self.read_bounded(SEMANTIC_FILE, MAX_PROJECT_BYTES)?;
            if security::sha256(&current) != expected {
                return Err(Error::new(
                    ErrorCode::StaleReference,
                    "semwright-motion.json changed since inspection",
                ));
            }
        }
        // A managed-island bridge is a derived integration surface, but a hostile
        // path must be detected before the authoritative semantic file changes.
        self.validate_island_bridge_target()?;
        let generated_dir = self.materialize_generated(project, &generated)?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let tmp = self.root.join(format!(".semwright-motion-{token}.tmp"));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let result = (|| -> Result<()> {
            let mut f = options.open(&tmp)?;
            f.write_all(&bytes)?;
            f.sync_all()?;
            let verify = fs::read(&tmp)?;
            let parsed = validate::parse(&verify)?;
            compiler::compile(&parsed)?;
            if let Some(expected) = expected {
                let current = self.read_bounded(SEMANTIC_FILE, MAX_PROJECT_BYTES)?;
                if security::sha256(&current) != expected {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "semwright-motion.json changed during commit",
                    ));
                }
            } else if self.semantic_path().exists() {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Managed project appeared during create",
                ));
            }
            fs::rename(&tmp, self.semantic_path())?;
            #[cfg(unix)]
            {
                File::open(&self.root)?.sync_all()?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result?;
        if self.publish_island_bridge {
            self.publish_island_bridge(project, &generated)?;
        }
        Ok(Snapshot {
            project: project.clone(),
            source_sha256: security::sha256(&bytes),
            generated,
            generated_dir: Some(generated_dir),
        })
    }

    fn island_bridge_source(&self, project: &Project, generated: &Generated) -> Result<Vec<u8>> {
        let generated_fingerprint = generated.fingerprint()?;
        let source_fingerprint = security::sha256(&serde_json::to_vec_pretty(project)?);
        let generated_dir = format!(".semwright-generated-{generated_fingerprint}");
        let mut source = String::from(
            "// Generated by Semwright. Import only the named scenes you need; do not edit.\n",
        );
        source.push_str(&format!(
            "export const semwrightMotionRevision = {};\nexport const semwrightMotionFingerprint = {};\nexport const semwrightGeneratedFingerprint = {};\n",
            project.revision,
            security::js_string(&source_fingerprint),
            security::js_string(&generated_fingerprint)
        ));
        source.push_str("export const semwrightSceneIds = [");
        for (index, scene) in project.scenes.iter().enumerate() {
            if index > 0 {
                source.push(',');
            }
            source.push_str(&security::js_string(&scene.id));
        }
        source.push_str("] as const;\n");
        for (index, scene) in project.scenes.iter().enumerate() {
            let specifier = format!("./{generated_dir}/src/scenes/{}?scene", scene.id);
            source.push_str(&format!(
                "export {{default as scene{index}}} from {};\n",
                security::js_string(&specifier)
            ));
        }
        Ok(source.into_bytes())
    }

    fn publish_island_bridge(&self, project: &Project, generated: &Generated) -> Result<()> {
        let bytes = self.island_bridge_source(project, generated)?;
        let token = uuid::Uuid::new_v4().simple().to_string();
        let tmp_name = format!(".managed-scenes-{token}.tmp");
        let tmp = self.root.join(&tmp_name);
        let final_path = self.root.join(ISLAND_BRIDGE);
        self.validate_island_bridge_target()?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let result = (|| -> Result<()> {
            let mut file = options.open(&tmp)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            fs::rename(&tmp, &final_path)?;
            #[cfg(unix)]
            File::open(&self.root)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result
    }

    pub fn island_bridge(&self) -> Result<Option<String>> {
        if !self.publish_island_bridge {
            return Ok(None);
        }
        match read_granted_file(&self.root, ISLAND_BRIDGE, MAX_PROJECT_BYTES) {
            Ok(bytes) => String::from_utf8(bytes)
                .map(Some)
                .map_err(|_| Error::new(ErrorCode::Conflict, "Managed island bridge is not UTF-8")),
            Err(error) if error.code == ErrorCode::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub fn install_asset(&self, relative: &str, bytes: &[u8]) -> Result<()> {
        security::relative_path(relative)?;
        if !relative.starts_with("assets/") || bytes.is_empty() || bytes.len() > MAX_ASSET_BYTES {
            return Err(Error::invalid("Managed asset path or byte size is invalid"));
        }
        let path = self.root.join(relative);
        if path.try_exists()? {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Managed asset destination already exists",
            ));
        }
        write_owned(&self.root, relative, bytes)?;
        if let Some(parent) = path.parent() {
            File::open(parent)?.sync_all()?;
        }
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }

    pub fn remove_asset_if_matches(&self, relative: &str, sha256: &str) -> Result<()> {
        security::relative_path(relative)?;
        if !relative.starts_with("assets/") || !security::digest(sha256) {
            return Err(Error::invalid(
                "Managed asset rollback parameters are invalid",
            ));
        }
        let path = self.root.join(relative);
        if !path.try_exists()? {
            return Ok(());
        }
        let bytes = self.read_bounded(relative, MAX_ASSET_BYTES)?;
        if security::sha256(&bytes) != sha256 {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Managed asset changed before rollback",
            ));
        }
        fs::remove_file(&path)?;
        if let Some(parent) = path.parent() {
            File::open(parent)?.sync_all()?;
        }
        File::open(&self.root)?.sync_all()?;
        Ok(())
    }

    pub fn materialize(&self, snapshot: &Snapshot) -> Result<PathBuf> {
        self.materialize_generated(&snapshot.project, &snapshot.generated)
    }

    fn materialize_generated(&self, project: &Project, generated: &Generated) -> Result<PathBuf> {
        let fingerprint = generated.fingerprint()?;
        let final_name = format!(".semwright-generated-{fingerprint}");
        let final_dir = self.root.join(&final_name);
        if final_dir.exists() {
            self.verify_generated(project, generated, &final_dir)?;
            return Ok(final_dir);
        }
        let stage = self.root.join(format!(
            ".semwright-stage-{}",
            uuid::Uuid::new_v4().simple()
        ));
        fs::create_dir(&stage)?;
        let result = (|| -> Result<()> {
            for (relative, bytes) in &generated.files {
                write_owned(&stage, relative, bytes)?;
            }
            for asset in &project.assets {
                let bytes = self.read_bounded(&asset.path, MAX_ASSET_BYTES)?;
                if bytes.len() as u64 != asset.bytes || security::sha256(&bytes) != asset.sha256 {
                    return Err(Error::new(
                        ErrorCode::Conflict,
                        "Managed asset hash or byte length changed",
                    ));
                }
                write_owned(&stage, &asset.path, &bytes)?;
            }
            sync_tree(&stage)?;
            fs::rename(&stage, &final_dir)?;
            #[cfg(unix)]
            {
                File::open(&self.root)?.sync_all()?;
            }
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_dir_all(&stage);
        }
        result?;
        self.verify_generated(project, generated, &final_dir)?;
        Ok(final_dir)
    }

    fn verify_generated(&self, project: &Project, generated: &Generated, dir: &Path) -> Result<()> {
        let meta = fs::symlink_metadata(dir)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Generated tree path is not a real directory",
            ));
        }
        for item in generated.inventory() {
            let bytes = read_owned(dir, &item.path, item.bytes as usize)?;
            if security::sha256(&bytes) != item.sha256 {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Generated tree does not match deterministic compiler output",
                ));
            }
        }
        for asset in &project.assets {
            let bytes = read_owned(dir, &asset.path, MAX_ASSET_BYTES)?;
            if bytes.len() as u64 != asset.bytes || security::sha256(&bytes) != asset.sha256 {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Generated asset copy does not match semantic hash",
                ));
            }
        }
        Ok(())
    }
}

pub(crate) fn read_granted_file(root: &Path, relative: &str, limit: usize) -> Result<Vec<u8>> {
    security::relative_path(relative)?;
    let root = fs::canonicalize(root).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::new(ErrorCode::NotFound, "Granted filesystem root is missing")
        } else {
            Error::from(error)
        }
    })?;
    if !root.is_dir() {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Granted filesystem root is not a directory",
        ));
    }
    let path = root.join(relative);
    let canonical = fs::canonicalize(&path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Error::new(ErrorCode::NotFound, "Granted file is missing")
        } else {
            Error::from(error)
        }
    })?;
    if !canonical.starts_with(&root) {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Granted file escapes its filesystem root",
        ));
    }
    let meta = fs::symlink_metadata(&path)?;
    if !meta.file_type().is_file() || meta.file_type().is_symlink() || meta.len() > limit as u64 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Granted input must be a bounded regular non-symlink file",
        ));
    }
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)?
    };
    #[cfg(not(unix))]
    let file = File::open(&path)?;
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Granted file exceeds byte budget",
        ));
    }
    Ok(bytes)
}

fn checked_parts(relative: &str) -> Result<Vec<&str>> {
    security::relative_path(relative)?;
    Ok(relative.split('/').collect())
}
fn write_owned(root: &Path, relative: &str, bytes: &[u8]) -> Result<()> {
    let parts = checked_parts(relative)?;
    let mut dir = root.to_path_buf();
    for part in &parts[..parts.len() - 1] {
        dir.push(part);
        if dir.exists() {
            let m = fs::symlink_metadata(&dir)?;
            if !m.is_dir() || m.file_type().is_symlink() {
                return Err(Error::new(
                    ErrorCode::PermissionDenied,
                    "Generated path contains a symlink",
                ));
            }
        } else {
            fs::create_dir(&dir)?;
        }
    }
    let path = dir.join(parts[parts.len() - 1]);
    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    }
    let mut f = opts.open(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    Ok(())
}
fn read_owned(root: &Path, relative: &str, limit: usize) -> Result<Vec<u8>> {
    let parts = checked_parts(relative)?;
    let mut path = root.to_path_buf();
    for part in parts {
        path.push(part);
    }
    let canonical = fs::canonicalize(&path)?;
    if !canonical.starts_with(root) {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Generated path escapes tree",
        ));
    }
    let meta = fs::symlink_metadata(&path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > limit as u64 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Generated file is not a bounded regular file",
        ));
    }
    let mut data = vec![];
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut data)?;
    if data.len() > limit {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Generated file exceeds budget",
        ));
    }
    Ok(data)
}
fn sync_tree(root: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        for entry in fs::read_dir(root)? {
            let p = entry?.path();
            if p.is_dir() {
                sync_tree(&p)?;
            }
        }
        File::open(root)?.sync_all()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Project {
        validate::parse(include_bytes!(
            "../../../fixtures/motion-canvas/hello-text/semwright-motion.json"
        ))
        .unwrap()
    }

    #[test]
    fn load_and_dry_read_do_not_materialize_generated_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(SEMANTIC_FILE),
            serde_json::to_vec_pretty(&fixture()).unwrap(),
        )
        .unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let before = fs::read_dir(&root).unwrap().count();
        let snapshot = ProjectStore::open(root.clone()).unwrap().load().unwrap();
        assert!(snapshot.generated_dir.is_none());
        assert_eq!(before, fs::read_dir(&root).unwrap().count());
    }

    #[test]
    fn create_materializes_content_addressed_generated_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let snapshot = ProjectStore::open(root.clone())
            .unwrap()
            .create(&fixture())
            .unwrap();
        assert!(root.join(SEMANTIC_FILE).is_file());
        let generated = snapshot.generated_dir.unwrap();
        assert!(generated.starts_with(&root));
        assert!(generated.join("src/project.ts").is_file());
        assert!(generated.join("src/semwright-exporter.ts").is_file());
    }

    #[test]
    fn stale_external_edit_is_rejected_before_commit() {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let store = ProjectStore::open(root.clone()).unwrap();
        let original = store.create(&fixture()).unwrap();
        let mut changed = original.project.clone();
        changed.revision += 1;
        changed.theme.spacing += 1.0;
        let external = serde_json::to_vec_pretty(&changed).unwrap();
        fs::write(root.join(SEMANTIC_FILE), &external).unwrap();
        let err = store.commit(&original.source_sha256, &changed).unwrap_err();
        assert_eq!(err.code, ErrorCode::StaleReference);
        assert_eq!(fs::read(root.join(SEMANTIC_FILE)).unwrap(), external);
    }
}

#[cfg(test)]
mod store_tests {
    use super::*;
    use crate::model::{Node, NodeKind, Properties, Scene};

    fn fixture() -> Project {
        let mut p = Project::empty("store-fixture".into());
        p.generation = "0123456789abcdef0123456789abcdef".into();
        p.scenes.push(Scene {
            id: "main".into(),
            name: "Main".into(),
            duration_ms: 1000,
            nodes: vec![Node {
                id: "title".into(),
                name: "Title".into(),
                kind: NodeKind::Text,
                parent: None,
                properties: Properties {
                    text: Some("atomic".into()),
                    ..Default::default()
                },
            }],
            animations: vec![],
            cues: vec![],
            transition: None,
        });
        p
    }
    fn store(temp: &tempfile::TempDir) -> ProjectStore {
        ProjectStore::open(std::fs::canonicalize(temp.path()).unwrap()).unwrap()
    }

    #[test]
    fn create_load_and_generated_tree_are_consistent() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        let p = fixture();
        let saved = store.create(&p).unwrap();
        assert!(saved.generated_dir.as_ref().unwrap().is_dir());
        let loaded = store.load().unwrap();
        assert_eq!(loaded.project, p);
        assert_eq!(loaded.source_sha256, saved.source_sha256);
        assert!(loaded.generated_dir.is_none());
        assert_eq!(
            store.materialize(&loaded).unwrap(),
            saved.generated_dir.unwrap()
        );
    }

    #[test]
    fn stale_commit_preserves_authoritative_file() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        let p = fixture();
        let saved = store.create(&p).unwrap();
        let before = std::fs::read(store.semantic_path()).unwrap();
        let mut changed = p.clone();
        changed.revision = 2;
        let error = store.commit(&"f".repeat(64), &changed).unwrap_err();
        assert_eq!(error.code, ErrorCode::StaleReference);
        assert_eq!(std::fs::read(store.semantic_path()).unwrap(), before);
        assert_eq!(store.load().unwrap().source_sha256, saved.source_sha256);
    }

    #[test]
    fn failed_validation_never_replaces_source_of_truth() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        let p = fixture();
        let saved = store.create(&p).unwrap();
        let before = std::fs::read(store.semantic_path()).unwrap();
        let mut invalid = p.clone();
        invalid.settings.fps = 0;
        assert!(store.commit(&saved.source_sha256, &invalid).is_err());
        assert_eq!(std::fs::read(store.semantic_path()).unwrap(), before);
    }

    #[cfg(unix)]
    #[test]
    fn semantic_symlink_escape_is_rejected() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), serde_json::to_vec(&fixture()).unwrap()).unwrap();
        symlink(outside.path(), temp.path().join(SEMANTIC_FILE)).unwrap();
        let error = store(&temp).load().unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }

    #[test]
    fn island_bridge_points_to_materialized_scene_and_hostile_path_preserves_source() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        let store = ProjectStore::open_island(root.clone()).unwrap();
        let project = fixture();
        let saved = store.create(&project).unwrap();
        let bridge = store.island_bridge().unwrap().unwrap();
        let specifier = bridge
            .lines()
            .find_map(|line| line.split(" from ").nth(1))
            .and_then(|quoted| serde_json::from_str::<String>(quoted.trim_end_matches(';')).ok())
            .unwrap();
        let target = specifier
            .trim_start_matches("./")
            .split('?')
            .next()
            .unwrap();
        assert!(root.join(format!("{target}.tsx")).is_file());

        let before = std::fs::read(store.semantic_path()).unwrap();
        std::fs::remove_file(root.join(ISLAND_BRIDGE)).unwrap();
        std::fs::create_dir(root.join(ISLAND_BRIDGE)).unwrap();
        let mut changed = project.clone();
        changed.revision = 2;
        changed.scenes[0].name = "Changed".into();
        let error = store.commit(&saved.source_sha256, &changed).unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(std::fs::read(store.semantic_path()).unwrap(), before);
    }

    #[test]
    fn generated_tree_tamper_is_detected_not_reused() {
        let temp = tempfile::tempdir().unwrap();
        let store = store(&temp);
        let p = fixture();
        let saved = store.create(&p).unwrap();
        let dir = saved.generated_dir.unwrap();
        std::fs::write(dir.join("src/project.ts"), b"tampered").unwrap();
        let loaded = store.load().unwrap();
        let error = store.materialize(&loaded).unwrap_err();
        assert_eq!(error.code, ErrorCode::Conflict);
    }
}
