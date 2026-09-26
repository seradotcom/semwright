#![cfg(target_os = "linux")]

use reis::{
    PendingRequestResult,
    eis::{self, device::DeviceType},
    request::{Connection, DeviceCapability, EisRequest, EisRequestConverter},
};
use semwright_platform_linux::eis::{EisClient, Requested};
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::{Seek, SeekFrom, Write},
    os::{fd::AsFd, unix::net::UnixStream},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use xkbcommon_dl::{
    xkb_context_flags, xkb_keymap_compile_flags, xkb_keymap_format, xkb_rule_names,
    xkbcommon_handle,
};

fn record(seen: &Arc<Mutex<Vec<String>>>, value: impl Into<String>) {
    seen.lock().unwrap().push(value.into());
}

fn add_device(
    seat: &reis::request::Seat,
    connection: &Connection,
    name: &str,
    capabilities: reis::enumflags2::BitFlags<DeviceCapability>,
) -> reis::request::Device {
    let device = seat.add_device(Some(name), DeviceType::Virtual, capabilities, |_| {});
    device.resumed();
    connection.flush().unwrap();
    device
}
fn protocol_request(context: &eis::Context) -> Option<eis::Request> {
    match context.pending_request()? {
        PendingRequestResult::Request(request) => Some(request),
        PendingRequestResult::ParseError(error) => panic!("EIS parse error: {error}"),
        PendingRequestResult::InvalidObject(id) => panic!("EIS invalid object: {id}"),
    }
}

#[derive(Clone, Copy)]
enum KeyboardMode {
    Text,
    Keycode,
    KeycodeWithKeymap,
}

fn us_keymap_file() -> File {
    let xkb = xkbcommon_handle();
    // SAFETY: the dynamically resolved xkbcommon function accepts this valid enum.
    let context = unsafe { (xkb.xkb_context_new)(xkb_context_flags::XKB_CONTEXT_NO_FLAGS) };
    assert!(!context.is_null());
    let layout = CString::new("us").unwrap();
    let names = xkb_rule_names {
        rules: std::ptr::null(),
        model: std::ptr::null(),
        layout: layout.as_ptr(),
        variant: std::ptr::null(),
        options: std::ptr::null(),
    };
    // SAFETY: context and names remain live for this call.
    let keymap = unsafe {
        (xkb.xkb_keymap_new_from_names)(
            context,
            &names,
            xkb_keymap_compile_flags::XKB_KEYMAP_COMPILE_NO_FLAGS,
        )
    };
    assert!(!keymap.is_null());
    // SAFETY: keymap is live and xkbcommon returns an allocated NUL-terminated string.
    let raw = unsafe {
        (xkb.xkb_keymap_get_as_string)(keymap, xkb_keymap_format::XKB_KEYMAP_FORMAT_TEXT_V1)
    };
    assert!(!raw.is_null());
    // SAFETY: raw is a NUL-terminated string returned by xkbcommon.
    let bytes = unsafe { CStr::from_ptr(raw) }.to_bytes_with_nul().to_vec();
    // SAFETY: xkb_keymap_get_as_string transfers ownership to the caller via malloc.
    unsafe { libc::free(raw as *mut libc::c_void) };
    // SAFETY: both objects were created above and are released exactly once.
    unsafe {
        (xkb.xkb_keymap_unref)(keymap);
        (xkb.xkb_context_unref)(context);
    }
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&bytes).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file
}

fn add_keyboard_device(
    seat: &reis::request::Seat,
    connection: &Connection,
    keymap: &File,
) -> reis::request::Device {
    let size = u32::try_from(keymap.metadata().unwrap().len()).unwrap();
    let device = seat.add_device(
        Some("keyboard"),
        DeviceType::Virtual,
        DeviceCapability::Keyboard.into(),
        |device| {
            let keyboard = device.interface::<eis::Keyboard>().unwrap();
            keyboard.keymap(eis::keyboard::KeymapType::Xkb, size, keymap.as_fd());
        },
    );
    device.resumed();
    connection.flush().unwrap();
    device
}

