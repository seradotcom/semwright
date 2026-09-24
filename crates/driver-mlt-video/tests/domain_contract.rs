mod common;

use common::{fixture, small};
use semwright_mlt_video::{adapters, domain};
use semwright_video_domain::{
    backend::{ProjectionFidelity, ProjectionLossImpact},
    support::{MutationSupport, VideoOperation},
};

#[test]
fn generated_mlt_exposes_complete_safe_semantic_contract() {
    let project = small();
    let contract = domain::contract(&project).unwrap();
    contract.validate().unwrap();

    assert_eq!(contract.identity.backend_id, "mlt");
    assert_eq!(contract.operations.len(), VideoOperation::ALL.len());
    for operation in VideoOperation::ALL {
        assert_eq!(
            contract.support(*operation),
            MutationSupport::SafeRoundtrip,
            "{operation}"
        );
    }
    assert_eq!(contract.projection_fidelity, ProjectionFidelity::Exact);
}
#[test]
fn kdenlive_contract_reuses_native_support_source_of_truth() {
    let project = adapters::load(&fixture("kdenlive/simple.kdenlive")).unwrap();
    let contract = domain::contract(&project).unwrap();
    let adapter = adapters::adapter(project.format);

    assert_eq!(contract.identity.backend_id, "kdenlive");
    for operation in VideoOperation::ALL {
        assert_eq!(
            contract.support(*operation),
            adapter.supported_mutation(&project, *operation),
            "{operation}"
        );
    }
    assert_eq!(
        contract.support(VideoOperation::TrackRename),
        MutationSupport::MetadataRisk
    );
}

#[test]
fn opaque_native_state_is_structured_as_read_only_projection_loss() {
    let mut project = small();
    project.sequences[0].tracks[0].opaque = true;

    let report = domain::report(&project).unwrap();
    assert_eq!(report.fidelity, ProjectionFidelity::LossyReadOnly);
    assert!(report.losses.iter().any(|loss| {
        loss.impact == ProjectionLossImpact::ReadOnly
            && loss.code == "native.opaque_track"
            && loss
                .semantic_path
                .as_deref()
                .is_some_and(|path| path.contains("track/t"))
    }));
}

#[test]
fn native_warnings_are_not_hidden_inside_portable_project_only() {
    let mut project = small();
    project
        .warnings
        .push("unknown native version requires inspection".into());

    let report = domain::report(&project).unwrap();
    assert!(report.project.warnings.is_empty());
    assert_eq!(report.fidelity, ProjectionFidelity::SemanticallyEquivalent);
    assert!(report.losses.iter().any(|loss| {
        loss.code == "native.warning" && loss.detail.contains("unknown native version")
    }));
}
