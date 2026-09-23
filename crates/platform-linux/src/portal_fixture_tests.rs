use std::{
    io::{BufRead, BufReader, Seek, SeekFrom, Write},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex as StdMutex},
};
use tokio_util::sync::CancellationToken;
use zbus::{interface, message::Header};

#[derive(Debug, Default)]
struct FixtureState {
    events: Vec<String>,
    restore_tokens: Vec<Option<String>>,
    persist_modes: Vec<u32>,
    clipboard_requested: bool,
    selection_mime_types: Vec<String>,
    starts: u32,
}

#[derive(Clone)]
struct RemoteFixture(Arc<StdMutex<FixtureState>>);

#[derive(Clone)]
struct ClipboardFixture(Arc<StdMutex<FixtureState>>);

struct PrivateBus {
    child: Child,
    address: String,
}

impl PrivateBus {
    fn start() -> Self {
        let mut child = Command::new("dbus-daemon")
            .args(["--session", "--nofork", "--nopidfile", "--print-address=1"])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("dbus-daemon must be available for the private portal fixture");
        let stdout = child.stdout.take().expect("dbus-daemon stdout");
        let mut reader = BufReader::new(stdout);
        let mut address = String::new();
        reader
            .read_line(&mut address)
            .expect("read private D-Bus address");
        let address = address.trim().to_owned();
        assert!(address.starts_with("unix:"));
        Self { child, address }
    }
}

impl Drop for PrivateBus {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn fdo_failed(message: impl Into<String>) -> zbus::fdo::Error {
    zbus::fdo::Error::Failed(message.into())
}

fn option_string(options: &Options, key: &str) -> Option<String> {
    options
        .get(key)
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| String::try_from(value).ok())
}

fn option_u32(options: &Options, key: &str) -> Option<u32> {
    options
        .get(key)
        .and_then(|value| value.try_clone().ok())
        .and_then(|value| u32::try_from(value).ok())
}

fn sender_component(header: &Header<'_>) -> zbus::fdo::Result<String> {
    let sender = header
        .sender()
        .ok_or_else(|| fdo_failed("portal fixture call has no sender"))?;
    Ok(sender
        .as_str()
        .trim_start_matches(':')
        .replace('.', "_"))
}

fn request_path(header: &Header<'_>, options: &Options) -> zbus::fdo::Result<OwnedObjectPath> {
    let sender = sender_component(header)?;
    let token = option_string(options, "handle_token")
        .ok_or_else(|| fdo_failed("missing handle_token"))?;
    OwnedObjectPath::try_from(format!(
        "/org/freedesktop/portal/desktop/request/{sender}/{token}"
    ))
    .map_err(|_| fdo_failed("invalid fixture request path"))
}

fn session_path(header: &Header<'_>, options: &Options) -> zbus::fdo::Result<OwnedObjectPath> {
    let sender = sender_component(header)?;
    let token = option_string(options, "session_handle_token")
        .ok_or_else(|| fdo_failed("missing session_handle_token"))?;
    OwnedObjectPath::try_from(format!(
        "/org/freedesktop/portal/desktop/session/{sender}/{token}"
    ))
    .map_err(|_| fdo_failed("invalid fixture session path"))
}

async fn response(
    connection: &Connection,
    path: &OwnedObjectPath,
    results: Options,
) -> zbus::fdo::Result<()> {
    connection
        .emit_signal(
            None::<&str>,
            path,
            "org.freedesktop.portal.Request",
            "Response",
            &(0u32, results),
        )
        .await
        .map_err(|error| fdo_failed(error.to_string()))
}

#[interface(name = "org.freedesktop.portal.RemoteDesktop")]
impl RemoteFixture {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        2
    }

    #[zbus(property, name = "AvailableDeviceTypes")]
    fn available_device_types(&self) -> u32 {
        3
    }

    #[zbus(name = "CreateSession")]
    async fn create_session(
        &self,
        options: Options,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let request = request_path(&header, &options)?;
        let session = session_path(&header, &options)?;
        self.0.lock().unwrap().events.push("create".into());
        let mut results = Options::new();
        results.insert("session_handle".into(), option(session.to_string()));
        response(connection, &request, results).await?;
        Ok(request)
    }

    #[zbus(name = "SelectDevices")]
    async fn select_devices(
        &self,
        _session: OwnedObjectPath,
        options: Options,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let request = request_path(&header, &options)?;
        let restore_token = option_string(&options, "restore_token");
        let persist_mode = option_u32(&options, "persist_mode").unwrap_or(0);
        {
            let mut state = self.0.lock().unwrap();
            state.events.push("select".into());
            state.restore_tokens.push(restore_token);
            state.persist_modes.push(persist_mode);
        }
        response(connection, &request, Options::new()).await?;
        Ok(request)
    }

    #[zbus(name = "Start")]
    async fn start(
        &self,
        _session: OwnedObjectPath,
        _parent_window: String,
        options: Options,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &Connection,
    ) -> zbus::fdo::Result<OwnedObjectPath> {
        let request = request_path(&header, &options)?;
        let (token, clipboard_enabled) = {
            let mut state = self.0.lock().unwrap();
            state.events.push("start".into());
            state.starts += 1;
            (
                format!("fixture-token-{}", state.starts),
                state.clipboard_requested,
            )
        };
        let mut results = Options::new();
        results.insert("devices".into(), option(3u32));
        results.insert("clipboard_enabled".into(), option(clipboard_enabled));
        results.insert("restore_token".into(), option(token));
        response(connection, &request, results).await?;
        Ok(request)
    }
}

