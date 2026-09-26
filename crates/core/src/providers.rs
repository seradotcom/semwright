//! The registry and provider leases commit together. No catalog lock crosses an await.
use super::*;
use semwright_backend_api::{NativeProvider, Provider, ProviderSignal};
use semwright_registry::{CapabilitySnapshot, Metadata};
use std::ops::{Deref, DerefMut};

#[derive(Clone)]
pub(super) struct ProviderCatalog {
    pub registry: Registry,
    pub providers: BTreeMap<String, Arc<ProviderLease>>,
}
impl Deref for ProviderCatalog {
    type Target = Registry;
    fn deref(&self) -> &Registry {
        &self.registry
    }
}
impl DerefMut for ProviderCatalog {
    fn deref_mut(&mut self) -> &mut Registry {
        &mut self.registry
    }
}
pub(super) struct ProviderLease {
    pub provider: Arc<dyn Provider>,
    pub identity: ProviderIdentity,
    pub generation: u64,
    pub connection: String,
    pub lifetime: CancellationToken,
    pub epoch: CancellationToken,
}
impl Deref for ProviderLease {
    type Target = dyn Provider;
    fn deref(&self) -> &Self::Target {
        self.provider.as_ref()
    }
}
impl ProviderLease {
    pub fn active(&self) -> bool {
        !self.lifetime.is_cancelled()
            && !self.epoch.is_cancelled()
            && self
                .provider
                .closed()
                .is_none_or(|closed| !closed.is_cancelled())
            && self.provider.identity() == &self.identity
    }
}
pub(super) struct Invocation {
    pub capability: CapabilitySnapshot,
    pub dynamic_provider: Option<Arc<ProviderLease>>,
}
impl ProviderCatalog {
    pub fn bootstrap(
        registry: Registry,
        backends: BTreeMap<String, Arc<dyn Backend>>,
        stop: &CancellationToken,
    ) -> Self {
        let mut providers = BTreeMap::new();
        for (name, backend) in backends {
            // These are explicit trusted composition-root identities, not command-prefix inference.
            let (id, app) = match name.as_str() {
                "blender" => (
                    "blender-native".to_owned(),
                    Some("org.blender.Blender".to_owned()),
                ),
                "chromium" => (
                    "browser-cdp".to_owned(),
                    Some("org.semwright.Chromium".to_owned()),
                ),
                "fake" => ("fixture-desktop".to_owned(), None),
                other => (format!("linux-{other}"), None),
            };
            let identity = ProviderIdentity {
                id,
                kind: SourceKind::Builtin,
                version: env!("CARGO_PKG_VERSION").into(),
                namespace: String::new(),
                application: app,
                origin: "semwright-compiled-provider".into(),
            };
            let capabilities = registry
                .all()
                .filter(|d| {
                    backend.supports(&d.name) && (d.backends.contains(&name) || name == "fake")
                })
                .cloned()
                .map(Into::into)
                .collect();
            let provider: Arc<dyn Provider> =
                Arc::new(NativeProvider::new(backend, identity.clone(), capabilities));
            let lifetime = stop.child_token();
            let epoch = lifetime.child_token();
            providers.insert(
                name,
                Arc::new(ProviderLease {
                    provider,
                    identity,
                    generation: 0,
                    connection: unique_id(),
                    lifetime,
                    epoch,
                }),
            );
        }
        Self {
            registry,
            providers,
        }
    }
}
impl Broker {
    pub fn catalog_revision(&self) -> Result<u64> {
        Ok(self
            .registry
            .read()
            .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?
            .revision())
    }
    pub(super) fn invocation(&self, name: &str) -> Result<Invocation> {
        let catalog = self
            .registry
            .read()
            .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
        let capability = catalog.snapshot(name)?;
        let dynamic_provider = if !matches!(
            capability.metadata.source,
            SourceKind::Builtin | SourceKind::Recipe
        ) && capability.descriptor.backends != ["plugin"]
        {
            Some(
                catalog
                    .providers
                    .get(&capability.metadata.provider)
                    .cloned()
                    .ok_or_else(|| Error::unavailable("Capability provider is not installed"))?,
            )
        } else {
            None
        };
        Ok(Invocation {
            capability,
            dynamic_provider,
        })
    }
    pub(super) fn provider(&self, route: &str) -> Option<Arc<ProviderLease>> {
        self.registry.read().ok()?.providers.get(route).cloned()
    }
    pub(super) fn providers(&self) -> Vec<Arc<ProviderLease>> {
        self.registry
            .read()
            .map(|c| c.providers.values().cloned().collect())
            .unwrap_or_default()
    }
    pub fn provider_status(&self) -> Result<Value> {
        let catalog = self
            .registry
            .read()
            .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
        let rows: Vec<_> = catalog.providers.iter().map(|(route, lease)| {
            let features = lease.interfaces();
            json!({"identity":lease.identity,"route":route,"generation":lease.generation,"connected":lease.active(),
                "interfaces":{"dynamic_capabilities":features.dynamic_capabilities,"cancellation":features.cooperative_cancellation,
                    "events":features.events,"progress":features.progress,"artifacts":features.artifacts,"health":features.health}})
        }).collect();
        Ok(
            json!({"providers":rows,"revision":catalog.revision(),"registration":"owner configuration only"}),
        )
    }
    async fn provider_catalog(
        provider: &Arc<dyn Provider>,
        identity: &ProviderIdentity,
    ) -> Result<Vec<(CommandDescriptor, Metadata)>> {
        let capabilities = tokio::time::timeout(Duration::from_secs(10), provider.capabilities())
            .await
            .map_err(|_| {
                Error::new(
                    ErrorCode::Timeout,
                    "Provider capability enumeration timed out",
                )
            })??;
        if provider.identity() != identity || provider.name() != identity.id {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Provider identity changed during enumeration",
            ));
        }
        if capabilities.len() > 2048 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Provider catalog exceeds its capability limit",
            ));
        }
        let mut rows = Vec::with_capacity(capabilities.len());
        for capability in capabilities {
            if !provider.supports(&capability.descriptor.name) {
                return Err(Error::invalid(
                    "Provider enumerated an unsupported operation",
                ));
            }
            let mut metadata = Metadata::for_provider(identity);
            metadata.aliases = capability.aliases;
            metadata.tags = capability.tags;
            metadata.object_types = capability.object_types;
            rows.push((capability.descriptor, metadata));
        }
        Ok(rows)
    }
    /// Owner-loaded bootstrap only. This is not an unauthenticated wire registration API.
    pub async fn mount_provider(self: &Arc<Self>, provider: Arc<dyn Provider>) -> Result<u64> {
        if self.fake {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Fake mode cannot connect dynamic application providers",
            ));
        }
        if self.runtime_stop.is_cancelled() {
            return Err(Error::unavailable("Broker is shutting down"));
        }
        let identity = provider.identity().clone();
        identity.validate_external()?;
        if provider.name() != identity.id {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Dynamic route must match its provider ID",
            ));
        }
        let signals = provider.events();
        let closed = provider.closed();
        let rows = Self::provider_catalog(&provider, &identity).await?;
        let mut candidate = self
            .registry
            .read()
            .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?
            .clone();
        if candidate.providers.contains_key(&identity.id) {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Provider ID is already connected",
            ));
        }
        if candidate.providers.len() >= 64 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Provider connection limit reached",
            ));
        }
        let expected = candidate.revision();
        let compile_identity = identity.clone();
        let mut candidate = tokio::task::spawn_blocking(move || -> Result<ProviderCatalog> {
            candidate.replace_provider_catalog(&compile_identity, rows, expected, false)?;
            Ok(candidate)
        })
        .await
        .map_err(|_| {
            Error::new(
                ErrorCode::Internal,
                "Provider catalog validation task failed",
            )
        })??;
        let lifetime = self.runtime_stop.child_token();
        let lease = Arc::new(ProviderLease {
            provider,
            identity: identity.clone(),
            generation: candidate.revision(),
            connection: unique_id(),
            epoch: lifetime.child_token(),
            lifetime,
        });
        if !lease.active() {
            return Err(Error::unavailable(
                "Provider disconnected before registration",
            ));
        }
        candidate
            .providers
            .insert(identity.id.clone(), lease.clone());
        let revision = candidate.revision();
        {
            let mut catalog = self
                .registry
                .write()
                .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
            if catalog.revision() != expected {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Catalog changed during provider registration",
                ));
            }
            *catalog = candidate;
        }
        self.provider_lifecycle("provider.connected", &lease, revision);
        self.watch_provider(identity.id.clone(), lease, signals, closed);
        Ok(revision)
    }
    fn provider_lifecycle(&self, kind: &str, lease: &ProviderLease, revision: u64) {
        self.event(
            EventEnvelope::new(kind, "semwright-core", event_time()).with_provider(
                lease.identity.id.clone(),
                Some(lease.generation),
                Some(revision),
            ),
        );
    }
    pub fn invalidate_provider(&self, id: &str) -> Result<u64> {
        let lease = self
            .provider(id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Provider not found"))?;
        self.invalidate_connection(id, &lease.connection)
    }
    fn terminate_connection(&self, id: &str, connection: &str) -> Result<u64> {
        let lease = self
            .provider(id)
            .filter(|provider| provider.connection == connection)
            .ok_or_else(|| {
                Error::new(ErrorCode::Conflict, "Provider connection has been replaced")
            })?;
        // Terminal transport/provider closure is distinct from an epoch invalidation:
        // once lifetime is cancelled, refresh cannot manufacture a new live epoch.
        lease.lifetime.cancel();
        self.invalidate_connection(id, connection)
    }

    fn invalidate_connection(&self, id: &str, connection: &str) -> Result<u64> {
        let mut catalog = self
            .registry
            .write()
            .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
        let lease = catalog
            .providers
            .get(id)
            .cloned()
            .filter(|p| p.connection == connection)
            .ok_or_else(|| {
                Error::new(ErrorCode::Conflict, "Provider connection has been replaced")
            })?;
        if lease.identity.kind == SourceKind::Builtin {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Native provider lifecycle is owned by the broker",
            ));
        }
        let previous = catalog.revision();
        let revision = catalog.touch(previous)?;
        lease.epoch.cancel();
        drop(catalog);
        self.provider_lifecycle("provider.disconnected", &lease, revision);
        Ok(revision)
    }
    pub async fn refresh_provider(self: &Arc<Self>, id: &str) -> Result<u64> {
        let lease = self
            .provider(id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Provider not found"))?;
        self.refresh_connection(id, &lease.connection, false).await
    }
    async fn refresh_connection(
        self: &Arc<Self>,
        id: &str,
        connection: &str,
        invalidate_previous_epoch: bool,
    ) -> Result<u64> {
        // A catalog change is not a transport cancellation. Existing calls keep their pinned
        // descriptor/lease; a successful refresh atomically publishes a new generation for future
        // calls. Invalid replacement metadata still revokes the current generation fail-closed.
        let lease = self
            .provider(id)
            .filter(|p| p.connection == connection && p.active())
            .ok_or_else(|| Error::new(ErrorCode::Conflict, "Provider connection changed"))?;
        let rows = match Self::provider_catalog(&lease.provider, &lease.identity).await {
            Ok(rows) => rows,
            Err(error) => {
                let _ = self.invalidate_connection(id, connection);
                return Err(error);
            }
        };
        let mut candidate = self
            .registry
            .read()
            .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?
            .clone();
        if !candidate
            .providers
            .get(id)
            .is_some_and(|p| Arc::ptr_eq(p, &lease))
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Provider lease changed during refresh",
            ));
        }
        let expected = candidate.revision();
        let identity = lease.identity.clone();
        let validated = tokio::task::spawn_blocking(move || -> Result<ProviderCatalog> {
            candidate.replace_provider_catalog(&identity, rows, expected, true)?;
            Ok(candidate)
        })
        .await
        .map_err(|_| {
            Error::new(
                ErrorCode::Internal,
                "Provider catalog validation task failed",
            )
        })?;
        let mut candidate = match validated {
            Ok(candidate) => candidate,
            Err(error) => {
                let _ = self.invalidate_connection(id, connection);
                return Err(error);
            }
        };
        let updated = Arc::new(ProviderLease {
            provider: lease.provider.clone(),
            identity: lease.identity.clone(),
            generation: candidate.revision(),
            connection: lease.connection.clone(),
            lifetime: lease.lifetime.clone(),
            epoch: lease.lifetime.child_token(),
        });
        if !updated.active() {
            return Err(Error::unavailable("Provider is no longer connected"));
        }
        candidate.providers.insert(id.into(), updated.clone());
        let revision = candidate.revision();
        {
            let mut catalog = self
                .registry
                .write()
                .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
            if catalog.revision() != expected
                || !catalog
                    .providers
                    .get(id)
                    .is_some_and(|p| Arc::ptr_eq(p, &lease))
            {
                return Err(Error::new(
                    ErrorCode::Conflict,
                    "Catalog changed during provider refresh",
                ));
            }
            *catalog = candidate;
        }
        if invalidate_previous_epoch {
            lease.epoch.cancel();
        }
        self.provider_lifecycle("provider.capabilities.changed", &updated, revision);
        Ok(revision)
    }
    pub async fn remove_provider(&self, id: &str) -> Result<u64> {
        let (lease, revision) = {
            let mut catalog = self
                .registry
                .write()
                .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
            let lease = catalog
                .providers
                .get(id)
                .cloned()
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "Provider not found"))?;
            let previous = catalog.revision();
            let revision = catalog.remove_provider_catalog(&lease.identity, previous)?;
            lease.lifetime.cancel();
            catalog.providers.remove(id);
            (lease, revision)
        };
        self.provider_lifecycle("provider.removed", &lease, revision);
        tokio::time::timeout(Duration::from_secs(5), lease.shutdown())
            .await
            .map_err(|_| {
                Error::new(
                    ErrorCode::Timeout,
                    "Provider was removed but shutdown timed out",
                )
            })??;
        Ok(revision)
    }
    pub(super) fn start_builtin_provider_watchers(self: &Arc<Self>) -> Result<()> {
        if !self.policy.capabilities().contains("ui.observe") {
            return Ok(());
        }
        let watchers = {
            let catalog = self
                .registry
                .read()
                .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
            catalog
                .providers
                .iter()
                .filter(|(_, lease)| lease.identity.kind == SourceKind::Builtin)
                .filter_map(|(route, lease)| {
                    let signals = lease.events();
                    let closed = lease.closed();
                    (signals.is_some() || closed.is_some())
                        .then(|| (route.clone(), lease.clone(), signals, closed))
                })
                .collect::<Vec<_>>()
        };
        if watchers.is_empty() {
            return Ok(());
        }
        tokio::runtime::Handle::try_current().map_err(|_| {
            Error::unavailable(
                "Event-capable native backends with ui.observe require an active Tokio runtime",
            )
        })?;
        for (route, lease, signals, closed) in watchers {
            self.watch_provider(route, lease, signals, closed);
        }
        Ok(())
    }

    fn touch_connection_generation(
        &self,
        route: &str,
        connection: &str,
        lifecycle: &str,
    ) -> Result<u64> {
        let (previous, updated, revision) = {
            let mut catalog = self
                .registry
                .write()
                .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
            let previous = catalog
                .providers
                .get(route)
                .cloned()
                .filter(|provider| provider.connection == connection && provider.active())
                .ok_or_else(|| Error::new(ErrorCode::Conflict, "Provider connection changed"))?;
            let mut candidate = catalog.clone();
            let expected = candidate.revision();
            let revision = candidate.touch(expected)?;
            let updated = Arc::new(ProviderLease {
                provider: previous.provider.clone(),
                identity: previous.identity.clone(),
                generation: revision,
                connection: previous.connection.clone(),
                lifetime: previous.lifetime.clone(),
                epoch: previous.lifetime.child_token(),
            });
            if !updated.active() {
                return Err(Error::unavailable("Provider is no longer connected"));
            }
            candidate
                .providers
                .insert(route.to_owned(), updated.clone());
            *catalog = candidate;
            (previous, updated, revision)
        };
        previous.epoch.cancel();
        self.provider_lifecycle(lifecycle, &updated, revision);
        Ok(revision)
    }

    async fn recover_signal_loss(self: &Arc<Self>, route: &str, connection: &str) -> Result<u64> {
        let kind = self
            .provider(route)
            .filter(|provider| provider.connection == connection && provider.active())
            .ok_or_else(|| Error::new(ErrorCode::Conflict, "Provider connection changed"))?
            .identity
            .kind;
        if kind == SourceKind::Builtin {
            self.touch_connection_generation(route, connection, "provider.generation.invalidated")
        } else {
            self.refresh_connection(route, connection, true).await
        }
    }

    fn watch_provider(
        self: &Arc<Self>,
        route: String,
        lease: Arc<ProviderLease>,
        mut signals: Option<broadcast::Receiver<ProviderSignal>>,
        closed: Option<CancellationToken>,
    ) {
        if signals.is_none() && closed.is_none() {
            return;
        }
        let broker = Arc::downgrade(self);
        self.provider_tasks.spawn(async move {
            let never = CancellationToken::new();
            let closed = closed.unwrap_or(never);
            let builtin = lease.identity.kind == SourceKind::Builtin;
            let connection = lease.connection.clone();
            loop {
                let signal = tokio::select! {
                    biased;
                    _ = lease.lifetime.cancelled() => break,
                    _ = closed.cancelled() => {
                        if let Some(broker) = broker.upgrade() {
                            if builtin {
                                let _ = broker.touch_connection_generation(
                                    &route,
                                    &connection,
                                    "provider.generation.invalidated",
                                );
                            } else {
                                let _ = broker.terminate_connection(&route, &connection);
                            }
                        }
                        break;
                    },
                    signal = async {
                        match signals.as_mut() { Some(rx) => rx.recv().await, None => std::future::pending().await }
                    } => signal,
                };
                let Some(broker) = broker.upgrade() else { break; };
                match signal {
                    Ok(ProviderSignal::Disconnected) | Err(broadcast::error::RecvError::Closed) => {
                        if builtin {
                            let _ = broker.touch_connection_generation(
                                &route,
                                &connection,
                                "provider.generation.invalidated",
                            );
                        } else {
                            let _ = broker.terminate_connection(&route, &connection);
                        }
                        break;
                    }
                    Ok(ProviderSignal::CapabilitiesChanged) => {
                        let result = if builtin {
                            broker.touch_connection_generation(
                                &route,
                                &connection,
                                "provider.capabilities.changed",
                            )
                        } else {
                            tokio::select! {
                                _ = lease.lifetime.cancelled() => break,
                                result = broker.refresh_connection(&route, &connection, false) => result,
                            }
                        };
                        if result.is_err() && !builtin {
                            let _ = broker.invalidate_connection(&route, &connection);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(dropped)) => {
                        let result = tokio::select! {
                            _ = lease.lifetime.cancelled() => break,
                            result = broker.recover_signal_loss(&route, &connection) => result,
                        };
                        if result.is_ok() {
                            let _ = broker.ingest_provider_event(
                                &route,
                                &connection,
                                semwright_types::semantic_ui_event::BACKEND_INVALIDATED,
                                json!({"reason":"provider_signal_lag","dropped":dropped}),
                            );
                        }
                    }
                    Ok(ProviderSignal::Event { kind, payload }) => {
                        if kind == semwright_types::semantic_ui_event::BACKEND_INVALIDATED {
                            let result = tokio::select! {
                                _ = lease.lifetime.cancelled() => break,
                                result = broker.recover_signal_loss(&route, &connection) => result,
                            };
                            if result.is_err() {
                                continue;
                            }
                        }
                        let _ = broker.ingest_provider_event(
                            &route,
                            &connection,
                            &kind,
                            payload,
                        );
                    }
                    Ok(ProviderSignal::Progress {
                        request_id,
                        progress,
                        artifacts,
                    }) => {
                        let _ = broker.ingest_provider_progress(
                            &route,
                            &connection,
                            &request_id,
                            progress,
                            artifacts,
                        );
                    }
                }
            }
        });
    }
    fn ingest_provider_progress(
        &self,
        id: &str,
        connection: &str,
        request_id: &str,
        progress: JobProgress,
        artifacts: Vec<JobArtifact>,
    ) -> Result<()> {
        if request_id.is_empty()
            || request_id.len() > 128
            || request_id.chars().any(char::is_control)
        {
            return Err(Error::invalid("Provider progress request ID is invalid"));
        }
        let current = self
            .provider(id)
            .filter(|provider| provider.connection == connection && provider.active())
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::Conflict,
                    "Progress belongs to an inactive provider",
                )
            })?;
        if !current.interfaces().progress {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Provider did not negotiate progress reporting",
            ));
        }

        let (owner, command) = self
            .jobs
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Job store lock poisoned"))?
            .correlation(request_id)
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::NotFound,
                    "Progress does not target an active job",
                )
            })?;

        let invocation = self.invocation(&command)?;
        if invocation
            .dynamic_provider
            .as_ref()
            .is_none_or(|provider| provider.identity.id != id || provider.connection != connection)
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Provider cannot report progress for another provider job",
            ));
        }

        let (event_owner, snapshot) = self
            .jobs
            .lock()
            .map_err(|_| Error::new(ErrorCode::Internal, "Job store lock poisoned"))?
            .update_progress(request_id, progress, artifacts)?;
        if event_owner != owner {
            return Err(Error::new(
                ErrorCode::Internal,
                "Job ownership changed during progress update",
            ));
        }
        self.job_event(&owner, "job.progress", &snapshot);
        Ok(())
    }

    fn ingest_provider_event(
        &self,
        id: &str,
        connection: &str,
        kind: &str,
        payload: Value,
    ) -> Result<()> {
        if kind.is_empty()
            || kind.len() > 128
            || kind.starts_with("provider.")
            || !kind.bytes().all(|b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
            })
        {
            return Err(Error::invalid("Provider event kind is invalid or reserved"));
        }
        semwright_registry::bounds::value_budget(&payload)?;
        if serde_json::to_vec(&payload)?.len() > 16384 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Provider event payload is too large",
            ));
        }
        let catalog = self
            .registry
            .read()
            .map_err(|_| Error::new(ErrorCode::Internal, "Catalog lock poisoned"))?;
        let current = catalog
            .providers
            .get(id)
            .filter(|p| p.connection == connection && p.active())
            .ok_or_else(|| {
                Error::new(ErrorCode::Conflict, "Event belongs to an inactive provider")
            })?;
        if current.identity.kind == SourceKind::Builtin {
            if !kind.starts_with("semantic.") {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Native providers may expose only normalized semantic events",
                ));
            }
            if !self.policy.capabilities().contains("ui.observe") {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Native semantic events require ui.observe",
                ));
            }
        } else if !self
            .policy
            .capabilities()
            .iter()
            .any(|scope| scope == &current.identity.id)
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Provider event scope is not granted",
            ));
        }
        self.event(
            EventEnvelope::new(kind, current.identity.id.clone(), event_time())
                .with_provider(
                    current.identity.id.clone(),
                    Some(current.generation),
                    Some(catalog.revision()),
                )
                .with_untrusted_payload(payload),
        );
        Ok(())
    }
}
