//! Owner opt-in for private stderr from the daemon's existing DriverProvider.
//! Uses the same retained Root publication boundary as Host stdin custody.
use super::private_custody::{PrivateDirectory, bounded_json};
use clap::Args;
use semwright_daemon::config::RetainedDriverManifest;
use semwright_driver_host::{DriverProvider, DriverStartupStderrCustody};
use semwright_driver_sdk::Manifest;
use semwright_types::{Error, ProviderIdentity, Result};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

const MAX_METADATA: usize = 8 * 1024;

#[derive(Args, Default)]
pub(super) struct Options {
    /// Existing empty private directory; original stderr never enters public logs.
    #[arg(long)]
    driver_startup_stderr_dir: Option<PathBuf>,
    /// One exact owner-configured provider identity, for example driver:motionwright.
    #[arg(long)]
    driver_startup_stderr_provider: Option<String>,
}

impl Options {
    fn request(&self) -> Result<Option<(&Path, &str)>> {
        match (
            &self.driver_startup_stderr_dir,
            &self.driver_startup_stderr_provider,
        ) {
            (None, None) => Ok(None),
            (Some(directory), Some(provider)) => Ok(Some((directory, provider))),
            _ => Err(Error::invalid(
                "Startup stderr custody requires directory and provider together",
            )),
        }
    }

    #[cfg(test)]
    pub(super) fn disabled(&self) -> bool {
        matches!(self.request(), Ok(None))
    }
}

pub(super) struct Bootstrap {
    manifests: Vec<(PathBuf, RetainedDriverManifest)>,
    selected: ProviderIdentity,
    directory: PathBuf,
    sink: DriverStartupStderrCustody,
    connection: Arc<Mutex<Option<Value>>>,
    stop: CancellationToken,
    worker: Option<oneshot::Receiver<()>>,
}

impl Bootstrap {
    pub(super) fn prepare(
        options: &Options,
        fake: bool,
        paths: &[PathBuf],
        load: impl Fn(&Path) -> Result<RetainedDriverManifest>,
    ) -> Result<Option<Self>> {
        let Some((directory, provider)) = options.request()? else {
            return Ok(None);
        };
        if fake {
            return Err(Error::invalid(
                "Fake mode cannot enable startup stderr custody",
            ));
        }
        Self::prepare_selected(directory, provider, paths, load)
            .map(Some)
            .map_err(|error| {
                let mut sanitized =
                    Error::new(error.code, "Private startup stderr custody setup failed");
                sanitized.outcome_known = error.outcome_known;
                sanitized
            })
    }

