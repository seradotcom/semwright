use crate::{
    DRIVER_ID, DRIVER_SCOPE, VERSION,
    live_projection::{ArdourSemanticProjection, ArdourSnapshot},
    native::{ArdourSnapshot as NativeArdourSnapshot, RouteKind},
    osc::{DEFAULT_ARDOUR_OSC_PORT, OscArg, OscClient, StripList},
    projection::{ArdourProjection, semantic_id},
    runtime::DeepRuntime,
    script::NativeMutation,
};
use async_trait::async_trait;
use semwright_audio_domain::{
    Error as DomainError,
    backend::{ProjectionFidelity, ProjectionLoss, ProjectionReport, SemanticAudioProjection},
    refs::RefStore,
    units::MilliDb,
};
use semwright_driver_sdk::{Capability, Driver, DriverInterfaces, descriptor_digest};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tokio::time::{Duration, sleep};

const PROJECT_ID: &str = "ardour_session";

struct Cached {
    revision: String,
    report: ProjectionReport,
    id_to_ssid: BTreeMap<String, u32>,
}

pub struct ArdourAudioDriver {
    port: u16,
    refs: RefStore,
    cache: Option<Cached>,
    generation: u64,
    deep_runtime: Option<DeepRuntime>,
    deep_runtime_reason: String,
    capabilities: Vec<Capability>,
}

impl ArdourAudioDriver {
    pub fn discover() -> Result<Self> {
        let port = match std::env::var("SEMWRIGHT_ARDOUR_OSC_PORT") {
            Ok(value) => value
                .parse::<u16>()
                .ok()
                .filter(|value| *value != 0)
                .ok_or_else(|| Error::invalid("Invalid SEMWRIGHT_ARDOUR_OSC_PORT"))?,
            Err(_) => DEFAULT_ARDOUR_OSC_PORT,
        };
        let (deep_runtime, deep_runtime_reason) = match DeepRuntime::load_production()? {
            Some(runtime) => (
                Some(runtime),
                "owner-pinned Ardour Lua runtime is available".to_string(),
            ),
            None => (
                None,
                if cfg!(unix) {
                    "owner-pinned Ardour runtime/project/output grants are absent".to_string()
                } else {
                    "deep Ardour Lua runtime is disabled until executable pinning is certified on this platform".to_string()
                },
            ),
        };
        Ok(Self {
            port,
            refs: RefStore::new(8_192),
            cache: None,
            generation: 0,
            deep_runtime,
            deep_runtime_reason,
            capabilities: capability_catalog(),
        })
    }

