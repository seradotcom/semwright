#![cfg(target_os = "linux")]
use semwright_audio_domain::{
    edit::{self, Edit},
    model::AudioProject,
};
use semwright_backend_api::{Context, Provider};
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
    SystemConfigMount, Transport,
};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use semwright_types::{Envelope, ErrorCode, ExecuteRequest, unique_id};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

fn digest(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}
fn required_path(name: &str) -> PathBuf {
    let path = PathBuf::from(
        std::env::var_os(name).unwrap_or_else(|| panic!("required native prerequisite: {name}")),
    );
    assert!(path.is_file(), "native prerequisite is not a file: {name}");
    path.canonicalize().unwrap()
}
async fn direct_host_call(
    provider: &DriverProvider,
    capabilities: &[semwright_backend_api::ProvidedCapability],
    command: &str,
    args: Value,
) -> semwright_types::Result<Value> {
    let descriptor = capabilities
        .iter()
        .find(|capability| capability.descriptor.name == format!("driver.ardour-audio.{command}"))
        .unwrap_or_else(|| panic!("missing direct host capability {command}"));
    Provider::execute(
        provider,
        &Context {
            session: "ardour-host-diagnostic".into(),
            request_id: unique_id(),
            cancellation: CancellationToken::new(),
        },
        &descriptor.descriptor,
        &args,
    )
    .await
}

fn clear_disposable_project(path: &Path) {
    for entry in fs::read_dir(path).unwrap() {
        let entry = entry.unwrap();
        let child = entry.path();
        if child.is_dir() {
            fs::remove_dir_all(child).unwrap();
        } else {
            fs::remove_file(child).unwrap();
        }
    }
}

async fn call(broker: &Arc<Broker>, command: &str, args: Value) -> Envelope {
    broker
        .clone()
        .execute(
            "ardour-host-conformance".into(),
            unique_id(),
            ExecuteRequest {
                command: format!("driver.ardour-audio.{command}"),
                args,
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await
}
fn parse_project(value: &Value) -> Value {
    serde_json::from_str(value["project_json"].as_str().unwrap()).unwrap()
}
fn pcm16_frames(path: &Path) -> (u16, u32, u64, bool) {
    let bytes = fs::read(path).unwrap();
    assert_eq!(&bytes[..4], b"RIFF");
    assert_eq!(&bytes[8..12], b"WAVE");
    let mut channels = None;
    let mut rate = None;
    let mut data = None;
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let start = offset + 8;
        assert!(start + size <= bytes.len());
        if id == b"fmt " {
            assert!(size >= 16);
            assert_eq!(
                u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap()),
                1
            );
            channels = Some(u16::from_le_bytes(
                bytes[start + 2..start + 4].try_into().unwrap(),
            ));
            rate = Some(u32::from_le_bytes(
                bytes[start + 4..start + 8].try_into().unwrap(),
            ));
            assert_eq!(
                u16::from_le_bytes(bytes[start + 14..start + 16].try_into().unwrap()),
                16
            );
        } else if id == b"data" {
            data = Some(bytes[start..start + size].to_vec());
        }
        offset = start + size + (size % 2);
    }
    let channels = channels.unwrap();
    let data = data.unwrap();
    let samples = data
        .as_chunks::<2>()
        .0
        .iter()
        .map(|value| i16::from_le_bytes(*value))
        .collect::<Vec<_>>();
    assert_eq!(samples.len() % usize::from(channels), 0);
    (
        channels,
        rate.unwrap(),
        samples.len() as u64 / u64::from(channels),
        samples.iter().all(|sample| *sample == 0),
    )
}

