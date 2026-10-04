use semwright_native_sdk::{Driver, ErrorCode, Model, NativeApp, Value, descriptor_digest, json};
#[path = "../../../examples/native/scene.rs"]
mod scene;
#[path = "../../../examples/native/table.rs"]
mod table;
fn args(app: &NativeApp<impl Model>, key: &str, p: Value) -> Value {
    let v = app.inspect().unwrap();
    json!({"expected_revision":v["revision"],"expected_generation":v["generation"],"operation_key":key,"parameters":p})
}
fn assert_code<T: std::fmt::Debug>(result: semwright_native_sdk::Result<T>, code: ErrorCode) {
    assert_eq!(
        format!("{:?}", result.unwrap_err().code),
        format!("{code:?}")
    );
}
#[test]
fn scene_cas_external_edits_reopen_and_denial() {
    let tmp = tempfile::tempdir().unwrap();
    let app = NativeApp::create(tmp.path(), scene::Scene).unwrap();
    let before = std::fs::read(tmp.path().join("document.json")).unwrap();
    app.inspect().unwrap();
    app.inspect().unwrap();
    assert_eq!(
        before,
        std::fs::read(tmp.path().join("document.json")).unwrap()
    );
    let old = args(&app, "a", json!({"object_id":"cube","color":"#ff0000"}));
    let r = app.apply("set-object", &old, || false).unwrap();
    assert_eq!(app.apply("set-object", &old, || false).unwrap(), r);
    assert_eq!(
        app.inspect().unwrap()["projection"]["objects"]["cube"]["color"],
        "#ff0000"
    );
    let mut stale = old.clone();
    stale["operation_key"] = json!("b");
    assert_code(
        app.apply("set-object", &stale, || false),
        ErrorCode::StaleReference,
    );
    let p = args(&app, "deny", json!({}));
    let bytes = std::fs::read(tmp.path().join("document.json")).unwrap();
    assert_code(
        app.apply("delete-all", &p, || false),
        ErrorCode::PermissionDenied,
    );
    assert_eq!(
        bytes,
        std::fs::read(tmp.path().join("document.json")).unwrap()
    );
    let reopen = NativeApp::open(tmp.path(), scene::Scene).unwrap();
    assert_code(
        reopen.apply("set-object", &old, || false),
        ErrorCode::StaleReference,
    );
    let next = args(&reopen, "external", json!({"object_id":"cube","scale":2}));
    let mut doc: Value =
        serde_json::from_slice(&std::fs::read(tmp.path().join("document.json")).unwrap()).unwrap();
    doc["state"]["objects"]["cube"]["scale"] = json!(3);
    std::fs::write(
        tmp.path().join("document.json"),
        serde_json::to_vec(&doc).unwrap(),
    )
    .unwrap();
    assert_code(
        reopen.apply("set-object", &next, || false),
        ErrorCode::StaleReference,
    );
}
#[test]
fn resource_identity_survives_directory_rename_but_not_delete_recreate() {
    let tmp = tempfile::tempdir().unwrap();
    let original = tmp.path().join("named-project");
    let renamed = tmp.path().join("renamed-project");

    let app = NativeApp::create(&original, scene::Scene).unwrap();
    let first = app.inspect().unwrap();
    let target = serde_json::from_value(first["native_target"].clone()).unwrap();
    drop(app);

    std::fs::rename(&original, &renamed).unwrap();
    let reopened = NativeApp::open(&renamed, scene::Scene).unwrap();
    let after_rename = reopened.inspect().unwrap();
    assert_eq!(after_rename["resource_id"], first["resource_id"]);
    assert_ne!(after_rename["generation"], first["generation"]);
    assert_code(reopened.validate_target(&target), ErrorCode::StaleReference);
    let durable_id = after_rename["resource_id"].clone();
    drop(reopened);

    std::fs::remove_dir_all(&renamed).unwrap();
    let recreated = NativeApp::create(&renamed, scene::Scene).unwrap();
    let after_recreate = recreated.inspect().unwrap();
    assert_ne!(after_recreate["resource_id"], durable_id);
    assert_ne!(after_recreate["generation"], after_rename["generation"]);
    assert_code(
        recreated.validate_target(&target),
        ErrorCode::StaleReference,
    );
}

