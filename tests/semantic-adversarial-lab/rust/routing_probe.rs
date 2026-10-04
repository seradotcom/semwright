//! Independent adversarial-lab Broker/Project-Graph routing adversarial probe.
//! All filesystem fixtures are synthetic and live inside the disposable enclosure.
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use semwright_types::{Envelope, ErrorCode, ExecuteRequest, unique_id};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

const SOURCE: &str = env!("G_LAB_COMPILED_SOURCE_SHA");
type ProbeResult<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn fixture(tag: &str) -> ProbeResult<(PathBuf, PathBuf, PathBuf)> {
    let base = PathBuf::from(format!("/tmp/g-routing-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&base)?;
    let root = base.join("workspace");
    std::fs::create_dir(&root)?;
    let root = std::fs::canonicalize(root)?;
    Ok((base.clone(), root, base.join("project-state")))
}

fn policy(root: &Path, manage: bool) -> ProbeResult<Policy> {
    let allow = if manage {
        ["project.manage"].into_iter().map(String::from).collect()
    } else {
        Default::default()
    };
    Ok(Policy::new(PolicyConfig {
        allow,
        filesystem: vec![FilesystemGrant {
            name: "workspace".into(),
            path: root.to_owned(),
            read: true,
            write: false,
        }],
        ..Default::default()
    })?)
}

fn broker(
    root: &Path,
    state: &Path,
    audit_name: &str,
    principal: &str,
    manage: bool,
    configure: bool,
) -> ProbeResult<Arc<Broker>> {
    let audit = Audit::open(
        &state.parent().expect("state parent").join(audit_name),
        65_536,
        2,
    )?;
    let broker = Broker::new(
        policy(root, manage)?,
        vec![],
        audit,
        Arc::new(NoApprover),
        None,
        json!({"fixture":"g-routing"}),
        true,
    )?;
    if configure {
        broker.configure_project_graphs(state, principal.into())?;
    }
    Ok(broker)
}

async fn call(broker: &Arc<Broker>, session: &str, command: &str, args: Value) -> Envelope {
    broker
        .clone()
        .execute(
            session.into(),
            unique_id(),
            ExecuteRequest {
                command: command.into(),
                args,
                dry_run: false,
                backend: None,
            },
            CancellationToken::new(),
        )
        .await
}

fn cleanup(path: &Path) {
    let _ = std::fs::remove_dir_all(path);
}

async fn probe(id: &str) -> ProbeResult<Value> {
    Ok(match id {
        "G-ROUTE-001" => {
            let (base, root, state) = fixture("unconfigured")?;
            let service = broker(
                &root,
                &state,
                "audit-unconfigured",
                "os-user-v1:g-unconfigured",
                true,
                false,
            )?;
            let result = call(
                &service,
                &unique_id(),
                "project.create",
                json!({"root":"workspace"}),
            )
            .await;
            let code = format!("{:?}", result.error.expect("error").code);
            cleanup(&base);
            json!({"unconfigured_code":code})
        }
        "G-ROUTE-002" => {
            let (base, root, state) = fixture("admission")?;
            let service = broker(
                &root,
                &state,
                "audit-admission",
                "os-user-v1:g-admission",
                true,
                true,
            )?;
            let receipt = service.describe("project.receipt.admit").is_err();
            let revision = service.describe("project.revision.admit").is_err();
            let rebuild = service.describe("project.rebuild.execute").is_err();
            cleanup(&base);
            json!({"receipt_admission_absent":receipt,"revision_admission_absent":revision,"rebuild_execute_absent":rebuild})
        }
        "G-ROUTE-003" => {
            let (base, root, state) = fixture("policy")?;
            let service = broker(
                &root,
                &state,
                "audit-policy",
                "os-user-v1:g-policy",
                false,
                true,
            )?;
            let result = call(
                &service,
                &unique_id(),
                "project.create",
                json!({"root":"workspace"}),
            )
            .await;
            let code = format!("{:?}", result.error.expect("error").code);
            cleanup(&base);
            json!({"filesystem_only_code":code})
        }
        "G-ROUTE-004" => {
            let (base, root, state) = fixture("principal")?;
            let first = broker(
                &root,
                &state,
                "audit-first",
                "os-user-v1:g-owner-a",
                true,
                true,
            )?;
            let created = call(
                &first,
                &unique_id(),
                "project.create",
                json!({"root":"workspace"}),
            )
            .await;
            let project = created.data.expect("created")["project"]
                .as_str()
                .expect("project")
                .to_owned();
            drop(first);
            let other = broker(
                &root,
                &state,
                "audit-other",
                "os-user-v1:g-owner-b",
                true,
                true,
            )?;
            let result = call(
                &other,
                &unique_id(),
                "project.query",
                json!({"root":"workspace","project":project}),
            )
            .await;
            let code = format!("{:?}", result.error.expect("error").code);
            cleanup(&base);
            json!({"different_principal_code":code})
        }
        "G-ROUTE-005" => {
            let (base, root, state) = fixture("session")?;
            let service = broker(
                &root,
                &state,
                "audit-session",
                "os-user-v1:g-session-owner",
                true,
                true,
            )?;
            let first = unique_id();
            let created = call(
                &service,
                &first,
                "project.create",
                json!({"root":"workspace"}),
            )
            .await;
            let project = created.data.expect("created")["project"]
                .as_str()
                .expect("project")
                .to_owned();
            let second = unique_id();
            let before = call(
                &service,
                &second,
                "project.query",
                json!({"root":"workspace","project":project}),
            )
            .await
            .ok;
            service.revoke_session(&first);
            let after = call(
                &service,
                &second,
                "project.query",
                json!({"root":"workspace","project":project}),
            )
            .await
            .ok;
            cleanup(&base);
            json!({"second_session_before_revoke":before,"second_session_after_revoke":after})
        }
        "G-ROUTE-006" => {
            let (base, root, state) = fixture("grant")?;
            let service = broker(
                &root,
                &state,
                "audit-grant",
                "os-user-v1:g-grant",
                true,
                true,
            )?;
            let created = call(
                &service,
                &unique_id(),
                "project.create",
                json!({"root":"workspace"}),
            )
            .await;
            let project = created.data.expect("created")["project"]
                .as_str()
                .expect("project")
                .to_owned();
            let denied = call(
                &service,
                &unique_id(),
                "project.query",
                json!({"root":"not-granted","project":project}),
            )
            .await;
            let code = format!("{:?}", denied.error.expect("error").code);
            cleanup(&base);
            json!({"ungranted_root_code":code})
        }
        "G-ROUTE-007" => {
            let (base, root, state) = fixture("preview")?;
            let service = broker(
                &root,
                &state,
                "audit-preview",
                "os-user-v1:g-preview",
                false,
                false,
            )?;
            let described = call(
                &service,
                &unique_id(),
                "capabilities.describe",
                json!({"name":"project.create"}),
            )
            .await;
            let data = described.data.expect("describe");
            let available = data["routes"][0]["available"].as_bool().unwrap_or(false);
            let is_authorization = data["availability_is_authorization"]
                .as_bool()
                .unwrap_or(true);
            let state = data["policy_preview"]["state"]
                .as_str()
                .unwrap_or("missing")
                .to_owned();
            cleanup(&base);
            json!({"available":available,"availability_is_authorization":is_authorization,"policy_state":state})
        }
        "G-ROUTE-008" => {
            let (base, root, _state) = fixture("overlap")?;
            let state = root.join("private-project-state");
            let service = broker(
                &root,
                &base.join("placeholder-state"),
                "audit-overlap",
                "os-user-v1:g-overlap",
                true,
                false,
            )?;
            let denied = service
                .configure_project_graphs(&state, "os-user-v1:g-overlap".into())
                .err()
                .is_some_and(|error| error.code == ErrorCode::PolicyDenied);
            let absent = !state.exists();
            cleanup(&base);
            json!({"overlap_rejected":denied,"state_not_created":absent})
        }
        "G-ROUTE-009" => {
            use std::os::unix::fs::symlink;
            let base = PathBuf::from(format!("/tmp/g-routing-symlink-{}", std::process::id()));
            std::fs::create_dir_all(&base)?;
            let private = std::fs::canonicalize({
                let p = base.join("private");
                std::fs::create_dir(&p)?;
                p
            })?;
            let state = private.join("projects");
            let alias = base.join("workspace-alias");
            symlink(&private, &alias)?;
            let service = broker(
                &alias,
                &base.join("placeholder-state"),
                "audit-symlink-overlap",
                "os-user-v1:g-symlink",
                true,
                false,
            )?;
            let denied = service
                .configure_project_graphs(&state, "os-user-v1:g-symlink".into())
                .err()
                .is_some_and(|error| error.code == ErrorCode::PolicyDenied);
            let absent = !state.exists();
            cleanup(&base);
            json!({"resolved_overlap_rejected":denied,"state_not_created":absent})
        }
        "G-ROUTE-010" => {
            use std::os::unix::fs::symlink;
            let base = PathBuf::from(format!("/tmp/g-routing-retarget-{}", std::process::id()));
            std::fs::create_dir_all(&base)?;
            let first = base.join("first");
            let second = base.join("second");
            std::fs::create_dir(&first)?;
            std::fs::create_dir(&second)?;
            let alias = base.join("workspace-link");
            symlink(&first, &alias)?;
            let state = base.join("state");
            let service = broker(
                &alias,
                &state,
                "audit-retarget",
                "os-user-v1:g-retarget",
                true,
                true,
            )?;
            let created = call(
                &service,
                &unique_id(),
                "project.create",
                json!({"root":"workspace"}),
            )
            .await;
            let project = created.data.expect("created")["project"]
                .as_str()
                .expect("project")
                .to_owned();
            std::fs::remove_file(&alias)?;
            symlink(&second, &alias)?;
            let denied = call(
                &service,
                &unique_id(),
                "project.query",
                json!({"root":"workspace","project":project}),
            )
            .await;
            let code = format!("{:?}", denied.error.expect("error").code);
            cleanup(&base);
            json!({"retarget_code":code})
        }
        "G-ROUTE-011" => {
            let (base, root, state) = fixture("provenance")?;
            std::fs::write(root.join("source.bin"), b"g-provenance")?;
            let service = broker(
                &root,
                &state,
                "audit-provenance",
                "os-user-v1:g-provenance",
                true,
                true,
            )?;
            let session = unique_id();
            let created = call(
                &service,
                &session,
                "project.create",
                json!({"root":"workspace"}),
            )
            .await;
            let project = created.data.expect("created")["project"]
                .as_str()
                .expect("project")
                .to_owned();
            let registered = call(
                &service,
                &session,
                "project.asset.register",
                json!({"root":"workspace","project":project,"label":"source","resource_type":"fixture","path":"source.bin","max_bytes":4096}),
            )
            .await;
            let asset = registered.data.expect("registered")["result"]["asset"]["id"]
                .as_str()
                .expect("asset")
                .to_owned();
            let provenance = call(
                &service,
                &session,
                "project.asset.provenance",
                json!({"root":"workspace","project":project,"asset":asset}),
            )
            .await;
            let data = provenance.data.expect("provenance");
            let result = &data["result"];
            let producer_null = result["producer"].is_null();
            let known = result["known_derivatives"]
                .as_array()
                .map_or(usize::MAX, Vec::len);
            cleanup(&base);
            json!({"producer_null":producer_null,"known_derivatives":known})
        }
        "G-ROUTE-012" => {
            let (base, root, state) = fixture("gc")?;
            let sentinel = root.join("sentinel.bin");
            std::fs::write(&sentinel, b"user-owned")?;
            let service = broker(&root, &state, "audit-gc", "os-user-v1:g-gc", true, true)?;
            let session = unique_id();
            let created = call(
                &service,
                &session,
                "project.create",
                json!({"root":"workspace"}),
            )
            .await;
            let project = created.data.expect("created")["project"]
                .as_str()
                .expect("project")
                .to_owned();
            let imported = call(
                &service,
                &session,
                "project.manifest.import",
                json!({"root":"workspace","project":project,"manifest":{
                    "version":1,
                    "source_project":"prj_00000000000000000000000000000001",
                    "source_snapshot":0,
                    "assets":[{"source_id":"asset_00000000000000000000000000000001","label":"portable garbage","resource_type":"fixture"}],
                    "declarations":[],
                    "coverage_complete":false
                }}),
            )
            .await;
            let local = imported.data.expect("imported")["result"]["mapping"][0]["local"]
                .as_str()
                .expect("local")
                .to_owned();
            let tombstoned = call(
                &service,
                &session,
                "project.asset.tombstone",
                json!({"root":"workspace","project":project,"asset":local}),
            )
            .await
            .ok;
            let preview = call(
                &service,
                &session,
                "project.gc.preview",
                json!({"root":"workspace","project":project,"limit":16}),
            )
            .await;
            let preview_data = preview.data.expect("preview");
            let preview_no_delete = preview_data["result"]["user_files_deleted"] == false;
            let collected = call(
                &service,
                &session,
                "project.gc.collect",
                json!({"root":"workspace","project":project,"assets":[local]}),
            )
            .await;
            let collect_data = collected.data.expect("collect");
            let collect_no_delete = collect_data["result"]["user_files_deleted"] == false;
            let preserved = std::fs::read(&sentinel)? == b"user-owned";
            cleanup(&base);
            json!({"tombstoned":tombstoned,"preview_no_user_delete":preview_no_delete,"collect_no_user_delete":collect_no_delete,"sentinel_preserved":preserved})
        }
        _ => {
            eprintln!("unregistered routing selector");
            std::process::exit(2)
        }
    })
}

fn cases() -> Vec<String> {
    (1..=12).map(|i| format!("G-ROUTE-{i:03}")).collect()
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 || std::env::var("G_LAB_TARGET_SHA").as_deref() != Ok(SOURCE) {
        std::process::exit(2)
    }
    if args[0] == "--list" {
        println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"cases":cases()})
        );
        return;
    }
    if !cases().contains(&args[0]) {
        std::process::exit(2)
    }
    match probe(&args[0]).await {
        Ok(observed) => println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"case_id":args[0],"observed":observed})
        ),
        Err(error) => {
            eprintln!("routing probe contract/setup error: {error:?}");
            std::process::exit(1)
        }
    }
}
