//! Private bounded observation of one real DriverProvider's stderr stream.
//! This Rust-only sink grants no launch, mount, tool, or native authority.

use semwright_driver_sdk::Manifest;
use semwright_platform_api::launch::{SandboxStderrObserver, SandboxStderrTerminal};
use semwright_types::{Error, ErrorCode, ProviderIdentity, Result};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

const MAX_PREFIX_BYTES: usize = 64 * 1024;

struct CaptureState {
    prefix: Vec<u8>,
    observed_bytes: u64,
    observed_digest: Sha256,
    count_overflowed: bool,
    started: bool,
    terminal: Option<SandboxStderrTerminal>,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            prefix: Vec::with_capacity(MAX_PREFIX_BYTES),
            observed_bytes: 0,
            observed_digest: Sha256::default(),
            count_overflowed: false,
            started: false,
            terminal: None,
        }
    }
}

#[derive(Default)]
struct Capture {
    selected: AtomicBool,
    state: Mutex<CaptureState>,
}

impl SandboxStderrObserver for Capture {
    fn started(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .started = true;
    }

    fn bytes(&self, bytes: &[u8]) {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        let retain = MAX_PREFIX_BYTES
            .saturating_sub(state.prefix.len())
            .min(bytes.len());
        state.prefix.extend_from_slice(&bytes[..retain]);
        state.observed_digest.update(bytes);
        match state.observed_bytes.checked_add(bytes.len() as u64) {
            Some(count) => state.observed_bytes = count,
            None => {
                state.observed_bytes = u64::MAX;
                state.count_overflowed = true;
            }
        }
    }

    fn terminal(&self, terminal: SandboxStderrTerminal) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .terminal = Some(terminal);
    }
}

/// Explicit owner selection, bound to the exact validated provider identity and
/// executable digest. The same sink cannot instrument a second launch.
#[derive(Clone)]
pub struct DriverStartupStderrCustody {
    provider: ProviderIdentity,
    executable_sha256: String,
    capture: Arc<Capture>,
}

/// Private plaintext snapshot. Deliberately not Debug, Clone or Serialize.
pub struct DriverStartupStderrSnapshot {
    prefix: Vec<u8>,
    metadata: Value,
}

impl DriverStartupStderrSnapshot {
    pub fn prefix(&self) -> &[u8] {
        &self.prefix
    }

    pub fn metadata(&self) -> &Value {
        &self.metadata
    }
}

impl DriverStartupStderrCustody {
    pub fn for_manifest(manifest: &Manifest) -> Result<Self> {
        manifest.validate()?;
        Ok(Self {
            provider: manifest.identity()?,
            executable_sha256: manifest.sha256.to_ascii_lowercase(),
            capture: Arc::new(Capture::default()),
        })
    }

