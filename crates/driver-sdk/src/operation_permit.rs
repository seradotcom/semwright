use crate::{DRIVER_PROTOCOL_OPERATION_PERMITS, Response};
use semwright_types::provider::canonical_slug;
use semwright_types::{Error, ErrorCode, Result, unique_id};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

fn unavailable() -> Error {
    Error::new(
        ErrorCode::Unavailable,
        "Owner operation-permit channel unavailable",
    )
}
fn cancelled() -> Error {
    Error::new(ErrorCode::Cancelled, "Owner operation permit is closing")
}
fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && !value.chars().any(char::is_control)
}
fn release(output: &mpsc::UnboundedSender<Response>, parent: &str, permit: &str) {
    let _ = output.send(Response::OperationPermitRelease {
        id: unique_id(),
        parent: parent.to_owned(),
        permit: permit.to_owned(),
    });
    // Queueing or dropping is not confirmed drain. On delivery loss the Host
    // keeps its owner reservation until matching release or proven child exit.
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Available,
    Borrowed,
    ClosingBorrowed,
    Released,
}
struct LocalPermit {
    parent: String,
    handle: String,
    logical_mount: String,
    identity: Arc<()>,
    channel_closed: Arc<AtomicBool>,
    phase: Mutex<Phase>,
    output: mpsc::UnboundedSender<Response>,
    cancellation: CancellationToken,
}
impl LocalPermit {
    fn close(&self) {
        let send = if let Ok(mut phase) = self.phase.lock() {
            match *phase {
                Phase::Available => {
                    *phase = Phase::Released;
                    true
                }
                Phase::Borrowed => {
                    *phase = Phase::ClosingBorrowed;
                    false
                }
                Phase::ClosingBorrowed | Phase::Released => false,
            }
        } else {
            false
        };
        if send {
            release(&self.output, &self.parent, &self.handle);
        }
    }
    fn finish_borrow(&self) {
        let send = if let Ok(mut phase) = self.phase.lock() {
            match *phase {
                Phase::Borrowed | Phase::ClosingBorrowed => {
                    *phase = Phase::Released;
                    true
                }
                Phase::Available | Phase::Released => false,
            }
        } else {
            false
        };
        if send {
            release(&self.output, &self.parent, &self.handle);
        }
    }
}

/// An existing owner reservation for one genuine Host invocation.
/// This type has no public constructor, Clone or serialization implementation.
/// It creates no permission; acquire it before locking application storage.
pub struct OperationPermit {
    state: Arc<LocalPermit>,
}
impl OperationPermit {
    /// Owner-selected logical mount from the canonical bound invocation.
    /// This label is not a filesystem handle, a live guard, or permission by itself.
    pub fn logical_mount(&self) -> &str {
        &self.state.logical_mount
    }
    pub(crate) fn try_enter(&self) -> Result<OperationPermitGuard<'_>> {
        let mut phase = self.state.phase.lock().map_err(|_| unavailable())?;
        if self.state.channel_closed.load(Ordering::SeqCst)
            || self.state.cancellation.is_cancelled()
            || self.state.output.is_closed()
        {
            return Err(cancelled());
        }
        match *phase {
            Phase::Available => {
                *phase = Phase::Borrowed;
                Ok(OperationPermitGuard { permit: self })
            }
            Phase::Borrowed => Err(Error::new(
                ErrorCode::Conflict,
                "Owner operation permit was already entered",
            )),
            Phase::ClosingBorrowed | Phase::Released => Err(cancelled()),
        }
    }
}
impl Drop for OperationPermit {
    fn drop(&mut self) {
        self.state.close();
    }
}

