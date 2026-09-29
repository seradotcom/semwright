//! Native acceptance TEST SOURCE. Runs only in the explicit GitHub-hosted native lane.
#![cfg(all(unix, feature = "authoring-native"))]
use semwright_backend_api::Provider;
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest, SystemConfigMount, Transport};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig, Profile};
use semwright_types::{ExecuteRequest, unique_id};
use serde_json::{Value,json};
use sha2::{Digest as _,Sha256};
use std::{collections::BTreeSet,fs,os::unix::fs::PermissionsExt,path::{Path,PathBuf},sync::Arc};
use tokio_util::sync::CancellationToken;

struct NativeFixture {
    broker: Arc<Broker>, provider:Arc<DriverProvider>, _state:tempfile::TempDir, session:String,
}
impl NativeFixture {
    async fn start(workspace:&Path,root:&Path,allow:bool)->Self {
        assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(),Ok("true"),"native suite is remote-only");
        assert_eq!(std::env::var("RUNNER_ENVIRONMENT").as_deref(),Ok("github-hosted"),"self-hosted is not this lane");
        let state=tempfile::tempdir().unwrap();
        fs::set_permissions(state.path(),fs::Permissions::from_mode(0o700)).unwrap();
        let package=state.path().join("package");fs::create_dir(&package).unwrap();
        let executable=package.join("driver");fs::copy(env!("CARGO_BIN_EXE_semwright-blender-driver"),&executable).unwrap();fs::set_permissions(&executable,fs::Permissions::from_mode(0o700)).unwrap();
        let digest=|p:&Path|format!("{:x}",Sha256::digest(fs::read(p).unwrap()));
        let blender=root.join("blender");
        let interfaces=DriverInterfaces{cooperative_cancellation:true,progress:true,health:true,..Default::default()};
        let manifest=Manifest{
            manifest_version:1,protocol:2,transport:Transport::StdioV1,id:"blender".into(),version:env!("CARGO_PKG_VERSION").into(),publisher:"semwright-native-tests".into(),
            sha256:digest(&executable),executable,
            application:ApplicationMatch{desktop_id:Some("org.blender.Blender".into()),process_names:vec!["blender".into()],supported_versions:vec!["4.5.14".into()]},
            mounts:vec![DriverMount{root:"workspace".into(),read_only:false,execute:false},DriverMount{root:"blender-runtime".into(),read_only:true,execute:false}],
            system_config:vec![SystemConfigMount{root:"font-config".into(),destination:"/etc/fonts".into()}],
            tools:vec![DriverToolMount{name:"blender".into(),root:"blender-executable".into(),sha256:digest(&blender)}],
            secrets:vec![],network:false,loopback_port:None,
            resources:DriverResources{open_files:256,processes:64,cpu_seconds:300,operation_cpu_seconds:0,address_space_bytes:4_294_967_296,file_size_bytes:1_073_741_824},
            request_timeout_ms:300_000,interfaces,
        };
        let grants=vec![
            FilesystemGrant{name:"workspace".into(),path:fs::canonicalize(workspace).unwrap(),read:true,write:true},
            FilesystemGrant{name:"blender-runtime".into(),path:fs::canonicalize(root).unwrap(),read:true,write:false},
            FilesystemGrant{name:"font-config".into(),path:fs::canonicalize("/etc/fonts").unwrap(),read:true,write:false},
            FilesystemGrant{name:"blender-executable".into(),path:fs::canonicalize(&blender).unwrap(),read:true,write:false},
        ];
        let helper=PathBuf::from(std::env::var("SEMWRIGHT_TEST_SANDBOX_HELPER").expect("native suite requires sandbox helper"));
        let provider=DriverProvider::connect(manifest,state.path(),&helper,&grants,false).await.expect("required native driver startup");
        let broker=Broker::new(Policy::new(PolicyConfig{profile:Profile::Workspace,allow:if allow {["driver:blender".into()].into()}else{BTreeSet::new()},..Default::default()}).unwrap(),vec![],Audit::open(&state.path().join("audit"),65536,2).unwrap(),Arc::new(NoApprover),None,json!({"native_authoring_test":true}),false).unwrap();
        broker.mount_provider(provider.clone()).await.unwrap();
        Self{broker,provider,_state:state,session:unique_id()}
    }
    async fn raw(&self,session:&str,command:&str,args:Value)->semwright_types::Envelope {
        self.broker.clone().execute(session.into(),unique_id(),ExecuteRequest{command:format!("driver.blender.{command}"),args,dry_run:false,backend:None},CancellationToken::new()).await
    }
    async fn call(&self,command:&str,args:Value)->Value {
        let out=self.raw(&self.session,command,args).await;assert!(out.ok,"{out:?}");out.data.expect("result data")
    }
}
#[tokio::test]
async fn broker_native_authoring_save_reopen_export_and_owner_denial() {
    let root=PathBuf::from(std::env::var("SEMWRIGHT_TEST_BLENDER_ROOT").expect("required Blender runtime"));
    let workspace=tempfile::tempdir().unwrap();let sentinel=workspace.path().join("external-sentinel.txt");fs::write(&sentinel,b"must remain unchanged").unwrap();
    let spec:Value=serde_json::from_str(include_str!("../../../fixtures/blender-authoring/articulated.json")).unwrap();
    let fixture=NativeFixture::start(workspace.path(),&root,true).await;
    let before=fixture.call("composition.inspect",json!({})).await;
    let plan=fixture.call("composition.plan",json!({"intent":{"kind":"create","spec":spec}})).await;
    let denied=fixture.raw("different-authenticated-session","composition.apply",json!({"plan_ref":plan["plan_ref"]})).await;assert!(!denied.ok);
    assert_eq!(before,fixture.call("composition.inspect",json!({})).await,"plan and denied apply do not author objects");
    let applied=fixture.call("composition.apply",json!({"plan_ref":plan["plan_ref"]})).await;
    assert_eq!(applied["snapshot"]["total"],3);assert_eq!(applied["report"]["execution_status"],"completed");
    let report:semwright_semantic_composition::VerificationReport=serde_json::from_value(applied["report"].clone()).unwrap();
    assert_eq!(report.verdict().unwrap(),semwright_semantic_composition::Verdict::Pass,"trusted F adapter must verify native readback and unmanaged preservation");
    let replay=fixture.raw(&fixture.session,"composition.apply",json!({"plan_ref":plan["plan_ref"]})).await;assert!(!replay.ok);
    let island=applied["island"].as_str().unwrap();
    let measured=fixture.call("composition.measure",json!({"island":island,"evaluated":true})).await;assert_eq!(measured["total"],3);assert_eq!(measured["coverage"],"single_frame");

    // Repair is explicit and transform-only. A second authorized writer creates real drift;
    // verification must fail before the parent-bound repair is planned and applied.
    let stable=fixture.call("composition.inspect",json!({"island":island})).await;
    let body=stable["items"].as_array().unwrap().iter().find(|row| row["entity"]=="body").unwrap();
    let body_name=body["name"].as_str().unwrap().to_owned();
    let transform_plan=fixture.call("composition.plan",json!({"intent":{
        "kind":"transform","island":island,"entity":"body",
        "transform":{"translation":[0.2,0.0,0.5],"rotation":[0.0,0.0,0.0],"scale":[1.0,1.0,1.0]},
        "meters_per_unit":1.0,"expected_fingerprint":stable["fingerprint"]
    }})).await;
    let transformed=fixture.call("composition.apply",json!({"plan_ref":transform_plan["plan_ref"]})).await;
    let transformed_report:semwright_semantic_composition::VerificationReport=serde_json::from_value(transformed["report"].clone()).unwrap();
    assert_eq!(transformed_report.verdict().unwrap(),semwright_semantic_composition::Verdict::Pass);
    fixture.call("object.transform",json!({"name":body_name,"location":[9.0,0.0,0.5]})).await;
    let drifted=fixture.call("composition.inspect",json!({"island":island})).await;
    assert_eq!(drifted["drift"],true);
    let failed=fixture.call("composition.verify",json!({"plan_ref":transform_plan["plan_ref"]})).await;
    let failed_report:semwright_semantic_composition::VerificationReport=serde_json::from_value(failed).unwrap();
    assert_eq!(failed_report.verdict().unwrap(),semwright_semantic_composition::Verdict::Fail);
    let repair=fixture.call("composition.repair.plan",json!({"parent_plan_ref":transform_plan["plan_ref"]})).await;
    assert!(!fixture.raw(&fixture.session,"composition.apply",json!({"plan_ref":repair["plan_ref"]})).await.ok,
        "repair child cannot use root apply capability");
    let repaired=fixture.call("composition.repair.apply",json!({"plan_ref":repair["plan_ref"]})).await;
    let repaired_report:semwright_semantic_composition::VerificationReport=serde_json::from_value(repaired["report"].clone()).unwrap();
    assert_eq!(repaired_report.verdict().unwrap(),semwright_semantic_composition::Verdict::Pass);
    assert_eq!(fixture.call("composition.inspect",json!({"island":island})).await["drift"],false);

    // Export uses the pre-existing GLB capability, never a second E exporter.
    let scene=fixture.call("composition.inspect",json!({"island":island})).await;
    let collection=scene["items"][0]["collections"][0].as_str().unwrap();
    let export=fixture.call("export.glb",json!({"collection":collection,"path":"articulated.glb","animations":true})).await;
    let bytes=fs::read(workspace.path().join("articulated.glb")).unwrap();assert_eq!(&bytes[0..4],b"glTF");assert_eq!(export["sha256"],format!("{:x}",Sha256::digest(&bytes)));
    let saved=fixture.call("composition.persist",json!({"island":island,"path":"articulated.blend"})).await;
    assert!(!fixture.raw(&fixture.session,"composition.persist",json!({"island":island,"path":"articulated.blend"})).await.ok);
    let writer=applied["snapshot"]["native_session"].clone();
    fixture.provider.shutdown().await.unwrap();drop(fixture);
    let fresh=NativeFixture::start(workspace.path(),&root,true).await;
    let reopened=fresh.call("composition.reopen",json!({"island":island,"path":"articulated.blend","sha256":saved["sha256"]})).await;
    assert_ne!(writer,reopened["native_session"],"must be a fresh native process");assert_eq!(reopened["total"],3);
    let native:semwright_driver_blender::authoring::NativeSnapshot=serde_json::from_value(reopened.clone()).unwrap();
    let intent:semwright_driver_blender::authoring::AuthoringIntent=serde_json::from_value(json!({"kind":"create","spec":spec})).unwrap();
    assert!(semwright_driver_blender::authoring::native_matches(&intent,&native));
    assert_eq!(fs::read(sentinel).unwrap(),b"must remain unchanged");
    fresh.provider.shutdown().await.unwrap();
    let evidence=PathBuf::from(std::env::var("SEMWRIGHT_AUTHORING_EVIDENCE").expect("evidence path"));
    fs::create_dir_all(&evidence).unwrap();
    fs::write(evidence.join("native-pipeline.json"),serde_json::to_vec_pretty(&json!({"version":1,"route":"Broker -> policy -> Driver Host -> Blender","writer_process":writer,"reader_process":reopened["native_session"],"glb":export,"blend":saved,"native_assertions_completed":true,"godot_reimport_verified":false,"cf_consumers_verified":false,"ready":false})).unwrap()).unwrap();
}

