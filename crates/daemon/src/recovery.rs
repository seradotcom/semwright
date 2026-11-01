//! Bounded recovery of already owner-configured external drivers.
//! Never retries a request, recycles native references, or enlarges sandbox grants.
use semwright_backend_api::Provider;
use semwright_core::Broker;
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::Manifest;
use semwright_policy::FilesystemGrant;
use semwright_types::ErrorCode;
use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

const RETRY_BACKOFF: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(3),
    Duration::from_secs(9),
];

pub struct DriverRecovery {
    pub manifest: Manifest,
    pub state: PathBuf,
    pub helper: PathBuf,
    pub roots: Vec<FilesystemGrant>,
    pub allow_network: bool,
    pub allow_nvidia_gpu: bool,
    pub allow_windows_native_profile: bool,
}

/// One supervisor per manifest, under the broker's lifetime. Only a terminated
/// child can trigger a new launch; no repeated health polling or speculative retries.
pub async fn supervise(
    broker: Arc<Broker>,
    initial: Arc<dyn Provider>,
    options: DriverRecovery,
    stop: CancellationToken,
) {
    let identity = initial.identity().clone();
    let mut current = initial;
    let mut attempts = 0usize;
    'connected: loop {
        let Some(closed) = current.closed() else {
            return;
        };
        tokio::select! {
            biased;
            _ = stop.cancelled() => return,
            _ = closed.cancelled() => {}
        }
        if !broker.provider_recovery_eligible(&current) {
            return;
        }
        loop {
            let Some(backoff) = RETRY_BACKOFF.get(attempts).copied() else {
                tracing::warn!(provider=%identity.id, "Driver recovery budget exhausted; owner intervention required");
                return;
            };
            attempts += 1;
            tokio::select! {
                biased;
                _ = stop.cancelled() => return,
                _ = tokio::time::sleep(backoff) => {}
            }
            if !broker.provider_recovery_eligible(&current) {
                return;
            }
            let restarted = DriverProvider::connect_with_native_profiles(
                options.manifest.clone(),
                &options.state,
                &options.helper,
                &options.roots,
                options.allow_network,
                options.allow_nvidia_gpu,
                options.allow_windows_native_profile,
            )
            .await;
            if stop.is_cancelled() {
                return;
            }
            let replacement: Arc<dyn Provider> = match restarted {
                Ok(provider) => provider,
                Err(error) => {
                    tracing::warn!(provider=%identity.id, attempt=attempts, code=?error.code,
                        "Owner-pinned driver restart failed");
                    if !matches!(
                        error.code,
                        ErrorCode::Unavailable
                            | ErrorCode::Timeout
                            | ErrorCode::BackendFailed
                            | ErrorCode::ResourceExhausted
                    ) {
                        return;
                    }
                    continue;
                }
            };
            match broker
                .replace_disconnected_provider(&current, replacement.clone())
                .await
            {
                Ok(revision) => {
                    tracing::info!(provider=%identity.id, attempt=attempts, revision,
                        "Owner-pinned driver reconnected with a new generation");
                    current = replacement;
                    continue 'connected;
                }
                Err(error) => {
                    tracing::warn!(provider=%identity.id, attempt=attempts, code=?error.code,
                        "Driver recovery publication refused");
                    if matches!(
                        error.code,
                        ErrorCode::Conflict
                            | ErrorCode::NotFound
                            | ErrorCode::PolicyDenied
                            | ErrorCode::PermissionDenied
                    ) {
                        return;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovery_is_limited_and_backed_off() {
        assert_eq!(RETRY_BACKOFF.len(), 3);
        assert_eq!(
            RETRY_BACKOFF,
            [
                Duration::from_secs(1),
                Duration::from_secs(3),
                Duration::from_secs(9),
            ]
        );
    }
}
