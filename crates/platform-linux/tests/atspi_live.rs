#![cfg(target_os = "linux")]

use semwright_backend_api::{Backend, Context};
use semwright_platform_linux::atspi::Atspi;
use semwright_types::{ErrorCode, NativeTarget};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use tokio_util::sync::CancellationToken;

fn context() -> Context {
    Context {
        session: "atspi-live".into(),
        request_id: semwright_types::unique_id(),
        cancellation: CancellationToken::new(),
    }
}

fn app_identity(value: &Value, needle: &str) -> Option<String> {
    let needle = needle.to_lowercase();
    value["apps"].as_array()?.iter().find_map(|row| {
        let name = row["name"].as_str().unwrap_or_default().to_lowercase();
        let app = row["app"].as_str().unwrap_or_default();
        (name.contains(&needle) || app.to_lowercase().contains(&needle)).then(|| app.to_owned())
    })
}

async fn exercise_fixture(mut child: tokio::process::Child, needle: &str) {
    let backend = Atspi::default();
    let ctx = context();
    let app = match tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Some(status) = child.try_wait().expect("query fixture status") {
                panic!("accessibility fixture exited before registration: {status}");
            }
            if let Ok(value) = backend.execute(&ctx, "app.list", &json!({})).await
                && let Some(app) = app_identity(&value, needle)
            {
                break app;
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    })
    .await
    {
        Ok(app) => app,
        Err(_) => {
            let observed = backend
                .execute(&ctx, "app.list", &json!({}))
                .await
                .unwrap_or_else(|error| json!({"error":error.to_string()}));
            panic!("fixture must appear on AT-SPI; observed apps: {observed}");
        }
    };

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
    .expect("complete fixture snapshot");

    let revision = snapshot["revision"].as_u64().unwrap();
    let nodes = snapshot["nodes"].as_array().unwrap();
    let target: NativeTarget = serde_json::from_value(nodes[0]["ref"]["$ref"].clone()).unwrap();

    let editable = nodes
        .iter()
        .find(|node| {
            node["states"]
                .as_array()
                .is_some_and(|states| states.iter().any(|state| state == "editable"))
                && node["role"] != "password-entry"
        })
        .expect("fixture entry should expose editable text semantics");
    assert_eq!(editable["facets"]["text"]["editable"], true);
    assert_eq!(editable["facets"]["text"]["password"], false);
    assert_eq!(
        editable["facets"]["text"]["character_count"].as_u64(),
        Some(0),
        "fixture editable text should begin empty while exposing bounded text metadata: {editable}"
    );

    let password = nodes
        .iter()
        .find(|node| node["role"] == "password-entry")
        .expect("fixture password field should remain semantically identifiable");
    assert_eq!(password["name"], "");
    assert_eq!(password["description"], "");
    assert_eq!(password["help"], "");
    assert_eq!(password["facets"]["text"]["password"], true);

    let slider = nodes
        .iter()
        .find(|node| node["role"] == "slider")
        .expect("fixture slider should expose a value facet");
    assert_eq!(slider["facets"]["value"]["minimum"].as_f64(), Some(0.0));
    assert_eq!(slider["facets"]["value"]["maximum"].as_f64(), Some(100.0));
    assert_eq!(slider["facets"]["value"]["current"].as_f64(), Some(25.0));

    let export = nodes
        .iter()
        .find(|node| node["role"] == "button" && node["name"] == "Export")
        .expect("fixture export button should be present");
    let bounds = export["bounds"]
        .as_object()
        .expect("export button should expose screen bounds");
    let x =
        (bounds["x"].as_f64().unwrap() + bounds["width"].as_f64().unwrap() / 2.0).round() as i64;
    let y =
        (bounds["y"].as_f64().unwrap() + bounds["height"].as_f64().unwrap() / 2.0).round() as i64;
    let hit = backend
        .execute(&ctx, "ui.hit_test", &json!({"x":x,"y":y}))
        .await
        .expect("native semantic hit-test");
    assert_eq!(hit["semantic_coverage"], "native_hit_test");
    assert_eq!(hit["node"]["app"], app);
    let _: NativeTarget = serde_json::from_value(hit["node"]["ref"]["$ref"].clone())
        .expect("hit-test must return a native semantic ref");

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
    let changed_editable = delta["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["node_id"] == editable_id)
        .expect("text mutation delta must contain the editable node");
    assert_eq!(
        changed_editable["facets"]["text"]["character_count"].as_u64(),
        Some("Semwright delta".chars().count() as u64),
        "text mutation delta should project the new AT-SPI character count: {changed_editable}"
    );
    let fresh_editable_target: NativeTarget =
        serde_json::from_value(changed_editable["ref"]["$ref"].clone())
            .expect("delta must refresh the semantic target");
    let read_back = backend
        .execute(
            &ctx,
            "ui.read_text",
            &json!({"_target":fresh_editable_target,"max_chars":64}),
        )
        .await
        .expect("fresh semantic target should read back edited text");
    assert_eq!(read_back["text"], "Semwright delta");
    assert_eq!(read_back["truncated"], false);
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

#[tokio::test]
#[ignore = "requires a live user AT-SPI bus and zenity"]
async fn live_atspi_gtk_delta_resync_and_stale_refs() {
    if std::env::var_os("SEMWRIGHT_TEST_ATSPI").is_none() {
        return;
    }
    let child = tokio::process::Command::new("/usr/bin/zenity")
        .env("GTK_A11Y", "atspi")
        .env_remove("NO_AT_BRIDGE")
        .args([
            "--entry",
            "--title=Semwright AT-SPI Fixture",
            "--text=Disposable accessibility fixture",
        ])
        .spawn()
        .expect("zenity must start");
    exercise_fixture(child, "zenity").await;
}

#[tokio::test]
#[ignore = "requires a live user AT-SPI bus and a native Qt fixture"]
async fn live_atspi_qt_delta_resync_and_stale_refs() {
    if std::env::var_os("SEMWRIGHT_TEST_ATSPI").is_none() {
        return;
    }
    let fixture = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_QT_FIXTURE")
            .expect("SEMWRIGHT_TEST_QT_FIXTURE must point to the native Qt fixture"),
    );
    assert!(fixture.is_file(), "native Qt fixture must exist");
    let platform = std::env::var("SEMWRIGHT_TEST_QT_PLATFORM").unwrap_or_else(|_| "xcb".into());
    let child = tokio::process::Command::new(fixture)
        .env("QT_ACCESSIBILITY", "1")
        .env("QT_LINUX_ACCESSIBILITY_ALWAYS_ON", "1")
        .env_remove("NO_AT_BRIDGE")
        .env(
            "QT_LOGGING_RULES",
            "qt.accessibility.atspi=true;qt.accessibility.atspi.creation=true",
        )
        .env("QT_QPA_PLATFORM", platform)
        .spawn()
        .expect("native Qt fixture must start");
    exercise_fixture(child, "semwright").await;
}
