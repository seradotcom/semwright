use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig};
use semwright_types::{Envelope, ErrorCode, ExecuteRequest, unique_id};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

fn broker(root: &Path, state: &Path, audit_name: &str, principal: &str) -> Arc<Broker> {
    let config = PolicyConfig {
        allow: ["project.manage"].into_iter().map(String::from).collect(),
        filesystem: vec![FilesystemGrant {
            name: "workspace".into(),
            path: root.to_owned(),
            read: true,
            write: false,
        }],
        ..Default::default()
    };
    let audit = Audit::open(&state.parent().unwrap().join(audit_name), 65_536, 2).unwrap();
    let broker = Broker::new(
        Policy::new(config).unwrap(),
        vec![],
        audit,
        Arc::new(NoApprover),
        None,
        json!({"fixture":"project-graph-broker"}),
        true,
    )
    .unwrap();
    broker
        .configure_project_graphs(state, principal.into())
        .unwrap();
    broker
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

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    std::fs::create_dir(&root).unwrap();
    let root = std::fs::canonicalize(root).unwrap();
    let state = temp.path().join("project-state");
    (temp, root, state)
}

#[tokio::test]
async fn project_routes_reenter_broker_policy_and_survive_session_rotation() {
    let (_temp, root, state) = fixture();
    let broker = broker(&root, &state, "audit-one", "os-user-v1:fixture:one");
    let first_session = unique_id();

    let created = call(
        &broker,
        &first_session,
        "project.create",
        json!({"root":"workspace"}),
    )
    .await;
    assert!(created.ok, "{created:?}");
    let project = created.data.unwrap()["project"]
        .as_str()
        .unwrap()
        .to_owned();

    let first = call(
        &broker,
        &first_session,
        "project.query",
        json!({"root":"workspace","project":project,"limit":10}),
    )
    .await;
    assert!(first.ok, "{first:?}");
    assert_eq!(first.data.unwrap()["result"]["items"], json!([]));

    let second_session = unique_id();
    let second = call(
        &broker,
        &second_session,
        "project.query",
        json!({"root":"workspace","project":project,"limit":10}),
    )
    .await;
    assert!(second.ok, "{second:?}");

    let denied = call(
        &broker,
        &second_session,
        "project.query",
        json!({"root":"not-granted","project":project}),
    )
    .await;
    assert_eq!(denied.error.unwrap().code, ErrorCode::PolicyDenied);

    broker.revoke_session(&first_session);
    let after_revoke = call(
        &broker,
        &second_session,
        "project.query",
        json!({"root":"workspace","project":project}),
    )
    .await;
    assert!(after_revoke.ok, "{after_revoke:?}");
}

#[tokio::test]
async fn private_project_store_is_bound_to_durable_principal_not_client_ids() {
    let (_temp, root, state) = fixture();
    let first = broker(&root, &state, "audit-owner", "os-user-v1:fixture:owner");
    let created = call(
        &first,
        &unique_id(),
        "project.create",
        json!({"root":"workspace"}),
    )
    .await;
    assert!(created.ok, "{created:?}");
    let project = created.data.unwrap()["project"]
        .as_str()
        .unwrap()
        .to_owned();

    let other = broker(&root, &state, "audit-other", "os-user-v1:fixture:other");
    let denied = call(
        &other,
        &unique_id(),
        "project.query",
        json!({"root":"workspace","project":project}),
    )
    .await;
    assert_eq!(denied.error.unwrap().code, ErrorCode::PolicyDenied);
}

#[tokio::test]
async fn project_service_requires_host_configuration_and_exposes_no_receipt_admission_command() {
    let (_temp, root, state) = fixture();
    let config = PolicyConfig {
        allow: ["project.manage"].into_iter().map(String::from).collect(),
        filesystem: vec![FilesystemGrant {
            name: "workspace".into(),
            path: root,
            read: true,
            write: false,
        }],
        ..Default::default()
    };
    let audit = Audit::open(
        &state.parent().unwrap().join("audit-unconfigured"),
        65_536,
        2,
    )
    .unwrap();
    let unconfigured = Broker::new(
        Policy::new(config).unwrap(),
        vec![],
        audit,
        Arc::new(NoApprover),
        None,
        json!({"fixture":"unconfigured-project-graph"}),
        true,
    )
    .unwrap();
    let unavailable = call(
        &unconfigured,
        &unique_id(),
        "project.create",
        json!({"root":"workspace"}),
    )
    .await;
    assert_eq!(unavailable.error.unwrap().code, ErrorCode::Unavailable);

    assert!(unconfigured.describe("project.receipt.admit").is_err());
    assert!(unconfigured.describe("project.rebuild.execute").is_err());
}

