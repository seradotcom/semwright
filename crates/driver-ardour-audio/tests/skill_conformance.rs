use semwright_ardour_audio::driver::capability_catalog as ardour_catalog;
use semwright_driver_sdk::{Capability, descriptor_digest};
use semwright_faust_audio::{
    analysis_driver::capability_catalog as analysis_catalog,
    driver::capability_catalog as faust_catalog,
};
use semwright_registry::Metadata;
use semwright_skills::{
    CatalogCapability, CatalogRoute, PolicyPreview, conformance_test, doctor, load,
    lock as skill_lock,
};
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
fn audio_skill_examples_are_schema_checked_and_do_not_execute_operations() {
    let skill = load(&skill_root()).unwrap();
    let report = conformance_test(&skill, &audio_catalog(), env!("CARGO_PKG_VERSION")).unwrap();
    assert!(report.pass, "{report:?}");
    assert_eq!(report.examples.checked, 3, "{report:?}");
    assert_eq!(report.examples.passed, 3, "{report:?}");
    assert!(report.examples.failures.is_empty());
    assert_eq!(report.executed_operations, 0);
    assert_eq!(report.script_execution, "disabled");
}

#[test]
fn audio_skill_lock_detects_descriptor_drift() {
    let skill = load(&skill_root()).unwrap();
    let mut catalog = audio_catalog();
    let generated = skill_lock(&skill, &catalog, env!("CARGO_PKG_VERSION")).unwrap();
    assert!(!generated.entries.is_empty());

    let mut locked_skill = skill.clone();
    locked_skill.lock = Some(generated);
    let current = doctor(&locked_skill, &catalog, env!("CARGO_PKG_VERSION")).unwrap();
    assert!(current.semwright_compatible, "{current:?}");
    assert!(current.drift.is_empty());

    let target = catalog
        .iter_mut()
        .find(|capability| capability.descriptor.name == "driver.faust-audio.synth.render")
        .unwrap();
    target.descriptor.version = "2".into();
    let drifted = doctor(&locked_skill, &catalog, env!("CARGO_PKG_VERSION")).unwrap();
    assert!(!drifted.semwright_compatible, "{drifted:?}");
    assert!(drifted.drift.iter().any(|drift| {
        drift.capability_id == "driver.faust-audio.synth.render"
            && drift.field == "capability_version"
    }));
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
