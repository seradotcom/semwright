#![cfg(target_os = "windows")]

use semwright_backend_api::{Backend, Context};
use semwright_platform_windows::Windows;
use semwright_types::{ErrorCode, NativeTarget, unique_id};
use serde_json::{Value, json};
use std::{sync::mpsc, thread, time::Duration};
use tokio_util::sync::CancellationToken;
use windows::{
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM},
        System::LibraryLoader::GetModuleHandleW,
        UI::{
            HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext},
            WindowsAndMessaging::*,
        },
    },
    core::{PCWSTR, w},
};

const ID_BUTTON: isize = 101;
const ID_EDIT: isize = 102;
const ID_PASSWORD: isize = 103;

unsafe extern "system" fn fixture_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_COMMAND if (wparam.0 & 0xffff) as isize == ID_BUTTON => {
            // SAFETY: hwnd is the live fixture window owned by this UI thread.
            if let Ok(edit) = unsafe { GetDlgItem(Some(hwnd), ID_EDIT as i32) } {
                // SAFETY: edit is a child HWND returned synchronously from the live fixture.
                let _ = unsafe { SetWindowTextW(edit, w!("invoked")) };
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            // SAFETY: called on the fixture UI thread while processing WM_DESTROY.
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        // SAFETY: forwards the live HWND and unmodified message tuple.
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

unsafe fn child(
    parent: HWND,
    class: PCWSTR,
    text: PCWSTR,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    id: isize,
) -> HWND {
    // SAFETY: parent/class/text remain valid for this synchronous child creation call.
    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            text,
            WS_CHILD | WS_VISIBLE | style,
            x,
            y,
            width,
            height,
            Some(parent),
            Some(HMENU(id as *mut core::ffi::c_void)),
            None,
            None,
        )
    }
    .expect("fixture child window")
}

fn start_fixture() -> (thread::JoinHandle<()>, isize, String) {
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let title = format!("Semwright native UIA fixture {}", std::process::id());
    let thread_title = title.clone();
    let handle = thread::spawn(move || {
        // SAFETY: querying the current process module does not transfer ownership.
        let module = unsafe { GetModuleHandleW(None) }.expect("fixture module");
        let instance = HINSTANCE(module.0);
        let class = w!("SemwrightNativeUiaFixture");
        let wc = WNDCLASSW {
            // SAFETY: IDC_ARROW is a predefined shared system cursor.
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.expect("fixture cursor"),
            hInstance: instance,
            lpszClassName: class,
            lpfnWndProc: Some(fixture_proc),
            ..Default::default()
        };
        // SAFETY: wc contains a live module handle, static class name and valid window proc.
        unsafe { RegisterClassW(&wc) };

        let title_wide: Vec<u16> = thread_title.encode_utf16().chain([0]).collect();
        // SAFETY: class is registered and title_wide remains live during window creation.
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST,
                class,
                PCWSTR(title_wide.as_ptr()),
                WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                120,
                120,
                520,
                240,
                None,
                None,
                Some(instance),
                None,
            )
        }
        .expect("fixture top-level window");

        // SAFETY: hwnd is live and all child class/text pointers are static wide strings.
        unsafe {
            child(
                hwnd,
                w!("BUTTON"),
                w!("Invoke me"),
                WINDOW_STYLE(BS_PUSHBUTTON as u32),
                20,
                20,
                130,
                32,
                ID_BUTTON,
            );
            child(
                hwnd,
                w!("EDIT"),
                w!("fixture text"),
                WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
                170,
                20,
                240,
                32,
                ID_EDIT,
            );
            child(
                hwnd,
                w!("EDIT"),
                w!("secret"),
                WS_BORDER | WINDOW_STYLE(ES_PASSWORD as u32),
                170,
                72,
                240,
                32,
                ID_PASSWORD,
            );
        }

        // Keep the native fixture visibly above runner bootstrap/setup windows. UIA
        // ElementFromPoint intentionally returns the topmost accessible element, so the
        // hit-test assertion is only meaningful if this fixture actually owns the pixels
        // described by its UIA bounds. This changes test z-order only; production targeting
        // semantics are unchanged.
        // SAFETY: hwnd is the live fixture-owned top-level window created on this thread;
        // the calls only adjust that window's z-order/focus and retain no borrowed handles.
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
            )
            .expect("fixture topmost visibility");
            let _ = SetForegroundWindow(hwnd);
        }

        ready_tx
            .send(hwnd.0 as isize)
            .expect("fixture readiness channel");

        let mut msg = MSG::default();
        // SAFETY: msg is writable storage owned by this UI thread.
        while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
            // SAFETY: msg was initialized by GetMessageW for this thread.
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    });
    let hwnd = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("fixture started");
    (handle, hwnd, title)
}

fn context() -> Context {
    Context {
        session: "windows-native-fixture".into(),
        request_id: unique_id(),
        cancellation: CancellationToken::new(),
    }
}

