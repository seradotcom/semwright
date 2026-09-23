#![cfg(target_os = "linux")]
use semwright_platform_linux::pipewire_capture::{StreamTarget, capture_one, write_png_create_new};
use semwright_types::ErrorCode;
use std::{
    os::{fd::OwnedFd, unix::net::UnixStream},
    path::PathBuf,
    time::Duration,
};
use tokio_util::sync::CancellationToken;

fn target() -> StreamTarget {
    if let Ok(serial) = std::env::var("SEMWRIGHT_TEST_PIPEWIRE_SERIAL") {
        return StreamTarget::Serial(serial.parse().expect("valid PipeWire object.serial"));
    }
    StreamTarget::LegacyNode(
        std::env::var("SEMWRIGHT_TEST_PIPEWIRE_NODE")
            .expect("SEMWRIGHT_TEST_PIPEWIRE_NODE or SERIAL required")
            .parse()
            .expect("valid PipeWire node id"),
    )
}

fn remote_fd() -> OwnedFd {
    let path = PathBuf::from(
        std::env::var("SEMWRIGHT_TEST_PIPEWIRE_SOCKET")
            .expect("SEMWRIGHT_TEST_PIPEWIRE_SOCKET required"),
    );
    UnixStream::connect(path)
        .expect("connect private PipeWire socket")
        .into()
}

#[test]
#[ignore = "requires a private PipeWire daemon and synthetic video source"]
fn real_pipewire_capture_negotiates_frame_png_and_cancellation() {
    if std::env::var_os("SEMWRIGHT_TEST_PIPEWIRE").is_none() {
        return;
    }
    let target = target();
    let frame = capture_one(
        remote_fd(),
        target,
        Duration::from_secs(10),
        CancellationToken::new(),
    )
    .expect("capture one real synthetic PipeWire frame");
    assert_eq!(frame.width, 64);
    assert_eq!(frame.height, 48);
    assert_eq!(
        frame.data.len(),
        frame.row_bytes as usize * frame.height as usize
    );
    assert!(frame.data.iter().any(|byte| *byte != 0));

    let directory = tempfile::tempdir().unwrap();
    let png = directory.path().join("frame.png");
    write_png_create_new(&frame, &png).expect("encode captured frame");
    let bytes = std::fs::read(&png).unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));

    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let error = capture_one(remote_fd(), target, Duration::from_secs(5), cancellation)
        .expect_err("pre-cancelled capture must stop");
    assert_eq!(error.code, ErrorCode::Cancelled);
}