    pub(super) fn validate_manifest(&self, manifest: &Manifest) -> Result<()> {
        if manifest.identity()? != self.provider
            || manifest.sha256.to_ascii_lowercase() != self.executable_sha256
        {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Startup stderr custody must select the retained provider identity and digest",
            ));
        }
        Ok(())
    }

    pub(super) fn observer(&self) -> Result<Arc<dyn SandboxStderrObserver>> {
        if self.capture.selected.swap(true, Ordering::AcqRel) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Startup stderr custody is restricted to one launch",
            ));
        }
        Ok(self.capture.clone())
    }

    pub fn drain_finished(&self) -> bool {
        self.capture
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .terminal
            .is_some()
    }

    pub fn snapshot(&self) -> DriverStartupStderrSnapshot {
        let state = self
            .capture
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let (eof, read_error, interrupted) = match state.terminal {
            Some(SandboxStderrTerminal::Eof) => (true, None, false),
            Some(SandboxStderrTerminal::ReadFailed { kind, raw_os_error }) => (
                false,
                Some(json!({"kind": format!("{kind:?}"), "raw_os_error": raw_os_error})),
                false,
            ),
            Some(SandboxStderrTerminal::Interrupted) => (false, None, true),
            None => (false, None, false),
        };
        let prefix = state.prefix.clone();
        let metadata = json!({
            "provider": self.provider,
            "executable_sha256": self.executable_sha256,
            "selected": self.capture.selected.load(Ordering::Acquire),
            "drain_started": state.started,
            "observed_bytes": state.observed_bytes,
            "observed_sha256": hex::encode(state.observed_digest.clone().finalize()),
            "retained_prefix_bytes": prefix.len(),
            "retained_prefix_sha256": hex::encode(Sha256::digest(&prefix)),
            "max_prefix_bytes": MAX_PREFIX_BYTES,
            "prefix_truncated": state.observed_bytes > prefix.len() as u64,
            "count_overflowed": state.count_overflowed,
            "eof": eof,
            "read_error": read_error,
            "observer_interrupted": interrupted,
            "drain_complete": state.started && eof && !state.count_overflowed,
            "snapshot_before_drain_terminal": state.terminal.is_none(),
            "scope": "observed process stderr; no claim that every byte preceded Hello or that native effects were absent"
        });
        DriverStartupStderrSnapshot { prefix, metadata }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn custody() -> DriverStartupStderrCustody {
        DriverStartupStderrCustody {
            provider: ProviderIdentity::external(
                semwright_types::SourceKind::Driver,
                "fixture",
                "1",
            )
            .unwrap(),
            executable_sha256: "a".repeat(64),
            capture: Arc::new(Capture::default()),
        }
    }

    fn manifest() -> Manifest {
        let executable = if cfg!(windows) {
            r"C:\semwright\fixture.exe"
        } else {
            "/tmp/semwright-fixture"
        };
        serde_json::from_value(json!({
            "manifest_version": 1, "protocol": 1, "id": "fixture", "version": "1",
            "publisher": "tests", "executable": executable, "sha256": "a".repeat(64),
            "application": {"desktop_id": "org.example.Fixture"}, "transport": "stdio_v1"
        }))
        .unwrap()
    }

    #[test]
    fn startup_stderr_selection_binds_identity_and_executable_without_requiring_a_host_tool() {
        let original = manifest();
        assert!(!original.interfaces.host_tools);
        let custody = DriverStartupStderrCustody::for_manifest(&original).unwrap();
        custody.validate_manifest(&original).unwrap();
        let mut changed = original.clone();
        changed.sha256 = "b".repeat(64);
        assert_eq!(
            custody.validate_manifest(&changed).unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        changed = original.clone();
        changed.id = "other".into();
        assert_eq!(
            custody.validate_manifest(&changed).unwrap_err().code,
            ErrorCode::PermissionDenied
        );
        changed = original;
        changed.version = "2".into();
        assert_eq!(
            custody.validate_manifest(&changed).unwrap_err().code,
            ErrorCode::PermissionDenied
        );
    }

    #[test]
    fn startup_stderr_retains_only_prefix_but_hashes_all_observed_bytes() {
        let custody = custody();
        let observer = custody.observer().unwrap();
        let bytes = vec![b'x'; MAX_PREFIX_BYTES + 8192];
        observer.started();
        for chunk in bytes.chunks(8192) {
            observer.bytes(chunk);
        }
        observer.terminal(SandboxStderrTerminal::Eof);
        let snapshot = custody.snapshot();
        assert_eq!(snapshot.prefix(), &bytes[..MAX_PREFIX_BYTES]);
        assert_eq!(snapshot.metadata()["observed_bytes"], bytes.len());
        assert_eq!(
            snapshot.metadata()["observed_sha256"],
            hex::encode(Sha256::digest(&bytes))
        );
        assert_eq!(snapshot.metadata()["prefix_truncated"], true);
        assert_eq!(snapshot.metadata()["drain_complete"], true);
        assert!(
            custody.observer().is_err(),
            "no second launch on the same observation"
        );
    }

    #[test]
    fn startup_stderr_pending_or_failed_capture_never_becomes_complete() {
        for terminal in [
            None,
            Some(SandboxStderrTerminal::Interrupted),
            Some(SandboxStderrTerminal::ReadFailed {
                kind: std::io::ErrorKind::Other,
                raw_os_error: Some(5),
            }),
        ] {
            let custody = custody();
            let observer = custody.observer().unwrap();
            observer.started();
            observer.bytes(b"original\0stderr");
            if let Some(terminal) = terminal {
                observer.terminal(terminal);
            }
            let snapshot = custody.snapshot();
            assert_eq!(snapshot.prefix(), b"original\0stderr");
            assert_eq!(snapshot.metadata()["eof"], false);
            assert_eq!(snapshot.metadata()["drain_complete"], false);
        }
    }
}
