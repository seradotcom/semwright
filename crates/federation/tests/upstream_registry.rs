use semwright_federation::{
    UpstreamRegistry, executable_sha256, load_upstream_registry, new_upstream,
    save_upstream_registry,
};
use std::os::unix::fs::PermissionsExt;

fn executable(dir: &tempfile::TempDir) -> std::path::PathBuf {
    let path = dir.path().join("server");
    std::fs::write(&path, b"fixture executable").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn owner_registry_roundtrips_atomically_without_granting_any_policy() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("config");
    std::fs::create_dir(&config_dir).unwrap();
    std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let server = executable(&dir);
    let registry_path = config_dir.join("mcp-upstreams.toml");
    let upstream = new_upstream(
        "fixture".into(),
        server.clone(),
        None,
        vec!["--stdio".into()],
        Some("fixture-server".into()),
        Some("1.0".into()),
        2500,
        true,
    )
    .unwrap();
    assert_eq!(upstream.sha256, executable_sha256(&server).unwrap());
    let mut registry = UpstreamRegistry::default();
    registry.add(upstream, false).unwrap();
    save_upstream_registry(&registry_path, &registry).unwrap();
    let meta = std::fs::metadata(&registry_path).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    let loaded = load_upstream_registry(&registry_path).unwrap();
    assert_eq!(loaded.upstreams.len(), 1);
    assert_eq!(loaded.upstreams[0].slug, "fixture");
    assert!(loaded.upstreams[0].enabled);
}

#[test]
fn registry_rejects_duplicates_links_and_unsafe_replacement() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("config");
    std::fs::create_dir(&config_dir).unwrap();
    std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    let server = executable(&dir);
    let upstream = new_upstream(
        "fixture".into(),
        server,
        None,
        vec![],
        None,
        None,
        1000,
        true,
    )
    .unwrap();
    let mut registry = UpstreamRegistry::default();
    registry.add(upstream.clone(), false).unwrap();
    assert!(registry.add(upstream.clone(), false).is_err());
    registry.add(upstream, true).unwrap();

    let registry_path = config_dir.join("mcp-upstreams.toml");
    save_upstream_registry(&registry_path, &registry).unwrap();
    std::fs::set_permissions(&registry_path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(load_upstream_registry(&registry_path).is_err());

    let linked = config_dir.join("linked.toml");
    std::os::unix::fs::symlink(&registry_path, &linked).unwrap();
    assert!(load_upstream_registry(&linked).is_err());
}

#[test]
fn disable_is_allowed_for_stale_binary_but_enable_revalidates_it() {
    let dir = tempfile::tempdir().unwrap();
    let server = executable(&dir);
    let upstream = new_upstream(
        "fixture".into(),
        server.clone(),
        None,
        vec![],
        None,
        None,
        1000,
        true,
    )
    .unwrap();
    let mut registry = UpstreamRegistry::default();
    registry.add(upstream, false).unwrap();
    std::fs::remove_file(server).unwrap();
    registry.set_enabled("fixture", false).unwrap();
    assert!(!registry.find("fixture").unwrap().enabled);
    assert!(registry.set_enabled("fixture", true).is_err());
    registry.remove("fixture").unwrap();
    assert!(registry.find("fixture").is_none());
}
