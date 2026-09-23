#![cfg(target_os = "linux")]

use semwright_backend_api::{Backend, Context};
use semwright_platform_linux::x11::X11;
use semwright_types::{CapabilityStatus, ErrorCode, NativeTarget};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use x11rb::{
    COPY_DEPTH_FROM_PARENT,
    connection::Connection,
    protocol::{Event, xproto::*},
    rust_connection::RustConnection,
    wrapper::ConnectionExt as _,
};

fn atom(conn: &RustConnection, name: &str) -> u32 {
    conn.intern_atom(false, name.as_bytes())
        .unwrap()
        .reply()
        .unwrap()
        .atom
}

fn context() -> Context {
    Context {
        session: "x11-ewmh-live".into(),
        request_id: semwright_types::unique_id(),
        cancellation: CancellationToken::new(),
    }
}

fn wm_is_ewmh(conn: &RustConnection, root: u32) -> bool {
    let supporting = atom(conn, "_NET_SUPPORTING_WM_CHECK");
    conn.get_property(false, root, supporting, AtomEnum::WINDOW, 0, 1)
        .ok()
        .and_then(|cookie| cookie.reply().ok())
        .and_then(|reply| reply.value32().and_then(|mut values| values.next()))
        .is_some()
}

fn create_managed_window(conn: &RustConnection, root: u32, window: u32) -> (u32, u32) {
    let wm_class = atom(conn, "WM_CLASS");
    let wm_pid = atom(conn, "_NET_WM_PID");
    let wm_name = atom(conn, "_NET_WM_NAME");
    let utf8 = atom(conn, "UTF8_STRING");
    let wm_protocols = atom(conn, "WM_PROTOCOLS");
    let wm_delete = atom(conn, "WM_DELETE_WINDOW");

    conn.create_window(
        COPY_DEPTH_FROM_PARENT,
        window,
        root,
        30,
        40,
        360,
        220,
        0,
        WindowClass::INPUT_OUTPUT,
        0,
        &CreateWindowAux::new()
            .background_pixel(conn.setup().roots[0].white_pixel)
            .event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE),
    )
    .unwrap()
    .check()
    .unwrap();
    conn.change_property8(
        PropMode::REPLACE,
        window,
        wm_class,
        AtomEnum::STRING,
        b"semwright-ewmh\0SemwrightEwmh\0",
    )
    .unwrap()
    .check()
    .unwrap();
    conn.change_property32(
        PropMode::REPLACE,
        window,
        wm_pid,
        AtomEnum::CARDINAL,
        &[std::process::id()],
    )
    .unwrap()
    .check()
    .unwrap();
    conn.change_property8(
        PropMode::REPLACE,
        window,
        wm_name,
        utf8,
        b"Semwright Openbox Fixture",
    )
    .unwrap()
    .check()
    .unwrap();
    conn.change_property32(
        PropMode::REPLACE,
        window,
        wm_protocols,
        AtomEnum::ATOM,
        &[wm_delete],
    )
    .unwrap()
    .check()
    .unwrap();
    conn.map_window(window).unwrap().check().unwrap();
    conn.flush().unwrap();
    (wm_protocols, wm_delete)
}

async fn wait_for_target(backend: &X11, ctx: &Context) -> NativeTarget {
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if let Ok(listed) = backend.execute(ctx, "window.list", &json!({})).await
                && let Some(row) = listed["windows"].as_array().and_then(|rows| {
                    rows.iter()
                        .find(|row| row["title"] == "Semwright Openbox Fixture")
                })
            {
                return serde_json::from_value::<NativeTarget>(row["ref"]["$ref"].clone())
                    .expect("managed target");
            }
            tokio::time::sleep(Duration::from_millis(75)).await;
        }
    })
    .await
    .expect("Openbox must publish the client through _NET_CLIENT_LIST")
}

fn wait_geometry(conn: &RustConnection, window: u32, width: u16, height: u16) {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        let geometry = conn.get_geometry(window).unwrap().reply().unwrap();
        if geometry.width == width && geometry.height == height {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "Openbox did not apply requested EWMH geometry"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn wait_delete_message(conn: &RustConnection, window: u32, wm_protocols: u32, wm_delete: u32) {
    let deadline = Instant::now() + Duration::from_secs(4);
    loop {
        while let Some(event) = conn.poll_for_event().unwrap() {
            if let Event::ClientMessage(message) = event
                && message.window == window
                && message.type_ == wm_protocols
                && message.data.as_data32()[0] == wm_delete
            {
                return;
            }
        }
        assert!(
            Instant::now() < deadline,
            "Openbox did not translate _NET_CLOSE_WINDOW to WM_DELETE_WINDOW"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an isolated X server with a real EWMH window manager"]
async fn real_openbox_ewmh_controls_focus_geometry_close_and_stale_ref() {
    if std::env::var_os("SEMWRIGHT_TEST_X11_EWMH").is_none() {
        return;
    }
    assert_eq!(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        Some("x11")
    );

    let (fixture, screen) = x11rb::connect(None).expect("fixture X11 connection");
    let root = fixture.setup().roots[screen].root;
    assert!(
        wm_is_ewmh(&fixture, root),
        "test requires a real EWMH window manager"
    );

    let window = fixture.generate_id().unwrap();
    let (wm_protocols, wm_delete) = create_managed_window(&fixture, root, window);

    let backend = X11::default();
    let ctx = context();
    let features = backend.probe().await;
    assert!(features.iter().any(|feature| {
        feature.capability == "window.manage" && feature.status == CapabilityStatus::Supported
    }));

    let target = wait_for_target(&backend, &ctx).await;
    assert_eq!(target.identity, window.to_string());
    backend.validate(&target).await.expect("fresh target");

    backend
        .execute(
            &ctx,
            "window.focus",
            &json!({"_target":serde_json::to_value(&target).unwrap()}),
        )
        .await
        .expect("focus through EWMH");
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            if backend.is_focused(&target).await.unwrap_or(false) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .expect("Openbox should focus the EWMH client");

    backend
        .execute(
            &ctx,
            "window.resize",
            &json!({
                "_target":serde_json::to_value(&target).unwrap(),
                "width":520,
                "height":300
            }),
        )
        .await
        .expect("resize through EWMH");
    wait_geometry(&fixture, window, 520, 300);

    backend
        .execute(
            &ctx,
            "window.move",
            &json!({
                "_target":serde_json::to_value(&target).unwrap(),
                "x":90,
                "y":110
            }),
        )
        .await
        .expect("move through EWMH");

    backend
        .execute(
            &ctx,
            "window.close",
            &json!({"_target":serde_json::to_value(&target).unwrap()}),
        )
        .await
        .expect("close through EWMH");
    wait_delete_message(&fixture, window, wm_protocols, wm_delete);
    fixture.destroy_window(window).unwrap().check().unwrap();
    fixture.flush().unwrap();

    let stale = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            match backend.validate(&target).await {
                Err(error) if error.code == ErrorCode::StaleReference => break error,
                _ => tokio::time::sleep(Duration::from_millis(60)).await,
            }
        }
    })
    .await
    .expect("destroyed managed X11 window must become stale");
    assert_eq!(stale.code, ErrorCode::StaleReference);
}
