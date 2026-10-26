//! Host-owned source observations. These are never execution authority.
#[cfg(any(unix, windows))]
use super::{SealedTool, StagedFile};
use semwright_driver_sdk::Manifest;
use semwright_types::{
    Error, ErrorCode, ProviderIdentity, ProviderMountProfileRequirement, Result, unique_id,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;

const MAX_MANIFEST_INPUT_BYTES: usize = 1024 * 1024;
const VOICE_LAUNCHER_NAME: &str = "audio-alignment-launcher";

/// Opaque capture of inputs used by one concrete DriverProvider.
/// No raw source, native handle, constructor or deserialization is exposed.
pub struct HostProfileObservation {
    identity: ProviderIdentity,
    facts: ProfileFacts,
    requirement: ProviderMountProfileRequirement,
    #[cfg(any(unix, windows))]
    _driver: Arc<StagedFile>,
    #[cfg(any(unix, windows))]
    _tools: Arc<Vec<SealedTool>>,
}

pub(crate) struct ValidatedManifestInput {
    original: Arc<[u8]>,
    sha256: String,
    semantic_value: Value,
}
#[cfg(any(unix, windows))]
pub(crate) struct PreparedHostProfileObservation {
    identity: ProviderIdentity,
    facts: ProfileFacts,
    requirement: ProviderMountProfileRequirement,
}
struct ProfileFacts {
    capture_id: String,
    manifest_original: Arc<[u8]>,
    manifest_sha256: String,
    driver_sha256: String,
    driver_bytes: u64,
    launcher: Option<LauncherFacts>,
}
struct LauncherFacts {
    sha256: String,
    mounts: Vec<String>,
}
pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}
fn manifest_value(manifest: &Manifest) -> Result<Value> {
    serde_json::to_value(manifest)
        .map_err(|_| Error::invalid("Driver manifest cannot be represented"))
}
pub(crate) fn validate_retained_manifest(
    manifest: &Manifest,
    original: Arc<[u8]>,
) -> Result<ValidatedManifestInput> {
    if original.is_empty() || original.len() > MAX_MANIFEST_INPUT_BYTES {
        return Err(Error::invalid(
            "Retained driver manifest input has an invalid size",
        ));
    }
    manifest.validate()?;
    let parsed: Manifest = serde_json::from_slice(original.as_ref())
        .map_err(|_| Error::invalid("Retained driver manifest input is invalid"))?;
    parsed.validate()?;
    let semantic_value = manifest_value(manifest)?;
    if manifest_value(&parsed)? != semantic_value {
        return Err(Error::invalid(
            "Retained driver manifest differs from the selected manifest",
        ));
    }
    let sha256 = sha256_bytes(original.as_ref());
    Ok(ValidatedManifestInput {
        original,
        sha256,
        semantic_value,
    })
}
#[cfg(any(unix, windows))]
pub(crate) fn prepare_profile_observation(
    input: ValidatedManifestInput,
    manifest: &Manifest,
    identity: &ProviderIdentity,
    driver_bytes: &[u8],
    sealed_tools: &[SealedTool],
) -> Result<PreparedHostProfileObservation> {
    if manifest_value(manifest)? != input.semantic_value || &manifest.identity()? != identity {
        return Err(Error::invalid(
            "Driver manifest changed before profile capture",
        ));
    }
    let driver_sha256 = sha256_bytes(driver_bytes);
    if driver_sha256 != manifest.sha256.to_ascii_lowercase()
        || manifest.tools.len() != sealed_tools.len()
    {
        return Err(Error::invalid(
            "Verified driver resources differ from the selected manifest",
        ));
    }
    let mut launcher = None;
    for (declared, retained) in manifest.tools.iter().zip(sealed_tools) {
        if declared.name != retained.name || declared.sha256.to_ascii_lowercase() != retained.sha256
        {
            return Err(Error::invalid(
                "Verified driver tool differs from the selected manifest",
            ));
        }
        if declared.name == VOICE_LAUNCHER_NAME {
            launcher = Some(LauncherFacts {
                sha256: retained.sha256.clone(),
                mounts: declared.mounts.clone(),
            });
        }
    }
    let facts = ProfileFacts {
        capture_id: unique_id(),
        manifest_original: input.original,
        manifest_sha256: input.sha256,
        driver_sha256,
        driver_bytes: driver_bytes.len() as u64,
        launcher,
    };
    let requirement = ProviderMountProfileRequirement::voice_alignment_v1_from_host(
        capsule_digest(identity, &facts)?,
    );
    Ok(PreparedHostProfileObservation {
        identity: identity.clone(),
        facts,
        requirement,
    })
}
#[cfg(any(unix, windows))]
impl PreparedHostProfileObservation {
    /// Infallible after launch: retain the exact resources stored in the provider.
    pub(crate) fn retain(
        self,
        driver: Arc<StagedFile>,
        tools: Arc<Vec<SealedTool>>,
    ) -> Arc<HostProfileObservation> {
        Arc::new(HostProfileObservation {
            identity: self.identity,
            facts: self.facts,
            requirement: self.requirement,
            _driver: driver,
            _tools: tools,
        })
    }
}
fn capsule_digest(identity: &ProviderIdentity, facts: &ProfileFacts) -> Result<[u8; 32]> {
    let identity = serde_json::to_value(identity)
        .map_err(|_| Error::invalid("Provider identity cannot be represented"))?;
    capsule_digest_from_identity_value(identity, facts)
}
fn capsule_digest_from_identity_value(identity: Value, facts: &ProfileFacts) -> Result<[u8; 32]> {
    let launcher = facts.launcher.as_ref().map(|launcher| {
        let mut mounts = launcher.mounts.clone();
        mounts.sort();
        json!({"name": VOICE_LAUNCHER_NAME, "sha256": launcher.sha256, "mounts": mounts})
    });
    let material = json!({
        "contract": {"id": "voice-alignment", "version": 1},
        "capture_id": facts.capture_id,
        "provider_identity": identity,
        "manifest": {"sha256": facts.manifest_sha256, "bytes": facts.manifest_original.len()},
        "driver": {"sha256": facts.driver_sha256, "bytes": facts.driver_bytes},
        "launcher": launcher,
        "execution_state": "not-observed", "execution_admission": "blocked",
        "unavailable": ["runtime-resources", "model-resources", "tool-child-linkage"],
    });
    let encoded = serde_json::to_vec(&material)
        .map_err(|_| Error::invalid("Provider profile cannot be represented"))?;
    let mut digest = Sha256::new();
    digest.update(b"semwright.host-profile-capsule\0voice-alignment\0v1\0");
    digest.update((encoded.len() as u64).to_be_bytes());
    digest.update(encoded);
    Ok(digest.finalize().into())
}
fn scoped_launcher<'a>(facts: &'a ProfileFacts, root: &str) -> Result<&'a LauncherFacts> {
    facts
        .launcher
        .as_ref()
        .filter(|launcher| launcher.mounts.iter().any(|mount| mount == root))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::Unsupported,
                "Voice alignment observation is unavailable for this mount",
            )
        })
}
impl HostProfileObservation {
    pub fn requirement(&self) -> ProviderMountProfileRequirement {
        self.requirement.clone()
    }
    pub fn check_profile_admission(
        &self,
        identity: &ProviderIdentity,
        root: &str,
        requirement: &ProviderMountProfileRequirement,
    ) -> Result<()> {
        if &self.identity != identity || &self.requirement != requirement {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Provider profile admission differs from the retained capture",
            ));
        }
        scoped_launcher(&self.facts, root)?;
        Ok(())
    }
    /// Current captures lack runtime/model/tool-child closure. A subsequent
    /// complete capture requires a new immutable admission; old refs cannot upgrade.
    pub fn require_execution_readiness(&self) -> Result<()> {
        Err(Error::new(
            ErrorCode::Unsupported,
            "Voice alignment execution requires runtime, model, and tool-child evidence",
        ))
    }
    pub fn observe_voice_alignment(
        &self,
        identity: &ProviderIdentity,
        root: &str,
    ) -> Result<Value> {
        self.check_profile_admission(identity, root, &self.requirement)?;
        let mut view = project_voice_alignment(&self.facts, root)?;
        view.as_object_mut()
            .expect("profile projection is an object")
            .insert(
                "capsule_sha256".into(),
                Value::String(hex::encode(self.requirement.capsule_sha256())),
            );
        Ok(view)
    }
}
fn project_voice_alignment(facts: &ProfileFacts, root: &str) -> Result<Value> {
    let launcher = scoped_launcher(facts, root)?;
    // Only facts for the selected provider and authorized logical root are
    // projected. Retaining a staged path does not prove immutable execution.
    Ok(json!({
        "version": 1, "name": "voice-alignment", "state": "unavailable",
        "scope": {"root": root},
        "manifest": {"state": "observed", "basis": "retained-input",
                     "sha256": facts.manifest_sha256, "bytes": facts.manifest_original.len()},
        "driver": {"state": "observed", "basis": "verified-input",
                   "sha256": facts.driver_sha256, "bytes": facts.driver_bytes},
        "launcher": {"state": "observed", "basis": "verified-input",
                     "name": VOICE_LAUNCHER_NAME, "sha256": launcher.sha256},
        "execution": {"state": "not-observed", "admission": "blocked"},
        "unavailable": ["runtime-resources", "model-resources", "tool-child-linkage"],
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    // Projection fixtures do not construct a Host capsule or simulate native execution.
    fn facts(mounts: &[&str], has_launcher: bool) -> ProfileFacts {
        let original: Arc<[u8]> = Arc::from(&b"private/path and other-root projection fixture"[..]);
        ProfileFacts {
            capture_id: "synthetic-capture".into(),
            manifest_sha256: sha256_bytes(original.as_ref()),
            manifest_original: original,
            driver_sha256: sha256_bytes(b"observed-driver-input"),
            driver_bytes: 21,
            launcher: has_launcher.then(|| LauncherFacts {
                sha256: sha256_bytes(b"observed-launcher-input"),
                mounts: mounts.iter().map(|v| (*v).to_owned()).collect(),
            }),
        }
    }
    #[test]
    fn projection_requires_the_actual_selected_launcher_mount() {
        let observed = facts(&["workspace"], true);
        assert!(project_voice_alignment(&observed, "workspace").is_ok());
        for root in ["other-root", "workspace/../other-root", ""] {
            assert!(project_voice_alignment(&observed, root).is_err());
        }
        assert!(project_voice_alignment(&facts(&["workspace"], false), "workspace").is_err());
    }
    #[test]
    fn projection_omits_private_inputs_and_keeps_execution_unavailable() {
        let view = project_voice_alignment(&facts(&["workspace", "other-root"], true), "workspace")
            .unwrap();
        assert_eq!(view["state"], "unavailable");
        assert_eq!(
            view["execution"],
            json!({"state":"not-observed","admission":"blocked"})
        );
        let encoded = serde_json::to_string(&view).unwrap();
        assert!(!encoded.contains("private/path"));
        assert!(!encoded.contains("other-root"));
        assert!(view.get("loaded").is_none());
        assert!(view.get("ready").is_none());
    }
    #[test]
    fn capture_digest_binds_instance_identity_and_observed_originals() {
        let mut value = facts(&["workspace"], true);
        let id = json!({"id":"synthetic-provider"});
        let initial = capsule_digest_from_identity_value(id.clone(), &value).unwrap();
        value.capture_id.push('x');
        assert_ne!(
            initial,
            capsule_digest_from_identity_value(id.clone(), &value).unwrap()
        );
        value.capture_id.pop();
        value.manifest_sha256 = sha256_bytes(b"different-original");
        assert_ne!(
            initial,
            capsule_digest_from_identity_value(id.clone(), &value).unwrap()
        );
        let original = facts(&["workspace"], true);
        assert_ne!(
            initial,
            capsule_digest_from_identity_value(json!({"id":"different-provider"}), &original)
                .unwrap()
        );
    }
}

#[cfg(test)]
mod retained_input_tests {
    use super::*;
    fn original() -> Arc<[u8]> {
        let executable = if cfg!(windows) {
            r"C:\semwright\fixture.exe"
        } else {
            "/tmp/semwright-fixture"
        };
        // Intentional whitespace is part of this raw UTF-8 fixture.
        let raw = format!(
            "\n {{ \"manifest_version\":1, \"protocol\":1,\n\
            \"id\":\"fixture\", \"version\":\"1\", \"publisher\":\"tests\",\n\
            \"executable\":{}, \"sha256\":\"{}\",\n\
            \"application\":{{\"desktop_id\":\"org.example.Fixture\"}},\n\
            \"transport\":\"stdio_v1\" }} \n",
            serde_json::to_string(executable).unwrap(),
            "a".repeat(64)
        );
        Arc::from(raw.into_bytes())
    }
    #[test]
    fn retained_input_uses_original_bytes_and_same_arc() {
        let raw = original();
        let manifest: Manifest = serde_json::from_slice(raw.as_ref()).unwrap();
        let retained = validate_retained_manifest(&manifest, raw.clone()).unwrap();
        assert!(Arc::ptr_eq(&retained.original, &raw));
        assert_eq!(retained.sha256, sha256_bytes(raw.as_ref()));
        let normalized = serde_json::to_vec(&manifest).unwrap();
        assert_ne!(retained.sha256, sha256_bytes(&normalized));
        assert_eq!(retained.original.as_ref(), raw.as_ref());
    }
    #[test]
    fn changed_selected_manifest_cannot_borrow_original_provenance() {
        let raw = original();
        let original: Manifest = serde_json::from_slice(raw.as_ref()).unwrap();
        let mut changed = original.clone();
        changed.network = true;
        assert!(validate_retained_manifest(&changed, raw.clone()).is_err());
        let mut changed = original.clone();
        changed.request_timeout_ms += 1;
        assert!(validate_retained_manifest(&changed, raw.clone()).is_err());
        let mut changed = original;
        changed.application.desktop_id = Some("org.example.Other".into());
        assert!(validate_retained_manifest(&changed, raw).is_err());
    }
    #[test]
    fn malformed_empty_and_oversized_original_inputs_fail_closed() {
        let manifest: Manifest = serde_json::from_slice(original().as_ref()).unwrap();
        for raw in [
            Arc::<[u8]>::from(&b""[..]),
            Arc::<[u8]>::from(&b"{bad json}"[..]),
            Arc::<[u8]>::from(vec![b' '; MAX_MANIFEST_INPUT_BYTES + 1]),
        ] {
            assert!(validate_retained_manifest(&manifest, raw).is_err());
        }
    }
}