#[tokio::test]
async fn hard_surface_and_product_scene_author_through_semwright() {
    let root=PathBuf::from(std::env::var("SEMWRIGHT_TEST_BLENDER_ROOT").expect("required Blender runtime"));
    let workspace=tempfile::tempdir().unwrap();
    let fixture=NativeFixture::start(workspace.path(),&root,true).await;
    for (label,source) in [
        ("hard_surface",include_str!("../../../fixtures/blender-authoring/hard_surface.json")),
        ("product_scene",include_str!("../../../fixtures/blender-authoring/product_scene.json")),
    ] {
        let spec:Value=serde_json::from_str(source).unwrap();
        let expected=spec["entities"].as_array().unwrap().len() as u64;
        let plan=fixture.call("composition.plan",json!({"intent":{"kind":"create","spec":spec}})).await;
        let applied=fixture.call("composition.apply",json!({"plan_ref":plan["plan_ref"]})).await;
        assert_eq!(applied["snapshot"]["total"].as_u64(),Some(expected),"{label}");
        let report:semwright_semantic_composition::VerificationReport=
            serde_json::from_value(applied["report"].clone()).unwrap();
        assert_eq!(
            report.verdict().unwrap(),
            semwright_semantic_composition::Verdict::Pass,
            "{label} native/F verification"
        );
        let island=applied["island"].as_str().unwrap();
        let measured=fixture.call("composition.measure",json!({"island":island,"evaluated":true})).await;
        assert_eq!(measured["total"].as_u64(),Some(expected),"{label} evaluated count");
        assert_eq!(measured["coverage"],"single_frame");
    }
    fixture.provider.shutdown().await.unwrap();
}
