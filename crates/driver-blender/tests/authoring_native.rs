//! Native acceptance TEST SOURCE. Runs only in the explicit GitHub-hosted native lane.
#![cfg(all(unix, feature = "authoring-native"))]
use semwright_backend_api::Provider;
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
    SystemConfigMount, Transport, descriptor_digest,
};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig, Profile};
use semwright_project_graph as graph;
use semwright_types::{ExecuteRequest, unique_id};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;

fn native_lane_root() -> Option<PathBuf> {
    if std::env::var("SEMWRIGHT_NATIVE_AUTHORING_ACCEPTANCE").as_deref() != Ok("1") {
        eprintln!(
            "SKIP_NATIVE_AUTHORING: explicit acceptance lane marker is not set;              all-features compile/coverage must not require a provisioned Blender runtime"
        );
        return None;
    }
    assert_eq!(
        std::env::var("GITHUB_ACTIONS").as_deref(),
        Ok("true"),
        "native acceptance marker is CI-only"
    );
    assert_eq!(
        std::env::var("RUNNER_ENVIRONMENT").as_deref(),
        Ok("github-hosted"),
        "native acceptance marker requires a GitHub-hosted disposable runner"
    );
    let root = PathBuf::from(
        std::env::var("SEMWRIGHT_TEST_BLENDER_ROOT")
            .expect("acceptance lane marker requires Blender runtime"),
    );
    let helper = PathBuf::from(
        std::env::var("SEMWRIGHT_TEST_SANDBOX_HELPER")
            .expect("acceptance lane marker requires sandbox helper"),
    );
    assert!(
        root.join("blender").is_file(),
        "pinned Blender binary missing"
    );
    assert!(helper.is_file(), "sandbox helper missing");
    Some(root)
}