    fn verify_digest(&self, command: &str, digest: &str) -> Result<()> {
        let capability = self
            .capabilities
            .iter()
            .find(|capability| capability.descriptor.name == command)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, "Ardour capability is absent"))?;
        if descriptor_digest(&capability.descriptor)? != digest {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Pinned Ardour descriptor digest mismatch",
            ));
        }
        Ok(())
    }

    async fn query(&self) -> Result<StripList> {
        OscClient::loopback(self.port)
            .await?
            .query_strip_list()
            .await
    }

    fn install(&mut self, strip_list: StripList) -> Result<Value> {
        self.generation = self.generation.wrapping_add(1).max(1);
        let snapshot = ArdourSnapshot { strip_list };
        let report = ArdourSemanticProjection::report(&snapshot).map_err(map_domain_error)?;
        let semantic = report.project.semantic_digest().map_err(map_domain_error)?;
        let revision = format!(
            "{:x}",
            Sha256::digest(format!("{semantic}:{}", self.generation).as_bytes())
        );
        let mut id_to_ssid = BTreeMap::new();
        for strip in &snapshot.strip_list.strips {
            let id = match strip.kind.as_str() {
                "AT" => Some(format!("ardour_track_{}", strip.ssid)),
                "B" | "FB" => Some(format!("ardour_bus_{}", strip.ssid)),
                _ => None,
            };
            if let Some(id) = id {
                id_to_ssid.insert(id, strip.ssid);
            }
        }

        self.refs.invalidate(PROJECT_ID);
        let project_ref = self
            .refs
            .issue(PROJECT_ID, &revision, "project", PROJECT_ID)
            .map_err(map_domain_error)?;

        let mut stems = Vec::new();
        for stem in &report.project.stems {
            let reference = self
                .refs
                .issue(PROJECT_ID, &revision, "stem", &stem.id)
                .map_err(map_domain_error)?;
            stems.push(json!({
                "reference": reference,
                "id": stem.id,
                "name": stem.name,
                "channels": stem.channels,
                "muted": stem.muted,
                "soloed": stem.soloed
            }));
        }
        let mut buses = Vec::new();
        for bus in &report.project.buses {
            if let Some(ssid) = id_to_ssid.get(&bus.id) {
                let reference = self
                    .refs
                    .issue(PROJECT_ID, &revision, "bus", &bus.id)
                    .map_err(map_domain_error)?;
                buses.push(json!({
                    "reference": reference,
                    "id": bus.id,
                    "ssid": ssid,
                    "name": bus.name,
                    "channels": bus.channels
                }));
            }
        }
        let response = json!({
            "project": project_ref,
            "revision": revision,
            "sample_rate": report.project.profile.sample_rate.0,
            "last_frame": snapshot.strip_list.last_frame,
            "monitor_present": snapshot.strip_list.monitor_present,
            "stems": stems,
            "buses": buses,
            "fidelity": "lossy_read_only",
            "losses": losses_json(&report.losses)
        });
        self.cache = Some(Cached {
            revision,
            report,
            id_to_ssid,
        });
        Ok(response)
    }

    async fn inspect(&mut self) -> Result<Value> {
        let list = self.query().await?;
        self.install(list)
    }

    fn resolve_target(&self, args: &Value, kind: &str) -> Result<(String, u32, String)> {
        let cache = self
            .cache
            .as_ref()
            .ok_or_else(|| Error::new(ErrorCode::StaleReference, "Inspect Ardour session first"))?;
        let expected = text_arg(args, "expected_revision", 64)?;
        if expected != cache.revision {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Ardour snapshot revision is stale",
            ));
        }
        let reference = text_arg(args, kind, 80)?;
        let id = self
            .refs
            .resolve(reference, PROJECT_ID, &cache.revision, kind)
            .map_err(map_domain_error)?;
        let ssid = cache
            .id_to_ssid
            .get(&id)
            .copied()
            .ok_or_else(|| Error::new(ErrorCode::StaleReference, "Ardour SSID is stale"))?;
        Ok((id, ssid, cache.revision.clone()))
    }

    async fn mutate_verified(&mut self, command: &str, args: &Value) -> Result<Value> {
        let (id, ssid, previous_revision) = self.resolve_target(args, "stem")?;
        let client = OscClient::loopback(self.port).await?;
        let ssid = i32::try_from(ssid).map_err(|_| Error::invalid("Ardour SSID exceeds i32"))?;
        match command {
            "driver.ardour-audio.stem.rename" => {
                let name = text_arg(args, "name", 4096)?;
                client
                    .send(
                        "/strip/name",
                        &[OscArg::Int(ssid), OscArg::String(name.into())],
                    )
                    .await?;
            }
            "driver.ardour-audio.stem.mute" => {
                let value = bool_arg(args, "value")?;
                client
                    .send(
                        "/strip/mute",
                        &[OscArg::Int(ssid), OscArg::Int(i32::from(value))],
                    )
                    .await?;
            }
            "driver.ardour-audio.stem.solo" => {
                let value = bool_arg(args, "value")?;
                client
                    .send(
                        "/strip/solo",
                        &[OscArg::Int(ssid), OscArg::Int(i32::from(value))],
                    )
                    .await?;
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "Invalid verified Ardour mutation",
                ));
            }
        }

        sleep(Duration::from_millis(50)).await;
        let list = client
            .query_strip_list()
            .await
            .map_err(|error| error.uncertain())?;
        let observed = list
            .strips
            .iter()
            .find(|strip| strip.ssid == ssid as u32 && strip.kind == "AT")
            .ok_or_else(|| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Ardour mutation target disappeared before verification",
                )
                .uncertain()
            })?;
        let verified = match command {
            "driver.ardour-audio.stem.rename" => observed.name == text_arg(args, "name", 4096)?,
            "driver.ardour-audio.stem.mute" => observed.muted == bool_arg(args, "value")?,
            "driver.ardour-audio.stem.solo" => observed.soloed == bool_arg(args, "value")?,
            _ => false,
        };
        if !verified {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Ardour mutation could not be verified",
            )
            .uncertain());
        }
        let refreshed = self.install(list)?;
        Ok(json!({
            "accepted": true,
            "verified": true,
            "target_id": id,
            "previous_revision": previous_revision,
            "revision": refreshed.get("revision").cloned().unwrap_or(Value::Null)
        }))
    }

    async fn mutate_unverified(&mut self, command: &str, args: &Value) -> Result<Value> {
        if !bool_arg(args, "allow_unverified")? {
            return Err(Error::new(
                ErrorCode::ConsentRequired,
                "Ardour numeric control requires explicit allow_unverified acknowledgement",
            ));
        }
        let kind = if command.contains(".bus.") {
            "bus"
        } else {
            "stem"
        };
        let (id, ssid, previous_revision) = self.resolve_target(args, kind)?;
        let client = OscClient::loopback(self.port).await?;
        let ssid = i32::try_from(ssid).map_err(|_| Error::invalid("Ardour SSID exceeds i32"))?;
        match command {
            "driver.ardour-audio.stem.gain.set" | "driver.ardour-audio.bus.gain.set" => {
                let milli = i32_arg(args, "gain_millidb", -193_000, 6_000)?;
                MilliDb::new(milli).map_err(map_domain_error)?;
                client
                    .send(
                        "/strip/gain",
                        &[OscArg::Int(ssid), OscArg::Float(milli as f32 / 1000.0)],
                    )
                    .await?;
            }
            "driver.ardour-audio.stem.pan.set" | "driver.ardour-audio.bus.pan.set" => {
                let pan = i32_arg(args, "pan_milli", -1000, 1000)?;
                let position = (pan as f32 + 1000.0) / 2000.0;
                client
                    .send(
                        "/strip/pan_stereo_position",
                        &[OscArg::Int(ssid), OscArg::Float(position)],
                    )
                    .await?;
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::Internal,
                    "Invalid Ardour numeric mutation",
                ));
            }
        }
        self.refs.invalidate(PROJECT_ID);
        self.cache = None;
        Ok(json!({
            "accepted": true,
            "verified": false,
            "reobserve_required": true,
            "target_id": id,
            "previous_revision": previous_revision
        }))
    }

    async fn deep_inspect(&self, args: &Value) -> Result<Value> {
        let state = text_arg(args, "state", 255)?;
        let runtime = self.deep_runtime.as_ref().ok_or_else(|| {
            Error::new(
                ErrorCode::Unavailable,
                "Pinned Ardour Lua runtime is unavailable",
            )
        })?;
        let native = runtime.inspect(state).await?;
        let report = ArdourProjection
            .project(&native)
            .map_err(map_domain_error)?;
        let contract = ArdourProjection
            .contract(&native)
            .map_err(map_domain_error)?;
        let project_json = serde_json::to_string(&report.project)?;
        let contract_json = serde_json::to_string(&contract)?;
        if project_json.len() > 900_000 || contract_json.len() > 131_072 {
            return Err(Error::new(
                ErrorCode::ResourceExhausted,
                "Ardour deep semantic projection exceeds response budget",
            ));
        }
        let native_bytes = serde_json::to_vec(&native)?;
        let revision = format!("{:x}", Sha256::digest(&native_bytes));
        Ok(json!({
            "revision": revision,
            "project_json": project_json,
            "contract_json": contract_json,
            "fidelity": fidelity_name(report.fidelity),
            "losses": losses_json(&report.losses),
            "ardour_version": runtime.version()
        }))
    }


    fn deep_runtime(&self) -> Result<&DeepRuntime> {
        self.deep_runtime.as_ref().ok_or_else(|| {
            Error::new(
                ErrorCode::Unavailable,
                "Pinned Ardour deep runtime is unavailable",
            )
        })
    }

    async fn deep_create(&self, args: &Value) -> Result<Value> {
        let state = text_arg(args, "state", 128)?;
        let sample_rate = u64_arg(args, "sample_rate", 8_000, 192_000)? as u32;
        let master_channels = u64_arg(args, "master_channels", 0, 64)? as u16;
        let native = self
            .deep_runtime()?
            .create(state, sample_rate, master_channels)
            .await?;
        let revision = native_revision(&native)?;
        let report = ArdourProjection.project(&native).map_err(map_domain_error)?;
        Ok(json!({
            "accepted": true,
            "verified": true,
            "state": state,
            "revision": revision,
            "project_json": serde_json::to_string(&report.project)?,
            "fidelity": fidelity_name(report.fidelity),
            "losses": losses_json(&report.losses)
        }))
    }

    async fn deep_save_as(&self, args: &Value) -> Result<Value> {
        let source_state = text_arg(args, "source_state", 128)?;
        let candidate_state = text_arg(args, "candidate_state", 128)?;
        let expected = text_arg(args, "expected_revision", 64)?;
        let runtime = self.deep_runtime()?;
        let before = runtime.inspect(source_state).await?;
        let previous_revision = native_revision(&before)?;
        if previous_revision != expected {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Ardour deep snapshot revision is stale",
            ));
        }
        let candidate = runtime.save_as(source_state, candidate_state).await?;
        let source_after = runtime.inspect(source_state).await?;
        if native_revision(&source_after)? != previous_revision {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Ardour save-as changed the protected source snapshot",
            ));
        }
        Ok(json!({
            "accepted": true,
            "verified": true,
            "source_state": source_state,
            "candidate_state": candidate_state,
            "source_revision": previous_revision,
            "candidate_revision": native_revision(&candidate)?,
            "source_preserved": true
        }))
    }

    async fn deep_export(&self, args: &Value) -> Result<Value> {
        let state = text_arg(args, "state", 128)?;
        let expected = text_arg(args, "expected_revision", 64)?;
        let file_name = text_arg(args, "file_name", 200)?;
        let sample_rate = u64_arg(args, "sample_rate", 8_000, 192_000)? as u32;
        let bit_depth = u64_arg(args, "bit_depth", 16, 32)? as u16;
        if !matches!(bit_depth, 16 | 24 | 32) {
            return Err(Error::invalid("Ardour export bit depth must be 16, 24 or 32"));
        }
        let runtime = self.deep_runtime()?;
        let before = runtime.inspect(state).await?;
        let previous_revision = native_revision(&before)?;
        if previous_revision != expected {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Ardour export source revision is stale",
            ));
        }
        let receipt = runtime
            .export_wav(state, file_name, sample_rate, bit_depth)
            .await?;
        let after = runtime.inspect(state).await?;
        if native_revision(&after)? != previous_revision {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Ardour export unexpectedly changed source semantic state",
            ));
        }
        Ok(json!({
            "accepted": true,
            "verified": true,
            "source_preserved": true,
            "source_revision": previous_revision,
            "artifact": receipt
        }))
    }

    async fn deep_mutate(&self, command: &str, args: &Value) -> Result<Value> {
        let state = text_arg(args, "state", 128)?;
        let expected = text_arg(args, "expected_revision", 64)?;
        let runtime = self.deep_runtime()?;
        let before = runtime.inspect(state).await?;
        let previous_revision = native_revision(&before)?;
        if previous_revision != expected {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Ardour deep snapshot revision is stale",
            ));
        }
        let mutation = deep_mutation(command, args, &before)?;
        if let NativeMutation::RouteRemove { route_id } = &mutation {
            let route = before
                .routes
                .iter()
                .find(|route| &route.id == route_id)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "Ardour route does not exist"))?;
            if route.kind == RouteKind::Master {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Ardour master route removal is outside this capability",
                ));
            }
        }
        let after = runtime.mutate(state, &mutation).await?;
        if !native_effect_verified(&before, &after, &mutation) {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "Ardour deep mutation did not satisfy its native postcondition",
            )
            .uncertain());
        }
        Ok(json!({
            "accepted": true,
            "verified": true,
            "previous_revision": previous_revision,
            "revision": native_revision(&after)?
        }))
    }

    async fn execute_inner(&mut self, command: &str, args: &Value) -> Result<Value> {
        match command {
            "driver.ardour-audio.doctor" => {
                let reachable = self.query().await.is_ok();
                Ok(json!({
                    "host": "127.0.0.1",
                    "port": self.port,
                    "reachable": reachable,
                    "transport": "official_osc",
                    "deep_runtime_available": self.deep_runtime.is_some(),
                    "deep_runtime_reason": self.deep_runtime_reason,
                    "arbitrary_lua": false,
                    "gui_coordinates": false
                }))
            }
            "driver.ardour-audio.session.inspect" => self.inspect().await,
            "driver.ardour-audio.session.deep.inspect" => self.deep_inspect(args).await,
            "driver.ardour-audio.session.deep.create" => self.deep_create(args).await,
            "driver.ardour-audio.session.deep.save-as" => self.deep_save_as(args).await,
            "driver.ardour-audio.session.deep.export" => self.deep_export(args).await,
            "driver.ardour-audio.session.deep.stem.create"
            | "driver.ardour-audio.session.deep.route.remove"
            | "driver.ardour-audio.session.deep.route.rename"
            | "driver.ardour-audio.session.deep.route.mute"
            | "driver.ardour-audio.session.deep.route.solo"
            | "driver.ardour-audio.session.deep.route.gain.set"
            | "driver.ardour-audio.session.deep.route.pan.set"
            | "driver.ardour-audio.session.deep.clip.move"
            | "driver.ardour-audio.session.deep.clip.trim"
            | "driver.ardour-audio.session.deep.clip.remove"
            | "driver.ardour-audio.session.deep.range.set" => self.deep_mutate(command, args).await,
            "driver.ardour-audio.session.project" => {
                self.inspect().await?;
                let cache = self.cache.as_ref().expect("installed snapshot");
                Ok(json!({
                    "revision": cache.revision,
                    "project_json": serde_json::to_string(&cache.report.project)?
                }))
            }
            "driver.ardour-audio.session.contract" => {
                let list = self.query().await?;
                let snapshot = ArdourSnapshot { strip_list: list };
                let contract =
                    ArdourSemanticProjection::contract(&snapshot).map_err(map_domain_error)?;
                Ok(json!({"contract_json": serde_json::to_string(&contract)?}))
            }
            "driver.ardour-audio.stem.rename"
            | "driver.ardour-audio.stem.mute"
            | "driver.ardour-audio.stem.solo" => self.mutate_verified(command, args).await,
            "driver.ardour-audio.stem.gain.set"
            | "driver.ardour-audio.stem.pan.set"
            | "driver.ardour-audio.bus.gain.set"
            | "driver.ardour-audio.bus.pan.set" => self.mutate_unverified(command, args).await,
            "driver.ardour-audio.transport.play" => {
                OscClient::loopback(self.port)
                    .await?
                    .send("/transport_play", &[])
                    .await?;
                Ok(json!({"accepted":true,"verified":false}))
            }
            "driver.ardour-audio.transport.stop" => {
                OscClient::loopback(self.port)
                    .await?
                    .send("/transport_stop", &[])
                    .await?;
                Ok(json!({"accepted":true,"verified":false}))
            }
            "driver.ardour-audio.transport.locate" => {
                let frame = u64_arg(args, "frame", 0, i64::MAX as u64)?;
                let roll = bool_arg(args, "roll")?;
                OscClient::loopback(self.port)
                    .await?
                    .send(
                        "/locate",
                        &[OscArg::Long(frame as i64), OscArg::Int(i32::from(roll))],
                    )
                    .await?;
                Ok(json!({"accepted":true,"verified":false}))
            }
            _ => Err(Error::new(
                ErrorCode::NotFound,
                "Ardour capability is absent",
            )),
        }
    }
}

