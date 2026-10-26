//! One-shot owner admission retained until matched release or owned-process proof.
use super::{Broker, provider_mounts::BoundProvider, providers::ProviderLease};
use semwright_backend_api::Context;
use semwright_backend_api::provider::{OwnerOperationAuthority, OwnerOperationLease};
use semwright_types::{
    CommandDescriptor, Error, ErrorCode, ProviderMountReference, Reference, Result, SourceKind,
};
use std::sync::{Arc, Mutex, Weak};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
const MAX_OPERATION_SCOPES: usize = 256;
#[derive(Default)]
struct GateState {
    closing: bool,
    failed: bool,
    held: usize,
    scopes: Vec<Weak<OperationScope>>,
}
#[derive(Default)]
pub(super) struct OperationGate {
    state: Mutex<GateState>,
    changed: Notify,
}
struct OperationScope {
    session: String,
    closing: CancellationToken,
    used: std::sync::atomic::AtomicBool,
    gate: Arc<OperationGate>,
}
fn stale() -> Error {
    Error::new(
        ErrorCode::StaleReference,
        "Owner operation admission is no longer current",
    )
}
fn poisoned() -> Error {
    Error::new(
        ErrorCode::Internal,
        "Owner operation lifecycle lock poisoned",
    )
}
impl OperationGate {
    fn scope(
        self: &Arc<Self>,
        session: &str,
        parent: &CancellationToken,
    ) -> Result<Arc<OperationScope>> {
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        if state.closing || parent.is_cancelled() {
            return Err(stale());
        }
        state.scopes.retain(|scope| scope.strong_count() != 0);
        if state.scopes.len() >= MAX_OPERATION_SCOPES {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Owner operation scope budget exhausted",
            ));
        }
        let scope = Arc::new(OperationScope {
            session: session.into(),
            closing: parent.child_token(),
            used: std::sync::atomic::AtomicBool::new(false),
            gate: self.clone(),
        });
        state.scopes.push(Arc::downgrade(&scope));
        Ok(scope)
    }
    pub(super) fn close(&self) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        state.closing = true;
        for scope in state.scopes.iter().filter_map(Weak::upgrade) {
            scope.closing.cancel();
        }
        Ok(())
    }
    pub(super) fn revoke_session(&self, session: &str) {
        if let Ok(state) = self.state.lock() {
            for scope in state.scopes.iter().filter_map(Weak::upgrade) {
                if scope.session == session {
                    scope.closing.cancel();
                }
            }
        }
    }
    pub(super) fn drained(&self) -> Result<bool> {
        let state = self.state.lock().map_err(|_| poisoned())?;
        if state.failed {
            return Err(poisoned());
        }
        Ok(state.held == 0)
    }
    pub(super) async fn wait_drained(&self) -> Result<()> {
        loop {
            let notified = self.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.drained()? {
                return Ok(());
            }
            notified.await;
        }
    }
    fn grant(&self, scope: &Arc<OperationScope>) -> Result<()> {
        use std::sync::atomic::Ordering;
        let mut state = self.state.lock().map_err(|_| poisoned())?;
        if state.closing || scope.closing.is_cancelled() {
            return Err(stale());
        }
        if scope.used.swap(true, Ordering::SeqCst) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "An operation permit is single-use",
            ));
        }
        state.held = state.held.checked_add(1).ok_or_else(poisoned)?;
        Ok(())
    }
}
struct GrantedOperation {
    scope: Arc<OperationScope>,
    logical_mount: String,
}
impl OwnerOperationLease for GrantedOperation {
    fn check_current(&self) -> Result<()> {
        let state = self.scope.gate.state.lock().map_err(|_| poisoned())?;
        if state.closing || self.scope.closing.is_cancelled() {
            return Err(stale());
        }
        Ok(())
    }
    fn closing(&self) -> CancellationToken {
        self.scope.closing.clone()
    }
    fn logical_mount(&self) -> &str {
        &self.logical_mount
    }
}
impl Drop for GrantedOperation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.scope.gate.state.lock() {
            if state.held == 0 {
                state.closing = true;
                state.failed = true;
            } else {
                state.held -= 1;
            }
        }
        self.scope.gate.changed.notify_waiters();
    }
}
struct CoreAuthority {
    broker: Weak<Broker>,
    lease: Weak<ProviderLease>,
    reference_id: String,
    expected: Reference,
    binding_id: String,
    binding: ProviderMountReference,
    scope: Arc<OperationScope>,
}
impl OwnerOperationAuthority for CoreAuthority {
    fn acquire(&self) -> Result<Box<dyn OwnerOperationLease>> {
        let broker = self.broker.upgrade().ok_or_else(stale)?;
        let lease = self.lease.upgrade().ok_or_else(stale)?;
        let bound = BoundProvider {
            reference_id: self.binding_id.clone(),
            lease: lease.clone(),
            admission: self.binding.clone(),
            parent_request_id: None,
        };
        broker.check_bound_provider(&self.scope.session, &bound, Some(&lease))?;
        let catalog = broker.registry.read().map_err(|_| poisoned())?;
        if !catalog
            .providers
            .get(&lease.identity.id)
            .is_some_and(|current| Arc::ptr_eq(current, &lease))
            || !lease.active()
            || lease.binding_epoch.is_cancelled()
        {
            return Err(stale());
        }
        let references = broker.references.lock().map_err(|_| poisoned())?;
        let current = references.get(&self.reference_id, &self.scope.session)?;
        let current_binding =
            references.get_provider_mount(&self.binding_id, &self.scope.session)?;
        let a = &current.target;
        let b = &self.expected.target;
        if current.backend != self.expected.backend
            || current.session != self.expected.session
            || current.expires != self.expected.expires
            || a.kind != b.kind
            || a.app != b.app
            || a.identity != b.identity
            || a.revision != b.revision
            || a.fingerprint != b.fingerprint
            || !current_binding.same_admission(&self.binding)
            || self.binding.is_revoked()
            || current_binding.connection != lease.connection
            || current_binding.generation != lease.generation
        {
            return Err(stale());
        }
        let capsule = lease
            .workspace_mounts
            .as_ref()
            .map_err(Clone::clone)?
            .iter()
            .find(|capsule| capsule.mount().logical_name == self.binding.mount)
            .ok_or_else(stale)?;
        capsule.validate_current().map_err(|_| stale())?;
        self.scope.gate.grant(&self.scope)?;
        drop(references);
        drop(catalog);
        Ok(Box::new(GrantedOperation {
            scope: self.scope.clone(),
            logical_mount: self.binding.mount.clone(),
        }))
    }
}
pub(super) fn authority(
    broker: &Arc<Broker>,
    context: &Context,
    descriptor: &CommandDescriptor,
    reference_id: Option<&str>,
    reference: Option<&Reference>,
    provider: Option<&Arc<ProviderLease>>,
    bound: Option<&BoundProvider>,
) -> Result<Option<Arc<dyn OwnerOperationAuthority>>> {
    let (Some(lease), Some(bound)) = (provider, bound) else {
        return Ok(None);
    };
    if !descriptor.risk.mutates()
        || lease.identity.kind != SourceKind::Driver
        || !lease.supports_owner_operation_permits()
    {
        return Ok(None);
    }
    let (Some(reference_id), Some(reference)) = (reference_id, reference) else {
        return Ok(None);
    };
    if reference.backend != lease.identity.id
        || reference.session != context.session
        || reference.target.kind != "native"
    {
        return Err(stale());
    }
    broker.check_bound_provider(&context.session, bound, Some(lease))?;
    let catalog = broker.registry.read().map_err(|_| poisoned())?;
    if !catalog
        .providers
        .get(&lease.identity.id)
        .is_some_and(|current| Arc::ptr_eq(current, lease))
        || !lease.active()
    {
        return Err(stale());
    }
    let scope = lease
        .operation_gate
        .scope(&context.session, &context.cancellation)?;
    drop(catalog);
    Ok(Some(Arc::new(CoreAuthority {
        broker: Arc::downgrade(broker),
        lease: Arc::downgrade(lease),
        reference_id: reference_id.into(),
        expected: reference.clone(),
        binding_id: bound.reference_id.clone(),
        binding: bound.admission.clone(),
        scope,
    })))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    fn held(
        gate: &Arc<OperationGate>,
        session: &str,
        parent: &CancellationToken,
    ) -> GrantedOperation {
        let scope = gate.scope(session, parent).unwrap();
        gate.grant(&scope).unwrap();
        GrantedOperation {
            scope,
            logical_mount: "test-data".into(),
        }
    }
    #[tokio::test]
    async fn closing_refuses_new_admission_and_waits_for_the_original_guard() {
        let gate = Arc::new(OperationGate::default());
        let parent = CancellationToken::new();
        let original = held(&gate, "a", &parent);
        let unused = gate.scope("a", &parent).unwrap();
        gate.close().unwrap();
        assert!(original.closing().is_cancelled());
        assert!(original.check_current().is_err());
        assert!(gate.scope("a", &parent).is_err());
        assert!(gate.grant(&unused).is_err());
        assert!(!gate.drained().unwrap());
        assert!(
            tokio::time::timeout(Duration::from_millis(20), gate.wait_drained())
                .await
                .is_err()
        );
        drop(original);
        tokio::time::timeout(Duration::from_secs(1), gate.wait_drained())
            .await
            .unwrap()
            .unwrap();
    }
    #[test]
    fn session_revocation_closes_only_its_admitted_scopes() {
        let gate = Arc::new(OperationGate::default());
        let parent = CancellationToken::new();
        let first = held(&gate, "a", &parent);
        let other = held(&gate, "b", &parent);
        let unused = gate.scope("a", &parent).unwrap();
        gate.revoke_session("a");
        assert!(first.check_current().is_err());
        assert!(first.closing().is_cancelled());
        assert!(gate.grant(&unused).is_err());
        assert!(other.check_current().is_ok());
        assert!(!other.closing().is_cancelled());
        drop(first);
        assert!(!gate.drained().unwrap());
        drop(other);
        assert!(gate.drained().unwrap());
    }
    #[test]
    fn parent_cancellation_and_single_use_cannot_mint_another_hold() {
        let gate = Arc::new(OperationGate::default());
        let parent = CancellationToken::new();
        let original = held(&gate, "a", &parent);
        assert!(matches!(gate.grant(&original.scope), Err(ref e) if e.code == ErrorCode::Conflict));
        let unused = gate.scope("a", &parent).unwrap();
        parent.cancel();
        assert!(gate.grant(&unused).is_err());
        assert!(gate.scope("a", &parent).is_err());
        assert!(original.check_current().is_err());
        assert!(!gate.drained().unwrap());
        drop(original);
        assert!(gate.drained().unwrap());
    }
}
