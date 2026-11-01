#![cfg(target_os = "linux")]

use semwright_backend_api::{Backend, Context};
use semwright_platform_linux::bridge::Kwin;
use semwright_types::{CapabilityStatus, ErrorCode, NativeTarget};
use serde_json::{Value, json};
use std::{os::unix::fs::FileTypeExt, path::Path, time::Duration};
use tokio::process::Command;
use tokio_util::sync::CancellationToken;
use zbus::{Connection, Proxy};

fn context() -> Context {
    Context {
        session: "kwin-live".into(),
        request_id: semwright_types::unique_id(),
        cancellation: CancellationToken::new(),
    }
}

async fn wait_for_name(connection: &Connection, name: &str) {
    let proxy = Proxy::new(
        connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await
    .expect("D-Bus proxy");
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let owned: bool = proxy
                .call("NameHasOwner", &(name,))
                .await
                .expect("NameHasOwner");
            if owned {
                break;
            }
            tokio::time::sleep(Duration::from_millis(75)).await;
        }
    })
    .await
    .expect("KWin must own its D-Bus name");
}

async fn wait_for_bridge(backend: &Kwin) {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let available = backend.probe().await.into_iter().any(|feature| {
                feature.capability == "window.manage"
                    && feature.status == CapabilityStatus::Supported
            });
            if available {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("enabled KWin script must publish and poll the mailbox");
}

async fn wait_for_fixture(
    backend: &Kwin,
    ctx: &Context,
    child: &mut tokio::process::Child,
) -> (Value, NativeTarget) {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut polls = 0_u32;
        loop {
            polls += 1;
            if let Some(status) = child.try_wait().expect("fixture child wait status") {
                panic!("KDE fixture exited before publishing a Wayland window: {status}");
            }
            match backend.execute(ctx, "window.list", &json!({})).await {
                Ok(listed) => {
                    if let Some(row) = listed["windows"].as_array().and_then(|rows| {
                        // KDE may append a translated application suffix to the
                        // title. The unique fixture prefix remains required.
                        rows.iter().find(|row| {
                            row["title"]
                                .as_str()
                                .is_some_and(|title| title.starts_with("Semwright Plasma Fixture"))
                        })
                    }) {
                        let target =
                            serde_json::from_value::<NativeTarget>(row["ref"]["$ref"].clone())
                                .expect("fixture target");
                        return (row.clone(), target);
                    }
                    if polls.is_multiple_of(25) {
                        let titles = listed["windows"]
                            .as_array()
                            .map(|rows| {
                                rows.iter()
                                    .take(12)
                                    .filter_map(|row| row["title"].as_str())
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        eprintln!("KWin mailbox fixture absent (poll {polls}): {titles:?}");
                    }
                }
                Err(error) if polls.is_multiple_of(25) => {
                    eprintln!("KWin mailbox list error (poll {polls}): {:?}", error.code);
                }
                Err(_) => {}
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("fixture window should appear through the KWin mailbox")
}

async fn wait_for_bounds(
    backend: &Kwin,
    ctx: &Context,
    target: &NativeTarget,
    width: i64,
    height: i64,
) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(listed) = backend.execute(ctx, "window.list", &json!({})).await
                && listed["windows"].as_array().is_some_and(|rows| {
                    rows.iter().any(|row| {
                        row["ref"]["$ref"]["identity"] == target.identity
                            && row["bounds"]["width"].as_i64() == Some(width)
                            && row["bounds"]["height"].as_i64() == Some(height)
                    })
                })
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(75)).await;
        }
    })
    .await
    .expect("KWin must publish the resized geometry");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a private D-Bus bus and live KWin 6 Wayland compositor"]
async fn real_kwin6_mailbox_controls_window_and_rejects_stale_ref() {
    if std::env::var_os("SEMWRIGHT_TEST_KWIN").is_none() {
        return;
    }
    assert_eq!(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        Some("wayland"),
        "KWin live test must run in a Wayland session"
    );
    assert!(
        std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
        "private session bus required"
    );

    let connection = Connection::session().await.expect("session bus");
    let backend = Kwin::attach(&connection)
        .await
        .expect("attach KWin mailbox");
    connection
        .request_name("org.semwright.Broker")
        .await
        .expect("test must own the broker well-known name");

    // The mailbox must reject any process that merely knows its object path.
    let foreign = Connection::session().await.expect("foreign bus connection");
    let foreign_proxy = Proxy::new(
        &foreign,
        "org.semwright.Broker",
        "/org/semwright/KWinBridge",
        "org.semwright.KWinMailbox1",
    )
    .await
    .expect("foreign mailbox proxy");
    let rejected: zbus::Result<()> = foreign_proxy
        .call("Publish", &(r#"{"version":1,"windows":[]}"#,))
        .await;
    assert!(
        rejected.is_err(),
        "non-KWin sender must not publish snapshots"
    );

    wait_for_name(&connection, "org.kde.KWin").await;
    wait_for_bridge(&backend).await;

    // The KWin D-Bus bridge may start before the Wayland listener is ready.
    // A client must not race compositor socket creation in this live test.
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").expect("private runtime directory");
    let display = std::env::var_os("WAYLAND_DISPLAY").expect("private Wayland socket name");
    let wayland_socket = Path::new(&runtime).join(display);
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if std::fs::metadata(&wayland_socket)
                .is_ok_and(|metadata| metadata.file_type().is_socket())
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(75)).await;
        }
    })
    .await
    .expect("virtual KWin must create its Wayland socket");

    let fixture =
        std::env::var("SEMWRIGHT_TEST_KWIN_FIXTURE").unwrap_or_else(|_| "/usr/bin/kdialog".into());
    assert!(
        Path::new(&fixture).is_file(),
        "KDE fixture executable missing"
    );
    let mut command = Command::new(&fixture);
    command.args([
        "--title",
        "Semwright Plasma Fixture",
        "--msgbox",
        "Disposable KWin 6 integration fixture",
    ]);
    if let Ok(directory) = std::env::var("SEMWRIGHT_KWIN_EVIDENCE_DIR") {
        let log = Path::new(&directory).join("kdialog.stderr.log");
        command.stderr(std::fs::File::create(log).expect("fixture stderr evidence"));
    }
    let mut child = command.spawn().expect("KDE fixture must start");

    let ctx = context();
    let (row, target) = wait_for_fixture(&backend, &ctx, &mut child).await;
    assert_eq!(row["coordinate_space"], "compositor_logical");
    backend.validate(&target).await.expect("fresh KWin target");

    backend
        .execute(
            &ctx,
            "window.focus",
            &json!({"_target":serde_json::to_value(&target).unwrap()}),
        )
        .await
        .expect("focus fixture");
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if backend.is_focused(&target).await.unwrap_or(false) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(60)).await;
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
                "width":620,
                "height":360
            }),
        )
        .await
        .expect("resize fixture");
    wait_for_bounds(&backend, &ctx, &target, 620, 360).await;

    backend
        .execute(
            &ctx,
            "window.move",
            &json!({
                "_target":serde_json::to_value(&target).unwrap(),
                "x":48,
                "y":64
            }),
        )
        .await
        .expect("move fixture");

    backend
        .execute(
            &ctx,
            "window.close",
            &json!({"_target":serde_json::to_value(&target).unwrap()}),
        )
        .await
        .expect("close fixture");
    let _ = tokio::time::timeout(Duration::from_secs(6), child.wait())
        .await
        .expect("fixture should exit after KWin close");

    let stale = tokio::time::timeout(Duration::from_secs(6), async {
        loop {
            match backend.validate(&target).await {
                Err(error) if error.code == ErrorCode::StaleReference => break error,
                _ => tokio::time::sleep(Duration::from_millis(75)).await,
            }
        }
    })
    .await
    .expect("closed KWin target must become stale");
    assert_eq!(stale.code, ErrorCode::StaleReference);
}
