#![cfg(target_os = "windows")]

use semwright_backend_api::{Backend, Context};
use semwright_platform_windows::Windows;
use semwright_types::{ErrorCode, unique_id};
use serde_json::json;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

fn require_interactive_certification_session() {
    assert_eq!(
        std::env::var("SEMWRIGHT_WINDOWS_INTERACTIVE").as_deref(),
        Ok("1"),
        "interactive certification requires SEMWRIGHT_WINDOWS_INTERACTIVE=1"
    );
    if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        assert_eq!(
            std::env::var("RUNNER_ENVIRONMENT").as_deref(),
            Ok("self-hosted"),
            "hosted GitHub runners must never claim PASS_WINDOWS_INTERACTIVE"
        );
    }
}

fn context(label: &str) -> Context {
    Context {
        session: "windows-interactive-capture".into(),
        request_id: format!("{label}-{}", unique_id()),
        cancellation: CancellationToken::new(),
    }
}

fn artifact_root(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "semwright-windows-interactive-{label}-{}",
        unique_id()
    ))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an unlocked Windows desktop and explicit picker selection"]
async fn interactive_capture_picker_selects_bounded_png() {
    require_interactive_certification_session();
    let root = artifact_root("capture-select");
    let backend = Windows::new(&root).expect("Windows backend");
    eprintln!("INTERACTIVE ACTION REQUIRED: select a non-sensitive window in the system picker");
    let result = backend
        .execute(&context("select"), "screen.capture", &json!({}))
        .await
        .expect("user-selected Windows.Graphics.Capture item");

    assert_eq!(result["selection"], "system_content_picker");
    assert_eq!(result["mime_type"], "image/png");
    assert_eq!(result["expires_in_seconds"], 60);
    let width = result["width"].as_u64().expect("capture width");
    let height = result["height"].as_u64().expect("capture height");
    let declared_bytes = result["bytes"].as_u64().expect("capture bytes");
    assert!((1..=4096).contains(&width));
    assert!((1..=4096).contains(&height));
    assert!(width * height <= 4_194_304);
    assert!((1..=8 * 1024 * 1024).contains(&declared_bytes));

    let path = PathBuf::from(result["path"].as_str().expect("capture artifact path"));
    assert!(
        path.starts_with(&root),
        "capture artifact must stay in private root"
    );
    let bytes = std::fs::read(&path).expect("read capture PNG");
    assert_eq!(bytes.len() as u64, declared_bytes);
    assert!(
        bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "capture artifact must be PNG"
    );

    std::fs::remove_file(&path).expect("remove capture artifact");
    backend.shutdown().await.expect("shutdown Windows backend");
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an unlocked Windows desktop and explicit picker cancellation"]
async fn interactive_capture_picker_user_cancel_is_cancelled() {
    require_interactive_certification_session();
    let root = artifact_root("capture-cancel");
    let backend = Windows::new(&root).expect("Windows backend");
    eprintln!("INTERACTIVE ACTION REQUIRED: cancel the Windows GraphicsCapturePicker");
    let error = backend
        .execute(&context("cancel"), "screen.capture", &json!({}))
        .await
        .expect_err("picker cancellation must not produce a capture");
    assert_eq!(error.code, ErrorCode::Cancelled);

    backend.shutdown().await.expect("shutdown Windows backend");
    let _ = std::fs::remove_dir_all(&root);
}