#[async_trait]
impl Driver for ArdourAudioDriver {
    fn id(&self) -> &str {
        DRIVER_ID
    }
    fn version(&self) -> &str {
        VERSION
    }
    fn interfaces(&self) -> DriverInterfaces {
        DriverInterfaces {
            health: true,
            ..Default::default()
        }
    }
    async fn capabilities(&mut self) -> Result<Vec<Capability>> {
        Ok(self.capabilities.clone())
    }
    async fn execute(
        &mut self,
        command: &str,
        descriptor_sha256: &str,
        args: Value,
    ) -> Result<Value> {
        self.verify_digest(command, descriptor_sha256)?;
        self.execute_inner(command, &args).await
    }
    async fn health(&mut self) -> Result<Value> {
        let reachable = self.query().await.is_ok();
        Ok(json!({
            "healthy": reachable || self.deep_runtime.is_some(),
            "osc_loopback": true,
            "osc_reachable": reachable,
            "deep_runtime_available": self.deep_runtime.is_some(),
            "port": self.port
        }))
    }
}

pub fn capability_catalog() -> Vec<Capability> {
    let mut values = vec![
        descriptor(
            "driver.ardour-audio.doctor",
            "Inspect the fixed loopback Ardour OSC surface",
            empty_schema(),
            json!({
                "type":"object",
                "properties":{
                    "host":{"const":"127.0.0.1"},
                    "port":{"type":"integer","minimum":1,"maximum":65535},
                    "reachable":{"type":"boolean"},
                    "transport":{"const":"official_osc"},
                    "deep_runtime_available":{"type":"boolean"},
                    "deep_runtime_reason":{"type":"string","maxLength":2048},
                    "arbitrary_lua":{"const":false},
                    "gui_coordinates":{"const":false}
                },
                "required":["host","port","reachable","transport","deep_runtime_available","deep_runtime_reason","arbitrary_lua","gui_coordinates"],
                "additionalProperties":false
            }),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            6_000,
            &["audio-session"],
        ),
        descriptor(
            "driver.ardour-audio.session.inspect",
            "Observe Ardour audio strips into snapshot-bound Semwright audio refs",
            empty_schema(),
            inspect_schema(),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            8_000,
            &["audio-session", "audio-stem", "audio-bus"],
        ),
        descriptor(
            "driver.ardour-audio.session.deep.inspect",
            "Inspect a mounted Ardour session through the owner-pinned fixed Lua adapter and project regions, samples, routes and buses into semwright-audio-domain",
            json!({
                "type":"object",
                "properties":{
                    "state":{"type":"string","minLength":1,"maxLength":255}
                },
                "required":["state"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "revision":{"type":"string","maxLength":64},
                    "project_json":{"type":"string","maxLength":900000},
                    "contract_json":{"type":"string","maxLength":131072},
                    "fidelity":{"type":"string","enum":["exact","semantically_equivalent","lossy_read_only"]},
                    "losses":{"type":"array","maxItems":4096},
                    "ardour_version":{"type":"string","maxLength":128}
                },
                "required":["revision","project_json","contract_json","fidelity","losses","ardour_version"],
                "additionalProperties":false
            }),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            30_000,
            &[
                "audio-project",
                "audio-session",
                "audio-clip",
                "audio-sample",
                "audio-stem",
                "audio-bus",
            ],
        ),
        descriptor(
            "driver.ardour-audio.session.project",
            "Project the observable Ardour session into strict semwright-audio-domain JSON",
            empty_schema(),
            json!({
                "type":"object",
                "properties":{
                    "revision":{"type":"string","maxLength":64},
                    "project_json":{"type":"string","maxLength":900000}
                },
                "required":["revision","project_json"],
                "additionalProperties":false
            }),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            8_000,
            &["audio-project"],
        ),
        descriptor(
            "driver.ardour-audio.session.contract",
            "Describe project-scoped Ardour support and fidelity without overstating unobservable session semantics",
            empty_schema(),
            json!({
                "type":"object",
                "properties":{"contract_json":{"type":"string","maxLength":131072}},
                "required":["contract_json"],
                "additionalProperties":false
            }),
            Risk::ReadOnly,
            Idempotency::ReadOnly,
            8_000,
            &["audio-backend"],
        ),
    ];

    for (name, field, field_schema, description) in [
        (
            "driver.ardour-audio.stem.rename",
            "name",
            json!({"type":"string","minLength":1,"maxLength":4096}),
            "Rename an observed Ardour audio track and verify through a fresh /strip/list snapshot",
        ),
        (
            "driver.ardour-audio.stem.mute",
            "value",
            json!({"type":"boolean"}),
            "Set Ardour audio-track mute and verify it through a fresh /strip/list snapshot",
        ),
        (
            "driver.ardour-audio.stem.solo",
            "value",
            json!({"type":"boolean"}),
            "Set Ardour audio-track solo and verify it through a fresh /strip/list snapshot",
        ),
    ] {
        values.push(ref_mutation_descriptor(
            name,
            "stem",
            field,
            field_schema,
            description,
            false,
        ));
    }

    for (name, kind, field, min, max, description) in [
        (
            "driver.ardour-audio.stem.gain.set",
            "stem",
            "gain_millidb",
            -193_000,
            6_000,
            "Set Ardour track gain in dB; requires explicit acknowledgement because /strip/list cannot verify numeric gain",
        ),
        (
            "driver.ardour-audio.stem.pan.set",
            "stem",
            "pan_milli",
            -1_000,
            1_000,
            "Set Ardour track stereo pan; requires explicit acknowledgement because /strip/list cannot verify pan",
        ),
        (
            "driver.ardour-audio.bus.gain.set",
            "bus",
            "gain_millidb",
            -193_000,
            6_000,
            "Set Ardour bus gain in dB with explicit unverified acknowledgement",
        ),
        (
            "driver.ardour-audio.bus.pan.set",
            "bus",
            "pan_milli",
            -1_000,
            1_000,
            "Set Ardour bus stereo pan with explicit unverified acknowledgement",
        ),
    ] {
        values.push(ref_mutation_descriptor(
            name,
            kind,
            field,
            json!({"type":"integer","minimum":min,"maximum":max}),
            description,
            true,
        ));
    }

    values.extend([
        descriptor(
            "driver.ardour-audio.transport.play",
            "Start Ardour transport through official OSC",
            empty_schema(),
            accepted_schema(),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
            3_000,
            &["audio-transport"],
        ),
        descriptor(
            "driver.ardour-audio.transport.stop",
            "Stop Ardour transport through official OSC",
            empty_schema(),
            accepted_schema(),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
            3_000,
            &["audio-transport"],
        ),
        descriptor(
            "driver.ardour-audio.transport.locate",
            "Locate Ardour transport to an explicit sample frame through official OSC",
            json!({
                "type":"object",
                "properties":{
                    "frame":{"type":"integer","minimum":0,"maximum":9223372036854775807u64},
                    "roll":{"type":"boolean"}
                },
                "required":["frame","roll"],
                "additionalProperties":false
            }),
            accepted_schema(),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
            3_000,
            &["audio-transport"],
        ),
    ]);
    values.extend(deep_capabilities());
    values
}

