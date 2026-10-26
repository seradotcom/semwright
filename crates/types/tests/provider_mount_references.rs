use semwright_types::*;
use std::time::Duration;
fn target() -> NativeTarget {
    NativeTarget {
        kind: "native".into(),
        identity: "claimed-mount".into(),
        revision: 1,
        fingerprint: "claimed-epoch".into(),
        app: "fixture".into(),
    }
}
#[test]
fn provider_mount_refs_are_typed_and_session_bound() {
    let mut store = RefStore::new(Duration::from_secs(60), 3);
    let binding = store
        .insert_provider_mount("s1", "driver:fixture", "c1", 7, "media")
        .unwrap();
    let native = store.insert("s1", "driver:fixture", target()).unwrap();
    let value = store.get_provider_mount(&binding, "s1").unwrap();
    assert_eq!(value.generation, 7);
    assert_eq!(value.connection, "c1");
    assert_eq!(value.mount, "media");
    assert!(store.get(&native, "s1").is_ok());
    assert_eq!(
        store.get(&binding, "s1").unwrap_err().code,
        ErrorCode::StaleReference
    );
    assert_eq!(
        store.get_provider_mount(&native, "s1").unwrap_err().code,
        ErrorCode::StaleReference
    );
    assert!(store.get_provider_mount(&binding, "s2").is_err());
    assert!(
        store
            .get_provider_mount("provider-mount:00000000000000000000000000000000", "s1")
            .is_err()
    );
    let mut forged = target();
    forged.kind = "provider-mount".into();
    assert_eq!(
        store
            .insert("s1", "driver:fixture", forged)
            .unwrap_err()
            .code,
        ErrorCode::BackendFailed
    );
}
#[test]
fn native_and_mount_refs_share_capacity() {
    let mut a = RefStore::new(Duration::from_secs(60), 1);
    a.insert("s", "p", target()).unwrap();
    assert_eq!(
        a.insert_provider_mount("s", "p", "c", 1, "m")
            .unwrap_err()
            .code,
        ErrorCode::ResourceExhausted
    );
    let mut b = RefStore::new(Duration::from_secs(60), 1);
    b.insert_provider_mount("s", "p", "c", 1, "m").unwrap();
    assert_eq!(
        b.insert("s", "p", target()).unwrap_err().code,
        ErrorCode::ResourceExhausted
    );
}
#[test]
fn mount_refs_share_revocation_and_backend_invalidation() {
    let mut s = RefStore::new(Duration::from_secs(60), 8);
    let n1 = s.insert("s1", "p", target()).unwrap();
    let m1 = s.insert_provider_mount("s1", "p", "c", 1, "m").unwrap();
    let n2 = s.insert("s2", "p", target()).unwrap();
    let m2 = s.insert_provider_mount("s2", "p", "c", 1, "m").unwrap();
    let m3 = s.insert_provider_mount("s2", "other", "d", 1, "m").unwrap();
    s.revoke_session("s1");
    assert!(s.get(&n1, "s1").is_err());
    assert!(s.get_provider_mount(&m1, "s1").is_err());
    assert!(s.get(&n2, "s2").is_ok());
    assert!(s.get_provider_mount(&m2, "s2").is_ok());
    s.invalidate_backend("p");
    assert!(s.get(&n2, "s2").is_err());
    assert!(s.get_provider_mount(&m2, "s2").is_err());
    assert!(s.get_provider_mount(&m3, "s2").is_ok());
}
#[test]
fn expired_mount_refs_fail_and_release_shared_capacity_without_sleep() {
    let mut s = RefStore::new(Duration::ZERO, 1);
    let n = s.insert("s", "p", target()).unwrap();
    assert!(s.get(&n, "s").is_err());
    let m = s.insert_provider_mount("s", "p", "c", 1, "m").unwrap();
    assert!(s.get_provider_mount(&m, "s").is_err());
    let n2 = s.insert("s", "p", target()).unwrap();
    assert_ne!(n, n2);
}

#[test]
fn expired_inflight_record_retains_shared_capacity_until_holder_finishes() {
    let mut store = RefStore::new(Duration::ZERO, 1);
    let id = store
        .insert_provider_mount("s", "p", "c", 1, "media")
        .unwrap();
    let holder = store.get_provider_mount_for_completion(&id, "s").unwrap();
    assert!(!holder.is_revoked());
    assert!(store.get_provider_mount(&id, "s").is_err());
    assert_eq!(
        store.insert("other", "p", target()).unwrap_err().code,
        ErrorCode::ResourceExhausted
    );
    assert!(
        store
            .get_provider_mount_for_completion(&id, "s")
            .unwrap()
            .same_admission(&holder)
    );
    drop(holder);
    assert!(store.insert("other", "p", target()).is_ok());
    assert!(store.get_provider_mount_for_completion(&id, "s").is_err());
}
#[test]
fn explicit_revocation_and_drop_reach_holders_after_admission_expired() {
    for by_backend in [false, true] {
        let mut store = RefStore::new(Duration::ZERO, 2);
        let id = store
            .insert_provider_mount("s", "p", "c", 1, "media")
            .unwrap();
        let holder = store.get_provider_mount_for_completion(&id, "s").unwrap();
        if by_backend {
            store.invalidate_backend("p");
        } else {
            store.revoke_session("s");
        }
        assert!(holder.is_revoked());
        assert!(store.get_provider_mount_for_completion(&id, "s").is_err());
    }
    let holder = {
        let mut store = RefStore::new(Duration::ZERO, 1);
        let id = store
            .insert_provider_mount("s", "p", "c", 1, "media")
            .unwrap();
        store.get_provider_mount_for_completion(&id, "s").unwrap()
    };
    assert!(holder.is_revoked());
}
