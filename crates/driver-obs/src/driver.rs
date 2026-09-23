use crate::{
    Fault, FaultKind, Result, bounds,
    capability::{Catalog, Plan, map_arguments, map_output},
    client::Client,
    config::Config,
    lifecycle::Phase,
    refs::{Identity, Kind},
    state::Stamp,
};
use async_trait::async_trait;
use semwright_driver_sdk::{Capability, Driver};
use serde_json::{Value, json};
use std::{collections::BTreeSet, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

pub struct ObsDriver {
    pub client: Client,
    catalog: Catalog,
    config: Config,
    mutation: Mutex<()>,
}
impl ObsDriver {
    pub fn new(client: Client, config: Config) -> Result<Self> {
        Ok(Self {
            client,
            catalog: Catalog::load()?,
            config,
            mutation: Mutex::new(()),
        })
    }
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }
    async fn read(&self, kind: &str, data: Value, stamp: Option<Stamp>) -> Result<Value> {
        self.client
            .request(kind, data, false, stamp, CancellationToken::new())
            .await
    }
    pub async fn invoke(&self, command: &str, pinned_digest: &str, args: Value) -> Result<Value> {
        let entry = self.catalog.get(command)?;
        if entry.digest != pinned_digest {
            return Err(Fault::new(FaultKind::StaleReference));
        }
        entry.input(&args)?;
        let _serial = if entry.plan.mutation {
            Some(self.mutation.lock().await)
        } else {
            None
        };
        let result = tokio::time::timeout(
            Duration::from_secs(14),
            self.execute_plan(&entry.plan, &args),
        )
        .await
        .map_err(|_| Fault::new(FaultKind::Timeout).for_mutation(entry.plan.mutation))??;
        entry
            .output(&result)
            .map_err(|error| error.for_mutation(entry.plan.mutation))?;
        Ok(result)
    }
    fn envelope(data: Value, stamp: Stamp) -> Result<Value> {
        let result = json!({
            "generation":stamp.generation,
            "graph_revision":stamp.graph_revision,
            "untrusted":true,
            "data":data
        });
        bounds::check(&result, bounds::MAX_FRAME)?;
        Ok(result)
    }
    async fn consistent(&self, stamp: Stamp) -> Result<()> {
        if self.client.stamp().await != stamp {
            return Err(Fault::new(FaultKind::StaleReference));
        }
        Ok(())
    }
    async fn resolve(
        &self,
        args: &Value,
        target: &str,
        stamp: Stamp,
        mutation: bool,
    ) -> Result<Identity> {
        let key = format!("{target}_ref");
        let reference = args[&key]
            .as_str()
            .ok_or_else(|| Fault::new(FaultKind::Configuration))?;
        let kind = target_kind(target)?;
        let shared = self.client.shared();
        let identity = shared
            .lock()
            .await
            .refs
            .resolve(reference, "driver:obs", kind, stamp)?;
        if mutation && !identity.mutable() && kind != Kind::Filter {
            return Err(Fault::new(FaultKind::Unsupported));
        }
        if mutation {
            bounds::name(&identity.name)?;
        }
        self.verify(&identity, stamp).await?;
        Ok(identity)
    }
    async fn verify(&self, identity: &Identity, stamp: Stamp) -> Result<()> {
        let rows = match identity.kind {
            Kind::Scene => {
                self.read("GetSceneList", json!({}), Some(stamp)).await?["scenes"].clone()
            }
            Kind::Input => {
                self.read("GetInputList", json!({}), Some(stamp)).await?["inputs"].clone()
            }
            Kind::SceneItem => self
                .read(
                    "GetSceneItemList",
                    json!({"sceneUuid":identity.parent_uuid}),
                    Some(stamp),
                )
                .await?["sceneItems"]
                .clone(),
            Kind::Filter => self
                .read(
                    "GetSourceFilterList",
                    json!({"sourceUuid":identity.parent_uuid}),
                    Some(stamp),
                )
                .await?["filters"]
                .clone(),
            Kind::Transition => self
                .read("GetSceneTransitionList", json!({}), Some(stamp))
                .await?["transitions"]
                .clone(),
            _ => return Err(Fault::new(FaultKind::Unsupported)),
        };
        let rows = rows
            .as_array()
            .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
        let (name_key, uuid_key, kind_key) = keys(identity.kind);
        let matches: Vec<_> = rows
            .iter()
            .filter(|row| {
                row[name_key].as_str() == Some(identity.name.as_str())
                    && (identity.kind == Kind::Filter
                        || row[uuid_key].as_str() == identity.uuid.as_deref())
                    && (identity.kind != Kind::SceneItem
                        || row["sceneItemId"].as_u64() == identity.item_id)
                    && (kind_key.is_empty()
                        || row[kind_key].as_str() == Some(identity.fingerprint.as_str()))
            })
            .collect();
        if matches.len() != 1 {
            return Err(Fault::new(FaultKind::StaleReference));
        }
        if identity.kind == Kind::Transition
            && rows
                .iter()
                .filter(|row| row[name_key].as_str() == Some(identity.name.as_str()))
                .count()
                != 1
        {
            return Err(Fault::new(FaultKind::Precondition));
        }
        self.consistent(stamp).await
    }
    async fn entity(
        &self,
        row: &Value,
        kind: Kind,
        parent: Option<&Identity>,
        stamp: Stamp,
    ) -> Result<Value> {
        let (name_key, uuid_key, kind_key) = keys(kind);
        let name = row[name_key]
            .as_str()
            .filter(|name| name.len() <= bounds::MAX_NAME)
            .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
        let uuid = if kind == Kind::Filter {
            None
        } else {
            row[uuid_key].as_str().map(String::from)
        };
        let fingerprint = if kind_key.is_empty() {
            String::new()
        } else {
            row[kind_key]
                .as_str()
                .filter(|value| value.len() <= 128)
                .ok_or_else(|| Fault::new(FaultKind::Protocol))?
                .to_owned()
        };
        let identity = Identity {
            kind,
            uuid: uuid.clone(),
            name: name.into(),
            parent_uuid: parent.and_then(|parent| parent.uuid.clone()),
            item_id: if kind == Kind::SceneItem {
                Some(unsigned(row, "sceneItemId")?)
            } else {
                None
            },
            fingerprint,
        };
        let mutable = identity.mutable()
            || (kind == Kind::Filter
                && identity.parent_uuid.is_some()
                && bounds::name(&identity.name).is_ok());
        let strength = match kind {
            Kind::SceneItem => "parent_uuid_item_id_source_uuid",
            Kind::Filter => "watched_parent_name",
            _ if uuid.is_some() => "uuid",
            _ => "name_only",
        };
        let shared = self.client.shared();
        let mut state = shared.lock().await;
        if state.stamp != stamp {
            return Err(Fault::new(FaultKind::StaleReference));
        }
        let reference = state.refs.insert("driver:obs", identity, stamp)?;
        let mut value = json!({
            "ref":reference,
            "name":bounds::display(name),
            "uuid":uuid,
            "mutable":mutable,
            "identity_strength":strength
        });
        match kind {
            Kind::Input => {
                value["input_kind"] = json!(bounds::display(
                    row["inputKind"]
                        .as_str()
                        .ok_or_else(|| Fault::new(FaultKind::Protocol))?
                ));
            }
            Kind::SceneItem => {
                value["scene_item_id"] = json!(unsigned(row, "sceneItemId")?);
                value["enabled"] = json!(boolean(row, "sceneItemEnabled")?);
                value["index"] = json!(unsigned(row, "sceneItemIndex")?);
                value["is_group"] = json!(row["isGroup"].as_bool().unwrap_or(false));
                value["source_kind"] = json!(bounds::display(
                    row["sourceType"]
                        .as_str()
                        .ok_or_else(|| Fault::new(FaultKind::Protocol))?
                ));
            }
            Kind::Filter => {
                value["filter_kind"] = json!(bounds::display(
                    row["filterKind"]
                        .as_str()
                        .ok_or_else(|| Fault::new(FaultKind::Protocol))?
                ));
                value["enabled"] = json!(boolean(row, "filterEnabled")?);
                value["index"] = json!(unsigned(row, "filterIndex")?);
            }
            Kind::Transition => {
                value["transition_kind"] = json!(bounds::display(
                    row["transitionKind"]
                        .as_str()
                        .ok_or_else(|| Fault::new(FaultKind::Protocol))?
                ));
            }
            _ => {}
        }
        Ok(value)
    }
    async fn entities(
        &self,
        rows: &Value,
        kind: Kind,
        parent: Option<&Identity>,
        stamp: Stamp,
        max: usize,
    ) -> Result<Value> {
        let rows = rows
            .as_array()
            .filter(|rows| rows.len() <= max)
            .ok_or_else(|| Fault::new(FaultKind::ResourceLimit))?;
        let (name_key, uuid_key, _) = keys(kind);
        let mut ids = BTreeSet::new();
        for row in rows {
            let id = match kind {
                Kind::SceneItem => format!("item:{}", unsigned(row, "sceneItemId")?),
                Kind::Filter => format!(
                    "filter:{}",
                    row[name_key]
                        .as_str()
                        .ok_or_else(|| Fault::new(FaultKind::Protocol))?
                ),
                _ => row[uuid_key]
                    .as_str()
                    .or_else(|| row[name_key].as_str())
                    .ok_or_else(|| Fault::new(FaultKind::Protocol))?
                    .to_owned(),
            };
            if !ids.insert(id) {
                return Err(Fault::new(FaultKind::Protocol));
            }
        }
        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            out.push(self.entity(row, kind, parent, stamp).await?);
        }
        self.consistent(stamp).await?;
        Ok(Value::Array(out))
    }
    async fn execute_plan(&self, plan: &Plan, args: &Value) -> Result<Value> {
        match plan.mode.as_str() {
            "health" => {
                let shared = self.client.shared();
                let state = shared.lock().await;
                return Self::envelope(state.health(), state.stamp);
            }
            "events" => {
                let shared = self.client.shared();
                let state = shared.lock().await;
                let data = state
                    .events
                    .poll(unsigned(args, "after")?, unsigned(args, "limit")? as usize)?;
                return Self::envelope(data, state.stamp);
            }
            "operations" => {
                let id = args["operation_ref"]
                    .as_str()
                    .ok_or_else(|| Fault::new(FaultKind::Configuration))?;
                let shared = self.client.shared();
                let state = shared.lock().await;
                let data = serde_json::to_value(state.operations.get(id)?)?;
                return Self::envelope(data, state.stamp);
            }
            _ => {}
        }
        self.client.ready().await?;
        let stamp = self.client.stamp().await;
        if plan.mutation && args["expected_generation"].as_u64() != Some(stamp.generation) {
            return Err(Fault::new(FaultKind::StaleReference));
        }
        if plan.command == "stream.start" && !self.config.allow_stream_start {
            return Err(Fault::new(FaultKind::Precondition));
        }
        let target = match plan.target.as_deref() {
            Some(kind) => Some(self.resolve(args, kind, stamp, plan.mutation).await?),
            None => None,
        };
        let mut request = map_arguments(plan, args)?;
        if let Some(target) = &target {
            attach_target(&mut request, target, plan)?;
        }
        if plan.command == "input.mute.set" {
            let target = target
                .as_ref()
                .ok_or_else(|| Fault::new(FaultKind::Configuration))?;
            let current = self
                .read(
                    "GetInputMute",
                    json!({"inputUuid":target.uuid}),
                    Some(stamp),
                )
                .await?;
            if current["inputMuted"].as_bool() != args["expected_muted"].as_bool() {
                return Err(Fault::new(FaultKind::Precondition));
            }
        }
        if let Some(expected) = args["expected_current_scene_ref"].as_str() {
            let shared = self.client.shared();
            let expected =
                shared
                    .lock()
                    .await
                    .refs
                    .resolve(expected, "driver:obs", Kind::Scene, stamp)?;
            let kind = if plan.command == "preview_scene.set" {
                "GetCurrentPreviewScene"
            } else {
                "GetCurrentProgramScene"
            };
            if expected.uuid.is_none() {
                return Err(Fault::new(FaultKind::Unsupported));
            }
            let current = self.read(kind, json!({}), Some(stamp)).await?;
            if current["sceneUuid"].as_str() != expected.uuid.as_deref() {
                return Err(Fault::new(FaultKind::Precondition));
            }
        }
        if plan.command == "transition.trigger" || plan.command.starts_with("preview_scene.") {
            let current = self
                .read("GetStudioModeEnabled", json!({}), Some(stamp))
                .await?;
            if current["studioModeEnabled"] != true {
                return Err(Fault::new(FaultKind::Precondition));
            }
        }
        if plan.command.starts_with("media.") && plan.mutation {
            let target = target
                .as_ref()
                .ok_or_else(|| Fault::new(FaultKind::Configuration))?;
            let current = self
                .read(
                    "GetMediaInputStatus",
                    json!({"inputUuid":target.uuid}),
                    Some(stamp),
                )
                .await?;
            if plan.command == "media.seek"
                && unsigned(args, "position_ms")? > unsigned(&current, "mediaDuration")?
            {
                return Err(Fault::new(FaultKind::Precondition));
            }
        }
        if plan.mode == "outputs" {
            let rows = vec![
                ("GetRecordStatus".into(), json!({})),
                ("GetStreamStatus".into(), json!({})),
                ("GetReplayBufferStatus".into(), json!({})),
                ("GetVirtualCamStatus".into(), json!({})),
            ];
            let raw = self
                .client
                .batch_read(rows, true, CancellationToken::new())
                .await?;
            let rows = raw["results"]
                .as_array()
                .filter(|rows| rows.len() == 4)
                .ok_or_else(|| Fault::new(FaultKind::Application))?;
            let mut data = serde_json::Map::new();
            for (name, row) in ["record", "stream", "replay", "virtual_camera"]
                .into_iter()
                .zip(rows)
            {
                data.insert(name.into(), output(&crate::protocol::status(row)?)?);
            }
            self.consistent(stamp).await?;
            return Self::envelope(Value::Object(data), stamp);
        }
        if plan.mode == "output_mutation" {
            return self.output_mutation(plan, args, stamp).await;
        }
        let raw = self
            .client
            .request(
                &plan.request,
                request,
                plan.mutation,
                Some(stamp),
                CancellationToken::new(),
            )
            .await?;
        if plan.mutation {
            let shared = self.client.shared();
            let mut state = shared.lock().await;
            if state.stamp.generation != stamp.generation {
                return Err(Fault::new(FaultKind::Transport).uncertain());
            }
            state
                .stamp
                .invalidate()
                .map_err(|error| error.uncertain())?;
            state.refs.invalidate();
            return Self::envelope(
                json!({"accepted":true,"operation":null,"state":"unknown"}),
                state.stamp,
            );
        }
        self.consistent(stamp).await?;
        let data = match plan.mode.as_str() {
            "scenes" => json!({
                "scenes":self.entities(&raw["scenes"],Kind::Scene,None,stamp,256).await?,
                "current_program_uuid":raw.get("currentProgramSceneUuid").cloned().unwrap_or(Value::Null),
                "current_preview_uuid":raw.get("currentPreviewSceneUuid").cloned().unwrap_or(Value::Null)
            }),
            "scene_current" => self.entity(&raw, Kind::Scene, None, stamp).await?,
            "items" => json!({
                "items":self.entities(&raw["sceneItems"],Kind::SceneItem,target.as_ref(),stamp,512).await?
            }),
            "inputs" => json!({
                "inputs":self.entities(&raw["inputs"],Kind::Input,None,stamp,1024).await?
            }),
            "filters" => json!({
                "filters":self.entities(&raw["filters"],Kind::Filter,target.as_ref(),stamp,128).await?
            }),
            "transitions" => json!({
                "transitions":self.entities(&raw["transitions"],Kind::Transition,None,stamp,128).await?
            }),
            "input_inspect" => {
                let id = target
                    .as_ref()
                    .ok_or_else(|| Fault::new(FaultKind::Configuration))?;
                let rows = raw["inputs"]
                    .as_array()
                    .ok_or_else(|| Fault::new(FaultKind::Protocol))?;
                let matches: Vec<_> = rows
                    .iter()
                    .filter(|row| {
                        row["inputUuid"].as_str() == id.uuid.as_deref()
                            && row["inputName"] == id.name
                    })
                    .collect();
                if matches.len() != 1 {
                    return Err(Fault::new(FaultKind::StaleReference));
                }
                self.entity(matches[0], Kind::Input, None, stamp).await?
            }
            "output_status" => output(&raw)?,
            _ => map_output(plan, &raw)?,
        };
        self.consistent(stamp).await?;
        Self::envelope(data, stamp)
    }
    async fn output_mutation(&self, plan: &Plan, args: &Value, stamp: Stamp) -> Result<Value> {
        let (family, action) = plan
            .command
            .split_once('.')
            .ok_or_else(|| Fault::new(FaultKind::Configuration))?;
        let status = match family {
            "record" => "GetRecordStatus",
            "stream" => "GetStreamStatus",
            "replay" => "GetReplayBufferStatus",
            "virtual_camera" => "GetVirtualCamStatus",
            _ => return Err(Fault::new(FaultKind::Configuration)),
        };
        let current = self.read(status, json!({}), Some(stamp)).await?;
        let active = boolean(&current, "outputActive")?;
        let paused = current["outputPaused"].as_bool().unwrap_or(false);
        if args["expected_active"].as_bool() != Some(active)
            || (action == "start" && active)
            || (action != "start" && !active)
            || (action == "pause" && paused)
            || (action == "resume" && !paused)
        {
            return Err(Fault::new(FaultKind::Precondition));
        }
        let operation = if matches!(action, "start" | "stop") {
            let shared = self.client.shared();
            let mut state = shared.lock().await;
            if state.stamp != stamp {
                return Err(Fault::new(FaultKind::StaleReference));
            }
            Some(
                state
                    .operations
                    .begin(family, action == "start", stamp.generation)?,
            )
        } else {
            None
        };
        let result = self
            .client
            .request(
                &plan.request,
                json!({}),
                true,
                Some(stamp),
                CancellationToken::new(),
            )
            .await;
        let shared = self.client.shared();
        let mut state = shared.lock().await;
        if let Some(id) = &operation {
            state.operations.acceptance(id, &result);
        }
        result?;
        let phase = operation
            .as_ref()
            .and_then(|id| state.operations.get(id).ok())
            .map_or(Phase::Unknown, |operation| operation.phase);
        if state.stamp.generation != stamp.generation {
            return Err(Fault::new(FaultKind::Transport).uncertain());
        }
        Self::envelope(
            json!({"accepted":true,"operation":operation,"state":phase}),
            state.stamp,
        )
    }
}

