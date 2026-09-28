use crate::{
    DRIVER_ID, DRIVER_SCOPE, VERSION,
    live_projection::{ArdourSemanticProjection, ArdourSnapshot},
    osc::{DEFAULT_ARDOUR_OSC_PORT, OscArg, OscClient, StripList},
    projection::ArdourProjection,
    runtime::DeepRuntime,
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
        let (deep_runtime, deep_runtime_reason) = match DeepRuntime::load_production()
            .map_err(map_domain_error)?
        {
            Some(runtime) => (
                Some(runtime),
                "owner-pinned Ardour Lua runtime is available".to_string(),
            ),
            None => (
                None,
                if cfg!(unix) {
                    "owner-pinned /workspace/runtime/ardour.json is absent".to_string()
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
            capabilities: capabilities(),
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
        let native = runtime.inspect(state).await.map_err(map_domain_error)?;
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

fn capabilities() -> Vec<Capability> {
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
            45_000,
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
    values
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
        let capabilities = capabilities();
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
