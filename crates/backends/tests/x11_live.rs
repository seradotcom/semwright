use semwright_backend_api::{Backend, Context};
use semwright_backends::x11::X11;
use semwright_types::{ErrorCode, NativeTarget};
use serde_json::json;
use tokio_util::sync::CancellationToken;
use x11rb::{
    COPY_DEPTH_FROM_PARENT, connection::Connection, protocol::xproto::*,
    rust_connection::RustConnection, wrapper::ConnectionExt as _,
};

fn atom(conn: &RustConnection, name: &str) -> u32 {
    conn.intern_atom(false, name.as_bytes())
        .unwrap()
        .reply()
        .unwrap()
        .atom
}

fn publish_window(conn: &RustConnection, root: u32, window: u32) {
    let wm_class = atom(conn, "WM_CLASS");
    let wm_pid = atom(conn, "_NET_WM_PID");
    let wm_name = atom(conn, "_NET_WM_NAME");
    let client_list = atom(conn, "_NET_CLIENT_LIST");
    let active = atom(conn, "_NET_ACTIVE_WINDOW");

    conn.change_property8(
        PropMode::REPLACE,
        window,
        wm_class,
        AtomEnum::STRING,
        b"semwright-test\0SemwrightTest\0",
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
        AtomEnum::STRING,
        b"Semwright X11 fixture",
    )
    .unwrap()
    .check()
    .unwrap();
    conn.change_property32(
        PropMode::REPLACE,
        root,
        client_list,
        AtomEnum::WINDOW,
        &[window],
    )
    .unwrap()
    .check()
    .unwrap();
    conn.change_property32(PropMode::REPLACE, root, active, AtomEnum::WINDOW, &[window])
        .unwrap()
        .check()
        .unwrap();
    conn.map_window(window).unwrap().check().unwrap();
    conn.flush().unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an isolated native X11 server such as Xvfb"]
async fn real_x11_window_ref_is_focus_checked_and_stales_after_destroy() {
    if std::env::var_os("SEMWRIGHT_TEST_X11").is_none() {
        return;
    }
    assert_eq!(
        std::env::var("XDG_SESSION_TYPE").ok().as_deref(),
        Some("x11"),
        "live fixture must not accidentally target XWayland from a Wayland session"
    );

    let (fixture, screen) = x11rb::connect(None).expect("connect fixture X11 client");
    let root = fixture.setup().roots[screen].root;
    let window = fixture.generate_id().unwrap();
    fixture
        .create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            root,
            20,
            20,
            320,
            180,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().background_pixel(fixture.setup().roots[screen].white_pixel),
        )
        .unwrap()
        .check()
        .unwrap();
    publish_window(&fixture, root, window);

    let backend = X11::default();
    let context = Context {
        session: "x11-live".into(),
        cancellation: CancellationToken::new(),
    };

    let features = backend.probe().await;
    assert!(features.iter().any(|feature| {
        feature.capability == "window.manage"
            && feature.status == semwright_types::CapabilityStatus::Supported
    }));

    let listed = backend
        .execute(&context, "window.list", &json!({}))
        .await
        .unwrap();
    let row = listed["windows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["title"] == "Semwright X11 fixture")
        .expect("fixture window should be discoverable");
    assert_eq!(row["focused"], true);
    let target: NativeTarget = serde_json::from_value(row["ref"]["$ref"].clone()).unwrap();
    assert_eq!(target.identity, window.to_string());
    assert!(target.revision > 0);
    backend.validate(&target).await.unwrap();
    assert!(backend.is_focused(&target).await.unwrap());

    fixture.destroy_window(window).unwrap().check().unwrap();
    fixture.flush().unwrap();

    let stale = backend.validate(&target).await.unwrap_err();
    assert_eq!(stale.code, ErrorCode::StaleReference);
}
