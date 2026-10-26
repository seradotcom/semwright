//! Project authenticated sandbox mounts onto this DriverHost's declared owner roots.
//!
//! A Host-created loopback transport remains a Workspace mount inside the sandbox,
//! but is not an owner filesystem grant and must never be exported as one. This
//! private projection preserves the platform capsules themselves. Core still
//! enforces the owner grant and revalidates every retained physical binding.

use semwright_driver_sdk::Manifest;
use semwright_platform_api::launch::{Mount, MountClass, SandboxSpec, WorkspaceMountBinding};
use semwright_types::{Error, ErrorCode, Result};
use std::{collections::BTreeMap, path::Path, sync::Arc};

type Bindings = Vec<Arc<dyn WorkspaceMountBinding>>;

fn mismatch(message: &str) -> Error {
    Error::new(ErrorCode::PolicyDenied, message)
}

fn same_mount(left: &Mount, right: &Mount) -> bool {
    left.class == right.class
        && left.logical_name == right.logical_name
        && left.source == right.source
        && left.read_only == right.read_only
        && left.execute == right.execute
}

/// `host_loopback` comes only from the retained Host LoopbackProxy and its
/// private logical-name constant, never from a driver response or caller DTO.
pub(super) fn owner_workspace_mount_bindings(
    manifest: &Manifest,
    spec: &SandboxSpec,
    host_loopback: Option<(&str, &Path)>,
    captured: Result<Bindings>,
) -> Result<Bindings> {
    // BEGIN OWNER WORKSPACE PROJECTION
    let captured = captured?;
    // An unsupported/empty capsule implementation grants no mount authority.
    // Keep ordinary provider availability; never synthesize a path capsule.
    if captured.is_empty() {
        return Ok(captured);
    }

    let mut declared = BTreeMap::new();
    for mount in &manifest.mounts {
        if declared.insert(mount.root.as_str(), mount).is_some() {
            return Err(mismatch("Driver declares duplicate owner workspace roots"));
        }
    }
    let internal = host_loopback.map(|(name, source)| Mount {
        source: source.to_path_buf(),
        class: MountClass::Workspace,
        logical_name: name.into(),
        read_only: false,
        execute: false,
    });
    let mut expected = BTreeMap::new();
    let mut owner_count = 0;
    let mut internal_count = 0;
    for mount in spec
        .mounts
        .iter()
        .filter(|mount| mount.class == MountClass::Workspace)
    {
        let owner = if let Some(declaration) = declared.get(mount.logical_name.as_str()) {
            if declaration.read_only != mount.read_only || declaration.execute != mount.execute {
                return Err(mismatch(
                    "Owner workspace rights differ from the driver declaration",
                ));
            }
            owner_count += 1;
            true
        } else if internal
            .as_ref()
            .is_some_and(|expected| same_mount(expected, mount))
        {
            internal_count += 1;
            false
        } else {
            return Err(mismatch("Sandbox contains an unrecognized workspace mount"));
        };
        if expected
            .insert(mount.logical_name.as_str(), (mount, owner))
            .is_some()
        {
            return Err(mismatch("Sandbox contains duplicate workspace mounts"));
        }
    }
    if owner_count != declared.len() || internal_count != if internal.is_some() { 1 } else { 0 } {
        return Err(mismatch(
            "Sandbox workspace mounts do not cover the Host-owned plan",
        ));
    }

    let mut retained = Vec::with_capacity(owner_count);
    for binding in captured {
        let mount = binding.mount();
        let (planned, owner) = expected
            .remove(mount.logical_name.as_str())
            .ok_or_else(|| mismatch("Captured workspace mount is unexpected or duplicated"))?;
        if !same_mount(planned, mount) {
            return Err(mismatch(
                "Captured workspace mount differs from the sandbox plan",
            ));
        }
        if owner {
            // Retain the exact platform-issued Arc, including its native lifetime.
            retained.push(binding);
        }
    }
    if !expected.is_empty() {
        return Err(mismatch("Captured workspace mount set is incomplete"));
    }
    Ok(retained)
    // END OWNER WORKSPACE PROJECTION
}

#[cfg(test)]
mod tests {
    use super::*;
    use semwright_policy::FilesystemGrant;
    use std::sync::atomic::{AtomicBool, Ordering};