#[interface(name = "org.freedesktop.portal.Clipboard")]
impl ClipboardFixture {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 {
        1
    }

    #[zbus(name = "RequestClipboard")]
    fn request_clipboard(
        &self,
        _session: OwnedObjectPath,
        _options: Options,
    ) -> zbus::fdo::Result<()> {
        let mut state = self.0.lock().unwrap();
        state.events.push("clipboard_request".into());
        state.clipboard_requested = true;
        Ok(())
    }

    #[zbus(name = "SelectionRead")]
    fn selection_read(
        &self,
        _session: OwnedObjectPath,
        mime_type: String,
    ) -> zbus::fdo::Result<ZOwnedFd> {
        if !matches!(
            mime_type.as_str(),
            "text/plain;charset=utf-8" | "text/plain"
        ) {
            return Err(fdo_failed("fixture only exposes text"));
        }
        let mut file = tempfile::tempfile().map_err(|error| fdo_failed(error.to_string()))?;
        file.write_all(b"fixture clipboard")
            .map_err(|error| fdo_failed(error.to_string()))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|error| fdo_failed(error.to_string()))?;
        let fd: std::os::fd::OwnedFd = file.into();
        Ok(fd.into())
    }

    #[zbus(name = "SetSelection")]
    fn set_selection(
        &self,
        _session: OwnedObjectPath,
        options: Options,
    ) -> zbus::fdo::Result<()> {
        let mime_types = options
            .get("mime_types")
            .and_then(|value| value.try_clone().ok())
            .and_then(|value| Vec::<String>::try_from(value).ok())
            .ok_or_else(|| fdo_failed("missing mime_types"))?;
        let mut state = self.0.lock().unwrap();
        state.events.push("set_selection".into());
        state.selection_mime_types = mime_types;
        Ok(())
    }
}

#[tokio::test]
async fn private_portal_fixture_rotates_restore_token_and_grants_clipboard() {
    let bus = PrivateBus::start();
    let fixture = Arc::new(StdMutex::new(FixtureState::default()));
    let builder = zbus::connection::Builder::address(bus.address.as_str())
        .unwrap()
        .name(DEST)
        .unwrap()
        .serve_at(PATH, RemoteFixture(fixture.clone()))
        .unwrap()
        .serve_at(PATH, ClipboardFixture(fixture.clone()))
        .unwrap();
    let _server = builder.build().await.unwrap();

    let root = tempfile::tempdir().unwrap();
    let artifacts = root.path().join("artifacts");
    let state = root.path().join("state");
    let ctx = Context {
        session: "portal-fixture".into(),
        cancellation: CancellationToken::new(),
    };

    let first = Portal::new_for_test(
        artifacts.clone(),
        state.clone(),
        bus.address.clone(),
    )
    .unwrap();
    let first_result = first
        .start(
            &ctx,
            &json!({
                "keyboard":true,
                "pointer":true,
                "persist_mode":2,
                "clipboard":true
            }),
        )
        .await
        .unwrap();
    assert_eq!(first_result["restore_saved"], true);
    assert_eq!(first_result["restore_attempted"], false);
    assert_eq!(first_result["clipboard_enabled"], true);
    drop(first);
    tokio::time::sleep(Duration::from_millis(20)).await;

    let second = Portal::new_for_test(
        artifacts,
        state.clone(),
        bus.address.clone(),
    )
    .unwrap();
    let second_result = second
        .start(
            &ctx,
            &json!({
                "keyboard":true,
                "pointer":true,
                "persist_mode":2,
                "clipboard":true
            }),
        )
        .await
        .unwrap();
    assert_eq!(second_result["restore_saved"], true);
    assert_eq!(second_result["restore_attempted"], true);
    assert_eq!(second_result["clipboard_enabled"], true);

    let read = second.clipboard_read(&ctx, &json!({})).await.unwrap();
    assert_eq!(read["text"], "fixture clipboard");
    assert_eq!(read["backend_route"], "portal");

    let write = second
        .clipboard_write(&ctx, &json!({"text":"semwright clipboard"}))
        .await
        .unwrap();
    assert_eq!(write["written"], true);

    let snapshot = fixture.lock().unwrap();
    assert_eq!(
        snapshot.events,
        vec![
            "create",
            "select",
            "clipboard_request",
            "start",
            "create",
            "select",
            "clipboard_request",
            "start",
            "set_selection",
        ]
    );
    assert_eq!(snapshot.persist_modes, vec![2, 2]);
    assert_eq!(snapshot.restore_tokens[0], None);
    assert_eq!(
        snapshot.restore_tokens[1].as_deref(),
        Some("fixture-token-1")
    );
    assert!(
        snapshot
            .selection_mime_types
            .iter()
            .any(|mime| mime == "text/plain;charset=utf-8")
    );
    drop(snapshot);

    let cleared = second.clear_restore().await.unwrap();
    assert_eq!(cleared["durable_token_cleared"], true);
    assert_eq!(second.restore_store.status(), RestoreStatus::Absent);
}
