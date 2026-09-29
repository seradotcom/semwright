use semwright_ardour_audio::driver::capability_catalog as ardour_catalog;
use semwright_driver_sdk::{Capability, descriptor_digest};
use semwright_faust_audio::{
    analysis_driver::capability_catalog as analysis_catalog,
    driver::capability_catalog as faust_catalog,
};
use semwright_registry::Metadata;
use semwright_skills::{CatalogCapability, CatalogRoute, PolicyPreview, doctor, load};
use semwright_types::SourceKind;
use std::path::PathBuf;

fn skill_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../skills/semwright-audio-production")
}

fn catalog_entry(provider: &str, capability: Capability) -> CatalogCapability {
    let descriptor_sha256 = descriptor_digest(&capability.descriptor).unwrap();
    CatalogCapability {
        descriptor: capability.descriptor,
        provenance: Metadata {
            source: SourceKind::Driver,
            provider: provider.into(),
            source_version: env!("CARGO_PKG_VERSION").into(),
            app: None,
            aliases: capability.aliases,
            tags: capability.tags,
            object_types: capability.object_types,
            untrusted_metadata: true,
            descriptor_sha256,
        },
        routes: vec![CatalogRoute {
            provider: provider.into(),
            status: "ready".into(),
            available: true,
        }],
        policy_preview: Some(PolicyPreview {
            state: "allow".into(),
            preview_only: true,
            execution_rechecks: true,
            reason: "unit fixture".into(),
        }),
    }
}

fn audio_catalog() -> Vec<CatalogCapability> {
    faust_catalog()
        .into_iter()
        .map(|capability| catalog_entry("driver:faust-audio", capability))
        .chain(
            analysis_catalog()
                .into_iter()
                .map(|capability| catalog_entry("driver:audio-analysis", capability)),
        )
        .chain(
            ardour_catalog()
                .into_iter()
                .map(|capability| catalog_entry("driver:ardour-audio", capability)),
        )
        .collect()
}

#[test]
fn audio_skill_resolves_against_real_audio_catalog_without_grant_escalation() {
    let skill = load(&skill_root()).unwrap();
    let report = doctor(&skill, &audio_catalog(), env!("CARGO_PKG_VERSION")).unwrap();
    assert!(report.semwright_compatible, "{report:?}");
    assert_eq!(report.result, "ready", "{report:?}");
    assert!(report.missing.is_empty());
    assert!(report.unavailable.is_empty());
    assert!(report.policy_denied.is_empty());
    assert!(!report.authority_summary.grants_changed);
    assert!(
        report
            .authority_summary
            .required_scopes
            .contains(&"driver:faust-audio".to_string())
    );
    assert!(
        report
            .authority_summary
            .required_scopes
            .contains(&"driver:audio-analysis".to_string())
    );
    assert!(
        report
            .authority_summary
            .required_scopes
            .contains(&"driver:ardour-audio".to_string())
    );
}

#[test]
fn audio_skill_missing_required_provider_is_incompatible() {
    let skill = load(&skill_root()).unwrap();
    let mut catalog = audio_catalog();
    catalog.retain(|capability| {
        capability.descriptor.name != "driver.audio-analysis.artifact.measure"
    });
    let report = doctor(&skill, &catalog, env!("CARGO_PKG_VERSION")).unwrap();
    assert!(!report.semwright_compatible, "{report:?}");
    assert!(
        report
            .missing
            .iter()
            .any(|value| value.contains("driver.audio-analysis.artifact.measure"))
    );
}

#[test]
fn audio_skill_required_unavailable_route_does_not_degrade_to_ready() {
    let skill = load(&skill_root()).unwrap();
    let mut catalog = audio_catalog();
    let target = catalog
        .iter_mut()
        .find(|capability| capability.descriptor.name == "driver.faust-audio.sample.render")
        .unwrap();
    target.routes[0].available = false;
    target.routes[0].status = "unavailable".into();
    let report = doctor(&skill, &catalog, env!("CARGO_PKG_VERSION")).unwrap();
    assert!(!report.semwright_compatible, "{report:?}");
    assert!(
        report
            .unavailable
            .iter()
            .any(|value| value.contains("driver.faust-audio.sample.render"))
    );
}

#[test]
fn audio_skill_policy_preview_denial_is_not_authority() {
    let skill = load(&skill_root()).unwrap();
    let mut catalog = audio_catalog();
    let target = catalog
        .iter_mut()
        .find(|capability| capability.descriptor.name == "driver.ardour-audio.session.deep.create")
        .unwrap();
    target.policy_preview = Some(PolicyPreview {
        state: "deny".into(),
        preview_only: true,
        execution_rechecks: true,
        reason: "unit deny fixture".into(),
    });
    let report = doctor(&skill, &catalog, env!("CARGO_PKG_VERSION")).unwrap();
    assert!(!report.semwright_compatible, "{report:?}");
    assert!(
        report
            .policy_denied
            .iter()
            .any(|value| value.contains("driver.ardour-audio.session.deep.create"))
    );
    assert!(!report.authority_summary.grants_changed);
}