    // These capsules exercise the private projection contract. They are not
    // platform issuers and do not claim a physical Linux sandbox acceptance.
    struct TestBinding {
        mount: Mount,
        current: Arc<AtomicBool>,
    }
    impl WorkspaceMountBinding for TestBinding {
        fn mount(&self) -> &Mount {
            &self.mount
        }
        fn validate_current(&self) -> Result<()> {
            if self.current.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err(Error::new(
                    ErrorCode::StaleReference,
                    "Fixture mount was revoked",
                ))
            }
        }
    }

    const INTERNAL_NAME: &str = "semwright-internal-loopback";

    struct Fixture {
        manifest: Manifest,
        spec: SandboxSpec,
        internal: std::path::PathBuf,
        owner_current: Arc<AtomicBool>,
        capsules: Bindings,
        has_internal: bool,
    }
    impl Fixture {
        fn new(has_internal: bool) -> Self {
            let directory = std::env::temp_dir().join("semwright-owner-workspace-projection-test");
            let internal = directory.join("host-loopback");
            let mut manifest = Manifest {
                manifest_version: 1,
                protocol: 1,
                id: "fixture".into(),
                version: "1".into(),
                publisher: "tests".into(),
                executable: directory.join("driver"),
                sha256: "a".repeat(64),
                application: semwright_driver_sdk::ApplicationMatch {
                    desktop_id: Some("org.example.Fixture".into()),
                    ..Default::default()
                },
                transport: semwright_driver_sdk::Transport::StdioV1,
                mounts: vec![],
                system_config: vec![],
                secrets: vec![],
                tools: vec![],
                network: false,
                nvidia_gpu: false,
                loopback_port: has_internal.then_some(43210),
                resources: semwright_driver_sdk::DriverResources::default(),
                request_timeout_ms: 1000,
                interfaces: semwright_driver_sdk::DriverInterfaces::default(),
            };
            manifest.mounts = vec![semwright_driver_sdk::DriverMount {
                root: "media".into(),
                read_only: true,
                execute: false,
            }];
            let roots = [FilesystemGrant {
                name: "media".into(),
                path: directory.join("media"),
                read: true,
                write: false,
            }];
            #[cfg(unix)]
            let spec = {
                assert_eq!(INTERNAL_NAME, crate::loopback::MOUNT_NAME);
                crate::sandbox_spec(
                    &manifest,
                    &directory.join("driver"),
                    &directory.join("helper"),
                    &roots,
                    has_internal.then_some(internal.as_path()),
                    &[],
                )
                .expect("actual Unix Host sandbox plan")
            };
            #[cfg(windows)]
            let spec = {
                let mut spec = crate::sandbox_spec_windows(
                    &manifest,
                    &directory.join("driver"),
                    &directory.join("helper"),
                    &roots,
                    None,
                )
                .expect("actual Windows Host owner plan");
                // Windows uses a named pipe. Model the Unix transport mount only
                // for the platform-independent projection boundary regression.
                if has_internal {
                    spec.mounts.push(Mount {
                        source: internal.clone(),
                        class: MountClass::Workspace,
                        logical_name: INTERNAL_NAME.into(),
                        read_only: false,
                        execute: false,
                    });
                }
                spec
            };
            let owner_current = Arc::new(AtomicBool::new(true));
            let capsules = spec
                .mounts
                .iter()
                .filter(|mount| mount.class == MountClass::Workspace)
                .map(|mount| {
                    Arc::new(TestBinding {
                        mount: mount.clone(),
                        current: if mount.logical_name == "media" {
                            owner_current.clone()
                        } else {
                            Arc::new(AtomicBool::new(true))
                        },
                    }) as Arc<dyn WorkspaceMountBinding>
                })
                .collect();
            Self {
                manifest,
                spec,
                internal,
                owner_current,
                capsules,
                has_internal,
            }
        }

        fn project(&self, captures: Result<Bindings>) -> Result<Bindings> {
            owner_workspace_mount_bindings(
                &self.manifest,
                &self.spec,
                self.has_internal
                    .then_some((INTERNAL_NAME, self.internal.as_path())),
                captures,
            )
        }
    }

    fn policy_denied(result: Result<Bindings>) {
        match result {
            Err(error) => assert_eq!(error.code, ErrorCode::PolicyDenied),
            Ok(_) => panic!("unexpected or incomplete bindings were admitted"),
        }
    }

    fn capsule(mount: Mount) -> Arc<dyn WorkspaceMountBinding> {
        Arc::new(TestBinding {
            mount,
            current: Arc::new(AtomicBool::new(true)),
        })
    }

    #[test]
    fn mixed_loopback_projection_preserves_exact_owner_capsule() {
        let fixture = Fixture::new(true);
        assert_eq!(fixture.capsules.len(), 2, "causal mixed preimage");
        let owner = fixture
            .capsules
            .iter()
            .find(|b| b.mount().logical_name == "media")
            .unwrap();
        let projected = fixture.project(Ok(fixture.capsules.clone())).unwrap();
        assert_eq!(
            projected.len(),
            1,
            "Host loopback must not become an owner grant"
        );
        assert_eq!(projected[0].mount().logical_name, "media");
        assert!(Arc::ptr_eq(owner, &projected[0]));
        fixture.owner_current.store(false, Ordering::SeqCst);
        assert_eq!(
            projected[0].validate_current().unwrap_err().code,
            ErrorCode::StaleReference
        );
    }

    #[test]
    fn ordinary_owner_only_capture_keeps_identity_and_rights() {
        let fixture = Fixture::new(false);
        let projected = fixture.project(Ok(fixture.capsules.clone())).unwrap();
        assert_eq!(projected.len(), 1);
        assert!(Arc::ptr_eq(&fixture.capsules[0], &projected[0]));
        assert!(projected[0].mount().read_only);
        assert!(!projected[0].mount().execute);
    }

    #[test]
    fn unsupported_and_empty_capture_never_fabricate_authority() {
        let fixture = Fixture::new(true);
        assert!(fixture.project(Ok(vec![])).unwrap().is_empty());
        let result = fixture.project(Err(Error::new(
            ErrorCode::Unsupported,
            "fixture unsupported",
        )));
        let error = match result {
            Err(error) => error,
            Ok(_) => panic!("capture error was swallowed"),
        };
        assert_eq!(error.code, ErrorCode::Unsupported);
        assert_eq!(error.message, "fixture unsupported");
    }

    #[test]
    fn loopback_name_alone_does_not_authorize_exclusion() {
        let fixture = Fixture::new(true);
        policy_denied(owner_workspace_mount_bindings(
            &fixture.manifest,
            &fixture.spec,
            None,
            Ok(fixture.capsules.clone()),
        ));
        policy_denied(owner_workspace_mount_bindings(
            &fixture.manifest,
            &fixture.spec,
            Some((
                INTERNAL_NAME,
                fixture.internal.with_file_name("another-host").as_path(),
            )),
            Ok(fixture.capsules.clone()),
        ));
    }

    #[test]
    fn unexpected_or_changed_capsules_are_not_silently_filtered() {
        let fixture = Fixture::new(true);
        for index in 0..fixture.capsules.len() {
            for field in 0..5 {
                let mut altered = fixture.capsules[index].mount().clone();
                match field {
                    0 => altered.source = altered.source.with_file_name("wrong-instance"),
                    1 => altered.logical_name.push_str("-unexpected"),
                    2 => altered.class = MountClass::SystemConfig,
                    3 => altered.read_only = !altered.read_only,
                    4 => altered.execute = !altered.execute,
                    _ => unreachable!(),
                }
                let mut captures = fixture.capsules.clone();
                captures[index] = capsule(altered);
                policy_denied(fixture.project(Ok(captures)));
            }
        }
    }

    #[test]
    fn nonempty_partial_or_duplicate_capsule_sets_are_rejected() {
        let fixture = Fixture::new(true);
        for index in 0..fixture.capsules.len() {
            policy_denied(fixture.project(Ok(vec![fixture.capsules[index].clone()])));
            let mut duplicate = fixture.capsules.clone();
            duplicate.push(fixture.capsules[index].clone());
            policy_denied(fixture.project(Ok(duplicate)));
        }
    }

    #[test]
    fn unknown_or_duplicate_plan_mounts_are_rejected() {
        let mut fixture = Fixture::new(true);
        let mut extra = fixture.spec.mounts[0].clone();
        extra.logical_name = "undeclared-owner".into();
        fixture.spec.mounts.push(extra.clone());
        fixture.capsules.push(capsule(extra));
        policy_denied(fixture.project(Ok(fixture.capsules.clone())));
        let mut fixture = Fixture::new(true);
        fixture.spec.mounts.push(fixture.spec.mounts[0].clone());
        policy_denied(fixture.project(Ok(fixture.capsules.clone())));
    }

    #[test]
    fn missing_or_changed_declarations_do_not_become_owner_bindings() {
        let mut fixture = Fixture::new(true);
        fixture.manifest.mounts[0].read_only = false;
        policy_denied(fixture.project(Ok(fixture.capsules.clone())));
        let mut fixture = Fixture::new(true);
        fixture.manifest.mounts.clear();
        policy_denied(fixture.project(Ok(fixture.capsules.clone())));
        let mut fixture = Fixture::new(true);
        fixture
            .spec
            .mounts
            .retain(|mount| mount.logical_name != "media");
        fixture
            .capsules
            .retain(|binding| binding.mount().logical_name != "media");
        policy_denied(fixture.project(Ok(fixture.capsules.clone())));
    }
}
