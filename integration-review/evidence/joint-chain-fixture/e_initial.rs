    // I fixture: retain the first E-authored native asset before the existing
    // typed transform/repair scenario. The product exporter writes the GLB.
    let joint_initial = fixture
        .call("composition.inspect", json!({"island":applied["island"]}))
        .await;
    let joint_collection = joint_initial["items"][0]["collections"][0]
        .as_str().unwrap().to_owned();
    let joint_export = fixture.call("export.glb", json!({
        "collection":joint_collection,"path":"initial-articulated.glb","animations":true
    })).await;
    let joint_bytes = fs::read(workspace.path().join("initial-articulated.glb")).unwrap();
    assert_eq!(&joint_bytes[..4], b"glTF");
    assert_eq!(joint_export["sha256"], format!("{:x}", Sha256::digest(&joint_bytes)));
    let joint_after_export = fixture
        .call("composition.inspect", json!({"island":applied["island"]}))
        .await;
    assert_eq!(joint_after_export["drift"], false);
    assert_eq!(joint_after_export["fingerprint"], joint_initial["fingerprint"]);
    let joint_evidence = PathBuf::from(std::env::var("SEMWRIGHT_AUTHORING_EVIDENCE").unwrap());
    fs::create_dir_all(&joint_evidence).unwrap();
    fs::write(joint_evidence.join("initial-articulated.glb"), &joint_bytes).unwrap();
    fs::write(joint_evidence.join("initial-export.json"), serde_json::to_vec_pretty(&joint_export).unwrap()).unwrap();