fn unsigned(value: &Value, key: &str) -> Result<u64> {
    value[key]
        .as_u64()
        .ok_or_else(|| Fault::new(FaultKind::Protocol))
}
fn boolean(value: &Value, key: &str) -> Result<bool> {
    value[key]
        .as_bool()
        .ok_or_else(|| Fault::new(FaultKind::Protocol))
}
fn target_kind(kind: &str) -> Result<Kind> {
    Ok(match kind {
        "scene" => Kind::Scene,
        "scene_item" => Kind::SceneItem,
        "input" => Kind::Input,
        "filter" => Kind::Filter,
        "transition" => Kind::Transition,
        _ => return Err(Fault::new(FaultKind::Configuration)),
    })
}
fn keys(kind: Kind) -> (&'static str, &'static str, &'static str) {
    match kind {
        Kind::Scene => ("sceneName", "sceneUuid", ""),
        Kind::Input => ("inputName", "inputUuid", "inputKind"),
        Kind::SceneItem => ("sourceName", "sourceUuid", "sourceType"),
        Kind::Filter => ("filterName", "", "filterKind"),
        Kind::Transition => ("transitionName", "transitionUuid", "transitionKind"),
        _ => ("name", "uuid", ""),
    }
}
fn attach_target(data: &mut Value, target: &Identity, plan: &Plan) -> Result<()> {
    let object = data
        .as_object_mut()
        .ok_or_else(|| Fault::new(FaultKind::Configuration))?;
    match target.kind {
        Kind::Scene => {
            if let Some(uuid) = &target.uuid {
                object.insert("sceneUuid".into(), json!(uuid));
            } else if !plan.mutation {
                object.insert("sceneName".into(), json!(target.name));
            } else {
                return Err(Fault::new(FaultKind::Unsupported));
            }
        }
        Kind::Input => {
            if plan.request == "GetInputList" {
                return Ok(());
            }
            let prefix = if plan.request == "GetSourceFilterList" {
                "source"
            } else {
                "input"
            };
            if let Some(uuid) = &target.uuid {
                object.insert(format!("{prefix}Uuid"), json!(uuid));
            } else if !plan.mutation {
                object.insert(format!("{prefix}Name"), json!(target.name));
            } else {
                return Err(Fault::new(FaultKind::Unsupported));
            }
        }
        Kind::SceneItem => {
            object.insert("sceneUuid".into(), json!(target.parent_uuid));
            object.insert("sceneItemId".into(), json!(target.item_id));
        }
        Kind::Filter => {
            if target.parent_uuid.is_none() {
                return Err(Fault::new(FaultKind::Unsupported));
            }
            object.insert("sourceUuid".into(), json!(target.parent_uuid));
            object.insert("filterName".into(), json!(target.name));
        }
        Kind::Transition => {
            object.insert("transitionName".into(), json!(target.name));
        }
        _ => return Err(Fault::new(FaultKind::Unsupported)),
    }
    Ok(())
}
fn output(raw: &Value) -> Result<Value> {
    let active = boolean(raw, "outputActive")?;
    Ok(json!({
        "active":active,
        "paused":raw["outputPaused"].as_bool(),
        "duration_ms":raw["outputDuration"].as_u64(),
        "timecode":raw["outputTimecode"].as_str().map(bounds::display)
    }))
}

#[async_trait]
impl Driver for ObsDriver {
    fn id(&self) -> &str {
        "obs"
    }
    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }
    async fn capabilities(&mut self) -> semwright_types::Result<Vec<Capability>> {
        Ok(self.catalog.capabilities())
    }
    async fn execute(
        &mut self,
        command: &str,
        pinned_digest: &str,
        args: Value,
    ) -> semwright_types::Result<Value> {
        self.invoke(command, pinned_digest, args)
            .await
            .map_err(Fault::semwright)
    }
    async fn health(&mut self) -> semwright_types::Result<Value> {
        Ok(self.client.health().await)
    }
}
