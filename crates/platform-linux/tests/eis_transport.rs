#![cfg(target_os = "linux")]

use reis::{
    PendingRequestResult,
    eis::{self, device::DeviceType},
    request::{Connection, DeviceCapability, EisRequest, EisRequestConverter},
};
use semwright_platform_linux::eis::{EisClient, Requested};
use std::{
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
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

fn server(stream: UnixStream, seen: Arc<Mutex<Vec<String>>>) {
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
    let _seat = connection.add_seat(
        Some("semwright-test"),
        DeviceCapability::Pointer
            | DeviceCapability::Button
            | DeviceCapability::Scroll
            | DeviceCapability::Text,
    );
    connection.flush().unwrap();
    record(&seen, "connected");

    let mut _pointer: Option<reis::request::Device> = None;
    let mut _text: Option<reis::request::Device> = None;
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
                    record(&seen, "bind");
                }
                EisRequest::DeviceStartEmulating(_) => record(&seen, "start"),
                EisRequest::TextKeysym(event) => {
                    record(&seen, format!("keysym:{}", event.keysym));
                }
                EisRequest::TextUtf8(event) => {
                    record(&seen, format!("text:{}", event.text));
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
    let thread = std::thread::spawn(move || server(server_stream, server_seen));

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
