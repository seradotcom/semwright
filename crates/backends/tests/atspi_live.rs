use semwright_backend_api::{Backend, Context};
use semwright_backends::atspi::Atspi;
use semwright_types::{ErrorCode, NativeTarget};
use serde_json::{Value, json};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

fn context() -> Context {
    Context {
        session: "atspi-live".into(),
        cancellation: CancellationToken::new(),
    }
}

fn app_identity(value: &Value) -> Option<String> {
    value["apps"].as_array()?.iter().find_map(|row| {
        let name = row["name"].as_str().unwrap_or_default().to_lowercase();
        let app = row["app"].as_str().unwrap_or_default();
        (name.contains("zenity") || app.to_lowercase().contains("zenity")).then(|| app.to_owned())
    })
}
#[tokio::test]
#[ignore = "requires a live user AT-SPI bus and zenity"]
async fn live_atspi_window_close_forces_resync_and_stales_refs() {
    if std::env::var_os("SEMWRIGHT_TEST_ATSPI").is_none() {
        return;
    }
    let mut child = tokio::process::Command::new("/usr/bin/zenity")
        .args([
            "--entry",
            "--title=Semwright AT-SPI Fixture",
            "--text=Disposable accessibility fixture",
        ])
        .spawn()
        .expect("zenity must start");

    let backend = Atspi::default();
    let ctx = context();
    let app = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Ok(value) = backend.execute(&ctx, "app.list", &json!({})).await
                && let Some(app) = app_identity(&value)
            {
                break app;
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    })
    .await
    .expect("zenity must appear on AT-SPI");
    let snapshot = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let value = backend
                .execute(
                    &ctx,
                    "ui.snapshot",
                    &json!({"app":app,"max_nodes":512,"max_depth":8}),
                )
                .await
                .expect("snapshot");
            if value["partial"] == false
                && value["nodes"]
                    .as_array()
                    .is_some_and(|nodes| !nodes.is_empty())
            {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    })
    .await
    .expect("complete zenity snapshot");

    let revision = snapshot["revision"].as_u64().unwrap();
    let target: NativeTarget =
        serde_json::from_value(snapshot["nodes"][0]["ref"]["$ref"].clone()).unwrap();
    let editable = snapshot["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| {
            node["states"]
                .as_array()
                .is_some_and(|states| states.iter().any(|state| state == "editable"))
        })
        .expect("zenity entry should be editable");
    let editable_id = editable["node_id"].as_str().unwrap().to_owned();
    let editable_target: NativeTarget =
        serde_json::from_value(editable["ref"]["$ref"].clone()).unwrap();
    backend
        .execute(
            &ctx,
            "ui.set_text",
            &json!({"_target":editable_target,"text":"Semwright delta"}),
        )
        .await
        .expect("editable text mutation");

    let delta = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let value = backend
                .execute(
                    &ctx,
                    "ui.snapshot",
                    &json!({
                        "app":app,
                        "max_nodes":512,
                        "max_depth":8,
                        "since_revision":revision
                    }),
                )
                .await
                .expect("delta snapshot");
            if value["mode"] == "delta" {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    })
    .await
    .expect("text mutation must produce a delta");
    assert_eq!(delta["resync_required"], false);
    assert!(
        delta["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|node| node["node_id"] == editable_id)
    );
    let delta_revision = delta["revision"].as_u64().unwrap();

    child.kill().await.ok();
    child.wait().await.ok();
    let after = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let value = backend
                .execute(
                    &ctx,
                    "ui.snapshot",
                    &json!({
                        "app":app,
                        "max_nodes":512,
                        "max_depth":8,
                        "since_revision":delta_revision
                    }),
                )
                .await
                .expect("post-close snapshot");
            if value["resync_required"] == true {
                break value;
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    })
    .await
    .expect("window close must force resync");

    assert_eq!(after["mode"], "full");
    assert_eq!(after["resync_required"], true);
    let stale = backend.validate(&target).await.unwrap_err();
    assert_eq!(stale.code, ErrorCode::StaleReference);
}