struct NativeFixture {
    broker: Arc<Broker>,
    provider: Arc<DriverProvider>,
    _state: tempfile::TempDir,
    session: String,
    apply_descriptor: semwright_semantic_composition::Digest,
    runtime_digest: semwright_semantic_composition::Digest,
}
impl NativeFixture {
    async fn start(workspace: &Path, root: &Path, allow: bool) -> Self {
        assert_eq!(
            std::env::var("GITHUB_ACTIONS").as_deref(),
            Ok("true"),
            "native suite is remote-only"
        );
        assert_eq!(
            std::env::var("RUNNER_ENVIRONMENT").as_deref(),
            Ok("github-hosted"),
            "self-hosted is not this lane"
        );
        let state = tempfile::tempdir().unwrap();
        fs::set_permissions(state.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let package = state.path().join("package");
        fs::create_dir(&package).unwrap();
        let executable = package.join("driver");
        fs::copy(env!("CARGO_BIN_EXE_semwright-blender-driver"), &executable).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let digest = |p: &Path| format!("{:x}", Sha256::digest(fs::read(p).unwrap()));
        let runtime_sha = digest(&executable);
        let blender = root.join("blender");
        let interfaces = DriverInterfaces {
            cooperative_cancellation: true,
            progress: true,
            health: true,
            ..Default::default()
        };
        let manifest = Manifest {
            manifest_version: 1,
            protocol: 3,
            transport: Transport::StdioV1,
            id: "blender".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            publisher: "semwright-native-tests".into(),
            sha256: runtime_sha.clone(),
            executable,
            application: ApplicationMatch {
                desktop_id: Some("org.blender.Blender".into()),
                process_names: vec!["blender".into()],
                supported_versions: vec!["4.5.14".into()],
            },
            mounts: vec![
                DriverMount {
                    root: "workspace".into(),
                    read_only: false,
                    execute: false,
                },
                DriverMount {
                    root: "blender-runtime".into(),
                    read_only: true,
                    execute: false,
                },
            ],
            system_config: vec![SystemConfigMount {
                root: "font-config".into(),
                destination: "/etc/fonts".into(),
            }],
            tools: vec![DriverToolMount {
                name: "blender".into(),
                root: "blender-executable".into(),
                sha256: digest(&blender),
            }],
            secrets: vec![],
            network: false,
            loopback_port: None,
            resources: DriverResources {
                open_files: 256,
                processes: 64,
                cpu_seconds: 300,
                operation_cpu_seconds: 0,
                address_space_bytes: 4_294_967_296,
                file_size_bytes: 1_073_741_824,
            },
            request_timeout_ms: 300_000,
            interfaces,
        };
        let grants = vec![
            FilesystemGrant {
                name: "workspace".into(),
                path: fs::canonicalize(workspace).unwrap(),
                read: true,
                write: true,
            },
            FilesystemGrant {
                name: "blender-runtime".into(),
                path: fs::canonicalize(root).unwrap(),
                read: true,
                write: false,
            },
            FilesystemGrant {
                name: "font-config".into(),
                path: fs::canonicalize("/etc/fonts").unwrap(),
                read: true,
                write: false,
            },
            FilesystemGrant {
                name: "blender-executable".into(),
                path: fs::canonicalize(&blender).unwrap(),
                read: true,
                write: false,
            },
        ];
        let helper = PathBuf::from(
            std::env::var("SEMWRIGHT_TEST_SANDBOX_HELPER")
                .expect("native suite requires sandbox helper"),
        );
        let provider = DriverProvider::connect(manifest, state.path(), &helper, &grants, false)
            .await
            .expect("required native driver startup");
        let capabilities = Provider::capabilities(provider.as_ref()).await.unwrap();
        let apply = capabilities
            .iter()
            .find(|capability| capability.descriptor.name == "driver.blender.composition.apply")
            .expect("apply capability");
        let apply_descriptor = semwright_semantic_composition::Digest::parse(
            descriptor_digest(&apply.descriptor).unwrap(),
        )
        .unwrap();
        let runtime_digest = semwright_semantic_composition::Digest::parse(runtime_sha).unwrap();
        let broker = Broker::new(
            Policy::new(PolicyConfig {
                profile: Profile::Workspace,
                allow: if allow {
                    ["driver:blender".into()].into()
                } else {
                    BTreeSet::new()
                },
                ..Default::default()
            })
            .unwrap(),
            vec![],
            Audit::open(&state.path().join("audit"), 65536, 2).unwrap(),
            Arc::new(NoApprover),
            None,
            json!({"native_authoring_test":true}),
            false,
        )
        .unwrap();
        broker.mount_provider(provider.clone()).await.unwrap();
        Self {
            broker,
            provider,
            _state: state,
            session: unique_id(),
            apply_descriptor,
            runtime_digest,
        }
    }
    async fn raw_with_token(
        &self,
        session: &str,
        request_id: &str,
        command: &str,
        args: Value,
        cancellation: CancellationToken,
    ) -> semwright_types::Envelope {
        self.broker
            .clone()
            .execute(
                session.into(),
                request_id.into(),
                ExecuteRequest {
                    command: format!("driver.blender.{command}"),
                    args,
                    dry_run: false,
                    backend: None,
                },
                cancellation,
            )
            .await
    }
    async fn raw_with_request(
        &self,
        session: &str,
        request_id: &str,
        command: &str,
        args: Value,
    ) -> semwright_types::Envelope {
        self.raw_with_token(session, request_id, command, args, CancellationToken::new())
            .await
    }
    async fn raw(&self, session: &str, command: &str, args: Value) -> semwright_types::Envelope {
        self.raw_with_request(session, &unique_id(), command, args)
            .await
    }
    async fn call_with_request(&self, request_id: &str, command: &str, args: Value) -> Value {
        let out = self
            .raw_with_request(&self.session, request_id, command, args)
            .await;
        assert!(out.ok, "{out:?}");
        out.data.expect("result data")
    }
    async fn call(&self, command: &str, args: Value) -> Value {
        let out = self.raw(&self.session, command, args).await;
        assert!(out.ok, "{out:?}");
        out.data.expect("result data")
    }
}

async fn enumerate_domain(
    fixture: &NativeFixture,
    island: &str,
    domain: &str,
) -> (Vec<String>, Option<String>) {
    let mut cursor: Option<String> = None;
    let mut first_cursor = None;
    let mut members = Vec::new();
    let mut expected_total = None;
    loop {
        let mut args = json!({"island":island,"domain":domain,"limit":1});
        if let Some(token) = &cursor {
            args["cursor"] = token.clone().into();
        }
        let page = fixture.call("composition.inspect.page", args).await;
        assert_eq!(page["domain"], domain);
        assert_eq!(page["island"], island);
        assert_eq!(page["exhaustive"], true);
        let total = page["total"].as_u64().unwrap();
        if let Some(previous) = expected_total {
            assert_eq!(previous, total, "{domain} total changed during paging");
        } else {
            expected_total = Some(total);
        }
        for item in page["items"].as_array().unwrap() {
            members.push(item["member"].as_str().unwrap().to_owned());
        }
        cursor = page["next_cursor"].as_str().map(str::to_owned);
        if first_cursor.is_none() {
            first_cursor = cursor.clone();
        }
        if cursor.is_none() {
            assert_eq!(page["final_page"], true);
            break;
        }
        assert_eq!(page["final_page"], false);
    }
    assert_eq!(members.len() as u64, expected_total.unwrap());
    let unique: std::collections::BTreeSet<_> = members.iter().collect();
    assert_eq!(unique.len(), members.len(), "{domain} duplicate members");
    (members, first_cursor)
}

#[tokio::test]
async fn broker_native_authoring_save_reopen_export_and_owner_denial() {
    let Some(root) = native_lane_root() else {
        return;
    };
    let workspace = tempfile::tempdir().unwrap();
    let sentinel = workspace.path().join("external-sentinel.txt");
    fs::write(&sentinel, b"must remain unchanged").unwrap();
    let spec: Value = serde_json::from_str(include_str!(
        "../../../fixtures/blender-authoring/articulated.json"
    ))
    .unwrap();
    let fixture = NativeFixture::start(workspace.path(), &root, true).await;
    let before = fixture.call("composition.inspect", json!({})).await;
    let plan = fixture
        .call(
            "composition.plan",
            json!({"intent":{"kind":"create","spec":spec}}),
        )
        .await;
    let denied = fixture
        .raw(
            "different-authenticated-session",
            "composition.apply",
            json!({"plan_ref":plan["plan_ref"]}),
        )
        .await;
    assert!(!denied.ok);
    assert_eq!(
        before,
        fixture.call("composition.inspect", json!({})).await,
        "plan and denied apply do not author objects"
    );
    let cancelled_token = CancellationToken::new();
    cancelled_token.cancel();
    let cancelled = fixture
        .raw_with_token(
            &fixture.session,
            "native-articulated-pre-cancelled",
            "composition.apply",
            json!({"plan_ref":plan["plan_ref"]}),
            cancelled_token,
        )
        .await;
    assert!(!cancelled.ok, "pre-cancelled apply must fail closed");
    assert_eq!(
        before,
        fixture.call("composition.inspect", json!({})).await,
        "pre-cancelled apply must not author objects or consume the plan"
    );
    let apply_request = "native-articulated-apply";
    let applied = fixture
        .call_with_request(
            apply_request,
            "composition.apply",
            json!({"plan_ref":plan["plan_ref"]}),
        )
        .await;
    assert_eq!(applied["snapshot"]["total"], 3);
    assert_eq!(applied["report"]["execution_status"], "completed");
    let report: semwright_semantic_composition::VerificationReport =
        serde_json::from_value(applied["report"].clone()).unwrap();
    assert_eq!(
        report.verdict().unwrap(),
        semwright_semantic_composition::Verdict::Pass,
        "trusted F adapter must verify native readback and unmanaged preservation"
    );
    let rig_row = applied["snapshot"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["entity"] == "rig")
        .unwrap();
    assert_eq!(rig_row["actions"].as_array().unwrap().len(), 1);
    assert_eq!(rig_row["nla_tracks"].as_array().unwrap().len(), 1);
    assert_eq!(rig_row["nla_tracks"][0]["name"], "SW_NLA_locomotion");
    assert_eq!(
        rig_row["nla_tracks"][0]["strips"][0]["name"],
        "SW_NLA_hinge_cycle"
    );
    assert_eq!(rig_row["nla_tracks"][0]["strips"][0]["repeat"], 2.0);
    assert_eq!(rig_row["nla_tracks"][0]["strips"][0]["scale"], 1.0);
    assert_eq!(rig_row["nla_tracks"][0]["strips"][0]["influence"], 1.0);
    eprintln!("ARTICULATED_STAGE initial_apply_ok");

    let typed_plan: semwright_semantic_composition::PreparedPlan<
        semwright_driver_blender::authoring::AuthoringIntent,
        semwright_driver_blender::authoring::NativeOperation,
    > = serde_json::from_value(plan["plan"].clone()).unwrap();
    let projection = semwright_semantic_composition::Digest::parse(
        applied["snapshot"]["fingerprint"]
            .as_str()
            .unwrap()
            .to_owned(),
    )
    .unwrap();
    let graph_context = semwright_driver_blender::authoring::GraphReceiptContext {
        id: graph::ReceiptId::new(),
        derivation: graph::DerivationId::new(),
        project: graph::ProjectId::new(),
        inputs: vec![],
        outputs: vec![graph::RevisionPin {
            asset: graph::LogicalAssetId::new(),
            revision: graph::AssetRevision::new(),
            fingerprint: graph::Fingerprint {
                bytes: None,
                projection: Some(graph::ProjectionDigest {
                    digest: projection,
                    method: "blender-source-projection-v4".into(),
                    method_version: 1,
                }),
            },
            equivalence: graph::Equivalence::Projection,
        }],
        additional_determinants: vec![],
        descriptor: fixture.apply_descriptor.clone(),
        runtime: fixture.runtime_digest.clone(),
        completed_unix_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64,
        coverage: graph::Coverage::unknown(),
    };
    let receipt = semwright_driver_blender::authoring::graph_receipt_candidate(
        graph_context,
        apply_request,
        &typed_plan,
        report.clone(),
    )
    .unwrap();
    let c_adapter = graph::ReceiptAdapter::registered(
        "driver.blender.composition.apply".into(),
        fixture.apply_descriptor.clone(),
        fixture.runtime_digest.clone(),
    )
    .unwrap();
    let admitted = c_adapter
        .admit(&receipt.owner.clone(), apply_request, receipt)
        .unwrap();
    assert_eq!(
        admitted.record().verification.verdict().unwrap(),
        semwright_semantic_composition::Verdict::Pass
    );
    assert!(!admitted.record().coverage.cache_safe());
    let c_receipt_digest = semwright_semantic_composition::canonical_digest(admitted.record())
        .unwrap()
        .as_str()
        .to_owned();

    let replay = fixture
        .raw(
            &fixture.session,
            "composition.apply",
            json!({"plan_ref":plan["plan_ref"]}),
        )
        .await;
    assert!(!replay.ok);
    let island = applied["island"].as_str().unwrap();
    let measured = fixture
        .call(
            "composition.measure",
            json!({"island":island,"evaluated":true}),
        )
        .await;
    assert_eq!(measured["total"], 3);
    assert_eq!(measured["coverage"], "single_frame");

    // Sample selected native animation times through A's media-time Rate. The driver
    // evaluates Blender at the requested frame, reports exact rational time, and
    // restores frame/subframe before returning so observation does not create drift.
    let sample0 = fixture
        .call(
            "composition.measure",
            json!({
                "island":island,
                "evaluated":true,
                "sample":{"frame":0,"rate":{"num":24,"den":1}}
            }),
        )
        .await;
    let sample12 = fixture
        .call(
            "composition.measure",
            json!({
                "island":island,
                "evaluated":true,
                "sample":{"frame":12,"rate":{"num":24,"den":1}}
            }),
        )
        .await;
    assert_eq!(sample0["sample_time"]["domain"], "media-time");
    assert_eq!(sample0["sample_time"]["frame"], 0);
    assert_eq!(sample0["sample_time"]["time"], json!({"num":"0","den":"1"}));
    assert_eq!(sample12["sample_time"]["frame"], 12);
    assert_eq!(
        sample12["sample_time"]["time"],
        json!({"num":"1","den":"2"})
    );
    let hinge_matrix = |sample: &Value| {
        sample["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["entity"] == "rig")
            .unwrap()["pose_bones"]
            .as_array()
            .unwrap()
            .iter()
            .find(|bone| bone["id"] == "hinge")
            .unwrap()["matrix"]
            .clone()
    };
    assert_ne!(
        hinge_matrix(&sample0),
        hinge_matrix(&sample12),
        "sampled articulated pose must change at frame 12"
    );
    let arm0 = sample0["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["entity"] == "arm")
        .unwrap();
    let arm12 = sample12["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["entity"] == "arm")
        .unwrap();
    assert_ne!(
        arm0["matrix_world"], arm12["matrix_world"],
        "bone-parented arm must have different evaluated world transform"
    );
    let visible0 = arm0["visible_viewport"]
        .as_bool()
        .expect("sampled viewport visibility must be observable");
    let visible12 = arm12["visible_viewport"]
        .as_bool()
        .expect("sampled viewport visibility must be observable");
    assert_eq!(
        visible0, visible12,
        "sampling animation must not change viewport visibility state"
    );
    assert_eq!(arm0["hide_render"], false);
    assert_eq!(arm12["hide_render"], false);
    let wrong_rate = fixture
        .raw(
            &fixture.session,
            "composition.measure",
            json!({
                "island":island,
                "evaluated":true,
                "sample":{"frame":12,"rate":{"num":30,"den":1}}
            }),
        )
        .await;
    assert!(!wrong_rate.ok, "sample rate mismatch must fail closed");
    let after_samples = fixture
        .call("composition.inspect", json!({"island":island}))
        .await;
    assert_eq!(
        after_samples["drift"], false,
        "sampled measurement must restore Blender frame state"
    );

    // Provider-owned pagination is bound to the native session + source fingerprint.
    let mut replay_cursor = None;
    for domain in [
        "objects",
        "bones",
        "actions",
        "curves",
        "keyframes",
        "properties",
    ] {
        let (members, first) = enumerate_domain(&fixture, island, domain).await;
        assert!(
            !members.is_empty(),
            "{domain} must have native evidence in articulated fixture"
        );
        if domain == "objects" {
            replay_cursor = first;
        }
    }
    if let Some(cursor) = replay_cursor {
        let replay = fixture
            .raw(
                &fixture.session,
                "composition.inspect.page",
                json!({
                    "island":island,"domain":"objects","limit":1,"cursor":cursor
                }),
            )
            .await;
        assert!(!replay.ok, "consumed provider cursor must fail closed");
    }
    let first = fixture
        .call(
            "composition.inspect.page",
            json!({
                "island":island,"domain":"properties","limit":1
            }),
        )
        .await;
    let stale_cursor = first["next_cursor"].as_str().unwrap().to_owned();
    let body_for_page = fixture
        .call("composition.inspect", json!({"island":island}))
        .await["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["entity"] == "body")
        .unwrap()["name"]
        .as_str()
        .unwrap()
        .to_owned();
    fixture
        .call(
            "object.transform",
            json!({"name":body_for_page,"location":[0.01,0.0,0.5]}),
        )
        .await;
    let stale = fixture
        .raw(
            &fixture.session,
            "composition.inspect.page",
            json!({
                "island":island,"domain":"properties","limit":1,"cursor":stale_cursor
            }),
        )
        .await;
    assert!(
        !stale.ok,
        "cursor must become stale after another authorized writer"
    );
    // Restore the articulated source state explicitly before the transform/repair scenario.
    fixture
        .call(
            "object.transform",
            json!({"name":body_for_page,"location":[0.0,0.0,0.5]}),
        )
        .await;
    eprintln!("ARTICULATED_STAGE pagination_ok");

    // Repair is explicit and transform-only. A second authorized writer creates real drift;
    // verification must fail before the parent-bound repair is planned and applied.
    let stable = fixture
        .call("composition.inspect", json!({"island":island}))
        .await;
    let body = stable["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["entity"] == "body")
        .unwrap();
    let body_name = body["name"].as_str().unwrap().to_owned();
    let transform_plan=fixture.call("composition.plan",json!({"intent":{
        "kind":"transform","island":island,"entity":"body",
        "transform":{"translation":[0.2,0.0,0.5],"rotation":[0.0,0.0,0.0],"scale":[1.0,1.0,1.0]},
        "meters_per_unit":1.0,"expected_fingerprint":stable["fingerprint"]
    }})).await;
    let transformed = fixture
        .call(
            "composition.apply",
            json!({"plan_ref":transform_plan["plan_ref"]}),
        )
        .await;
    let transformed_report: semwright_semantic_composition::VerificationReport =
        serde_json::from_value(transformed["report"].clone()).unwrap();
    assert_eq!(
        transformed_report.verdict().unwrap(),
        semwright_semantic_composition::Verdict::Pass
    );
    fixture
        .call(
            "object.transform",
            json!({"name":body_name,"location":[9.0,0.0,0.5]}),
        )
        .await;
    let drifted = fixture
        .call("composition.inspect", json!({"island":island}))
        .await;
    assert_eq!(drifted["drift"], true);
    let failed = fixture
        .call(
            "composition.verify",
            json!({"plan_ref":transform_plan["plan_ref"]}),
        )
        .await;
    let failed_report: semwright_semantic_composition::VerificationReport =
        serde_json::from_value(failed).unwrap();
    assert_eq!(
        failed_report.verdict().unwrap(),
        semwright_semantic_composition::Verdict::Fail
    );
    let repair = fixture
        .call(
            "composition.repair.plan",
            json!({"parent_plan_ref":transform_plan["plan_ref"]}),
        )
        .await;
    assert!(
        !fixture
            .raw(
                &fixture.session,
                "composition.apply",
                json!({"plan_ref":repair["plan_ref"]})
            )
            .await
            .ok,
        "repair child cannot use root apply capability"
    );
    let repaired = fixture
        .call(
            "composition.repair.apply",
            json!({"plan_ref":repair["plan_ref"]}),
        )
        .await;
    let repaired_report: semwright_semantic_composition::VerificationReport =
        serde_json::from_value(repaired["report"].clone()).unwrap();
    assert_eq!(
        repaired_report.verdict().unwrap(),
        semwright_semantic_composition::Verdict::Pass
    );
    assert_eq!(
        fixture
            .call("composition.inspect", json!({"island":island}))
            .await["drift"],
        false
    );
    eprintln!("ARTICULATED_STAGE repair_ok");

    // Export uses the pre-existing GLB capability, never a second E exporter.
    let scene = fixture
        .call("composition.inspect", json!({"island":island}))
        .await;
    assert_eq!(scene["drift"], false, "pre-export source must be stable");
    let collection = scene["items"][0]["collections"][0].as_str().unwrap();
    let export = fixture
        .call(
            "export.glb",
            json!({"collection":collection,"path":"articulated.glb","animations":true}),
        )
        .await;
    eprintln!("ARTICULATED_STAGE export_call_ok");
    let bytes = fs::read(workspace.path().join("articulated.glb")).unwrap();
    assert_eq!(&bytes[0..4], b"glTF");
    assert_eq!(export["sha256"], format!("{:x}", Sha256::digest(&bytes)));
    let after_export = fixture
        .call("composition.inspect", json!({"island":island}))
        .await;
    if after_export["drift"] != false {
        eprintln!(
            "ARTICULATED_EXPORT_BEFORE={}\nARTICULATED_EXPORT_AFTER={}",
            serde_json::to_string_pretty(&scene).unwrap(),
            serde_json::to_string_pretty(&after_export).unwrap()
        );
    }
    assert_eq!(
        after_export["drift"], false,
        "GLB export must restore all observable source/context state without invalidating managed source"
    );
    eprintln!("ARTICULATED_STAGE after_export_ok");
    let evidence =
        PathBuf::from(std::env::var("SEMWRIGHT_AUTHORING_EVIDENCE").expect("evidence path"));
    fs::create_dir_all(&evidence).unwrap();
    fs::write(evidence.join("articulated.glb"), &bytes).unwrap();
    let saved = fixture
        .call(
            "composition.persist",
            json!({"island":island,"path":"articulated.blend"}),
        )
        .await;
    eprintln!("ARTICULATED_STAGE persist_ok");
    assert!(
        !fixture
            .raw(
                &fixture.session,
                "composition.persist",
                json!({"island":island,"path":"articulated.blend"})
            )
            .await
            .ok
    );
    let writer = applied["snapshot"]["native_session"].clone();
    fixture.provider.shutdown().await.unwrap();
    drop(fixture);
    let fresh = NativeFixture::start(workspace.path(), &root, true).await;
    let reopened = fresh
        .call(
            "composition.reopen",
            json!({"island":island,"path":"articulated.blend","sha256":saved["sha256"]}),
        )
        .await;
    assert_ne!(
        writer, reopened["native_session"],
        "must be a fresh native process"
    );
    eprintln!("ARTICULATED_STAGE reopen_ok");
    assert_eq!(reopened["total"], 3);
    let native: semwright_driver_blender::authoring::NativeSnapshot =
        serde_json::from_value(reopened.clone()).unwrap();
    assert_eq!(
        native.fingerprint.as_str(),
        saved["source_fingerprint"].as_str().unwrap(),
        "fresh-process source projection must equal the exact pre-save incremental state"
    );
    assert!(!native.drift);
    assert_eq!(fs::read(sentinel).unwrap(), b"must remain unchanged");
    fresh.provider.shutdown().await.unwrap();
    fs::write(
        evidence.join("native-pipeline.json"),
        serde_json::to_vec_pretty(&json!({
            "version":1,
            "route":"Broker -> policy -> Driver Host -> Blender",
            "writer_process":writer,
            "reader_process":reopened["native_session"],
            "glb":export,
            "blend":saved,
            "c_authoring_receipt_digest":c_receipt_digest,
            "native_authoring_c_f_verified":true,
            "native_assertions_completed":true,
            "godot_reimport_verified":false,
            "cross_app_c_receipt_verified":false,
            "ready":false
        }))
        .unwrap(),
    )
    .unwrap();
}

#[tokio::test]
async fn hard_surface_and_product_scene_author_through_semwright() {
    let Some(root) = native_lane_root() else {
        return;
    };
    let workspace = tempfile::tempdir().unwrap();
    fs::create_dir_all(workspace.path().join("textures")).unwrap();
    fs::write(
        workspace.path().join("textures/surface.png"),
        include_bytes!("../../../fixtures/blender-authoring/surface.png"),
    )
    .unwrap();
    let fixture = NativeFixture::start(workspace.path(), &root, true).await;
    let mut hard_surface_saved: Option<Value> = None;
    let mut hard_surface_island: Option<String> = None;
    for (label, source) in [
        (
            "hard_surface",
            include_str!("../../../fixtures/blender-authoring/hard_surface.json"),
        ),
        (
            "product_scene",
            include_str!("../../../fixtures/blender-authoring/product_scene.json"),
        ),
    ] {
        let spec: Value = serde_json::from_str(source).unwrap();
        let expected = spec["entities"].as_array().unwrap().len() as u64;
        let plan = fixture
            .call(
                "composition.plan",
                json!({"intent":{"kind":"create","spec":spec}}),
            )
            .await;
        let applied = fixture
            .call("composition.apply", json!({"plan_ref":plan["plan_ref"]}))
            .await;
        assert_eq!(
            applied["snapshot"]["total"].as_u64(),
            Some(expected),
            "{label}"
        );
        let report: semwright_semantic_composition::VerificationReport =
            serde_json::from_value(applied["report"].clone()).unwrap();
        if report.verdict().unwrap() != semwright_semantic_composition::Verdict::Pass {
            eprintln!(
                "{label} verification_report={}\n{label} native_snapshot={}",
                serde_json::to_string_pretty(&applied["report"]).unwrap(),
                serde_json::to_string_pretty(&applied["snapshot"]).unwrap()
            );
        }
        assert_eq!(
            report.verdict().unwrap(),
            semwright_semantic_composition::Verdict::Pass,
            "{label} native/F verification"
        );
        let island = applied["island"].as_str().unwrap();
        let measured = fixture
            .call(
                "composition.measure",
                json!({"island":island,"evaluated":true}),
            )
            .await;
        assert_eq!(
            measured["total"].as_u64(),
            Some(expected),
            "{label} evaluated count"
        );
        assert_eq!(measured["coverage"], "single_frame");
        let items = applied["snapshot"]["items"].as_array().unwrap();
        if label == "hard_surface" {
            let source = items.iter().find(|row| row["entity"] == "housing").unwrap();
            let copy = items
                .iter()
                .find(|row| row["entity"] == "housing_copy")
                .unwrap();
            assert_ne!(
                source["data_name"], copy["data_name"],
                "mesh_copy must isolate the datablock"
            );
            assert_eq!(copy["data_users"], 1);
            assert_eq!(source["materials"][0]["id"], "housing");
            assert_eq!(copy["materials"][0]["id"], "insert");
            let cutter = items.iter().find(|row| row["entity"] == "cutter").unwrap();
            assert_eq!(cutter["type"], "MESH");
            assert_eq!(cutter["smooth_polygons"], cutter["polygons"]);
            assert_eq!(cutter["normal_method"], "source-polygon-normal-v1");
            assert_eq!(cutter["normal_digest"].as_str().map(str::len), Some(64));
            let cutter_attributes = cutter["attributes"].as_array().unwrap();
            assert_eq!(cutter_attributes.len(), 3);
            for (id, domain, data_type, count) in [
                ("wear", "POINT", "FLOAT", 8usize),
                ("flow", "POINT", "FLOAT_VECTOR", 8usize),
                ("panel", "FACE", "FLOAT_COLOR", 6usize),
            ] {
                let attribute = cutter_attributes
                    .iter()
                    .find(|attribute| attribute["id"] == id)
                    .unwrap();
                assert_eq!(attribute["domain"], domain);
                assert_eq!(attribute["data_type"], data_type);
                assert_eq!(attribute["values"].as_array().unwrap().len(), count);
            }
            let measured_cutter = measured["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["entity"] == "cutter")
                .unwrap();
            assert_eq!(measured_cutter["method"], "evaluated-depsgraph");
            assert_eq!(
                measured_cutter["normal_method"],
                "evaluated-polygon-normal-v1"
            );
            assert_eq!(
                measured_cutter["attributes"].as_array().unwrap().len(),
                3,
                "evaluated measurement must preserve managed attributes"
            );
            assert!(
                source["modifiers"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|modifier| {
                        modifier["type"] == "BOOLEAN"
                            && modifier["target"] == "cutter"
                            && modifier["operation"] == "DIFFERENCE"
                            && modifier["solver"] == "EXACT"
                    })
            );
            let cable = items.iter().find(|row| row["entity"] == "cable").unwrap();
            assert_eq!(cable["type"], "CURVE");
            assert_eq!(cable["curve"]["bevel_resolution"], 2);
            assert_eq!(cable["materials"][0]["id"], "insert");
            assert!((cable["materials"][0]["opacity"].as_f64().unwrap() - 0.82).abs() < 1e-5);
            assert!(
                (cable["materials"][0]["emission_strength"].as_f64().unwrap() - 0.35).abs() < 1e-5
            );
            let bindings = cable["materials"][0]["texture_bindings"]
                .as_array()
                .unwrap();
            let expected_sha = format!(
                "{:x}",
                Sha256::digest(include_bytes!(
                    "../../../fixtures/blender-authoring/surface.png"
                ))
            );
            for role in ["base_color", "roughness", "normal", "emission", "opacity"] {
                let binding = bindings
                    .iter()
                    .find(|binding| binding["role"] == role)
                    .unwrap();
                assert_eq!(binding["topology_valid"], true, "{role}");
                assert_eq!(binding["sha256"], expected_sha, "{role}");
            }
            assert_eq!(
                bindings
                    .iter()
                    .find(|binding| binding["role"] == "base_color")
                    .unwrap()["colorspace"],
                "sRGB"
            );
            assert_eq!(
                bindings
                    .iter()
                    .find(|binding| binding["role"] == "normal")
                    .unwrap()["colorspace"],
                "Non-Color"
            );

            let before_material_update = fixture
                .call("composition.inspect", json!({"island":island}))
                .await;
            let material_plan = fixture
                .call(
                    "composition.plan",
                    json!({"intent":{
                        "kind":"material_slots",
                        "island":island,
                        "entity":"insert",
                        "materials":["housing"],
                        "expected_fingerprint":before_material_update["fingerprint"]
                    }}),
                )
                .await;
            let material_applied = fixture
                .call(
                    "composition.apply",
                    json!({"plan_ref":material_plan["plan_ref"]}),
                )
                .await;
            let material_report: semwright_semantic_composition::VerificationReport =
                serde_json::from_value(material_applied["report"].clone()).unwrap();
            assert_eq!(
                material_report.verdict().unwrap(),
                semwright_semantic_composition::Verdict::Pass
            );
            let updated_insert = material_applied["snapshot"]["items"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["entity"] == "insert")
                .unwrap();
            assert_eq!(updated_insert["materials"][0]["id"], "housing");

            hard_surface_saved = Some(
                fixture
                    .call(
                        "composition.persist",
                        json!({"island":island,"path":"hard-surface.blend"}),
                    )
                    .await,
            );
            hard_surface_island = Some(island.to_owned());
        } else {
            let source = items.iter().find(|row| row["entity"] == "product").unwrap();
            let instance = items
                .iter()
                .find(|row| row["entity"] == "instance")
                .unwrap();
            assert_eq!(
                source["data_name"], instance["data_name"],
                "mesh_instance must stay shared"
            );
            assert_eq!(source["data_users"], 2);
            let align = instance["constraints"]
                .as_array()
                .unwrap()
                .iter()
                .find(|constraint| {
                    constraint["type"] == "COPY_LOCATION" && constraint["target"] == "product"
                })
                .expect("axis-selective align constraint");
            assert_eq!(align["axes"], json!([false, true, true]));
            assert_eq!(align["offset"], false);
            assert_eq!(align["owner_space"], "WORLD");
            assert_eq!(align["target_space"], "WORLD");

            let pair_measurement = fixture
                .call(
                    "composition.measure",
                    json!({
                        "island":island,
                        "evaluated":true,
                        "pairs":[["product","instance"]]
                    }),
                )
                .await;
            let pair = &pair_measurement["pair_measurements"]["pairs"][0];
            assert_eq!(pair["a"], "product");
            assert_eq!(pair["b"], "instance");
            assert!((pair["origin_distance_m"].as_f64().unwrap() - 1.0).abs() < 1e-5);
            assert!((pair["aabb_clearance_m"].as_f64().unwrap() - 0.3).abs() < 1e-5);
            assert_eq!(pair["aabb_overlap"], false);
            assert_eq!(pair["contact_evidence"], "AABB_ONLY");
            let center_offset = pair["center_offset_m"].as_array().unwrap();
            assert!((center_offset[0].as_f64().unwrap() - 1.0).abs() < 1e-5);
            assert!(center_offset[1].as_f64().unwrap().abs() < 1e-5);
            assert!(center_offset[2].as_f64().unwrap().abs() < 1e-5);

            let shared_snapshot = fixture
                .call("composition.inspect", json!({"island":island}))
                .await;
            let denied_shared_material = fixture
                .raw(
                    &fixture.session,
                    "composition.plan",
                    json!({"intent":{
                        "kind":"material_slots",
                        "island":island,
                        "entity":"instance",
                        "materials":["ceramic"],
                        "expected_fingerprint":shared_snapshot["fingerprint"]
                    }}),
                )
                .await;
            assert!(
                !denied_shared_material.ok,
                "shared mesh material plan must fail before apply"
            );

            // Product-scene preview reuses the public Blender capabilities through the
            // same Broker/Driver Host path. The harness only verifies the resulting PNG.
            let camera_name = items
                .iter()
                .find(|row| row["entity"] == "camera")
                .and_then(|row| row["name"].as_str())
                .unwrap()
                .to_owned();
            let scenes = fixture
                .call("semantic.objects", json!({"root":"scenes","limit":8}))
                .await;
            let scene_ref = scenes["items"]
                .as_array()
                .and_then(|rows| rows.first())
                .and_then(|row| row["ref"].as_str())
                .unwrap()
                .to_owned();
            let cameras = fixture
                .call(
                    "semantic.objects",
                    json!({"root":"objects","query":camera_name,"limit":8}),
                )
                .await;
            let camera_ref = cameras["items"]
                .as_array()
                .and_then(|rows| {
                    rows.iter()
                        .find(|row| row["name"].as_str() == Some(camera_name.as_str()))
                })
                .and_then(|row| row["ref"].as_str())
                .unwrap()
                .to_owned();
            fixture
                .call(
                    "semantic.relation.set",
                    json!({"ref":scene_ref,"property":"camera","target_ref":camera_ref}),
                )
                .await;
            let world = fixture
                .call(
                    "semantic.datablock.create",
                    json!({"root":"worlds","name":"SemwrightProductWorld"}),
                )
                .await;
            let world = fixture
                .call(
                    "semantic.property.set",
                    json!({
                        "ref":world["ref"],
                        "property":"color",
                        "value":[0.03,0.03,0.03]
                    }),
                )
                .await;
            // Every semantic mutation rotates the generation. Re-resolve the Scene
            // rather than reusing a deliberately stale ref from the camera mutation.
            let scenes = fixture
                .call("semantic.objects", json!({"root":"scenes","limit":8}))
                .await;
            let scene_ref = scenes["items"]
                .as_array()
                .and_then(|rows| rows.first())
                .and_then(|row| row["ref"].as_str())
                .unwrap()
                .to_owned();
            let world_relation = fixture
                .call(
                    "semantic.relation.set",
                    json!({"ref":scene_ref,"property":"world","target_ref":world["ref"]}),
                )
                .await;
            let world_color = fixture
                .call(
                    "semantic.property.get",
                    json!({"ref":world_relation["target_ref"],"property":"color"}),
                )
                .await;
            let world_color = world_color["value"].as_array().unwrap();
            assert_eq!(world_color.len(), 3);
            assert!(
                world_color
                    .iter()
                    .all(|value| (value.as_f64().unwrap() - 0.03).abs() < 1e-5)
            );
            fixture
                .call(
                    "render.settings",
                    json!({"width":64,"height":64,"samples":1,"engine":"CYCLES"}),
                )
                .await;
            let rendered = fixture
                .call("render", json!({"path":"product-preview.png"}))
                .await;
            assert_eq!(rendered["format"], "png");
            let preview = fs::read(workspace.path().join("product-preview.png")).unwrap();
            assert_eq!(&preview[..8], b"\x89PNG\r\n\x1a\n");
            let scene = fixture.call("scene.inspect", json!({})).await;
            assert_eq!(scene["camera"].as_str(), Some(camera_name.as_str()));
            assert_eq!(scene["resolution"], json!([64, 64]));
            if let Ok(evidence) = std::env::var("SEMWRIGHT_AUTHORING_EVIDENCE") {
                fs::create_dir_all(&evidence).unwrap();
                fs::write(
                    PathBuf::from(evidence).join("product-preview.png"),
                    &preview,
                )
                .unwrap();
            }
        }
    }
    fixture.provider.shutdown().await.unwrap();
    drop(fixture);

    let saved = hard_surface_saved.expect("hard-surface persistence receipt");
    let island = hard_surface_island.expect("hard-surface island identity");
    let fresh = NativeFixture::start(workspace.path(), &root, true).await;
    let reopened = fresh
        .call(
            "composition.reopen",
            json!({
                "island":island,
                "path":"hard-surface.blend",
                "sha256":saved["sha256"]
            }),
        )
        .await;
    let expected_spec: Value = serde_json::from_str(include_str!(
        "../../../fixtures/blender-authoring/hard_surface.json"
    ))
    .unwrap();
    assert_eq!(
        reopened["total"].as_u64(),
        Some(expected_spec["entities"].as_array().unwrap().len() as u64)
    );
    assert_eq!(reopened["drift"], false);
    let items = reopened["items"].as_array().unwrap();
    let source = items.iter().find(|row| row["entity"] == "housing").unwrap();
    let copy = items
        .iter()
        .find(|row| row["entity"] == "housing_copy")
        .unwrap();
    assert_ne!(
        source["data_name"], copy["data_name"],
        "copy-on-write mesh identity must survive fresh-process persistence"
    );
    let cutter = items.iter().find(|row| row["entity"] == "cutter").unwrap();
    assert_eq!(cutter["smooth_polygons"], cutter["polygons"]);
    assert_eq!(cutter["normal_method"], "source-polygon-normal-v1");
    assert_eq!(
        cutter["attributes"].as_array().map(Vec::len),
        Some(3),
        "managed attributes must survive fresh-process save/reopen"
    );
    let cable = items.iter().find(|row| row["entity"] == "cable").unwrap();
    let expected_sha = format!(
        "{:x}",
        Sha256::digest(include_bytes!(
            "../../../fixtures/blender-authoring/surface.png"
        ))
    );
    for binding in cable["materials"][0]["texture_bindings"]
        .as_array()
        .unwrap()
    {
        assert_eq!(
            binding["sha256"], expected_sha,
            "fresh process must re-resolve and hash the declared texture"
        );
        assert_eq!(binding["topology_valid"], true);
    }
    fresh.provider.shutdown().await.unwrap();
}

#[tokio::test]
async fn aabb_overlap_requires_narrow_phase_before_collision_claim() {
    let Some(root) = native_lane_root() else {
        return;
    };
    let workspace = tempfile::tempdir().unwrap();
    let fixture = NativeFixture::start(workspace.path(), &root, true).await;
    let spec: Value = serde_json::from_str(include_str!(
        "../../../fixtures/blender-authoring/collision_false_positive.json"
    ))
    .unwrap();
    let plan = fixture
        .call(
            "composition.plan",
            json!({"intent":{"kind":"create","spec":spec}}),
        )
        .await;
    let applied = fixture
        .call("composition.apply", json!({"plan_ref":plan["plan_ref"]}))
        .await;
    let island = applied["island"].as_str().unwrap();
    let first = fixture
        .call(
            "composition.measure",
            json!({"island":island,"evaluated":true}),
        )
        .await;
    let pair = &first["mesh_pair_intersections"]["pairs"][0];
    assert_eq!(pair["broad_phase_aabb"], true);
    assert_eq!(pair["narrow_phase"], "SEPARATE");
    assert_eq!(pair["verdict"], "SEPARATE");
    assert_eq!(first["mesh_pair_intersections"]["complete"], true);
    for item in first["items"].as_array().unwrap() {
        assert_eq!(item["mesh_self_intersections"]["verdict"], "UNKNOWN");
    }

    let stable = fixture
        .call("composition.inspect", json!({"island":island}))
        .await;
    let move_plan=fixture.call("composition.plan",json!({"intent":{
        "kind":"transform","island":island,"entity":"triangle_b",
        "transform":{"translation":[-1.5,-1.5,0.0],"rotation":[0.0,0.0,0.0],"scale":[1.0,1.0,1.0]},
        "meters_per_unit":1.0,"expected_fingerprint":stable["fingerprint"]
    }})).await;
    let moved = fixture
        .call(
            "composition.apply",
            json!({"plan_ref":move_plan["plan_ref"]}),
        )
        .await;
    let moved_report: semwright_semantic_composition::VerificationReport =
        serde_json::from_value(moved["report"].clone()).unwrap();
    assert_eq!(
        moved_report.verdict().unwrap(),
        semwright_semantic_composition::Verdict::Pass
    );
    let second = fixture
        .call(
            "composition.measure",
            json!({"island":island,"evaluated":true}),
        )
        .await;
    let pair = &second["mesh_pair_intersections"]["pairs"][0];
    assert_eq!(pair["broad_phase_aabb"], true);
    assert_eq!(pair["narrow_phase"], "INTERSECT");
    assert_eq!(pair["verdict"], "INTERSECT");
    assert!(pair["triangle_pairs_tested"].as_u64().unwrap() > 0);
    fixture.provider.shutdown().await.unwrap();
}
