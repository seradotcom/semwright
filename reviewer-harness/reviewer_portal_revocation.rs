// Included into the existing private portal fixture module for defensive fault injection only.
#[derive(Default)]
struct ReviewerPendingStart {
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
    session: StdMutex<Option<OwnedObjectPath>>,
    key_calls: std::sync::atomic::AtomicUsize,
}
#[derive(Clone)]
struct ReviewerRevokingPortal(Arc<ReviewerPendingStart>);

#[interface(name = "org.freedesktop.portal.RemoteDesktop")]
impl ReviewerRevokingPortal {
    #[zbus(property, name = "version")]
    fn version(&self) -> u32 { 2 }
    #[zbus(property, name = "AvailableDeviceTypes")]
    fn available_device_types(&self) -> u32 { 3 }
    #[zbus(name = "CreateSession")]
    async fn create_session(&self, options: Options, #[zbus(header)] header: Header<'_>, #[zbus(connection)] connection: &Connection) -> zbus::fdo::Result<OwnedObjectPath> {
        let request = request_path(&header, &options)?;
        let session = session_path(&header, &options)?;
        *self.0.session.lock().unwrap() = Some(session.clone());
        let mut results = Options::new();
        results.insert("session_handle".into(), option(session.to_string()));
        response(connection, &request, results).await?;
        Ok(request)
    }
    #[zbus(name = "SelectDevices")]
    async fn select_devices(&self, _session: OwnedObjectPath, options: Options, #[zbus(header)] header: Header<'_>, #[zbus(connection)] connection: &Connection) -> zbus::fdo::Result<OwnedObjectPath> {
        let request = request_path(&header, &options)?;
        response(connection, &request, Options::new()).await?;
        Ok(request)
    }
    #[zbus(name = "Start")]
    async fn start(&self, _session: OwnedObjectPath, _parent_window: String, options: Options, #[zbus(header)] header: Header<'_>, #[zbus(connection)] connection: &Connection) -> zbus::fdo::Result<OwnedObjectPath> {
        let request = request_path(&header, &options)?;
        self.0.entered.notify_one();
        self.0.release.notified().await;
        let mut results = Options::new();
        results.insert("devices".into(), option(1u32));
        response(connection, &request, results).await?;
        Ok(request)
    }
    #[zbus(name = "NotifyKeyboardKeysym")]
    fn notify_keyboard_keysym(&self, _session: OwnedObjectPath, _options: Options, _keysym: i32, _state: u32) -> zbus::fdo::Result<()> {
        self.0.key_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn reviewer_portal_revocation_during_pending_start_cannot_report_granted() {
    let bus = PrivateBus::start();
    let fixture = Arc::new(ReviewerPendingStart::default());
    let server = zbus::connection::Builder::address(bus.address.as_str()).unwrap()
        .name(DEST).unwrap().serve_at(PATH, ReviewerRevokingPortal(fixture.clone())).unwrap()
        .build().await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let portal = Arc::new(Portal::new_for_test(root.path().join("artifacts"), root.path().join("state"), bus.address.clone()).unwrap());
    let task_portal = portal.clone();
    let task = tokio::spawn(async move {
        task_portal.start(&Context { session: "reviewer-pending-revocation".into(), request_id: semwright_types::unique_id(), cancellation: CancellationToken::new() },
            &json!({"keyboard":true,"pointer":false,"persist_mode":0,"clipboard":false})).await
    });
    tokio::time::timeout(Duration::from_secs(5), fixture.entered.notified()).await.expect("Start must be pending");
    let session = fixture.session.lock().unwrap().clone().unwrap();
    server.emit_signal(None::<&str>, &session, "org.freedesktop.portal.Session", "Closed", &()).await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if *portal.consent.lock().await == ConsentState::Expired { break; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("Closed signal must be processed while Start remains pending");
    fixture.release.notify_one();
    let result = tokio::time::timeout(Duration::from_secs(5), task).await.unwrap().unwrap();
    let input = portal.notify(&Context { session: "reviewer-pending-revocation".into(), request_id: semwright_types::unique_id(), cancellation: CancellationToken::new() },
        "input.type", &json!({"text":"A"})).await;
    println!("REVIEWER_ASSERT pending_revocation start={result:?} input={input:?} consent={:?}", *portal.consent.lock().await);
    assert!(input.is_err(), "revoked grant must never send input");
    assert_eq!(fixture.key_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert!(result.is_err(), "revocation processed before Start response must not be reported as a fresh grant");
    assert_ne!(*portal.consent.lock().await, ConsentState::Granted);
}