/// Holds the reservation through the synchronous local commit.
/// The Host must not equate Cancel accepted, future Drop or transport closure
/// with release of this guard. Drop queues one matching release.
pub struct OperationPermitGuard<'a> {
    permit: &'a OperationPermit,
}
impl OperationPermitGuard<'_> {
    pub fn check_current(&self) -> Result<()> {
        let state = &self.permit.state;
        let phase = state.phase.lock().map_err(|_| unavailable())?;
        if *phase != Phase::Borrowed
            || state.channel_closed.load(Ordering::SeqCst)
            || state.cancellation.is_cancelled()
            || state.output.is_closed()
        {
            return Err(cancelled());
        }
        Ok(())
    }
}
impl Drop for OperationPermitGuard<'_> {
    fn drop(&mut self) {
        self.permit.state.finish_borrow();
    }
}

struct Pending {
    parent: String,
    identity: Arc<()>,
    cancellation: CancellationToken,
    sender: oneshot::Sender<Result<OperationPermit>>,
}
#[derive(Default)]
pub(crate) struct PermitChannel {
    closed: Arc<AtomicBool>,
    pending: Mutex<BTreeMap<String, Pending>>,
    active: Mutex<BTreeMap<String, Weak<LocalPermit>>>,
}
struct PendingDrop {
    channel: Arc<PermitChannel>,
    id: String,
}
impl Drop for PendingDrop {
    fn drop(&mut self) {
        if let Ok(mut pending) = self.channel.pending.lock() {
            pending.remove(&self.id);
        }
    }
}
impl PermitChannel {
    async fn acquire(
        self: &Arc<Self>,
        parent: &str,
        protocol: u32,
        output: &mpsc::UnboundedSender<Response>,
        cancellation: &CancellationToken,
        requested: &AtomicBool,
        identity: &Arc<()>,
    ) -> Result<OperationPermit> {
        if protocol < DRIVER_PROTOCOL_OPERATION_PERMITS {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Current Host protocol does not provide operation permits",
            ));
        }
        if self.closed.load(Ordering::SeqCst) || cancellation.is_cancelled() || output.is_closed() {
            return Err(cancelled());
        }
        if requested.swap(true, Ordering::SeqCst) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "An operation permit was already requested for this invocation",
            ));
        }
        let id = unique_id();
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.pending.lock().map_err(|_| unavailable())?;
            if self.closed.load(Ordering::SeqCst) {
                return Err(cancelled());
            }
            pending.insert(
                id.clone(),
                Pending {
                    parent: parent.to_owned(),
                    identity: identity.clone(),
                    cancellation: cancellation.clone(),
                    sender,
                },
            );
        }
        let _pending_drop = PendingDrop {
            channel: self.clone(),
            id: id.clone(),
        };
        output
            .send(Response::OperationPermitAcquire {
                id,
                parent: parent.to_owned(),
            })
            .map_err(|_| unavailable())?;
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(cancelled()),
            result = receiver => result.map_err(|_| unavailable())?,
        }
    }
    pub(crate) fn granted(
        &self,
        id: &str,
        parent: &str,
        handle: &str,
        logical_mount: &str,
        output: &mpsc::UnboundedSender<Response>,
    ) -> Result<()> {
        if !valid_id(id)
            || !valid_id(parent)
            || !valid_id(handle)
            || !canonical_slug(logical_mount)
            || logical_mount.starts_with("semwright-internal-")
        {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Invalid operation-permit reply",
            ));
        }
        let pending = self.pending.lock().map_err(|_| unavailable())?.remove(id);
        let Some(pending) = pending else {
            // The original waiter ended. Cleanup this exact Host handle;
            // never turn a late/unsolicited reply into a public permit.
            release(output, parent, handle);
            return Ok(());
        };
        if pending.parent != parent {
            release(output, parent, handle);
            let _ = pending.sender.send(Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Operation-permit reply belongs to a different invocation",
            )));
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Operation-permit parent mismatch",
            ));
        }
        let state = Arc::new(LocalPermit {
            parent: parent.to_owned(),
            handle: handle.to_owned(),
            logical_mount: logical_mount.to_owned(),
            identity: pending.identity,
            channel_closed: self.closed.clone(),
            phase: Mutex::new(Phase::Available),
            output: output.clone(),
            cancellation: pending.cancellation,
        });
        {
            let mut active = self.active.lock().map_err(|_| unavailable())?;
            if self.closed.load(Ordering::SeqCst) {
                drop(active);
                release(output, parent, handle);
                let _ = pending.sender.send(Err(cancelled()));
                return Ok(());
            }
            active.retain(|_, prior| prior.strong_count() != 0);
            if active.get(handle).and_then(Weak::upgrade).is_some() {
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Host reused an active operation-permit handle",
                ));
            }
            active.insert(handle.to_owned(), Arc::downgrade(&state));
        }
        let permit = OperationPermit { state };
        if permit.state.cancellation.is_cancelled() {
            drop(permit);
            let _ = pending.sender.send(Err(cancelled()));
        } else {
            // If the waiter concurrently disappeared, the returned value is
            // dropped here, which queues its one matching release.
            let _ = pending.sender.send(Ok(permit));
        }
        Ok(())
    }
    pub(crate) fn failed(&self, id: &str, parent: &str, error: Error) -> Result<()> {
        let pending = self.pending.lock().map_err(|_| unavailable())?.remove(id);
        if let Some(pending) = pending {
            if pending.parent != parent {
                let _ = pending.sender.send(Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Operation-permit failure belongs to another invocation",
                )));
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Operation-permit parent mismatch",
                ));
            }
            let _ = pending.sender.send(Err(error));
        }
        Ok(())
    }
    pub(crate) fn close(&self, parent: &str, handle: &str) -> Result<()> {
        let state = self
            .active
            .lock()
            .map_err(|_| unavailable())?
            .get(handle)
            .and_then(Weak::upgrade);
        if let Some(state) = state {
            if state.parent != parent {
                return Err(Error::new(
                    ErrorCode::ProtocolMismatch,
                    "Operation-permit close parent mismatch",
                ));
            }
            state.close();
        }
        Ok(())
    }
    pub(crate) fn close_parent(&self, parent: &str) {
        let states = self
            .active
            .lock()
            .ok()
            .map(|mut active| {
                active.retain(|_, state| state.strong_count() != 0);
                active
                    .values()
                    .filter_map(Weak::upgrade)
                    .filter(|state| state.parent == parent)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // Never hold the registry while locking a local state or sending.
        for state in states {
            state.close();
        }
        let pending = self
            .pending
            .lock()
            .ok()
            .map(|mut pending| {
                let ids: Vec<_> = pending
                    .iter()
                    .filter(|(_, item)| item.parent == parent)
                    .map(|(id, _)| id.clone())
                    .collect();
                ids.into_iter()
                    .filter_map(|id| pending.remove(&id))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for item in pending {
            let _ = item.sender.send(Err(cancelled()));
        }
    }
    pub(crate) fn close_all(&self) {
        self.closed.store(true, Ordering::SeqCst);
        let states = self
            .active
            .lock()
            .ok()
            .map(|active| {
                active
                    .values()
                    .filter_map(Weak::upgrade)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for state in states {
            state.close();
        }
        let pending = self
            .pending
            .lock()
            .ok()
            .map(|mut pending| std::mem::take(&mut *pending))
            .unwrap_or_default();
        for (_, item) in pending {
            let _ = item.sender.send(Err(cancelled()));
        }
    }
}
pub(crate) struct PermitChannelCloseGuard(pub Arc<PermitChannel>);
impl Drop for PermitChannelCloseGuard {
    fn drop(&mut self) {
        self.0.close_all();
    }
}

pub(crate) struct PermitInvocation {
    parent: String,
    identity: Arc<()>,
    channel: Arc<PermitChannel>,
    cancellation: CancellationToken,
    requested: AtomicBool,
}
impl PermitInvocation {
    pub(crate) fn new(
        parent: impl Into<String>,
        cancellation: &CancellationToken,
        channel: Arc<PermitChannel>,
    ) -> Arc<Self> {
        Arc::new(Self {
            parent: parent.into(),
            identity: Arc::new(()),
            channel,
            cancellation: cancellation.child_token(),
            requested: AtomicBool::new(false),
        })
    }
    pub(crate) async fn acquire(
        &self,
        protocol: u32,
        output: &mpsc::UnboundedSender<Response>,
    ) -> Result<OperationPermit> {
        self.channel
            .acquire(
                &self.parent,
                protocol,
                output,
                &self.cancellation,
                &self.requested,
                &self.identity,
            )
            .await
    }
    pub(crate) fn enter<'a>(
        &self,
        permit: &'a OperationPermit,
    ) -> Result<OperationPermitGuard<'a>> {
        if !Arc::ptr_eq(&self.identity, &permit.state.identity) {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Owner operation permit belongs to another invocation",
            ));
        }
        permit.try_enter()
    }
    fn close(&self) {
        self.cancellation.cancel();
        self.channel.close_parent(&self.parent);
    }
}
pub(crate) struct PermitInvocationCloseGuard(pub Arc<PermitInvocation>);
impl Drop for PermitInvocationCloseGuard {
    fn drop(&mut self) {
        self.0.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tokio::sync::mpsc::error::TryRecvError;
    struct Fixture {
        channel: Arc<PermitChannel>,
        invocation: Arc<PermitInvocation>,
        output: mpsc::UnboundedSender<Response>,
        received: mpsc::UnboundedReceiver<Response>,
    }
    impl Fixture {
        fn new() -> Self {
            let channel = Arc::new(PermitChannel::default());
            let invocation = PermitInvocation::new(
                "original-parent",
                &CancellationToken::new(),
                channel.clone(),
            );
            let (output, received) = mpsc::unbounded_channel();
            Self {
                channel,
                invocation,
                output,
                received,
            }
        }
        fn start(&self) -> tokio::task::JoinHandle<Result<OperationPermit>> {
            let invocation = self.invocation.clone();
            let output = self.output.clone();
            tokio::spawn(async move {
                invocation
                    .acquire(DRIVER_PROTOCOL_OPERATION_PERMITS, &output)
                    .await
            })
        }
        async fn acquire_frame(&mut self) -> (String, String) {
            match tokio::time::timeout(Duration::from_secs(2), self.received.recv())
                .await
                .unwrap()
                .unwrap()
            {
                Response::OperationPermitAcquire { id, parent } => (id, parent),
                other => panic!("unexpected frame: {other:?}"),
            }
        }
        async fn grant(&mut self) -> OperationPermit {
            let task = self.start();
            let (id, parent) = self.acquire_frame().await;
            self.channel
                .granted(
                    &id,
                    &parent,
                    "owner-handle",
                    "motionwright-data",
                    &self.output,
                )
                .unwrap();
            tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap()
        }
        fn released_once(&mut self) {
            match self.received.try_recv().unwrap() {
                Response::OperationPermitRelease { parent, permit, .. } => {
                    assert_eq!(parent, "original-parent");
                    assert_eq!(permit, "owner-handle");
                }
                other => panic!("unexpected frame: {other:?}"),
            }
            self.no_frame();
        }
        fn no_frame(&mut self) {
            assert!(matches!(self.received.try_recv(), Err(TryRecvError::Empty)));
        }
    }
    #[tokio::test]
    async fn legacy_protocol_refuses_before_emitting_any_frame() {
        let mut fixture = Fixture::new();
        assert!(
            matches!(fixture.invocation.acquire(7, &fixture.output).await,
            Err(error) if error.code == ErrorCode::Unsupported)
        );
        fixture.no_frame();
    }
    #[tokio::test]
    async fn positive_entry_is_one_shot_and_releases_once() {
        let mut fixture = Fixture::new();
        let permit = fixture.grant().await;
        let guard = fixture.invocation.enter(&permit).unwrap();
        guard.check_current().unwrap();
        assert!(matches!(fixture.invocation.enter(&permit),
            Err(error) if error.code == ErrorCode::Conflict));
        drop(guard);
        fixture.released_once();
        assert!(fixture.invocation.enter(&permit).is_err());
        drop(permit);
        fixture.no_frame();
    }
    #[tokio::test]
    async fn owner_close_before_entry_prevents_entry_and_releases() {
        let mut fixture = Fixture::new();
        let permit = fixture.grant().await;
        fixture
            .channel
            .close("original-parent", "owner-handle")
            .unwrap();
        assert!(fixture.invocation.enter(&permit).is_err());
        fixture.released_once();
        drop(permit);
        fixture.no_frame();
    }
    #[tokio::test]
    async fn closing_while_guard_is_held_cannot_release_it_early() {
        let mut fixture = Fixture::new();
        let permit = fixture.grant().await;
        let guard = fixture.invocation.enter(&permit).unwrap();
        let invocation = fixture.invocation.clone();
        std::thread::spawn(move || invocation.close())
            .join()
            .unwrap();
        assert!(guard.check_current().is_err());
        fixture.no_frame();
        drop(guard);
        fixture.released_once();
        drop(permit);
        fixture.no_frame();
    }
    #[tokio::test]
    async fn equal_request_strings_do_not_allow_cross_invocation_entry() {
        let mut fixture = Fixture::new();
        let permit = fixture.grant().await;
        let other = PermitInvocation::new(
            "original-parent",
            &CancellationToken::new(),
            fixture.channel.clone(),
        );
        assert!(matches!(other.enter(&permit),
            Err(error) if error.code == ErrorCode::PermissionDenied));
        fixture.no_frame();
        let guard = fixture.invocation.enter(&permit).unwrap();
        guard.check_current().unwrap();
        drop(guard);
        fixture.released_once();
    }
    #[tokio::test]
    async fn dropped_acquire_cleans_late_grant_without_returning_a_permit() {
        let mut fixture = Fixture::new();
        let task = fixture.start();
        let (id, parent) = fixture.acquire_frame().await;
        task.abort();
        assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        fixture
            .channel
            .granted(
                &id,
                &parent,
                "owner-handle",
                "motionwright-data",
                &fixture.output,
            )
            .unwrap();
        fixture.released_once();
        assert!(fixture.channel.pending.lock().unwrap().is_empty());
        assert!(
            fixture
                .channel
                .active
                .lock()
                .unwrap()
                .values()
                .all(|state| state.upgrade().is_none())
        );
        assert!(
            matches!(fixture.invocation.acquire(9, &fixture.output).await,
            Err(error) if error.code == ErrorCode::Conflict)
        );
        fixture.no_frame();
    }
    #[tokio::test]
    async fn channel_shutdown_rejects_pending_late_and_new_acquisition() {
        let mut fixture = Fixture::new();
        let task = fixture.start();
        let (id, parent) = fixture.acquire_frame().await;
        fixture.channel.close_all();
        assert!(matches!(task.await.unwrap(), Err(error) if error.code == ErrorCode::Cancelled));
        fixture
            .channel
            .granted(
                &id,
                &parent,
                "owner-handle",
                "motionwright-data",
                &fixture.output,
            )
            .unwrap();
        fixture.released_once();
        let other = PermitInvocation::new(
            "new-parent",
            &CancellationToken::new(),
            fixture.channel.clone(),
        );
        assert!(matches!(other.acquire(9, &fixture.output).await,
            Err(error) if error.code == ErrorCode::Cancelled));
        fixture.no_frame();
    }

    #[tokio::test]
    async fn global_close_latch_denies_entry_and_check_before_registry_drain() {
        for already_borrowed in [false, true] {
            let mut fixture = Fixture::new();
            let permit = fixture.grant().await;
            let guard = if already_borrowed {
                Some(fixture.invocation.enter(&permit).unwrap())
            } else {
                None
            };
            let held_registry = fixture.channel.active.lock().unwrap();
            let channel = fixture.channel.clone();
            let closer = std::thread::spawn(move || channel.close_all());
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !fixture.channel.closed.load(Ordering::SeqCst)
                && std::time::Instant::now() < deadline
            {
                std::thread::yield_now();
            }
            let closing_seen = fixture.channel.closed.load(Ordering::SeqCst);
            let denied_before_drain = if let Some(guard) = &guard {
                guard.check_current().is_err()
            } else {
                fixture.invocation.enter(&permit).is_err()
            };
            drop(held_registry);
            closer.join().unwrap();
            drop(guard);
            fixture.released_once();
            drop(permit);
            fixture.no_frame();
            assert!(
                closing_seen,
                "closer did not publish its latch within the bound"
            );
            assert!(
                denied_before_drain,
                "global closure allowed entry/check before registry drain; borrowed={already_borrowed}"
            );
        }
    }

    #[tokio::test]
    async fn owner_logical_mount_is_preserved_but_is_not_current_authority() {
        for logical_mount in ["motionwright-data", "other-data"] {
            let mut fixture = Fixture::new();
            let task = fixture.start();
            let (id, parent) = fixture.acquire_frame().await;
            let reply = crate::Request::OperationPermitGranted {
                id,
                parent,
                permit: "owner-handle".into(),
                logical_mount: logical_mount.into(),
            };
            let encoded = serde_json::to_vec(&reply).unwrap();
            let reply: crate::Request = serde_json::from_slice(&encoded).unwrap();
            let crate::Request::OperationPermitGranted {
                id,
                parent,
                permit: handle,
                logical_mount: observed,
            } = reply
            else {
                panic!("wrong real protocol frame");
            };
            fixture
                .channel
                .granted(&id, &parent, &handle, &observed, &fixture.output)
                .unwrap();
            let permit = tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            assert_eq!(permit.logical_mount(), logical_mount);
            let guard = fixture.invocation.enter(&permit).unwrap();
            guard.check_current().unwrap();
            fixture.channel.close(&parent, &handle).unwrap();
            assert_eq!(permit.logical_mount(), logical_mount);
            assert!(guard.check_current().is_err());
            fixture.no_frame();
            drop(guard);
            fixture.released_once();
            drop(permit);
            fixture.no_frame();
        }
    }
    #[tokio::test]
    async fn invalid_or_missing_owner_logical_mount_never_becomes_a_permit() {
        assert!(
            serde_json::from_value::<crate::Request>(serde_json::json!({
                "type":"operation_permit_granted", "id":"a", "parent":"b", "permit":"c"
            }))
            .is_err()
        );
        for logical_mount in [
            "",
            "data\n",
            "data\0",
            "Data",
            "/data",
            "data/child",
            "semwright-internal-data",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        ] {
            let mut fixture = Fixture::new();
            let task = fixture.start();
            let (id, parent) = fixture.acquire_frame().await;
            let result = fixture.channel.granted(
                &id,
                &parent,
                "owner-handle",
                logical_mount,
                &fixture.output,
            );
            fixture.channel.close_all();
            let terminal = tokio::time::timeout(Duration::from_secs(2), task)
                .await
                .unwrap()
                .unwrap();
            assert!(matches!(result, Err(error) if error.code == ErrorCode::ProtocolMismatch));
            assert!(matches!(terminal, Err(error) if error.code == ErrorCode::Cancelled));
            assert!(fixture.channel.pending.lock().unwrap().is_empty());
            assert!(fixture.channel.active.lock().unwrap().is_empty());
            fixture.no_frame();
        }
    }
}