fn deep_capabilities() -> Vec<Capability> {
    let state = json!({
        "type":"string",
        "minLength":1,
        "maxLength":128,
        "pattern":"^[A-Za-z0-9][A-Za-z0-9_-]*$"
    });
    let revision = json!({"type":"string","pattern":"^[0-9a-f]{64}$"});
    let verified = json!({
        "type":"object",
        "properties":{
            "accepted":{"const":true},
            "verified":{"const":true},
            "previous_revision":{"type":"string","pattern":"^[0-9a-f]{64}$"},
            "revision":{"type":"string","pattern":"^[0-9a-f]{64}$"}
        },
        "required":["accepted","verified","previous_revision","revision"],
        "additionalProperties":false
    });
    let mut values = vec![
        deep_descriptor(
            "driver.ardour-audio.session.deep.create",
            "Create and reopen a managed Ardour 8.4 candidate using the official Dummy-backend session utility",
            json!({
                "type":"object",
                "properties":{
                    "state":state.clone(),
                    "sample_rate":{"type":"integer","minimum":8000,"maximum":192000},
                    "master_channels":{"type":"integer","minimum":0,"maximum":64}
                },
                "required":["state","sample_rate","master_channels"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "accepted":{"const":true},
                    "verified":{"const":true},
                    "state":state.clone(),
                    "revision":{"type":"string","pattern":"^[0-9a-f]{64}$"},
                    "project_json":{"type":"string","maxLength":900000},
                    "fidelity":{"type":"string"},
                    "losses":{"type":"array","maxItems":4096}
                },
                "required":["accepted","verified","state","revision","project_json","fidelity","losses"],
                "additionalProperties":false
            }),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
            false,
            &["audio-project"],
            &[],
        ),
        deep_descriptor(
            "driver.ardour-audio.session.deep.save-as",
            "Save an Ardour session as a distinct snapshot, reopen it, and reobserve the protected source",
            json!({
                "type":"object",
                "properties":{
                    "source_state":state.clone(),
                    "candidate_state":state.clone(),
                    "expected_revision":revision.clone()
                },
                "required":["source_state","candidate_state","expected_revision"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "accepted":{"const":true},
                    "verified":{"const":true},
                    "source_state":state.clone(),
                    "candidate_state":state.clone(),
                    "source_revision":revision.clone(),
                    "candidate_revision":revision.clone(),
                    "source_preserved":{"const":true}
                },
                "required":["accepted","verified","source_state","candidate_state","source_revision","candidate_revision","source_preserved"],
                "additionalProperties":false
            }),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
            false,
            &["audio-project"],
            &[],
        ),
        deep_descriptor(
            "driver.ardour-audio.session.deep.export",
            "Export the Ardour session range through the native master bus and verify the resulting WAV container",
            json!({
                "type":"object",
                "properties":{
                    "state":state.clone(),
                    "expected_revision":revision.clone(),
                    "file_name":{"type":"string","minLength":5,"maxLength":200,"pattern":"^[A-Za-z0-9][A-Za-z0-9._-]*\.wav$"},
                    "sample_rate":{"type":"integer","minimum":8000,"maximum":192000},
                    "bit_depth":{"type":"integer","enum":[16,24,32]}
                },
                "required":["state","expected_revision","file_name","sample_rate","bit_depth"],
                "additionalProperties":false
            }),
            json!({
                "type":"object",
                "properties":{
                    "accepted":{"const":true},
                    "verified":{"const":true},
                    "source_preserved":{"const":true},
                    "source_revision":revision.clone(),
                    "artifact":{"type":"object"}
                },
                "required":["accepted","verified","source_preserved","source_revision","artifact"],
                "additionalProperties":false
            }),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
            false,
            &["audio-project","audio-artifact"],
            &["artifact-out:audio/wav"],
        ),
    ];

    let mutation_specs: [(&str, Value, Risk, Idempotency, bool, &[&str]); 11] = [
        (
            "driver.ardour-audio.session.deep.stem.create",
            json!({"channels":{"type":"integer","minimum":1,"maximum":64},"name":{"type":"string","minLength":1,"maxLength":4096}}),
            Risk::MutatingReversible,
            Idempotency::NonIdempotent,
            false,
            &["audio-stem"],
        ),
        (
            "driver.ardour-audio.session.deep.route.remove",
            json!({"route_id":{"type":"string","minLength":1,"maxLength":256}}),
            Risk::Destructive,
            Idempotency::Destructive,
            true,
            &["audio-stem","audio-bus"],
        ),
        (
            "driver.ardour-audio.session.deep.route.rename",
            json!({"route_id":{"type":"string","minLength":1,"maxLength":256},"name":{"type":"string","minLength":1,"maxLength":4096}}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
            false,
            &["audio-stem","audio-bus"],
        ),
        (
            "driver.ardour-audio.session.deep.route.mute",
            json!({"route_id":{"type":"string","minLength":1,"maxLength":256},"value":{"type":"boolean"}}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
            false,
            &["audio-stem","audio-bus"],
        ),
        (
            "driver.ardour-audio.session.deep.route.solo",
            json!({"route_id":{"type":"string","minLength":1,"maxLength":256},"value":{"type":"boolean"}}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
            false,
            &["audio-stem","audio-bus"],
        ),
        (
            "driver.ardour-audio.session.deep.route.gain.set",
            json!({"route_id":{"type":"string","minLength":1,"maxLength":256},"gain_millidb":{"type":"integer","minimum":-120000,"maximum":24000}}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
            false,
            &["audio-stem","audio-bus"],
        ),
        (
            "driver.ardour-audio.session.deep.route.pan.set",
            json!({"route_id":{"type":"string","minLength":1,"maxLength":256},"pan_milli":{"type":"integer","minimum":-1000,"maximum":1000}}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
            false,
            &["audio-stem","audio-bus"],
        ),
        (
            "driver.ardour-audio.session.deep.clip.move",
            json!({"region_id":{"type":"string","minLength":1,"maxLength":256},"start":{"type":"integer","minimum":0,"maximum":9223372036854775807u64}}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
            false,
            &["audio-clip"],
        ),
        (
            "driver.ardour-audio.session.deep.clip.trim",
            json!({"region_id":{"type":"string","minLength":1,"maxLength":256},"source_start":{"type":"integer","minimum":0,"maximum":9223372036854775807u64},"length":{"type":"integer","minimum":1,"maximum":9223372036854775807u64}}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
            false,
            &["audio-clip"],
        ),
        (
            "driver.ardour-audio.session.deep.clip.remove",
            json!({"region_id":{"type":"string","minLength":1,"maxLength":256}}),
            Risk::Destructive,
            Idempotency::Destructive,
            true,
            &["audio-clip"],
        ),
        (
            "driver.ardour-audio.session.deep.range.set",
            json!({"start":{"type":"integer","minimum":0,"maximum":9223372036854775806u64},"end":{"type":"integer","minimum":1,"maximum":9223372036854775807u64}}),
            Risk::MutatingReversible,
            Idempotency::Idempotent,
            false,
            &["audio-range"],
        ),
    ];
    for (name, fields, risk, idempotency, consent, objects) in mutation_specs {
        let mut properties = fields.as_object().cloned().expect("object fields");
        properties.insert("state".into(), state.clone());
        properties.insert("expected_revision".into(), revision.clone());
        let mut required = vec!["state".to_string(), "expected_revision".to_string()];
        required.extend(
            properties
                .keys()
                .filter(|key| key.as_str() != "state" && key.as_str() != "expected_revision")
                .cloned(),
        );
        required.sort();
        values.push(deep_descriptor(
            name,
            "Apply one typed fixed Ardour Lua operation to semantic project IDs and verify it through native readback",
            json!({
                "type":"object",
                "properties":properties,
                "required":required,
                "additionalProperties":false
            }),
            verified.clone(),
            risk,
            idempotency,
            consent,
            objects,
            &[],
        ));
    }
    values
}

fn deep_descriptor(
    name: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
    risk: Risk,
    idempotency: Idempotency,
    interactive_consent: bool,
    object_types: &[&str],
    extra_tags: &[&str],
) -> Capability {
    let mut tags = vec!["audio".into(), "ardour".into(), "native-session".into()];
    tags.extend(extra_tags.iter().map(|value| (*value).to_owned()));
    Capability {
        descriptor: CommandDescriptor {
            name: name.into(),
            version: "1".into(),
            description: description.into(),
            input_schema,
            output_schema,
            requires: vec![DRIVER_SCOPE.into()],
            risk,
            idempotency,
            timeout_ms: 30_000,
            dry_run: false,
            interactive_consent,
            backends: vec![DRIVER_SCOPE.into()],
        },
        aliases: vec![],
        tags,
        object_types: object_types.iter().map(|value| (*value).into()).collect(),
    }
}

fn descriptor(
    name: &str,
    description: &str,
    input_schema: Value,
    output_schema: Value,
    risk: Risk,
    idempotency: Idempotency,
    timeout_ms: u64,
    object_types: &[&str],
) -> Capability {
    Capability {
        descriptor: CommandDescriptor {
            name: name.into(),
            version: "1".into(),
            description: description.into(),
            input_schema,
            output_schema,
            requires: vec![DRIVER_SCOPE.into()],
            risk,
            idempotency,
            timeout_ms,
            dry_run: risk == Risk::ReadOnly,
            interactive_consent: false,
            backends: vec![DRIVER_SCOPE.into()],
        },
        aliases: vec![],
        tags: vec!["audio".into(), "ardour".into(), "osc".into()],
        object_types: object_types.iter().map(|value| (*value).into()).collect(),
    }
}

fn ref_mutation_descriptor(
    name: &str,
    kind: &str,
    field: &str,
    field_schema: Value,
    description: &str,
    unverified: bool,
) -> Capability {
    let mut properties = serde_json::Map::from_iter([
        (kind.into(), json!({"type":"string","maxLength":80})),
        (
            "expected_revision".into(),
            json!({"type":"string","maxLength":64}),
        ),
        (field.into(), field_schema),
    ]);
    let mut required = vec![kind, "expected_revision", field];
    if unverified {
        properties.insert("allow_unverified".into(), json!({"const":true}));
        required.push("allow_unverified");
    }
    descriptor(
        name,
        description,
        json!({
            "type":"object",
            "properties":properties,
            "required":required,
            "additionalProperties":false
        }),
        if unverified {
            json!({
                "type":"object",
                "properties":{
                    "accepted":{"const":true},
                    "verified":{"const":false},
                    "reobserve_required":{"const":true},
                    "target_id":{"type":"string","maxLength":256},
                    "previous_revision":{"type":"string","maxLength":64}
                },
                "required":["accepted","verified","reobserve_required","target_id","previous_revision"],
                "additionalProperties":false
            })
        } else {
            json!({
                "type":"object",
                "properties":{
                    "accepted":{"const":true},
                    "verified":{"const":true},
                    "target_id":{"type":"string","maxLength":256},
                    "previous_revision":{"type":"string","maxLength":64},
                    "revision":{"type":"string","maxLength":64}
                },
                "required":["accepted","verified","target_id","previous_revision","revision"],
                "additionalProperties":false
            })
        },
        Risk::MutatingReversible,
        Idempotency::Idempotent,
        8_000,
        &[if kind == "bus" {
            "audio-bus"
        } else {
            "audio-stem"
        }],
    )
}

fn empty_schema() -> Value {
    json!({"type":"object","properties":{},"additionalProperties":false})
}
fn accepted_schema() -> Value {
    json!({
        "type":"object",
        "properties":{"accepted":{"const":true},"verified":{"const":false}},
        "required":["accepted","verified"],
        "additionalProperties":false
    })
}
fn inspect_schema() -> Value {
    json!({
        "type":"object",
        "properties":{
            "project":{"type":"string","maxLength":80},
            "revision":{"type":"string","maxLength":64},
            "sample_rate":{"type":"integer","minimum":8000,"maximum":384000},
            "last_frame":{"type":"integer","minimum":0},
            "monitor_present":{"type":"boolean"},
            "stems":{"type":"array","maxItems":1024},
            "buses":{"type":"array","maxItems":1024},
            "fidelity":{"const":"lossy_read_only"},
            "losses":{"type":"array","maxItems":4096}
        },
        "required":["project","revision","sample_rate","last_frame","monitor_present","stems","buses","fidelity","losses"],
        "additionalProperties":false
    })
}


fn native_revision(snapshot: &NativeArdourSnapshot) -> Result<String> {
    snapshot.validate().map_err(map_domain_error)?;
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(snapshot)?)
    ))
}

fn deep_mutation(
    command: &str,
    args: &Value,
    snapshot: &NativeArdourSnapshot,
) -> Result<NativeMutation> {
    let route_id = || {
        let semantic = text_arg(args, "route_id", 256)?;
        resolve_native_route_id(snapshot, semantic)
    };
    let region_id = || {
        let semantic = text_arg(args, "region_id", 256)?;
        resolve_native_region_id(snapshot, semantic)
    };
    match command {
        "driver.ardour-audio.session.deep.stem.create" => Ok(NativeMutation::StemCreate {
            channels: u64_arg(args, "channels", 1, 64)? as u16,
            name: text_arg(args, "name", 4096)?.to_owned(),
        }),
        "driver.ardour-audio.session.deep.route.remove" => Ok(NativeMutation::RouteRemove {
            route_id: route_id()?,
        }),
        "driver.ardour-audio.session.deep.route.rename" => Ok(NativeMutation::RouteRename {
            route_id: route_id()?,
            name: text_arg(args, "name", 4096)?.to_owned(),
        }),
        "driver.ardour-audio.session.deep.route.mute" => Ok(NativeMutation::RouteMute {
            route_id: route_id()?,
            value: bool_arg(args, "value")?,
        }),
        "driver.ardour-audio.session.deep.route.solo" => Ok(NativeMutation::RouteSolo {
            route_id: route_id()?,
            value: bool_arg(args, "value")?,
        }),
        "driver.ardour-audio.session.deep.route.gain.set" => Ok(NativeMutation::RouteGain {
            route_id: route_id()?,
            gain_millidb: i32_arg(args, "gain_millidb", -120_000, 24_000)?,
        }),
        "driver.ardour-audio.session.deep.route.pan.set" => Ok(NativeMutation::RoutePan {
            route_id: route_id()?,
            pan_milli: i32_arg(args, "pan_milli", -1_000, 1_000)? as i16,
        }),
        "driver.ardour-audio.session.deep.clip.move" => Ok(NativeMutation::ClipMove {
            region_id: region_id()?,
            start: u64_arg(args, "start", 0, i64::MAX as u64)?,
        }),
        "driver.ardour-audio.session.deep.clip.trim" => Ok(NativeMutation::ClipTrim {
            region_id: region_id()?,
            source_start: u64_arg(args, "source_start", 0, i64::MAX as u64)?,
            length: u64_arg(args, "length", 1, i64::MAX as u64)?,
        }),
        "driver.ardour-audio.session.deep.clip.remove" => Ok(NativeMutation::ClipRemove {
            region_id: region_id()?,
        }),
        "driver.ardour-audio.session.deep.range.set" => Ok(NativeMutation::SessionRange {
            start: u64_arg(args, "start", 0, i64::MAX as u64 - 1)?,
            end: u64_arg(args, "end", 1, i64::MAX as u64)?,
        }),
        _ => Err(Error::new(
            ErrorCode::Internal,
            "Unknown typed Ardour deep mutation",
        )),
    }
}


fn resolve_native_route_id(snapshot: &NativeArdourSnapshot, semantic: &str) -> Result<String> {
    snapshot
        .routes
        .iter()
        .find(|route| match route.kind {
            RouteKind::Master => semantic == "master",
            RouteKind::Track => semantic_id("stem", &route.id) == semantic,
            RouteKind::Bus => semantic_id("bus", &route.id) == semantic,
            RouteKind::Other => false,
        })
        .map(|route| route.id.clone())
        .ok_or_else(|| Error::new(ErrorCode::NotFound, "Semantic Ardour route does not exist"))
}

fn resolve_native_region_id(snapshot: &NativeArdourSnapshot, semantic: &str) -> Result<String> {
    snapshot
        .routes
        .iter()
        .flat_map(|route| route.regions.iter())
        .find(|region| semantic_id("clip", &region.id) == semantic)
        .map(|region| region.id.clone())
        .ok_or_else(|| Error::new(ErrorCode::NotFound, "Semantic Ardour clip does not exist"))
}

fn native_effect_verified(
    before: &NativeArdourSnapshot,
    after: &NativeArdourSnapshot,
    mutation: &NativeMutation,
) -> bool {
    let route_after = |id: &str| after.routes.iter().find(|route| route.id == id);
    let region_after = |id: &str| {
        after
            .routes
            .iter()
            .flat_map(|route| route.regions.iter())
            .find(|region| region.id == id)
    };
    match mutation {
        NativeMutation::StemCreate { channels, name } => {
            let old_ids: std::collections::BTreeSet<_> =
                before.routes.iter().map(|route| route.id.as_str()).collect();
            let created: Vec<_> = after
                .routes
                .iter()
                .filter(|route| !old_ids.contains(route.id.as_str()))
                .collect();
            created.len() == 1
                && created[0].kind == RouteKind::Track
                && created[0].channels == *channels
                && created[0].name == *name
        }
        NativeMutation::RouteRemove { route_id } => {
            before.routes.iter().any(|route| route.id == *route_id)
                && route_after(route_id).is_none()
        }
        NativeMutation::RouteRename { route_id, name } => {
            route_after(route_id).is_some_and(|route| route.name == *name)
        }
        NativeMutation::RouteMute { route_id, value } => {
            route_after(route_id).is_some_and(|route| route.muted == *value)
        }
        NativeMutation::RouteSolo { route_id, value } => {
            route_after(route_id).is_some_and(|route| route.soloed == *value)
        }
        NativeMutation::RouteGain {
            route_id,
            gain_millidb,
        } => route_after(route_id)
            .is_some_and(|route| route.gain_millidb.abs_diff(*gain_millidb) <= 1),
        NativeMutation::RoutePan {
            route_id,
            pan_milli,
        } => route_after(route_id)
            .is_some_and(|route| route.pan_milli.abs_diff(*pan_milli) <= 1),
        NativeMutation::ClipMove { region_id, start } => {
            region_after(region_id).is_some_and(|region| region.position == *start)
        }
        NativeMutation::ClipTrim {
            region_id,
            source_start,
            length,
        } => region_after(region_id).is_some_and(|region| {
            region.source_start == *source_start && region.length == *length
        }),
        NativeMutation::ClipRemove { region_id } => {
            before
                .routes
                .iter()
                .flat_map(|route| route.regions.iter())
                .any(|region| region.id == *region_id)
                && region_after(region_id).is_none()
        }
        NativeMutation::SessionRange { start, end } => {
            after.session_start == *start && after.session_end == *end
        }
        NativeMutation::SaveAs { .. } => false,
    }
}

fn fidelity_name(value: ProjectionFidelity) -> &'static str {
    match value {
        ProjectionFidelity::Exact => "exact",
        ProjectionFidelity::SemanticallyEquivalent => "semantically_equivalent",
        ProjectionFidelity::LossyReadOnly => "lossy_read_only",
    }
}

fn losses_json(losses: &[ProjectionLoss]) -> Value {
    serde_json::to_value(losses).unwrap_or_else(|_| json!([]))
}

fn text_arg<'a>(args: &'a Value, key: &str, max: usize) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
        })
        .ok_or_else(|| Error::invalid(format!("Invalid Ardour {key}")))
}
fn bool_arg(args: &Value, key: &str) -> Result<bool> {
    args.get(key)
        .and_then(Value::as_bool)
        .ok_or_else(|| Error::invalid(format!("Invalid Ardour {key}")))
}
fn i32_arg(args: &Value, key: &str, min: i32, max: i32) -> Result<i32> {
    args.get(key)
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
        .filter(|value| (min..=max).contains(value))
        .ok_or_else(|| Error::invalid(format!("Invalid Ardour {key}")))
}
fn u64_arg(args: &Value, key: &str, min: u64, max: u64) -> Result<u64> {
    args.get(key)
        .and_then(Value::as_u64)
        .filter(|value| (min..=max).contains(value))
        .ok_or_else(|| Error::invalid(format!("Invalid Ardour {key}")))
}

