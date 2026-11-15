//! Exercise the MCP forwarding boundary over a real local socket.
use super::*;
use semwright_protocol::{ClientMessage, ServerMessage, read_frame, write_frame};
use semwright_types::{MAX_FRAME, PROTOCOL_VERSION};
use std::os::unix::fs::PermissionsExt;
use tokio::io::AsyncReadExt;
use tokio::net::{UnixListener, UnixStream};

fn endpoint() -> (tempfile::TempDir, PathBuf, UnixListener) {
    let directory = tempfile::tempdir().unwrap();
    semwright_protocol::private_directory(directory.path()).unwrap();
    let socket = directory.path().join("broker.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    (directory, socket, listener)
}

async fn welcome(listener: &UnixListener) -> UnixStream {
    let (mut stream, _) = listener.accept().await.unwrap();
    let hello: ClientMessage = read_frame(&mut stream).await.unwrap();
    assert!(matches!(hello, ClientMessage::Hello { version, .. } if version == PROTOCOL_VERSION));
    write_frame(
        &mut stream,
        &ServerMessage::Welcome {
            version: PROTOCOL_VERSION,
            session: unique_id(),
        },
    )
    .await
    .unwrap();
    stream
}

fn request(text: String) -> ExecuteRequest {
    ExecuteRequest {
        command: "filesystem.write".into(),
        args: json!({"root": "workspace", "path": "file.txt", "text": text}),
        dry_run: false,
        backend: None,
    }
}

#[tokio::test]
async fn oversized_request_remains_known_without_sending_execute() {
    let (_directory, socket, listener) = endpoint();
    let peer = tokio::spawn(async move {
        let mut stream = welcome(&listener).await;
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), stream.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0,
            "A locally rejected request must close without sending Execute"
        );
    });
    let server = Server::new(socket, None).unwrap();
    let envelope = tokio::time::timeout(
        Duration::from_secs(5),
        server.forward(request("x".repeat(MAX_FRAME)), CancellationToken::new()),
    )
    .await
    .unwrap();
    peer.await.unwrap();
    assert!(!envelope.ok);
    let error = envelope.error.unwrap();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(error.outcome_known, "{error:?}");
}

#[tokio::test]
async fn lost_response_remains_uncertain_without_reconnecting() {
    let (_directory, socket, listener) = endpoint();
    let peer = tokio::spawn(async move {
        let mut stream = welcome(&listener).await;
        let message: ClientMessage = read_frame(&mut stream).await.unwrap();
        assert!(matches!(message, ClientMessage::Execute { request, .. }
            if request.command == "filesystem.write"));
        drop(stream);
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err(),
            "A lost response must not cause a new connection or replay"
        );
    });
    let server = Server::new(socket, None).unwrap();
    let envelope = tokio::time::timeout(
        Duration::from_secs(5),
        server.forward(request("change".into()), CancellationToken::new()),
    )
    .await
    .unwrap();
    peer.await.unwrap();
    assert!(!envelope.ok);
    assert!(!envelope.error.unwrap().outcome_known);
}

#[tokio::test]
async fn cancellation_after_dispatch_remains_uncertain() {
    let (_directory, socket, listener) = endpoint();
    let cancellation = CancellationToken::new();
    let peer_cancellation = cancellation.clone();
    let peer = tokio::spawn(async move {
        let mut stream = welcome(&listener).await;
        let message: ClientMessage = read_frame(&mut stream).await.unwrap();
        let ClientMessage::Execute { id, .. } = message else {
            panic!("Expected one Execute before cancellation");
        };
        peer_cancellation.cancel();
        let cancelled: ClientMessage =
            tokio::time::timeout(Duration::from_secs(5), read_frame(&mut stream))
                .await
                .unwrap()
                .unwrap();
        assert!(
            matches!(cancelled, ClientMessage::Cancel { id: cancelled_id }
            if cancelled_id == id)
        );
    });
    let server = Server::new(socket, None).unwrap();
    let envelope = tokio::time::timeout(
        Duration::from_secs(5),
        server.forward(request("change".into()), cancellation),
    )
    .await
    .unwrap();
    peer.await.unwrap();
    assert!(!envelope.ok);
    let error = envelope.error.unwrap();
    assert_eq!(error.code, ErrorCode::Cancelled);
    assert!(!error.outcome_known, "{error:?}");
}
