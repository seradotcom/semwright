//! Retain the canonical owner reservation across request/transport loss.
use semwright_backend_api::provider::{OwnerOperationAuthority, OwnerOperationLease};
use semwright_driver_sdk::Request;
use semwright_platform_api::launch::SandboxStdin;
use semwright_protocol::write_frame;
use semwright_types::{Error, ErrorCode, Result, unique_id};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
const MAX_OWNER_PERMITS: usize = 64;
const CONTROL_TIMEOUT: Duration = Duration::from_secs(2);
struct Held {
    parent: String,
    _lease: Box<dyn OwnerOperationLease>,
    released: CancellationToken,
}
#[derive(Default)]
struct State {
    physically_closed: bool,
    held: BTreeMap<String, Held>,
    // Dropping transport wrappers cannot invent matched release or physical proof.
    quarantine: Option<Arc<OwnerOperations>>,
}
#[derive(Default)]
pub(super) struct OwnerOperations {
    state: StdMutex<State>,
    physical_exit: CancellationToken,
}
pub(super) fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}
fn valid_logical_mount(value: &str) -> bool {
    semwright_types::provider::canonical_slug(value) && !value.starts_with("semwright-internal-")
}
fn denied() -> Error {
    Error::new(
        ErrorCode::ProtocolMismatch,
        "Owner operation permit binding mismatch",
    )
}
fn sanitized(error: Error) -> Error {
    let mut safe = Error::new(error.code, "Owner operation permit is unavailable");
    safe.outcome_known = error.outcome_known;
    safe
}
async fn send_control(
    input: &Arc<Mutex<SandboxStdin>>,
    request: &Request,
    terminate: &CancellationToken,
) -> Result<()> {
    let result = tokio::time::timeout(CONTROL_TIMEOUT, async {
        let mut input = input.lock().await;
        write_frame(&mut *input, request).await
    })
    .await;
    match result {
        Ok(Ok(())) => Ok(()),
        _ => {
            terminate.cancel();
            Err(Error::new(
                ErrorCode::Unavailable,
                "Owner operation control delivery failed",
            )
            .uncertain())
        }
    }
}
impl OwnerOperations {
    pub(super) fn release(&self, parent: &str, permit: &str) -> Result<()> {
        let (held, quarantine) = {
            let mut state = self.state.lock().map_err(|_| denied())?;
            if state.physically_closed {
                return Ok(());
            }
            if !state
                .held
                .get(permit)
                .is_some_and(|held| held.parent == parent)
            {
                return Err(denied());
            }
            let held = state.held.remove(permit).ok_or_else(denied)?;
            let quarantine = if state.held.is_empty() {
                state.quarantine.take()
            } else {
                None
            };
            (held, quarantine)
        };
        held.released.cancel();
        drop(held);
        drop(quarantine);
        Ok(())
    }
    pub(super) fn confirmed_physical_exit(&self) -> Result<()> {
        let (held, quarantine) = {
            let mut state = self.state.lock().map_err(|_| denied())?;
            state.physically_closed = true;
            (std::mem::take(&mut state.held), state.quarantine.take())
        };
        for (_, held) in held {
            held.released.cancel();
            drop(held);
        }
        drop(quarantine);
        self.physical_exit.cancel();
        Ok(())
    }
    pub(super) async fn wait_physical_exit(&self) {
        self.physical_exit.cancelled().await;
    }
    fn contains(&self, parent: &str, permit: &str) -> bool {
        self.state.lock().ok().is_some_and(|state| {
            !state.physically_closed
                && state
                    .held
                    .get(permit)
                    .is_some_and(|held| held.parent == parent)
        })
    }
    fn retain(
        self: &Arc<Self>,
        parent: &str,
        lease: Box<dyn OwnerOperationLease>,
    ) -> Result<(String, String, CancellationToken, CancellationToken)> {
        lease.check_current()?;
        let logical_mount = lease.logical_mount();
        if !valid_logical_mount(logical_mount) {
            return Err(denied());
        }
        let logical_mount = logical_mount.to_owned();
        let closing = lease.closing();
        let released = CancellationToken::new();
        let permit = unique_id();
        let mut state = self.state.lock().map_err(|_| denied())?;
        if state.physically_closed {
            return Err(Error::unavailable("Owner process has exited"));
        }
        if state.held.len() >= MAX_OWNER_PERMITS {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Owner operation permit budget exhausted",
            ));
        }
        if state.held.contains_key(&permit) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Owner operation permit identity collision",
            ));
        }
        state.quarantine.get_or_insert_with(|| self.clone());
        state.held.insert(
            permit.clone(),
            Held {
                parent: parent.into(),
                _lease: lease,
                released: released.clone(),
            },
        );
        Ok((permit, logical_mount, closing, released))
    }
    pub(super) async fn acquire_and_reply(
        self: &Arc<Self>,
        id: String,
        parent: String,
        authority: Option<Arc<dyn OwnerOperationAuthority>>,
        input: Arc<Mutex<SandboxStdin>>,
        terminate: CancellationToken,
    ) -> Result<()> {
        let granted = authority
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::PermissionDenied,
                    "This invocation has no owner operation authority",
                )
            })
            .and_then(|authority| authority.acquire())
            .and_then(|lease| self.retain(&parent, lease));
        let (permit, logical_mount, closing, released) = match granted {
            Ok(granted) => granted,
            Err(error) => {
                return send_control(
                    &input,
                    &Request::OperationPermitFailure {
                        id,
                        parent,
                        error: sanitized(error),
                    },
                    &terminate,
                )
                .await;
            }
        };
        let result = send_control(
            &input,
            &Request::OperationPermitGranted {
                id,
                parent: parent.clone(),
                permit: permit.clone(),
                logical_mount,
            },
            &terminate,
        )
        .await;
        let owner = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::select! { biased;
                _ = released.cancelled() => return,
                _ = closing.cancelled() => {}
            }
            let Some(owner) = owner.upgrade() else {
                return;
            };
            if owner.contains(&parent, &permit) {
                let _ = send_control(
                    &input,
                    &Request::OperationPermitClose { parent, permit },
                    &terminate,
                )
                .await;
            }
        });
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct TrackedLease {
        drops: Arc<AtomicUsize>,
        close: CancellationToken,
        logical_mount: String,
    }
    impl OwnerOperationLease for TrackedLease {
        fn check_current(&self) -> Result<()> {
            if self.close.is_cancelled() {
                Err(Error::new(ErrorCode::StaleReference, "test lease closing"))
            } else {
                Ok(())
            }
        }
        fn closing(&self) -> CancellationToken {
            self.close.clone()
        }
        fn logical_mount(&self) -> &str {
            &self.logical_mount
        }
    }
    impl Drop for TrackedLease {
        fn drop(&mut self) {
            self.drops.fetch_add(1, Ordering::SeqCst);
        }
    }
    fn lease(drops: &Arc<AtomicUsize>) -> Box<dyn OwnerOperationLease> {
        Box::new(TrackedLease {
            drops: drops.clone(),
            close: CancellationToken::new(),
            logical_mount: "test-data".into(),
        })
    }
    #[test]
    fn release_requires_exact_parent_and_opaque_handle() {
        let owner = Arc::new(OwnerOperations::default());
        let drops = Arc::new(AtomicUsize::new(0));
        let (permit, _, _, released) = owner.retain("parent", lease(&drops)).unwrap();
        assert!(owner.release("other", &permit).is_err());
        assert!(owner.release("parent", "not-the-handle").is_err());
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        assert!(!released.is_cancelled());
        owner.release("parent", &permit).unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(released.is_cancelled());
    }
    #[test]
    fn dropping_transport_owners_does_not_discharge_an_exposed_hold() {
        let owner = Arc::new(OwnerOperations::default());
        let weak = Arc::downgrade(&owner);
        let drops = Arc::new(AtomicUsize::new(0));
        let (permit, _, _, _) = owner.retain("parent", lease(&drops)).unwrap();
        drop(owner);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        let retained = weak
            .upgrade()
            .expect("quarantine must retain the owner hold");
        retained.release("parent", &permit).unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        drop(retained);
        assert!(weak.upgrade().is_none());
    }
    #[tokio::test]
    async fn physical_proof_callback_clears_holds_and_latches_once() {
        // Unit callback semantics only; no actual process/descendant exit claim.
        let owner = Arc::new(OwnerOperations::default());
        let drops = Arc::new(AtomicUsize::new(0));
        let (a, _, _, ar) = owner.retain("a", lease(&drops)).unwrap();
        let (_, _, _, br) = owner.retain("b", lease(&drops)).unwrap();
        assert!(!owner.physical_exit.is_cancelled());
        owner.confirmed_physical_exit().unwrap();
        tokio::time::timeout(Duration::from_secs(1), owner.wait_physical_exit())
            .await
            .unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        assert!(ar.is_cancelled() && br.is_cancelled());
        assert!(!owner.contains("a", &a));
        owner.release("a", &a).unwrap();
        owner.confirmed_physical_exit().unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        assert!(owner.retain("new", lease(&drops)).is_err());
        assert_eq!(drops.load(Ordering::SeqCst), 3);
    }
    #[test]
    fn retained_unknown_holds_keep_their_shared_capacity_budget() {
        let owner = Arc::new(OwnerOperations::default());
        let drops = Arc::new(AtomicUsize::new(0));
        for i in 0..MAX_OWNER_PERMITS {
            owner.retain(&format!("parent-{i}"), lease(&drops)).unwrap();
        }
        let extra = owner.retain("over-budget", lease(&drops));
        assert!(matches!(extra, Err(ref e) if e.code == ErrorCode::ResourceExhausted));
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        owner.confirmed_physical_exit().unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), MAX_OWNER_PERMITS + 1);
    }

    struct FixedAuthority {
        drops: Arc<AtomicUsize>,
        logical_mount: String,
    }
    impl OwnerOperationAuthority for FixedAuthority {
        fn acquire(&self) -> Result<Box<dyn OwnerOperationLease>> {
            Ok(Box::new(TrackedLease {
                drops: self.drops.clone(),
                close: CancellationToken::new(),
                logical_mount: self.logical_mount.clone(),
            }))
        }
    }
    async fn private_grant_frame(
        owner: &Arc<OwnerOperations>,
        drops: &Arc<AtomicUsize>,
        logical_mount: &str,
    ) -> Request {
        // Actual Host framing; only upstream owner is a test double.
        // This does not claim a physical sandbox or real project authority.
        let (writer, mut reader) = tokio::io::duplex(8192);
        let input: SandboxStdin = Box::new(writer);
        owner
            .acquire_and_reply(
                "0123456789abcdef0123456789abcdef".into(),
                "fedcba9876543210fedcba9876543210".into(),
                Some(Arc::new(FixedAuthority {
                    drops: drops.clone(),
                    logical_mount: logical_mount.into(),
                })),
                Arc::new(Mutex::new(input)),
                CancellationToken::new(),
            )
            .await
            .unwrap();
        let frame: Request = tokio::time::timeout(
            Duration::from_secs(1),
            semwright_protocol::read_frame(&mut reader),
        )
        .await
        .unwrap()
        .unwrap();
        frame
    }
    #[tokio::test]
    async fn grant_frame_preserves_the_exact_owner_mount_instead_of_an_expected_name() {
        for expected in ["motionwright-data", "font-cache"] {
            let owner = Arc::new(OwnerOperations::default());
            let drops = Arc::new(AtomicUsize::new(0));
            let frame = private_grant_frame(&owner, &drops, expected).await;
            match frame {
                Request::OperationPermitGranted {
                    id,
                    parent,
                    permit,
                    logical_mount,
                } => {
                    assert_eq!(id, "0123456789abcdef0123456789abcdef");
                    assert_eq!(parent, "fedcba9876543210fedcba9876543210");
                    assert_eq!(logical_mount, expected);
                    assert_eq!(drops.load(Ordering::SeqCst), 0);
                    if expected == "font-cache" {
                        assert_ne!(logical_mount, "motionwright-data");
                    }
                    owner.release(&parent, &permit).unwrap();
                }
                _ => panic!("valid owner metadata did not produce a private grant frame"),
            }
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    }
    #[tokio::test]
    async fn invalid_owner_mount_labels_never_expose_a_grant_or_consume_a_hold() {
        let invalid = [
            String::new(),
            "Motionwright-data".into(),
            "data/path".into(),
            "semwright-internal-loopback".into(),
            "a".repeat(41),
        ];
        for label in invalid {
            let owner = Arc::new(OwnerOperations::default());
            let drops = Arc::new(AtomicUsize::new(0));
            match private_grant_frame(&owner, &drops, &label).await {
                Request::OperationPermitFailure { id, parent, error } => {
                    assert_eq!(id, "0123456789abcdef0123456789abcdef");
                    assert_eq!(parent, "fedcba9876543210fedcba9876543210");
                    assert_eq!(error.code, ErrorCode::ProtocolMismatch);
                    assert!(error.outcome_known);
                }
                _ => panic!("invalid owner mount label exposed a usable grant"),
            }
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            let state = owner.state.lock().unwrap();
            assert!(state.held.is_empty());
            assert!(state.quarantine.is_none());
        }
    }
}