#[test]
fn snapshot_restore_export_and_cancel_are_real() {
    let tmp = tempfile::tempdir().unwrap();
    let app = NativeApp::create(tmp.path(), table::Table).unwrap();
    let snap = app
        .apply("snapshot", &args(&app, "snapshot", json!({})), || false)
        .unwrap();
    app.apply(
        "set-cell",
        &args(&app, "edit", json!({"cell":"A1","value":50})),
        || false,
    )
    .unwrap();
    assert_eq!(
        app.inspect().unwrap()["projection"]["rows"][2]["value"],
        70.0
    );
    let before = std::fs::read(tmp.path().join("document.json")).unwrap();
    assert_code(
        app.apply(
            "set-cell",
            &args(&app, "cancel", json!({"cell":"A1","value":1})),
            || true,
        ),
        ErrorCode::Cancelled,
    );
    assert_eq!(
        before,
        std::fs::read(tmp.path().join("document.json")).unwrap()
    );
    app.apply(
        "restore",
        &args(&app, "restore", json!({"snapshot_id":snap["snapshot_id"]})),
        || false,
    )
    .unwrap();
    assert_eq!(
        app.inspect().unwrap()["projection"]["rows"][2]["value"],
        30.0
    );
    let export = app
        .apply(
            "export",
            &args(
                &app,
                "export",
                json!({"output_namespace":"op_one","slot":"table_csv"}),
            ),
            || false,
        )
        .unwrap();
    let bytes = std::fs::read(
        tmp.path()
            .join("outputs")
            .join(export["artifact"]["path"].as_str().unwrap()),
    )
    .unwrap();
    assert_eq!(
        export["artifact"]["sha256"],
        semwright_native_sdk::sha256(&bytes)
    );
    assert!(String::from_utf8(bytes).unwrap().contains("A3,30"));
    assert_code(
        app.apply(
            "export",
            &args(
                &app,
                "export2",
                json!({"output_namespace":"op_one","slot":"table_csv"}),
            ),
            || false,
        ),
        ErrorCode::Conflict,
    );
    let view = app.inspect().unwrap();
    let target = serde_json::from_value(view["native_target"].clone()).unwrap();
    app.validate_target(&target).unwrap();
    assert_code(
        NativeApp::open(tmp.path(), table::Table)
            .unwrap()
            .validate_target(&target),
        ErrorCode::StaleReference,
    );
}
#[test]
fn formula_cycles_and_unknown_interfaces_fail_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let app = NativeApp::create(tmp.path(), table::Table).unwrap();
    let bytes = std::fs::read(tmp.path().join("document.json")).unwrap();
    assert_code(
        app.apply(
            "set-cell",
            &args(&app, "cycle", json!({"cell":"A1","value":{"sum":["A3"]}})),
            || false,
        ),
        ErrorCode::InvalidArgument,
    );
    assert_eq!(
        bytes,
        std::fs::read(tmp.path().join("document.json")).unwrap()
    );
    assert_code(
        app.apply("run-code", &args(&app, "code", json!({})), || false),
        ErrorCode::PermissionDenied,
    );
}
#[tokio::test]
async fn fork_isolation_and_descriptor_generation() {
    let tmp = tempfile::tempdir().unwrap();
    let mut app = NativeApp::create(tmp.path(), scene::Scene).unwrap();
    let source_view = app.inspect().unwrap();
    let source = source_view["projection"].clone();
    let source_bytes = std::fs::read(tmp.path().join("document.json")).unwrap();
    let a = args(&app, "fork-one", json!({"workspace_id":"consumer_one"}));
    let first = app.apply("fork", &a, || false).unwrap();
    assert_eq!(first, app.apply("fork", &a, || false).unwrap());
    assert_eq!(
        source_bytes,
        std::fs::read(tmp.path().join("document.json")).unwrap()
    );
    let second = args(&app, "fork-two", json!({"workspace_id":"consumer_two"}));
    app.apply("fork", &second, || false).unwrap();
    assert_eq!(app.inspect().unwrap()["revision"], source_view["revision"]);
    let caps = app.capabilities().await.unwrap();
    let cap = caps
        .iter()
        .find(|x| x.descriptor.name.ends_with(".inspect"))
        .unwrap();
    let hash = descriptor_digest(&cap.descriptor).unwrap();
    let view = app
        .execute(
            &cap.descriptor.name,
            &hash,
            json!({"workspace_id":"consumer_one"}),
        )
        .await
        .unwrap();
    let mut edit = json!({"workspace_id":"consumer_one","expected_revision":view["revision"],"expected_generation":view["generation"],"operation_key":"edit-child","parameters":{"object_id":"cube","color":"#00ff00"}});
    let op = caps
        .iter()
        .find(|x| x.descriptor.name.ends_with(".set-object"))
        .unwrap();
    app.execute(
        &op.descriptor.name,
        &descriptor_digest(&op.descriptor).unwrap(),
        edit.clone(),
    )
    .await
    .unwrap();
    assert_eq!(app.inspect().unwrap()["projection"], source);
    edit["operation_key"] = json!("stale");
    assert_code(
        app.execute(
            &op.descriptor.name,
            &descriptor_digest(&op.descriptor).unwrap(),
            edit,
        )
        .await,
        ErrorCode::StaleReference,
    );
    assert_code(
        app.execute(&op.descriptor.name, "0", json!({})).await,
        ErrorCode::StaleReference,
    );
}
#[test]
fn concurrency_has_one_cas_winner() {
    let tmp = tempfile::tempdir().unwrap();
    let app = NativeApp::create(tmp.path(), table::Table).unwrap();
    let other = NativeApp::open(tmp.path(), table::Table).unwrap();
    let a = args(&app, "a", json!({"cell":"A1","value":1}));
    let b = args(&other, "b", json!({"cell":"A1","value":2}));
    let one = std::thread::spawn(move || app.apply("set-cell", &a, || false));
    let two = std::thread::spawn(move || other.apply("set-cell", &b, || false));
    let results = [one.join().unwrap(), two.join().unwrap()];
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
}
#[test]
fn events_are_durable_bounded_hints() {
    let tmp = tempfile::tempdir().unwrap();
    let app = NativeApp::create(tmp.path(), table::Table).unwrap();
    for i in 0..130 {
        app.apply(
            "set-cell",
            &args(&app, &format!("op{i}"), json!({"cell":"A1","value":i})),
            || false,
        )
        .unwrap();
    }
    assert_code(app.events(0), ErrorCode::StaleReference);
    assert_eq!(
        app.events(2).unwrap()["events"].as_array().unwrap().len(),
        128
    );
    let reopen = NativeApp::open(tmp.path(), table::Table).unwrap();
    assert_eq!(
        reopen.events(129).unwrap()["events"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[derive(Clone)]
struct NoSnapshots;
impl Model for NoSnapshots {
    fn id(&self) -> &'static str {
        "native-minimal"
    }
    fn initial(&self) -> Value {
        json!({"value":0})
    }
    fn validate(&self, state: &Value) -> semwright_native_sdk::Result<()> {
        if state.get("value").and_then(Value::as_i64).is_none() {
            return Err(semwright_native_sdk::Error::invalid(
                "Numeric value required",
            ));
        }
        Ok(())
    }
    fn operations(&self) -> Vec<semwright_native_sdk::Operation> {
        vec![]
    }
    fn apply(&self, _: &Value, _: &str, _: &Value) -> semwright_native_sdk::Result<Value> {
        Err(semwright_native_sdk::Error::new(
            ErrorCode::Unsupported,
            "No model mutations supported",
        ))
    }
    fn snapshot_supported(&self) -> bool {
        false
    }
}
#[test]
fn optional_interfaces_are_negotiated_and_fail_closed() {
    let t = tempfile::tempdir().unwrap();
    let app = NativeApp::create(t.path(), NoSnapshots).unwrap();
    assert_eq!(app.interfaces_value()["snapshot"], false);
    assert!(!app.capabilities_value().iter().any(
        |c| c.descriptor.name.ends_with(".snapshot") || c.descriptor.name.ends_with(".restore")
    ));
    assert_code(
        app.apply("snapshot", &args(&app, "snapshot", json!({})), || false),
        ErrorCode::Unsupported,
    );
}

#[test]
fn persistent_resource_and_object_ids_survive_requests_and_reopened_app_sessions() {
    // Application-session/request contract, not a Broker or product-job fixture.
    // Distinct request keys represent separate owned intentions, never authority.
    fn exercise<M: Model>(
        model: M,
        operation: &str,
        first: Value,
        second: Value,
        ids: fn(&Value) -> Vec<String>,
    ) {
        let tmp = tempfile::tempdir().unwrap();
        let app = NativeApp::create(tmp.path(), model.clone()).unwrap();
        let initial = app.inspect().unwrap();
        let object_ids = ids(&initial["projection"]);
        let first_receipt = app
            .apply(operation, &args(&app, "owned-request-one", first), || false)
            .unwrap();
        let current = app.inspect().unwrap();
        let target = serde_json::from_value(current["native_target"].clone()).unwrap();
        app.validate_target(&target).unwrap();
        assert_eq!(first_receipt["resource_id"], initial["resource_id"]);
        assert_eq!(ids(&current["projection"]), object_ids);
        let bytes = std::fs::read(tmp.path().join("document.json")).unwrap();
        drop(app);
        let reopened = NativeApp::open(tmp.path(), model).unwrap();
        let view = reopened.inspect().unwrap();
        assert_eq!(
            bytes,
            std::fs::read(tmp.path().join("document.json")).unwrap()
        );
        assert_eq!(view["resource_id"], initial["resource_id"]);
        assert_eq!(view["revision"], current["revision"]);
        assert_eq!(ids(&view["projection"]), object_ids);
        assert_ne!(view["generation"], current["generation"]);
        assert_ne!(
            view["native_target"]["identity"],
            current["native_target"]["identity"]
        );
        assert_code(reopened.validate_target(&target), ErrorCode::StaleReference);
        let stale = json!({"expected_revision":current["revision"],"expected_generation":current["generation"],"operation_key":"owned-stale-request","parameters":second});
        assert_code(
            reopened.apply(operation, &stale, || false),
            ErrorCode::StaleReference,
        );
        assert_eq!(
            bytes,
            std::fs::read(tmp.path().join("document.json")).unwrap()
        );
        let second_receipt = reopened
            .apply(
                operation,
                &args(&reopened, "owned-request-two", second),
                || false,
            )
            .unwrap();
        assert_eq!(second_receipt["resource_id"], first_receipt["resource_id"]);
        assert_ne!(
            second_receipt["operation_key"],
            first_receipt["operation_key"]
        );
        assert_eq!(ids(&reopened.inspect().unwrap()["projection"]), object_ids);
    }
    exercise(
        scene::Scene,
        "set-object",
        json!({"object_id":"cube","color":"#112233"}),
        json!({"object_id":"cube","color":"#445566"}),
        |p| p["objects"].as_object().unwrap().keys().cloned().collect(),
    );
    exercise(
        table::Table,
        "set-cell",
        json!({"cell":"A1","value":11}),
        json!({"cell":"A1","value":12}),
        |p| {
            p["rows"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["cell"].as_str().unwrap().to_string())
                .collect()
        },
    );
}

#[test]
fn omitted_dependencies_remain_unknown_and_each_inspect_uses_one_report() {
    let tmp = tempfile::tempdir().unwrap();
    let app = NativeApp::create(tmp.path(), NoSnapshots).unwrap();
    let view = app.inspect().unwrap();
    assert_eq!(
        view["dependencies"],
        json!({"relations":[],"coverage":"not_declared"})
    );
    assert_eq!(view["source"]["coverage"], "not_declared");
    assert_eq!(app.interfaces_value()["dependencies"], true);
    #[derive(Clone)]
    struct Counted(std::sync::Arc<std::sync::atomic::AtomicUsize>);
    impl Model for Counted {
        fn id(&self) -> &'static str {
            "native-counted"
        }
        fn initial(&self) -> Value {
            NoSnapshots.initial()
        }
        fn validate(&self, s: &Value) -> semwright_native_sdk::Result<()> {
            NoSnapshots.validate(s)
        }
        fn operations(&self) -> Vec<semwright_native_sdk::Operation> {
            vec![]
        }
        fn apply(&self, s: &Value, o: &str, p: &Value) -> semwright_native_sdk::Result<Value> {
            NoSnapshots.apply(s, o, p)
        }
        fn dependencies(&self, _: &Value) -> semwright_native_sdk::Result<Value> {
            let sequence = self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(json!({"relations":[],"coverage":"not_declared","observation":sequence}))
        }
    }
    let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let root = tempfile::tempdir().unwrap();
    let counted = NativeApp::create(root.path(), Counted(count.clone())).unwrap();
    let view = counted.inspect().unwrap();
    assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    assert_eq!(
        view["dependencies_digest"],
        format!(
            "sha256:{}",
            semwright_native_sdk::sha256(&serde_json::to_vec(&view["dependencies"]).unwrap())
        )
    );
    assert_eq!(
        scene::Scene.dependencies(&scene::Scene.initial()).unwrap(),
        json!({"relations":[],"coverage":"complete_for_model"})
    );
    let table = table::Table.dependencies(&table::Table.initial()).unwrap();
    assert_eq!(table["coverage"], "complete_for_model");
    assert_eq!(table["relations"].as_array().unwrap().len(), 2);
}
