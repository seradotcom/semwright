#![cfg(target_os = "linux")]
use semwright_backend_api::Provider;
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
    Transport,
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
            assert_eq!(u16::from_le_bytes(bytes[start..start + 2].try_into().unwrap()), 1);
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
        serde_json::to_vec(&json!({"schema_version":1,"ardour_version":"8.4.0"})).unwrap(),
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
        system_config: vec![],
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
            processes: 32,
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

    let created = call(
        &broker,
        "session.deep.create",
        json!({"state":"Base","sample_rate":48000,"master_channels":2}),
    )
    .await;
    assert!(created.ok, "{created:?}");
    let revision0 = created.data.unwrap()["revision"].as_str().unwrap().to_owned();

    let ranged = call(
        &broker,
        "session.deep.range.set",
        json!({"state":"Base","expected_revision":revision0,"start":0,"end":48000}),
    )
    .await;
    assert!(ranged.ok, "{ranged:?}");
    let revision1 = ranged.data.unwrap()["revision"].as_str().unwrap().to_owned();

    let stem = call(
        &broker,
        "session.deep.stem.create",
        json!({"state":"Base","expected_revision":revision1,"channels":2,"name":"Proof Stem"}),
    )
    .await;
    assert!(stem.ok, "{stem:?}");
    let revision2 = stem.data.unwrap()["revision"].as_str().unwrap().to_owned();

    let inspected = call(&broker, "session.deep.inspect", json!({"state":"Base"})).await;
    assert!(inspected.ok, "{inspected:?}");
    let inspected_data = inspected.data.unwrap();
    assert_eq!(inspected_data["revision"], revision2);
    let project_json = parse_project(&inspected_data);
    let stems = project_json["stems"].as_array().unwrap();
    assert_eq!(stems.len(), 1, "{project_json}");
    assert_eq!(stems[0]["name"], "Proof Stem");
    let stem_id = stems[0]["id"].as_str().unwrap().to_owned();

    let renamed = call(
        &broker,
        "session.deep.route.rename",
        json!({"state":"Base","expected_revision":revision2,"route_id":stem_id,"name":"Renamed Proof"}),
    )
    .await;
    assert!(renamed.ok, "{renamed:?}");
    let revision3 = renamed.data.unwrap()["revision"].as_str().unwrap().to_owned();

    let stale = call(
        &broker,
        "session.deep.route.mute",
        json!({"state":"Base","expected_revision":revision2,"route_id":project_json["stems"][0]["id"],"value":true}),
    )
    .await;
    assert!(!stale.ok);
    assert_eq!(stale.error.unwrap().code, ErrorCode::StaleReference);

    let saved = call(
        &broker,
        "session.deep.save-as",
        json!({"source_state":"Base","candidate_state":"Candidate","expected_revision":revision3}),
    )
    .await;
    assert!(saved.ok, "{saved:?}");
    let saved_data = saved.data.unwrap();
    assert_eq!(saved_data["source_preserved"], true);
    let candidate_revision = saved_data["candidate_revision"].as_str().unwrap().to_owned();

    let reopened = call(&broker, "session.deep.inspect", json!({"state":"Candidate"})).await;
    assert!(reopened.ok, "{reopened:?}");
    assert_eq!(reopened.data.as_ref().unwrap()["revision"], candidate_revision);
    let candidate_project = parse_project(reopened.data.as_ref().unwrap());
    assert_eq!(candidate_project["stems"][0]["name"], "Renamed Proof");

    let rendered = call(
        &broker,
        "session.deep.export",
        json!({"state":"Candidate","expected_revision":candidate_revision,"file_name":"proof.wav","sample_rate":48000,"bit_depth":16}),
    )
    .await;
    assert!(rendered.ok, "{rendered:?}");
    let receipt = rendered.data.unwrap();
    assert_eq!(receipt["source_preserved"], true);
    assert_eq!(receipt["artifact"]["sha256"], digest(&output.join("proof.wav")));
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
        json!({"state":"Candidate","expected_revision":candidate_revision,"route_id":candidate_project["stems"][0]["id"]}),
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
            "create_reopen":true,
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