#[tokio::test]
#[ignore = "requires Ardour 8.4 utilities and Driver Host sandbox on a disposable runner"]
async fn broker_host_ardour_create_edit_save_reopen_export_is_native_and_fail_closed() {
    let sandbox = required_path("SEMWRIGHT_TEST_SANDBOX_HELPER");
    let lua = required_path("SEMWRIGHT_TEST_ARDOUR_LUA");
    let create = required_path("SEMWRIGHT_TEST_ARDOUR_CREATE");
    let export = required_path("SEMWRIGHT_TEST_ARDOUR_EXPORT");
    let root = tempfile::tempdir().unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    for dir in ["binary", "state", "runtime", "project", "output"] {
        fs::create_dir(root.path().join(dir)).unwrap();
        fs::set_permissions(root.path().join(dir), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let executable = root.path().join("binary/driver");
    fs::copy(
        env!("CARGO_BIN_EXE_semwright-ardour-audio-driver"),
        &executable,
    )
    .unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o500)).unwrap();
    let runtime = root.path().join("runtime");
    fs::write(
        runtime.join("semwright-runtime.json"),
        serde_json::to_vec(&json!({
            "schema_version":1,
            "ardour_version":"8.4.0",
            "allowed_plugins":[{
                "id":"ace-inline-scope",
                "native_name":"ACE Inline Scope",
                "kind":"lua",
                "preset":"",
                "unique_id":null
            }]
        }))
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(
        runtime.join("semwright-runtime.json"),
        fs::Permissions::from_mode(0o400),
    )
    .unwrap();
    let project = root.path().join("project").canonicalize().unwrap();
    let output = root.path().join("output").canonicalize().unwrap();
    let manifest = Manifest {
        manifest_version: 1,
        protocol: 4,
        id: "ardour-audio".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        publisher: "semwright-native-tests".into(),
        executable: executable.clone(),
        sha256: digest(&executable),
        application: ApplicationMatch {
            desktop_id: None,
            process_names: vec!["ardour8".into(), "luasession".into()],
            supported_versions: vec!["8.4.0".into()],
        },
        transport: Transport::StdioV1,
        mounts: vec![
            DriverMount {
                root: "ardour-runtime".into(),
                read_only: true,
                execute: false,
            },
            DriverMount {
                root: "ardour-project".into(),
                read_only: false,
                execute: false,
            },
            DriverMount {
                root: "ardour-output".into(),
                read_only: false,
                execute: false,
            },
        ],
        system_config: vec![SystemConfigMount {
            root: "ardour-config".into(),
            destination: "/etc/ardour8".into(),
        }],
        secrets: vec![],
        tools: vec![
            DriverToolMount {
                root: "ardour-lua-tool".into(),
                name: "ardour-lua".into(),
                sha256: digest(&lua),
            },
            DriverToolMount {
                root: "ardour-create-tool".into(),
                name: "ardour-new-session".into(),
                sha256: digest(&create),
            },
            DriverToolMount {
                root: "ardour-export-tool".into(),
                name: "ardour-export".into(),
                sha256: digest(&export),
            },
        ],
        network: false,
        loopback_port: None,
        resources: DriverResources {
            open_files: 512,
            processes: 128,
            cpu_seconds: 180,
            operation_cpu_seconds: 30,
            address_space_bytes: 3_221_225_472,
            file_size_bytes: 536_870_912,
        },
        request_timeout_ms: 30_000,
        interfaces: DriverInterfaces {
            health: true,
            ..Default::default()
        },
    };
    let grants = vec![
        FilesystemGrant {
            name: "ardour-runtime".into(),
            path: runtime.canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "ardour-config".into(),
            path: PathBuf::from("/etc/ardour8").canonicalize().unwrap(),
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "ardour-project".into(),
            path: project.clone(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "ardour-output".into(),
            path: output.clone(),
            read: true,
            write: true,
        },
        FilesystemGrant {
            name: "ardour-lua-tool".into(),
            path: lua,
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "ardour-create-tool".into(),
            path: create,
            read: true,
            write: false,
        },
        FilesystemGrant {
            name: "ardour-export-tool".into(),
            path: export,
            read: true,
            write: false,
        },
    ];
    let provider = DriverProvider::connect(
        manifest,
        &root.path().join("state"),
        &sandbox,
        &grants,
        false,
    )
    .await
    .unwrap();
    let direct_capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
    let direct_create = direct_host_call(
        provider.as_ref(),
        &direct_capabilities,
        "session.deep.create",
        json!({"state":"Diagnostic","sample_rate":48000,"master_channels":2}),
    )
    .await;
    assert!(
        direct_create.is_ok(),
        "raw Driver Host Ardour create failed before Broker redaction: {direct_create:?}"
    );
    clear_disposable_project(&project);

    let broker = Broker::new(
        Policy::new(PolicyConfig {
            allow: ["driver:ardour-audio".into()].into(),
            ..Default::default()
        })
        .unwrap(),
        vec![],
        Audit::open(&root.path().join("audit"), 65536, 2).unwrap(),
        Arc::new(NoApprover),
        None,
        json!({}),
        false,
    )
    .unwrap();
    broker.mount_provider(provider.clone()).await.unwrap();

    let runtime_probe = call(&broker, "session.deep.runtime.probe", json!({})).await;
    assert!(runtime_probe.ok, "{runtime_probe:?}");
    let runtime_probe_data = runtime_probe.data.unwrap();
    assert_eq!(runtime_probe_data["ardour_version"], "8.4.0");
    for key in ["lua_banner", "create_banner", "export_banner"] {
        assert!(
            runtime_probe_data[key].as_str().unwrap().contains("8.4"),
            "{runtime_probe_data}"
        );
    }

    let created = call(
        &broker,
        "session.deep.create",
        json!({"state":"Base","sample_rate":48000,"master_channels":2}),
    )
    .await;
    assert!(created.ok, "{created:?}");
    let revision0 = created.data.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let ranged = call(
        &broker,
        "session.deep.range.set",
        json!({"state":"Base","expected_revision":revision0,"start":0,"end":48000}),
    )
    .await;
    assert!(ranged.ok, "{ranged:?}");
    let revision1 = ranged.data.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let stem = call(
        &broker,
        "session.deep.stem.create",
        json!({"state":"Base","expected_revision":revision1,"channels":2,"name":"Proof Stem"}),
    )
    .await;
    assert!(stem.ok, "{stem:?}");
    let revision2 = stem.data.unwrap()["revision"].as_str().unwrap().to_owned();

    let bus = call(
        &broker,
        "session.deep.bus.create",
        json!({"state":"Base","expected_revision":revision2,"channels":2,"name":"Proof Bus"}),
    )
    .await;
    assert!(bus.ok, "{bus:?}");
    let revision3 = bus.data.unwrap()["revision"].as_str().unwrap().to_owned();

    let aux_stem = call(
        &broker,
        "session.deep.stem.create",
        json!({"state":"Base","expected_revision":revision3,"channels":2,"name":"Group Aux"}),
    )
    .await;
    assert!(aux_stem.ok, "{aux_stem:?}");
    let revision4 = aux_stem.data.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let inspected = call(&broker, "session.deep.inspect", json!({"state":"Base"})).await;
    assert!(inspected.ok, "{inspected:?}");
    let inspected_data = inspected.data.unwrap();
    assert_eq!(inspected_data["revision"], revision4);
    let project_json = parse_project(&inspected_data);
    let stems = project_json["stems"].as_array().unwrap();
    assert_eq!(stems.len(), 2, "{project_json}");
    let stem_id = stems
        .iter()
        .find(|stem| stem["name"] == "Proof Stem")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let aux_stem_id = stems
        .iter()
        .find(|stem| stem["name"] == "Group Aux")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let bus_id = project_json["buses"]
        .as_array()
        .unwrap()
        .iter()
        .find(|bus| bus["name"] == "Proof Bus")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let send = call(
        &broker,
        "session.deep.send.create",
        json!({
            "state":"Base",
            "expected_revision":revision4,
            "source_route_id":stem_id,
            "target_bus_id":bus_id,
            "pre_fader":false
        }),
    )
    .await;
    assert!(send.ok, "{send:?}");
    let revision5 = send.data.unwrap()["revision"].as_str().unwrap().to_owned();

    let send_gain = call(
        &broker,
        "session.deep.send.gain.set",
        json!({
            "state":"Base",
            "expected_revision":revision5,
            "source_route_id":stem_id,
            "target_bus_id":bus_id,
            "gain_millidb":-6000
        }),
    )
    .await;
    assert!(send_gain.ok, "{send_gain:?}");
    let revision6 = send_gain.data.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let group = call(
        &broker,
        "session.deep.group.create",
        json!({
            "state":"Base",
            "expected_revision":revision6,
            "name":"Proof Group",
            "route_id":stem_id
        }),
    )
    .await;
    assert!(group.ok, "{group:?}");
    let revision7 = group.data.unwrap()["revision"].as_str().unwrap().to_owned();

    let group_snapshot = call(&broker, "session.deep.inspect", json!({"state":"Base"})).await;
    assert!(group_snapshot.ok, "{group_snapshot:?}");
    let group_project = parse_project(group_snapshot.data.as_ref().unwrap());
    let group_id = group_project["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["name"] == "Proof Group")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();

    let group_add = call(
        &broker,
        "session.deep.group.add",
        json!({
            "state":"Base",
            "expected_revision":revision7,
            "group_id":group_id,
            "route_id":aux_stem_id
        }),
    )
    .await;
    assert!(group_add.ok, "{group_add:?}");
    let revision8 = group_add.data.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let group_remove = call(
        &broker,
        "session.deep.group.remove",
        json!({
            "state":"Base",
            "expected_revision":revision8,
            "group_id":group_id,
            "route_id":aux_stem_id
        }),
    )
    .await;
    assert!(group_remove.ok, "{group_remove:?}");
    let revision9 = group_remove.data.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let plugin_insert = call(
        &broker,
        "session.deep.plugin.insert",
        json!({
            "state":"Base",
            "expected_revision":revision9,
            "route_id":stem_id,
            "plugin":"ace-inline-scope"
        }),
    )
    .await;
    assert!(plugin_insert.ok, "{plugin_insert:?}");
    let revision10 = plugin_insert.data.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let plugin_inventory = call(
        &broker,
        "session.deep.plugins.inspect",
        json!({"state":"Base"}),
    )
    .await;
    assert!(plugin_inventory.ok, "{plugin_inventory:?}");
    let plugin_data = plugin_inventory.data.unwrap();
    assert_eq!(plugin_data["revision"], revision10);
    let plugin_row = plugin_data["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["route_id"] == stem_id && plugin["name"] == "ACE Inline Scope")
        .unwrap()
        .clone();
    let plugin_id = plugin_row["plugin_id"].as_str().unwrap().to_owned();
    let parameter = plugin_row["parameters"]
        .as_array()
        .unwrap()
        .first()
        .expect("allowlisted acceptance plugin exposes parameters")
        .clone();
    let parameter_index = parameter["index"].as_u64().unwrap();
    let parameter_label = parameter["label"].as_str().unwrap().to_owned();
    let parameter_current = parameter["value_microunits"].as_i64().unwrap();
    let parameter_lower = parameter["lower_microunits"].as_i64().unwrap();
    let parameter_upper = parameter["upper_microunits"].as_i64().unwrap();
    let parameter_target = if parameter_current != parameter_lower {
        parameter_lower
    } else {
        parameter_upper
    };
    assert_ne!(parameter_lower, parameter_upper);
    let plugin_unique_id = plugin_row["unique_id"].clone();

    let parameter_set = call(
        &broker,
        "session.deep.plugin.parameter.set",
        json!({
            "state":"Base",
            "expected_revision":revision10,
            "route_id":stem_id,
            "plugin_id":plugin_id,
            "expected_plugin_unique_id":plugin_unique_id,
            "parameter_index":parameter_index,
            "expected_parameter_label":parameter_label,
            "value_microunits":parameter_target
        }),
    )
    .await;
    assert!(parameter_set.ok, "{parameter_set:?}");
    let revision11 = parameter_set.data.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let parameter_observed = call(
        &broker,
        "session.deep.plugins.inspect",
        json!({"state":"Base"}),
    )
    .await;
    assert!(parameter_observed.ok, "{parameter_observed:?}");
    let observed_data = parameter_observed.data.unwrap();
    assert_eq!(observed_data["revision"], revision11);
    let observed_plugin = observed_data["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["plugin_id"] == plugin_id)
        .unwrap();
    let observed_parameter = observed_plugin["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|parameter| parameter["index"] == parameter_index)
        .unwrap();
    assert_eq!(observed_parameter["value_microunits"], parameter_target);
    let automation_before = observed_parameter["automation_points"].as_u64().unwrap();

    let automation = call(
        &broker,
        "session.deep.plugin.automation.point.add",
        json!({
            "state":"Base",
            "expected_revision":revision11,
            "route_id":stem_id,
            "plugin_id":plugin_id,
            "expected_plugin_unique_id":plugin_unique_id,
            "parameter_index":parameter_index,
            "expected_parameter_label":parameter_label,
            "frame":24000,
            "value_microunits":parameter_target
        }),
    )
    .await;
    assert!(automation.ok, "{automation:?}");
    let revision12 = automation.data.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let automation_observed = call(
        &broker,
        "session.deep.plugins.inspect",
        json!({"state":"Base"}),
    )
    .await;
    assert!(automation_observed.ok, "{automation_observed:?}");
    let automation_data = automation_observed.data.unwrap();
    assert_eq!(automation_data["revision"], revision12);
    let automation_parameter = automation_data["plugins"]
        .as_array()
        .unwrap()
        .iter()
        .find(|plugin| plugin["plugin_id"] == plugin_id)
        .unwrap()["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|parameter| parameter["index"] == parameter_index)
        .unwrap();
    assert_eq!(
        automation_parameter["automation_points"].as_u64().unwrap(),
        automation_before + 1
    );

    for (command, args) in [
        (
            "session.deep.send.remove",
            json!({
                "state":"Base",
                "expected_revision":revision12,
                "source_route_id":stem_id,
                "target_bus_id":bus_id
            }),
        ),
        (
            "session.deep.group.delete",
            json!({
                "state":"Base",
                "expected_revision":revision12,
                "group_id":group_id
            }),
        ),
        (
            "session.deep.plugin.remove",
            json!({
                "state":"Base",
                "expected_revision":revision12,
                "route_id":stem_id,
                "plugin_id":plugin_id
            }),
        ),
    ] {
        let denied = call(&broker, command, args).await;
        assert!(!denied.ok, "{denied:?}");
        assert_eq!(denied.error.unwrap().code, ErrorCode::ConsentRequired);
    }

    let before_rename = call(&broker, "session.deep.inspect", json!({"state":"Base"})).await;
    assert!(before_rename.ok, "{before_rename:?}");
    let before_rename_data = before_rename.data.unwrap();
    assert_eq!(before_rename_data["revision"], revision12);
    let project_json = parse_project(&before_rename_data);
    let proof_stem = project_json["stems"]
        .as_array()
        .unwrap()
        .iter()
        .find(|stem| stem["id"] == stem_id)
        .unwrap();
    assert_eq!(proof_stem["sends"].as_array().unwrap().len(), 1);
    assert_eq!(proof_stem["sends"][0]["gain"], -6000);
    let proof_group = project_json["groups"]
        .as_array()
        .unwrap()
        .iter()
        .find(|group| group["id"] == group_id)
        .unwrap();
    assert_eq!(
        proof_group["stems"].as_array().unwrap(),
        &[Value::String(stem_id.clone())]
    );

    let reference_before: AudioProject = serde_json::from_value(project_json.clone()).unwrap();
    let reference_expected = edit::apply(
        &reference_before,
        &reference_before.semantic_digest().unwrap(),
        Edit::StemRename {
            stem: stem_id.clone(),
            name: "Renamed Proof".into(),
        },
        "differential-rename",
    )
    .unwrap()
    .result;
    let revision_before_rename = revision12.clone();

    let renamed = call(
        &broker,
        "session.deep.route.rename",
        json!({"state":"Base","expected_revision":revision_before_rename,"route_id":stem_id,"name":"Renamed Proof"}),
    )
    .await;
    assert!(renamed.ok, "{renamed:?}");
    let revision_after_rename = renamed.data.unwrap()["revision"]
        .as_str()
        .unwrap()
        .to_owned();
    let after_rename = call(&broker, "session.deep.inspect", json!({"state":"Base"})).await;
    assert!(after_rename.ok, "{after_rename:?}");
    let native_after: AudioProject =
        serde_json::from_value(parse_project(after_rename.data.as_ref().unwrap())).unwrap();
    assert_eq!(
        native_after.semantic_digest().unwrap(),
        reference_expected.semantic_digest().unwrap(),
        "native Ardour rename diverged from the portable reference edit"
    );

    let stale = call(
        &broker,
        "session.deep.route.mute",
        json!({"state":"Base","expected_revision":revision_before_rename,"route_id":stem_id,"value":true}),
    )
    .await;
    assert!(!stale.ok);
    assert_eq!(stale.error.unwrap().code, ErrorCode::StaleReference);

    let saved = call(
        &broker,
        "session.deep.save-as",
        json!({"source_state":"Base","candidate_state":"Candidate","expected_revision":revision_after_rename}),
    )
    .await;
    assert!(saved.ok, "{saved:?}");
    let saved_data = saved.data.unwrap();
    assert_eq!(saved_data["source_preserved"], true);
    let candidate_revision = saved_data["candidate_revision"]
        .as_str()
        .unwrap()
        .to_owned();

    let reopened = call(
        &broker,
        "session.deep.inspect",
        json!({"state":"Candidate"}),
    )
    .await;
    assert!(reopened.ok, "{reopened:?}");
    assert_eq!(
        reopened.data.as_ref().unwrap()["revision"],
        candidate_revision
    );
    let candidate_project = parse_project(reopened.data.as_ref().unwrap());
    assert!(
        candidate_project["stems"]
            .as_array()
            .unwrap()
            .iter()
            .any(|stem| stem["id"] == stem_id && stem["name"] == "Renamed Proof"),
        "{candidate_project}"
    );

    let rendered = call(
        &broker,
        "session.deep.export",
        json!({"state":"Candidate","expected_revision":candidate_revision,"file_name":"proof.wav","sample_rate":48000,"bit_depth":16}),
    )
    .await;
    assert!(rendered.ok, "{rendered:?}");
    let receipt = rendered.data.unwrap();
    assert_eq!(receipt["source_preserved"], true);
    assert_eq!(
        receipt["artifact"]["sha256"],
        digest(&output.join("proof.wav"))
    );
    assert_eq!(receipt["artifact"]["frames"], 48_000);
    let (channels, rate, frames, silent) = pcm16_frames(&output.join("proof.wav"));
    assert_eq!((channels, rate, frames), (2, 48_000, 48_000));
    assert!(silent);

    let overwrite = call(
        &broker,
        "session.deep.export",
        json!({"state":"Candidate","expected_revision":candidate_revision,"file_name":"proof.wav","sample_rate":48000,"bit_depth":16}),
    )
    .await;
    assert!(!overwrite.ok);
    assert_eq!(overwrite.error.unwrap().code, ErrorCode::Conflict);

    let destructive = call(
        &broker,
        "session.deep.route.remove",
        json!({"state":"Candidate","expected_revision":candidate_revision,"route_id":stem_id}),
    )
    .await;
    assert!(!destructive.ok);
    assert_eq!(destructive.error.unwrap().code, ErrorCode::ConsentRequired);

    Provider::shutdown(provider.as_ref()).await.unwrap();
    let evidence = PathBuf::from("verification/audio/ardour-host.json");
    fs::create_dir_all(evidence.parent().unwrap()).unwrap();
    fs::write(
        evidence,
        serde_json::to_vec_pretty(&json!({
            "schema_version":1,
            "route":"broker-policy-driver-host-sealed-ardour-8.4",
            "ardour_version":"8.4.0",
            "runtime_probe":true,
            "create_reopen":true,
            "native_bus_create":true,
            "native_internal_send_create_and_gain":true,
            "native_route_group_membership":true,
            "allowlisted_plugin_insert":true,
            "plugin_parameter_readback_and_write":true,
            "plugin_automation_point_readback":true,
            "destructive_send_group_plugin_denied_without_consent":true,
            "differential_rename_digest":true,
            "semantic_target_mapping":true,
            "stale_revision_denied":true,
            "save_as_source_preserved":true,
            "export_source_preserved":true,
            "export_sha256":digest(&output.join("proof.wav")),
            "export_frames":48000,
            "export_sample_rate":48000,
            "export_channels":2,
            "export_fixture":"native_silent_session_range",
            "destructive_confirmation_preserved":true,
            "no_overwrite":true
        }))
        .unwrap(),
    )
    .unwrap();
}
