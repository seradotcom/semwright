//! Private SQLite journal; local transactions do not make external app writes atomic.
use crate::graph::GraphEvent;
use crate::*;
use composition::{Digest, PrincipalBinding, canonical_bytes, strict_decode};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Duration;
const MAX_DB_BYTES: u64 = 64 * 1024 * 1024;
const MAX_LOG_ROWS: usize = 100_000;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    version: u32,
    project: ProjectId,
    principal: PrincipalBinding,
}
#[derive(Debug, Clone, Serialize)]
pub struct RecoveryReport {
    pub journal_records: u64,
    pub assets: usize,
    pub indexes_rebuilt: bool,
    pub observation_epoch_reset: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Boundary {
    BeforeJournal,
    AfterJournal,
    AfterIndexes,
    BeforeCommit,
    AfterCommit,
}
pub struct GraphStore {
    connection: Connection,
    graph: ProjectGraph,
    directory: PathBuf,
    poisoned: bool,
    report: RecoveryReport,
}
fn storage_error(error: rusqlite::Error) -> GraphError {
    match error {
        rusqlite::Error::SqliteFailure(e, _)
            if matches!(
                e.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            GraphError::Conflict
        }
        rusqlite::Error::SqliteFailure(e, _)
            if matches!(
                e.code,
                rusqlite::ErrorCode::DatabaseCorrupt | rusqlite::ErrorCode::NotADatabase
            ) =>
        {
            GraphError::Corrupt
        }
        _ => GraphError::Storage,
    }
}
fn chain(previous: &Digest, sequence: u64, bytes: &[u8]) -> Result<Digest> {
    Ok(composition::canonical_digest(&(
        "project-graph-journal-v1",
        previous,
        sequence,
        Digest::of_bytes(bytes),
    ))?)
}
fn check_private_file(path: &Path) -> Result<()> {
    let metadata = std::fs::symlink_metadata(path)?;
    ensure(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= MAX_DB_BYTES,
        "private bounded graph database required",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure(
            metadata.uid() == semwright_platform_services::current_uid()
                && metadata.nlink() == 1
                && metadata.mode() & 0o777 == 0o600,
            "private graph database ownership",
        )?;
    }
    #[cfg(target_os = "windows")]
    semwright_platform_services::verify_private_data_file(path, MAX_DB_BYTES)
        .map_err(|_| GraphError::Denied)?;
    Ok(())
}
impl GraphStore {
    /// Host configuration only: path must be under the protected daemon state directory.
    pub fn open(
        directory: &Path,
        project: ProjectId,
        principal: PrincipalBinding,
        repair_indexes: bool,
    ) -> Result<Self> {
        semwright_platform_services::private_data_directory(directory)
            .map_err(|_| GraphError::Denied)?;
        ensure(directory.is_absolute(), "absolute private state directory")?;
        #[cfg(unix)]
        ensure(
            std::fs::canonicalize(directory)? == directory,
            "canonical private state directory",
        )?;
        #[cfg(target_os = "windows")]
        {
            // Windows canonicalization commonly returns an extended \\?\ spelling.
            // The private-directory primitive already rejects reparse points; keep
            // the normal absolute spelling for SQLite and scoped Windows APIs.
            let _ = std::fs::canonicalize(directory)?;
        }
        let path = directory.join("project.sqlite3");
        let existed = path.try_exists()?;
        #[cfg(unix)]
        if !existed {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
            options.open(&path)?.sync_all()?;
        }
        // Existing Windows stores are verified before SQLite receives the path.
        // New Windows files inherit the protected owner-only parent DACL and are
        // verified on every subsequent reopen. Unix pre-creates mode 0600 above.
        #[cfg(unix)]
        check_private_file(&path)?;
        #[cfg(target_os = "windows")]
        if existed {
            check_private_file(&path)?;
        }
        for suffix in [
            "project.sqlite3-journal",
            "project.sqlite3-wal",
            "project.sqlite3-shm",
        ] {
            let p = directory.join(suffix);
            if std::fs::symlink_metadata(&p).is_ok() {
                check_private_file(&p)?;
            }
        }
        let mut flags = OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NOFOLLOW
            | OpenFlags::SQLITE_OPEN_NO_MUTEX;
        if !existed {
            flags |= OpenFlags::SQLITE_OPEN_CREATE;
        }
        let mut connection = Connection::open_with_flags(&path, flags).map_err(storage_error)?;
        connection
            .busy_timeout(Duration::from_millis(1000))
            .map_err(storage_error)?;
        connection.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON; PRAGMA journal_mode=DELETE; PRAGMA synchronous=EXTRA; PRAGMA mmap_size=0; PRAGMA temp_store=MEMORY; PRAGMA page_size=4096; PRAGMA max_page_count=16384; PRAGMA journal_size_limit=1048576;").map_err(storage_error)?;
        for (pragma, expected) in [
            ("synchronous", 3i64),
            ("foreign_keys", 1),
            ("trusted_schema", 0),
            ("page_size", 4096),
            ("max_page_count", 16384),
        ] {
            let actual: i64 = connection
                .pragma_query_value(None, pragma, |r| r.get(0))
                .map_err(storage_error)?;
            ensure(
                actual == expected,
                "SQLite durability/budget pragma mismatch",
            )?;
        }
        let mode: String = connection
            .pragma_query_value(None, "journal_mode", |r| r.get(0))
            .map_err(storage_error)?;
        ensure(mode == "delete", "bounded rollback journal required")?;
        let version: u32 = connection
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .map_err(storage_error)?;
        if version == 0 {
            let count: i64 = connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'",
                    [],
                    |r| r.get(0),
                )
                .map_err(storage_error)?;
            ensure(count == 0, "unrecognized pre-schema database preserved")?;
            let tx = connection.transaction().map_err(storage_error)?;
            tx.execute_batch("CREATE TABLE header (id INTEGER PRIMARY KEY CHECK(id=1), payload BLOB NOT NULL); CREATE TABLE journal (sequence INTEGER PRIMARY KEY CHECK(sequence>0), previous TEXT NOT NULL, digest TEXT NOT NULL UNIQUE, payload BLOB NOT NULL CHECK(length(payload)<=524288)); CREATE TABLE assets (id TEXT PRIMARY KEY, digest TEXT NOT NULL, payload BLOB NOT NULL); PRAGMA application_id=1398228785; PRAGMA user_version=1;").map_err(storage_error)?;
            let header = Header {
                version: SCHEMA_VERSION,
                project: project.clone(),
                principal: principal.clone(),
            };
            tx.execute(
                "INSERT INTO header VALUES(1,?1)",
                [canonical_bytes(&header)?],
            )
            .map_err(storage_error)?;
            tx.commit().map_err(storage_error)?;
        } else {
            ensure(
                version == SCHEMA_VERSION,
                "unsupported store version; no destructive migration",
            )?;
        }
        let app: u32 = connection
            .pragma_query_value(None, "application_id", |r| r.get(0))
            .map_err(storage_error)?;
        ensure(app == 1398228785, "database application identity")?;
        let (graph, report) = Self::load(&mut connection, &project, &principal, repair_indexes)?;
        Ok(Self {
            connection,
            graph,
            directory: directory.to_owned(),
            poisoned: false,
            report,
        })
    }
    fn load(
        connection: &mut Connection,
        project: &ProjectId,
        principal: &PrincipalBinding,
        repair: bool,
    ) -> Result<(ProjectGraph, RecoveryReport)> {
        let tx = connection.transaction().map_err(storage_error)?;
        let integrity: String = tx
            .query_row("PRAGMA quick_check", [], |r| r.get(0))
            .map_err(storage_error)?;
        ensure(
            integrity == "ok",
            "SQLite integrity check failed; preserve store",
        )?;
        let objects: Vec<(String, String)> = {
            let mut s = tx.prepare("SELECT name,type FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY name").map_err(storage_error)?;
            s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                .map_err(storage_error)?
                .collect::<std::result::Result<_, _>>()
                .map_err(storage_error)?
        };
        ensure(
            objects
                == vec![
                    ("assets".into(), "table".into()),
                    ("header".into(), "table".into()),
                    ("journal".into(), "table".into()),
                ],
            "unexpected graph database objects",
        )?;
        let header_bytes: Vec<u8> = tx
            .query_row(
                "SELECT payload FROM header WHERE id=1 AND length(payload)<=524288",
                [],
                |r| r.get(0),
            )
            .map_err(storage_error)?;
        let header: Header = strict_decode(&header_bytes)?;
        if header.version != SCHEMA_VERSION
            || header.project != *project
            || header.principal != *principal
        {
            return Err(GraphError::Denied);
        }
        let count: i64 = tx
            .query_row("SELECT count(*) FROM journal", [], |r| r.get(0))
            .map_err(storage_error)?;
        ensure(
            (0..=MAX_LOG_ROWS as i64).contains(&count),
            "journal row budget",
        )?;
        let mut graph = ProjectGraph::new(project.clone(), principal.clone())?;
        let mut previous = Digest::of_bytes(&header_bytes);
        {
            let mut stmt = tx
                .prepare("SELECT sequence,previous,digest,payload FROM journal ORDER BY sequence")
                .map_err(storage_error)?;
            let mut rows = stmt.query([]).map_err(storage_error)?;
            while let Some(row) = rows.next().map_err(storage_error)? {
                let sequence = u64::try_from(row.get::<_, i64>(0).map_err(storage_error)?)
                    .map_err(|_| GraphError::Corrupt)?;
                let before: String = row.get(1).map_err(storage_error)?;
                let digest: String = row.get(2).map_err(storage_error)?;
                let bytes: Vec<u8> = row.get(3).map_err(storage_error)?;
                ensure(
                    sequence == graph.sequence + 1 && before == previous.as_str(),
                    "journal sequence/hash chain",
                )?;
                let expected = chain(&previous, sequence, &bytes)?;
                ensure(digest == expected.as_str(), "journal payload checksum")?;
                let event: GraphEvent = strict_decode(&bytes)?;
                graph.apply(event, false)?;
                ensure(
                    graph.sequence == sequence,
                    "journal contains noncanonical duplicate event",
                )?;
                graph.pending.clear();
                previous = expected;
            }
        }
        let indexed: i64 = tx
            .query_row("SELECT count(*) FROM assets", [], |r| r.get(0))
            .map_err(storage_error)?;
        let mut mismatch = indexed != graph.assets.len() as i64;
        for (id, asset) in &graph.assets {
            let expected = canonical_bytes(asset)?;
            let stored: Option<(String, Vec<u8>)> = tx
                .query_row(
                    "SELECT digest,payload FROM assets WHERE id=?1",
                    [id.as_str()],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(storage_error)?;
            mismatch |= stored.is_none_or(|(hash, bytes)| {
                hash != Digest::of_bytes(&expected).as_str() || bytes != expected
            });
        }
        if mismatch {
            if !repair {
                return Err(GraphError::Corrupt);
            }
            Self::write_indexes(&tx, &graph)?;
        }
        let report = RecoveryReport {
            journal_records: graph.sequence,
            assets: graph.assets.len(),
            indexes_rebuilt: mismatch,
            observation_epoch_reset: true,
        };
        tx.commit().map_err(storage_error)?;
        Ok((graph, report))
    }
    fn write_indexes(tx: &rusqlite::Transaction<'_>, graph: &ProjectGraph) -> Result<()> {
        tx.execute("DELETE FROM assets", [])
            .map_err(storage_error)?;
        let mut stmt = tx
            .prepare_cached("INSERT INTO assets(id,digest,payload) VALUES(?1,?2,?3)")
            .map_err(storage_error)?;
        for (id, state) in &graph.assets {
            let bytes = canonical_bytes(state)?;
            stmt.execute(params![
                id.as_str(),
                Digest::of_bytes(&bytes).as_str(),
                bytes
            ])
            .map_err(storage_error)?;
        }
        Ok(())
    }
    pub fn recovery_report(&self) -> &RecoveryReport {
        &self.report
    }
    pub fn graph(&self) -> Result<&ProjectGraph> {
        if self.poisoned {
            Err(GraphError::Conflict)
        } else {
            Ok(&self.graph)
        }
    }
    pub fn transact<T>(
        &mut self,
        access: &ProjectAccess,
        operation: impl FnOnce(&mut ProjectGraph) -> Result<T>,
    ) -> Result<T> {
        self.transact_inner(access, operation, None)
    }
    fn transact_inner<T>(
        &mut self,
        access: &ProjectAccess,
        operation: impl FnOnce(&mut ProjectGraph) -> Result<T>,
        fault: Option<Boundary>,
    ) -> Result<T> {
        self.graph()?.access(access, true)?;
        let mut candidate = self.graph.clone();
        candidate.pending.clear();
        let output = operation(&mut candidate)?;
        ensure(
            candidate.sequence <= MAX_LOG_ROWS as u64,
            "journal history capacity",
        )?;
        if candidate.pending.is_empty() {
            return Ok(output);
        }
        let payloads = candidate
            .pending
            .iter()
            .map(canonical_bytes)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure(
            payloads.len() <= 4096
                && payloads.iter().map(Vec::len).sum::<usize>() <= 16 * 1024 * 1024,
            "transaction event/byte budget",
        )?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage_error)?;
        let head: i64 = tx
            .query_row("SELECT coalesce(max(sequence),0) FROM journal", [], |r| {
                r.get(0)
            })
            .map_err(storage_error)?;
        if head < 0 || head as u64 != self.graph.sequence {
            return Err(GraphError::Conflict);
        }
        if fault == Some(Boundary::BeforeJournal) {
            return Err(GraphError::Conflict);
        }
        let mut previous = if head == 0 {
            let bytes: Vec<u8> = tx
                .query_row("SELECT payload FROM header WHERE id=1", [], |r| r.get(0))
                .map_err(storage_error)?;
            Digest::of_bytes(&bytes)
        } else {
            let text: String = tx
                .query_row(
                    "SELECT digest FROM journal WHERE sequence=?1",
                    [head],
                    |r| r.get(0),
                )
                .map_err(storage_error)?;
            Digest::parse(text)?
        };
        for (index, bytes) in payloads.iter().enumerate() {
            let sequence = head + index as i64 + 1;
            let digest = chain(
                &previous,
                u64::try_from(sequence).map_err(|_| GraphError::Corrupt)?,
                bytes,
            )?;
            tx.execute(
                "INSERT INTO journal(sequence,previous,digest,payload) VALUES(?1,?2,?3,?4)",
                params![sequence, previous.as_str(), digest.as_str(), bytes],
            )
            .map_err(storage_error)?;
            previous = digest;
        }
        if fault == Some(Boundary::AfterJournal) {
            return Err(GraphError::Conflict);
        }
        Self::update_indexes(&tx, &candidate)?;
        if fault == Some(Boundary::AfterIndexes) || fault == Some(Boundary::BeforeCommit) {
            return Err(GraphError::Conflict);
        }
        if let Err(error) = tx.commit() {
            self.poisoned = true;
            return Err(storage_error(error));
        }
        if fault == Some(Boundary::AfterCommit) {
            self.poisoned = true;
            return Err(GraphError::Conflict);
        }
        candidate.pending.clear();
        self.graph = candidate;
        Ok(output)
    }
    fn update_indexes(tx: &rusqlite::Transaction<'_>, graph: &ProjectGraph) -> Result<()> {
        let mut ids = std::collections::BTreeSet::new();
        for event in &graph.pending {
            match event {
                GraphEvent::Register(a) => {
                    ids.insert(a.id.clone());
                }
                GraphEvent::BindInstance { id, .. }
                | GraphEvent::Rename { id, .. }
                | GraphEvent::Rebind { id, .. }
                | GraphEvent::Probe { id, .. }
                | GraphEvent::Tombstone(id) => {
                    ids.insert(id.clone());
                }
                GraphEvent::Observe(r) => {
                    ids.insert(r.pin.asset.clone());
                }
                GraphEvent::Receipt(r) => {
                    ids.extend(r.outputs.iter().map(|p| p.asset.clone()));
                }
                GraphEvent::Gap(values) => ids.extend(values.iter().cloned()),
                _ => (),
            }
        }
        let mut stmt = tx.prepare_cached("INSERT INTO assets(id,digest,payload) VALUES(?1,?2,?3) ON CONFLICT(id) DO UPDATE SET digest=excluded.digest,payload=excluded.payload").map_err(storage_error)?;
        for id in ids {
            let state = graph.assets.get(&id).ok_or(GraphError::Corrupt)?;
            let bytes = canonical_bytes(state)?;
            stmt.execute(params![
                id.as_str(),
                Digest::of_bytes(&bytes).as_str(),
                bytes
            ])
            .map_err(storage_error)?;
        }
        Ok(())
    }
    /// One bounded backup slot. Existing backups are preserved, never overwritten.
    pub fn backup(&mut self, access: &ProjectAccess) -> Result<BackupManifest> {
        self.graph()?.access(access, true)?;
        if access.visible.is_some() {
            return Err(GraphError::Denied);
        }
        let path = self.directory.join("backup.sqlite3");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
        }
        options.open(&path)?.sync_all()?;
        check_private_file(&path)?;
        let mut destination = Connection::open_with_flags(
            &path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )
        .map_err(storage_error)?;
        {
            let backup = rusqlite::backup::Backup::new(&self.connection, &mut destination)
                .map_err(storage_error)?;
            let start = std::time::Instant::now();
            loop {
                if start.elapsed() > Duration::from_secs(5) {
                    return Err(GraphError::Limit("backup time; incomplete slot preserved"));
                }
                match backup.step(128).map_err(storage_error)? {
                    rusqlite::backup::StepResult::Done => break,
                    rusqlite::backup::StepResult::More => (),
                    _ => return Err(GraphError::Conflict),
                }
            }
        }
        let (_, report) = Self::load(
            &mut destination,
            &self.graph.project,
            &self.graph.principal,
            false,
        )?;
        destination.close().map_err(|(_, e)| storage_error(e))?;
        let bytes = read_private(&path)?;
        Ok(BackupManifest {
            version: SCHEMA_VERSION,
            project: self.graph.project.clone(),
            sequence: report.journal_records,
            bytes: bytes.len() as u64,
            sha256: Digest::of_bytes(&bytes),
        })
    }
    /// Restore into a NEW private directory only. Never replace live evidence.
    pub fn restore(
        backup_directory: &Path,
        destination: &Path,
        principal: PrincipalBinding,
        manifest: &BackupManifest,
    ) -> Result<Self> {
        ensure(
            manifest.version == SCHEMA_VERSION && manifest.bytes <= MAX_DB_BYTES,
            "backup schema/budget",
        )?;
        semwright_platform_services::private_data_directory(backup_directory)
            .map_err(|_| GraphError::Denied)?;
        let bytes = read_private(&backup_directory.join("backup.sqlite3"))?;
        ensure(
            bytes.len() as u64 == manifest.bytes && Digest::of_bytes(&bytes) == manifest.sha256,
            "backup checksum",
        )?;
        ensure(
            !destination.try_exists()?,
            "restore must preserve existing state",
        )?;
        semwright_platform_services::private_data_directory(destination)
            .map_err(|_| GraphError::Denied)?;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        use std::io::Write;
        let mut file = options.open(destination.join("project.sqlite3"))?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        let restored = Self::open(destination, manifest.project.clone(), principal, false)?;
        ensure(
            restored.graph.sequence == manifest.sequence,
            "backup index/journal revision",
        )?;
        Ok(restored)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BackupManifest {
    pub version: u32,
    pub project: ProjectId,
    pub sequence: u64,
    pub bytes: u64,
    pub sha256: Digest,
}
fn read_private(path: &Path) -> Result<Vec<u8>> {
    check_private_file(path)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    ensure(
        metadata.is_file() && metadata.len() <= MAX_DB_BYTES,
        "bounded backup file",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure(
            metadata.uid() == semwright_platform_services::current_uid()
                && metadata.nlink() == 1
                && metadata.mode() & 0o777 == 0o600,
            "backup ownership",
        )?;
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    file.take(MAX_DB_BYTES + 1).read_to_end(&mut bytes)?;
    ensure(
        bytes.len() as u64 <= MAX_DB_BYTES,
        "backup grew beyond budget",
    )?;
    Ok(bytes)
}
#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