fn map_domain_error(error: DomainError) -> Error {
    let code = match error.code {
        "InvalidArgument" => ErrorCode::InvalidArgument,
        "ResourceExhausted" => ErrorCode::ResourceExhausted,
        "Unsupported" => ErrorCode::Unsupported,
        "NotFound" => ErrorCode::NotFound,
        "StaleReference" => ErrorCode::StaleReference,
        _ => ErrorCode::BackendFailed,
    };
    Error::new(code, error.message)
}

#[cfg(test)]
mod descriptor_tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalog_is_unique_pinned_and_does_not_accept_code_or_remote_hosts() {
        let capabilities = capability_catalog();
        let mut names = BTreeSet::new();
        assert!(capabilities.len() >= 10);
        for capability in &capabilities {
            assert!(names.insert(capability.descriptor.name.clone()));
            assert!(
                capability
                    .descriptor
                    .name
                    .starts_with("driver.ardour-audio.")
            );
            assert_eq!(
                capability.descriptor.backends,
                vec![DRIVER_SCOPE.to_string()]
            );
            assert!(descriptor_digest(&capability.descriptor).unwrap().len() == 64);

            let input = capability.descriptor.input_schema.to_string();
            for forbidden in [
                "lua_source",
                "script_source",
                "executable",
                "remote_host",
                "host_address",
            ] {
                assert!(
                    !input.contains(forbidden),
                    "{}: {forbidden}",
                    capability.descriptor.name
                );
            }
        }

        let deep = capabilities
            .iter()
            .find(|capability| {
                capability.descriptor.name == "driver.ardour-audio.session.deep.inspect"
            })
            .unwrap();
        assert_eq!(deep.descriptor.risk, Risk::ReadOnly);
        assert_eq!(deep.descriptor.idempotency, Idempotency::ReadOnly);
    }
}