fn fixture_owns_point(hwnd: isize, x: i32, y: i32) -> bool {
    let fixture = HWND(hwnd as *mut core::ffi::c_void);
    // SAFETY: fixture is a live test-owned top-level HWND. Reasserting topmost immediately
    // before WindowFromPoint closes the race with hosted-runner bootstrap windows that may
    // independently adjust z-order between fixture placement and the ownership check.
    unsafe {
        let _ = SetWindowPos(
            fixture,
            Some(HWND_TOPMOST),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
        );
        let _ = BringWindowToTop(fixture);
        let hit = WindowFromPoint(POINT { x, y });
        !hit.is_invalid() && GetAncestor(hit, GA_ROOT) == fixture
    }
}

fn fixture_button_points(hwnd: isize) -> Vec<(i32, i32)> {
    let fixture = HWND(hwnd as *mut core::ffi::c_void);
    // SAFETY: fixture is live for this test and ID_BUTTON names a child created by start_fixture.
    let button =
        unsafe { GetDlgItem(Some(fixture), ID_BUTTON as i32) }.expect("fixture button HWND");
    let mut rect = RECT::default();
    // SAFETY: button is a live child HWND and rect is writable local storage.
    unsafe { GetWindowRect(button, &mut rect) }.expect("fixture button physical bounds");
    let width = (rect.right - rect.left).max(1);
    let height = (rect.bottom - rect.top).max(1);
    let inset_x = (width / 4).clamp(1, 8);
    let inset_y = (height / 4).clamp(1, 8);
    vec![
        (rect.left + width / 2, rect.top + height / 2),
        (rect.left + inset_x, rect.top + inset_y),
        (rect.right - inset_x - 1, rect.top + inset_y),
        (rect.left + inset_x, rect.bottom - inset_y - 1),
        (rect.right - inset_x - 1, rect.bottom - inset_y - 1),
    ]
}