fn server(stream: UnixStream, seen: Arc<Mutex<Vec<String>>>, keyboard_mode: KeyboardMode) {
    let context = eis::Context::new(stream).unwrap();
    let mut handshaker = reis::handshake::EisHandshaker::new(&context, 1);
    let deadline = Instant::now() + Duration::from_secs(5);
    let response = 'handshake: loop {
        context.read().unwrap();
        while let Some(request) = protocol_request(&context) {
            if let Some(response) = handshaker.handle_request(request).unwrap() {
                break 'handshake response;
            }
        }
        assert!(Instant::now() < deadline, "EIS server handshake timed out");
        std::thread::sleep(Duration::from_millis(2));
    };

    let mut converter = EisRequestConverter::new(&context, response, 1);
    let connection = converter.handle().clone();
    let mut advertised =
        DeviceCapability::Pointer | DeviceCapability::Button | DeviceCapability::Scroll;
    advertised.insert(match keyboard_mode {
        KeyboardMode::Text => DeviceCapability::Text,
        KeyboardMode::Keycode | KeyboardMode::KeycodeWithKeymap => DeviceCapability::Keyboard,
    });
    let _seat = connection.add_seat(Some("semwright-test"), advertised);
    connection.flush().unwrap();
    record(&seen, "connected");

    let mut _pointer: Option<reis::request::Device> = None;
    let mut _text: Option<reis::request::Device> = None;
    let mut _keyboard: Option<reis::request::Device> = None;
    let keymap = matches!(keyboard_mode, KeyboardMode::KeycodeWithKeymap).then(us_keymap_file);
    let deadline = Instant::now() + Duration::from_secs(10);
    'events: loop {
        context.read().unwrap();
        while let Some(request) = protocol_request(&context) {
            converter.handle_request(request).unwrap();
        }
        while let Some(request) = converter.next_request() {
            match request {
                EisRequest::Bind(bind) => {
                    if bind.capabilities.contains(DeviceCapability::Pointer) {
                        _pointer = Some(add_device(
                            &bind.seat,
                            &connection,
                            "pointer",
                            DeviceCapability::Pointer
                                | DeviceCapability::Button
                                | DeviceCapability::Scroll,
                        ));
                    }
                    if bind.capabilities.contains(DeviceCapability::Text) {
                        _text = Some(add_device(
                            &bind.seat,
                            &connection,
                            "text",
                            DeviceCapability::Text.into(),
                        ));
                    }
                    if bind.capabilities.contains(DeviceCapability::Keyboard) {
                        _keyboard = Some(if let Some(keymap) = keymap.as_ref() {
                            add_keyboard_device(&bind.seat, &connection, keymap)
                        } else {
                            add_device(
                                &bind.seat,
                                &connection,
                                "keyboard",
                                DeviceCapability::Keyboard.into(),
                            )
                        });
                    }
                    record(&seen, "bind");
                }
                EisRequest::DeviceStartEmulating(_) => record(&seen, "start"),
                EisRequest::TextKeysym(event) => {
                    record(&seen, format!("keysym:{}", event.keysym));
                }
                EisRequest::TextUtf8(event) => {
                    record(&seen, format!("text:{}", event.text));
                }
                EisRequest::KeyboardKey(event) => {
                    record(&seen, format!("key:{}:{:?}", event.key, event.state));
                }
                EisRequest::PointerMotion(event) => {
                    record(&seen, format!("motion:{:.1}:{:.1}", event.dx, event.dy));
                }
                EisRequest::Button(event) => {
                    record(&seen, format!("button:{}", event.button));
                }
                EisRequest::ScrollDelta(event) => {
                    record(&seen, format!("scroll:{:.1}:{:.1}", event.dx, event.dy));
                }
                EisRequest::Disconnect => break 'events,
                _ => {}
            }
        }
        connection.flush().unwrap();
        assert!(Instant::now() < deadline, "EIS server event loop timed out");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[tokio::test]
