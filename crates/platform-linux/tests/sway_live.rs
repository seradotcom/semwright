#![cfg(target_os = "linux")]

use semwright_backend_api::{Backend, Context};
use semwright_platform_linux::sway::Sway;
use semwright_types::{CapabilityStatus, ErrorCode, NativeTarget};
use serde_json::{Value, json};
use std::{os::unix::fs::FileTypeExt, process::Stdio, time::Duration};
use tokio_util::sync::CancellationToken;

fn context() -> Context {
    Context {
        session: "sway-live".into(),
        cancellation: CancellationToken::new(),
    }
}

async fn wait_for_fixture(backend: &Sway, ctx: &Context) -> (Value, NativeTarget) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(listed) = backend.execute(ctx, "window.list", &json!({})).await
                && let Some(row) = listed["windows"].as_array().and_then(|rows| {
                    rows.iter()
                        .find(|row| row["title"].as_str() == Some("Semwright Sway Fixture"))
                })
            {
                let target = serde_json::from_value::<NativeTarget>(row["ref"]["$ref"].clone())
                    .expect("fixture target");
                return (row.clone(), target);
            }
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    })
    .await
    .expect("fixture window should appear")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an isolated live Sway compositor and GTK fixture"]
async fn real_sway_window_lifecycle_uses_native_ipc() {
    if std::env::var_os("SEMWRIGHT_TEST_SWAY").is_none() {
        return;
    }
    assert_eq!(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        Some("wayland"),
        "Sway live test must run on a Wayland compositor"
    );
    let socket = std::env::var("SWAYSOCK").expect("SWAYSOCK required");
    assert!(std::fs::metadata(&socket).unwrap().file_type().is_socket());

    let backend = Sway::default();
    let ctx = context();
    let features = backend.probe().await;
    assert!(features.iter().any(|feature| {
        feature.capability == "window.manage" && feature.status == CapabilityStatus::Supported
    }));

    let mut child = tokio::process::Command::new("/usr/bin/zenity")
        .env("GDK_BACKEND", "wayland")
        // Headless wlroots has no GPU. Force GTK4 onto its software renderer so
        // the fixture exercises Sway IPC instead of failing in EGL/Zink setup.
        .env("GSK_RENDERER", "cairo")
        .env("LIBGL_ALWAYS_SOFTWARE", "1")
        // A panic must not leave the GUI process holding the Actions tee pipe.
        .kill_on_drop(true)
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .args([
            "--entry",
            "--title=Semwright Sway Fixture",
            "--text=Disposable Sway window",
        ])
        .spawn()
        .expect("zenity fixture must start");

    tokio::time::sleep(Duration::from_millis(250)).await;
    if let Some(status) = child.try_wait().expect("inspect zenity fixture status") {
        panic!("zenity fixture exited before mapping a Wayland window: {status}");
    }

    let (row, target) = wait_for_fixture(&backend, &ctx).await;
    assert_eq!(row["coordinate_space"], "compositor_logical");
    backend.validate(&target).await.expect("fresh target");

    let target_json = serde_json::to_value(&target).unwrap();
    backend
        .execute(&ctx, "window.focus", &json!({"_target":target_json}))
        .await
        .expect("focus fixture");

    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if backend.is_focused(&target).await.unwrap_or(false) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("fixture should become focused");

    backend
        .execute(
            &ctx,
            "window.resize",
            &json!({
                "_target":serde_json::to_value(&target).unwrap(),
                "width":480,
                "height":260
            }),
        )
        .await
        .expect("resize floating fixture");
    backend
        .execute(
            &ctx,
            "window.move",
            &json!({
                "_target":serde_json::to_value(&target).unwrap(),
                "x":40,
                "y":50
            }),
        )
        .await
        .expect("move floating fixture");

    backend
        .execute(
            &ctx,
            "window.close",
            &json!({"_target":serde_json::to_value(&target).unwrap()}),
        )
        .await
        .expect("close fixture");

    let _ = tokio::time::timeout(Duration::from_secs(5), child.wait())
        .await
        .expect("fixture should exit after compositor close");

    let stale = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match backend.validate(&target).await {
                Err(error) if error.code == ErrorCode::StaleReference => break error,
                _ => tokio::time::sleep(Duration::from_millis(60)).await,
            }
        }
    })
    .await
    .expect("closed target must become stale");
    assert_eq!(stale.code, ErrorCode::StaleReference);
}