async fn snapshot_with_owned_hit_point(
    backend: &Windows,
    ctx: &Context,
    window_target: &NativeTarget,
    hwnd: isize,
) -> (Value, i32, i32) {
    let fixture = HWND(hwnd as *mut core::ffi::c_void);
    // SAFETY: read-only query for the primary display width.
    let screen_width = unsafe { GetSystemMetrics(SM_CXSCREEN) }.max(640);
    // SAFETY: read-only query for the primary display height.
    let screen_height = unsafe { GetSystemMetrics(SM_CYSCREEN) }.max(480);
    let max_left = (screen_width - 540).max(0);
    let max_top = (screen_height - 280).max(0);
    let candidates = [
        (120.min(max_left), 120.min(max_top)),
        (8.min(max_left), 8.min(max_top)),
        (max_left, 8.min(max_top)),
        (8.min(max_left), max_top),
        (max_left, max_top),
        (max_left / 2, max_top / 2),
    ];
    let mut last_point = (0, 0);

    for (left, top) in candidates {
        // SAFETY: fixture remains live for the duration of this test; only its test z-order and
        // position change. Production UIA targeting is not affected.
        unsafe {
            SetWindowPos(
                fixture,
                Some(HWND_TOPMOST),
                left,
                top,
                0,
                0,
                SWP_NOSIZE | SWP_SHOWWINDOW,
            )
            .expect("position fixture for native hit-test");
            let _ = BringWindowToTop(fixture);
            let _ = SetForegroundWindow(fixture);
        }
        tokio::time::sleep(Duration::from_millis(120)).await;

        for (x, y) in fixture_button_points(hwnd) {
            last_point = (x, y);
            let mut owned = false;
            for _ in 0..20 {
                if fixture_owns_point(hwnd, x, y) {
                    owned = true;
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
            if !owned {
                continue;
            }

            let snapshot = backend
                .execute(
                    ctx,
                    "ui.snapshot",
                    &json!({"_target":window_target.clone()}),
                )
                .await
                .expect("scoped UIA snapshot after fixture placement");
            let Some(button) = snapshot["nodes"].as_array().and_then(|nodes| {
                nodes
                    .iter()
                    .find(|node| node["role"] == "button" && node["name"] == "Invoke me")
            }) else {
                continue;
            };
            let Some(bounds) = button["bounds"].as_object() else {
                continue;
            };
            assert!(
                bounds["width"].as_f64().is_some_and(|width| width > 0.0)
                    && bounds["height"].as_f64().is_some_and(|height| height > 0.0),
                "UIA button bounds must remain non-empty: {button}"
            );
            return (snapshot, x, y);
        }
    }

    // SAFETY: diagnostic-only lookup using the final bounded screen point.
    let observed = unsafe {
        WindowFromPoint(POINT {
            x: last_point.0,
            y: last_point.1,
        })
    };
    // SAFETY: WindowFromPoint returns either null or a live window; GetAncestor tolerates null.
    let observed_root = unsafe { GetAncestor(observed, GA_ROOT) };
    panic!(
        "fixture could not own any bounded hit-test placement; last=({},{}) screen={}x{} observed={:?} root={:?} fixture={:?}",
        last_point.0, last_point.1, screen_width, screen_height, observed, observed_root, fixture
    );
}

fn native_ref(value: &Value) -> NativeTarget {
    serde_json::from_value(value["ref"]["$ref"].clone()).expect("native target marker")
}

fn find_node(snapshot: &Value, predicate: impl Fn(&Value) -> bool) -> &Value {
    snapshot["nodes"]
        .as_array()
        .expect("snapshot nodes")
        .iter()
        .find(|node| predicate(node))
        .expect("semantic node")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn real_win32_fixture_exercises_uia_without_pixel_fallback() {
    // Keep UIA bounding rectangles and Win32 point ownership in the same physical-pixel
    // coordinate space. This must happen before the fixture creates any HWND.
    // SAFETY: process DPI awareness is configured once at test startup before GUI creation.
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) }
        .expect("native UIA fixture requires per-monitor-v2 DPI awareness");
    let (fixture, hwnd, title) = start_fixture();
    let artifacts = std::env::temp_dir().join(format!("semwright-uia-native-{}", unique_id()));
    let backend = Windows::new(&artifacts).expect("Windows backend");
    let ctx = context();

    let windows = backend
        .execute(&ctx, "window.list", &json!({}))
        .await
        .expect("window enumeration");
    let row = windows["windows"]
        .as_array()
        .expect("windows")
        .iter()
        .find(|row| row["title"] == title)
        .expect("fixture window");
    let window_target = native_ref(row);

    let (snapshot, hit_x, hit_y) =
        snapshot_with_owned_hit_point(&backend, &ctx, &window_target, hwnd).await;
    assert_eq!(snapshot["partial"], false);

    let button = find_node(&snapshot, |node| {
        node["role"] == "button" && node["name"] == "Invoke me"
    });
    assert!(
        button["actions"]
            .as_array()
            .is_some_and(|actions| actions.iter().any(|action| action == "click"))
    );
    let button_target = native_ref(button);

    let inspected = backend
        .execute(&ctx, "ui.inspect", &json!({"_target":button_target}))
        .await
        .expect("exact UIA inspection");
    assert_eq!(inspected["node"]["name"], "Invoke me");
    assert_eq!(inspected["semantic_coverage"], "exact_ref");

    let hit = backend
        .execute(
            &ctx,
            "ui.hit_test",
            &json!({"x":i64::from(hit_x),"y":i64::from(hit_y)}),
        )
        .await
        .expect("native UIA hit-test");
    assert_eq!(hit["node"]["name"], "Invoke me");
    assert_eq!(hit["semantic_coverage"], "native_hit_test");

    backend
        .execute(
            &ctx,
            "ui.invoke",
            &json!({"_target":button_target,"action":"click"}),
        )
        .await
        .expect("InvokePattern");
    tokio::time::sleep(Duration::from_millis(150)).await;

    let refreshed = backend
        .execute(&ctx, "ui.snapshot", &json!({"_target":window_target}))
        .await
        .expect("snapshot after invoke");
    let edit = find_node(&refreshed, |node| {
        node["role"] == "text"
            && node["facets"]["text"]["password"] != true
            && node["actions"]
                .as_array()
                .is_some_and(|actions| actions.iter().any(|action| action == "set_text"))
    });
    let edit_target = native_ref(edit);
    let text = backend
        .execute(&ctx, "ui.read_text", &json!({"_target":edit_target}))
        .await
        .expect("ValuePattern read");
    assert_eq!(text["text"], "invoked");

    let password = find_node(&refreshed, |node| {
        node["facets"]["text"]["password"] == true
    });
    assert_eq!(password["name"], "");
    let password_target = native_ref(password);
    let denied = backend
        .execute(&ctx, "ui.read_text", &json!({"_target":password_target}))
        .await
        .expect_err("password text must stay protected");
    assert_eq!(denied.code, ErrorCode::PolicyDenied);

    // SAFETY: hwnd came from the live fixture and WM_CLOSE is a standard async message.
    unsafe {
        PostMessageW(
            Some(HWND(hwnd as *mut core::ffi::c_void)),
            WM_CLOSE,
            WPARAM(0),
            LPARAM(0),
        )
    }
    .expect("close fixture");
    fixture.join().expect("fixture thread");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let stale = backend
        .validate(&button_target)
        .await
        .expect_err("destroyed UIA ref must be stale");
    assert_eq!(stale.code, ErrorCode::StaleReference);

    backend.shutdown().await.expect("shutdown backend");
    std::fs::remove_dir_all(&artifacts).expect("remove private artifact directory");
}