async fn real_eis_protocol_negotiates_and_sends_input() {
    let (server_stream, client_stream) = UnixStream::pair().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let server_seen = seen.clone();
    let thread = std::thread::spawn(move || server(server_stream, server_seen, KeyboardMode::Text));

    let client = EisClient::connect(
        client_stream,
        Requested {
            keyboard: true,
            pointer: true,
        },
    )
    .await
    .unwrap();
    let caps = client.capabilities();
    assert!(caps.text && caps.pointer && caps.button && caps.scroll);
    client.keysym(0x61).await.unwrap();
    client.type_text("héllo").await.unwrap();
    client.motion(3.5, -2.0).await.unwrap();
    client.click(272).await.unwrap();
    client.scroll(1.0, -4.0).await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let snapshot = seen.lock().unwrap().clone();
        if snapshot.iter().any(|v| v == "text:héllo")
            && snapshot.iter().any(|v| v == "motion:3.5:-2.0")
            && snapshot.iter().any(|v| v == "button:272")
            && snapshot.iter().any(|v| v == "scroll:1.0:-4.0")
        {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "EIS events: {snapshot:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    client.stop().await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), client.wait_closed())
        .await
        .expect("EIS client should observe transport shutdown");
    assert!(client.is_closed());
    thread.join().unwrap();
    let snapshot = seen.lock().unwrap();
    assert!(snapshot.iter().any(|v| v == "keysym:97"));
    assert!(snapshot.iter().any(|v| v == "start"));
}

#[tokio::test]
async fn keycode_only_keyboard_negotiates_without_claiming_text_support() {
    let (server_stream, client_stream) = UnixStream::pair().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let server_seen = seen.clone();
    let thread =
        std::thread::spawn(move || server(server_stream, server_seen, KeyboardMode::Keycode));

    let client = EisClient::connect(
        client_stream,
        Requested {
            keyboard: true,
            pointer: false,
        },
    )
    .await
    .unwrap();
    let caps = client.capabilities();
    assert!(caps.keyboard);
    assert!(!caps.text);

    let error = client.keysym(0x61).await.unwrap_err();
    assert_eq!(error.code, semwright_types::ErrorCode::Unsupported);
    assert!(error.message.contains("XKB keymap"));
    let error = client.type_text("a").await.unwrap_err();
    assert_eq!(error.code, semwright_types::ErrorCode::Unsupported);
    assert!(error.message.contains("XKB keymap"));

    client.stop().await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), client.wait_closed())
        .await
        .expect("EIS client should observe transport shutdown");
    thread.join().unwrap();
}

#[tokio::test]
async fn keycode_keyboard_uses_announced_xkb_keymap_for_text_and_keysym() {
    let (server_stream, client_stream) = UnixStream::pair().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let server_seen = seen.clone();
    let thread = std::thread::spawn(move || {
        server(server_stream, server_seen, KeyboardMode::KeycodeWithKeymap)
    });

    let client = EisClient::connect(
        client_stream,
        Requested {
            keyboard: true,
            pointer: false,
        },
    )
    .await
    .unwrap();
    assert!(client.capabilities().keyboard);
    assert!(!client.capabilities().text);

    client.keysym(0x61).await.unwrap();
    client.type_text("A!").await.unwrap();

    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    loop {
        let snapshot = seen.lock().unwrap().clone();
        let keys = snapshot
            .iter()
            .filter(|row| row.starts_with("key:"))
            .cloned()
            .collect::<Vec<_>>();
        if keys.len() >= 10 {
            assert_eq!(
                &keys[..10],
                &[
                    "key:30:Press",
                    "key:30:Released",
                    "key:42:Press",
                    "key:30:Press",
                    "key:30:Released",
                    "key:42:Released",
                    "key:42:Press",
                    "key:2:Press",
                    "key:2:Released",
                    "key:42:Released",
                ]
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "EIS key events: {keys:?}"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    client.stop().await.unwrap();
    tokio::time::timeout(Duration::from_secs(1), client.wait_closed())
        .await
        .expect("EIS client should observe transport shutdown");
    thread.join().unwrap();
}