#[tokio::test]
async fn filesystem_read_grant_alone_cannot_mutate_project_state() {
    let (_temp, root, state) = fixture();
    let config = PolicyConfig {
        filesystem: vec![FilesystemGrant {
            name: "workspace".into(),
            path: root,
            read: true,
            write: false,
        }],
        ..Default::default()
    };
    let audit = Audit::open(
        &state.parent().unwrap().join("audit-no-project-manage"),
        65_536,
        2,
    )
    .unwrap();
    let broker = Broker::new(
        Policy::new(config).unwrap(),
        vec![],
        audit,
        Arc::new(NoApprover),
        None,
        json!({"fixture":"project-graph-policy-deny"}),
        true,
    )
    .unwrap();
    broker
        .configure_project_graphs(&state, "os-user-v1:fixture:deny".into())
        .unwrap();

    let denied = call(
        &broker,
        &unique_id(),
        "project.create",
        json!({"root":"workspace"}),
    )
    .await;
    assert_eq!(denied.error.unwrap().code, ErrorCode::PolicyDenied);
}

#[tokio::test]
async fn project_catalog_preview_exposes_availability_without_granting_management() {
    let (_temp, root, state) = fixture();
    let config = PolicyConfig {
        filesystem: vec![FilesystemGrant {
            name: "workspace".into(),
            path: root,
            read: true,
            write: false,
        }],
        ..Default::default()
    };
    let audit = Audit::open(
        &state.parent().unwrap().join("audit-project-preview"),
        65_536,
        2,
    )
    .unwrap();
    let broker = Broker::new(
        Policy::new(config).unwrap(),
        vec![],
        audit,
        Arc::new(NoApprover),
        None,
        json!({"fixture":"project-graph-preview"}),
        true,
    )
    .unwrap();
    let described = call(
        &broker,
        &unique_id(),
        "capabilities.describe",
        json!({"name":"project.create"}),
    )
    .await;
    assert!(described.ok, "{described:?}");
    let data = described.data.unwrap();
    assert_eq!(data["routes"][0]["available"], true);
    assert_eq!(data["availability_is_authorization"], false);
    assert_eq!(data["policy_preview"]["state"], "deny");
}

#[cfg(unix)]
#[test]
fn project_state_rejects_a_symlinked_grant_that_resolves_to_its_parent() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let private_parent = temp.path().join("private");
    std::fs::create_dir(&private_parent).unwrap();
    let private_parent = std::fs::canonicalize(private_parent).unwrap();
    let state = private_parent.join("projects");
    let alias = temp.path().join("workspace-alias");
    symlink(&private_parent, &alias).unwrap();

    let config = PolicyConfig {
        allow: ["project.manage"].into_iter().map(String::from).collect(),
        filesystem: vec![FilesystemGrant {
            name: "workspace".into(),
            path: alias,
            read: true,
            write: false,
        }],
        ..Default::default()
    };
    let audit = Audit::open(&temp.path().join("audit-overlap"), 65_536, 2).unwrap();
    let broker = Broker::new(
        Policy::new(config).unwrap(),
        vec![],
        audit,
        Arc::new(NoApprover),
        None,
        json!({"fixture":"project-graph-overlap"}),
        true,
    )
    .unwrap();

    let error = broker
        .configure_project_graphs(&state, "os-user-v1:fixture:overlap".into())
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PolicyDenied);
}

#[cfg(unix)]
#[tokio::test]
async fn project_root_symlink_retarget_invalidates_the_persistent_boundary() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().unwrap();
    let first_root = temp.path().join("root-one");
    let second_root = temp.path().join("root-two");
    std::fs::create_dir(&first_root).unwrap();
    std::fs::create_dir(&second_root).unwrap();
    let alias = temp.path().join("workspace-link");
    symlink(&first_root, &alias).unwrap();
    let state = temp.path().join("project-state");
    let broker = broker(
        &alias,
        &state,
        "audit-retarget",
        "os-user-v1:fixture:retarget",
    );

    let created = call(
        &broker,
        &unique_id(),
        "project.create",
        json!({"root":"workspace"}),
    )
    .await;
    assert!(created.ok, "{created:?}");
    let project = created.data.unwrap()["project"]
        .as_str()
        .unwrap()
        .to_owned();

    std::fs::remove_file(&alias).unwrap();
    symlink(&second_root, &alias).unwrap();

    let denied = call(
        &broker,
        &unique_id(),
        "project.query",
        json!({"root":"workspace","project":project}),
    )
    .await;
    assert_eq!(denied.error.unwrap().code, ErrorCode::PolicyDenied);
}