    fn prepare_selected(
        directory: &Path,
        provider: &str,
        paths: &[PathBuf],
        load: impl Fn(&Path) -> Result<RetainedDriverManifest>,
    ) -> Result<Self> {
        let manifests = paths
            .iter()
            .map(|path| load(path).map(|manifest| (path.clone(), manifest)))
            .collect::<Result<Vec<_>>>()?;
        let mut matches = manifests.iter().filter(|(_, retained)| {
            retained
                .parsed()
                .identity()
                .is_ok_and(|identity| identity.id == provider)
        });
        let (_, retained) = matches
            .next()
            .ok_or_else(|| Error::invalid("Custody must select one configured provider"))?;
        if matches.next().is_some() {
            return Err(Error::invalid("Custody provider selection is ambiguous"));
        }
        let selected = retained.parsed().identity()?;
        let sink = DriverStartupStderrCustody::for_manifest(retained.parsed())?;
        let (_, original) = retained.clone().into_parts();
        let output = Arc::new(PrivateDirectory::open(directory)?);
        let retained_directory = output.path.clone();
        let session = bounded_json(
            &json!({
                "schema": "semwright.driver-startup-stderr-custody/1",
                "provider": selected,
                "executable_sha256": retained.parsed().sha256,
                "max_prefix_bytes": 64 * 1024,
                "max_metadata_bytes": MAX_METADATA,
                "connection_result_observed": false,
                "scope": "private process stderr observation only; no execution or absence-of-effects claim"
            }),
            MAX_METADATA,
        )?;
        // These are real no-replace writes before any selected launch. The same
        // retained manifest bytes are subsequently passed into DriverProvider.
        output.write_new("session.json", &session)?;
        output.write_new("manifest.original.json", &original)?;
        let stop = CancellationToken::new();
        let connection = Arc::new(Mutex::new(None));
        let (finished, worker) = oneshot::channel();
        let observed_sink = sink.clone();
        let observed_connection = connection.clone();
        let observed_stop = stop.clone();
        let thread = std::thread::Builder::new()
            .name("semwright-stderr-custody".into())
            .spawn(move || {
                // Independent of the Tokio runtime, including startup-error unwinding.
                // This collector never owns or closes the child's pipe reader.
                while !observed_stop.is_cancelled() {
                    if observed_sink.drain_finished()
                        && observed_connection
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .is_some()
                    {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                let snapshot = observed_sink.snapshot();
                let connection = observed_connection
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .clone();
                // Publication errors remain custody failures. They never replace an
                // original connection error or create a successful native terminal.
                let _ = publish(&output, &snapshot, connection);
                let _ = finished.send(());
            })
            .map_err(|_| Error::unavailable("Private startup custody collector unavailable"))?;
        drop(thread);
        Ok(Self {
            manifests,
            selected,
            directory: retained_directory,
            sink,
            connection,
            stop,
            worker: Some(worker),
        })
    }

    pub(super) fn directory(&self) -> &Path {
        &self.directory
    }

    pub(super) fn retained_manifest(&self, path: &Path) -> Result<RetainedDriverManifest> {
        self.manifests
            .iter()
            .find(|(candidate, _)| candidate == path)
            .map(|(_, manifest)| manifest.clone())
            .ok_or_else(|| Error::invalid("Startup custody manifest plan changed"))
    }

    pub(super) fn sink_for(&self, manifest: &Manifest) -> Option<DriverStartupStderrCustody> {
        manifest
            .identity()
            .ok()
            .filter(|identity| identity == &self.selected)
            .map(|_| self.sink.clone())
    }

    pub(super) fn connection_result(&self, result: &Result<Arc<DriverProvider>>) {
        let value = match result {
            Ok(_) => json!({"returned": "ok"}),
            // Retain the exact original serialized Error, including outcome_known.
            // This observation does not modify the Result returned by the caller.
            Err(error) => json!({"returned": "error", "error": error}),
        };
        *self
            .connection
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some(value);
    }

    pub(super) async fn finish(&mut self) {
        // Give an already-cleaned-up child a bounded chance to reach EOF. This
        // is outside Hello/native deadlines and never stops or retries a child.
        if self.worker.is_some() && !self.sink.drain_finished() {
            let _ = tokio::time::timeout(Duration::from_millis(200), async {
                while !self.sink.drain_finished() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await;
        }
        self.stop.cancel();
        if let Some(mut worker) = self.worker.take() {
            let _ = tokio::time::timeout(Duration::from_secs(2), &mut worker).await;
        }
    }
}

impl Drop for Bootstrap {
    fn drop(&mut self) {
        // A dropped future requests best-effort partial publication; abrupt
        // process exit may leave only session.json, which never claims closure.
        // Ordinary Ok/Err returns use finish(). The pipe drain remains alive.
        self.stop.cancel();
    }
}

fn publish(
    output: &PrivateDirectory,
    snapshot: &semwright_driver_host::DriverStartupStderrSnapshot,
    connection: Option<Value>,
) -> Result<()> {
    let metadata = bounded_json(
        &json!({
            "schema": "semwright.driver-startup-stderr-capture/1",
            "stderr": snapshot.metadata(),
            "connection_result_observed": connection.is_some(),
            "connection_result": connection,
            "native_effects_absent_asserted": false
        }),
        MAX_METADATA,
    )?;
    output.write_new("stderr.original", snapshot.prefix())?;
    output.write_new("capture.json", &metadata)?;
    output.write_new(
        "closed.json",
        b"{\"custody_publication_complete\":true,\"native_acceptance\":false}\n",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_stderr_selection_is_explicit_and_fake_is_rejected() {
        let mut options = Options::default();
        assert!(options.disabled());
        options.driver_startup_stderr_dir = Some("unused".into());
        assert!(options.request().is_err());
        options.driver_startup_stderr_provider = Some("driver:fixture".into());
        assert!(options.request().unwrap().is_some());
        let result = Bootstrap::prepare(&options, true, &[], |_| {
            panic!("fake must not load a manifest")
        });
        assert!(result.is_err());
    }

    #[test]
    fn startup_stderr_disabled_never_reads_manifests_or_opens_directories() {
        let result = Bootstrap::prepare(&Options::default(), false, &["unused".into()], |_| {
            panic!("disabled")
        });
        assert!(matches!(result, Ok(None)));
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn startup_stderr_retains_original_error_and_manifest_with_incomplete_observation() {
        use std::os::unix::fs::PermissionsExt;
        for outcome_known in [true, false] {
            let parent = tempfile::tempdir().unwrap();
            std::fs::set_permissions(parent.path(), std::fs::Permissions::from_mode(0o700))
                .unwrap();
            let directory = parent.path().join("custody");
            std::fs::create_dir(&directory).unwrap();
            std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
            let source = serde_json::to_vec(&json!({
                "manifest_version": 1, "protocol": 1, "id": "fixture", "version": "1",
                "publisher": "tests", "executable": "/tmp/fixture", "sha256": "a".repeat(64),
                "application": {"desktop_id": "org.example.Fixture"}, "transport": "stdio_v1"
            }))
            .unwrap();
            let path = parent.path().join("manifest.json");
            std::fs::write(&path, &source).unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
            let options = Options {
                driver_startup_stderr_dir: Some(directory.clone()),
                driver_startup_stderr_provider: Some("driver:fixture".into()),
            };
            let mut custody = Bootstrap::prepare(
                &options,
                false,
                std::slice::from_ref(&path),
                semwright_daemon::config::retained_driver_manifest,
            )
            .unwrap()
            .unwrap();
            // Mutation of the owner file cannot replace the retained source used
            // by either capture selection or the subsequent DriverProvider call.
            std::fs::write(&path, b"replacement").unwrap();
            assert_eq!(
                custody
                    .retained_manifest(&path)
                    .unwrap()
                    .into_parts()
                    .1
                    .as_ref(),
                source
            );
            let mut error = Error::new(
                semwright_types::ErrorCode::BackendFailed,
                "Driver exited before protocol Hello completed (exit=0x00000001)",
            );
            error.outcome_known = outcome_known;
            error.candidates.push("original-correlation".into());
            let result = Err(error.clone());
            custody.connection_result(&result);
            custody.finish().await;
            let record: Value =
                serde_json::from_slice(&std::fs::read(directory.join("capture.json")).unwrap())
                    .unwrap();
            assert_eq!(
                record["connection_result"]["error"],
                serde_json::to_value(&error).unwrap()
            );
            assert_eq!(record["stderr"]["drain_complete"], false);
            assert_eq!(record["stderr"]["eof"], false);
            assert_eq!(record["native_effects_absent_asserted"], false);
            assert_eq!(
                std::fs::read(directory.join("manifest.original.json")).unwrap(),
                source
            );
            assert!(
                std::fs::read(directory.join("stderr.original"))
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(result.err().unwrap().outcome_known, outcome_known);
        }
    }
}
