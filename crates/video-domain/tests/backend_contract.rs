use semwright_video_domain::{
    backend::{
        BackendContract, BackendIdentity, ProjectionFidelity, ProjectionLoss, ProjectionLossImpact,
        ProjectionLossKind, ProjectionReport, SemanticVideoProjection,
    },
    model::{Profile, Project},
    support::{MutationSupport, VideoOperation},
};

fn contract() -> BackendContract {
    BackendContract::from_support(
        BackendIdentity {
            backend_id: "fixture".into(),
            backend_version: Some("1.0".into()),
            adapter_id: "fixture-adapter/1".into(),
        },
        ProjectionFidelity::Exact,
        |_| (MutationSupport::SafeRoundtrip, None),
    )
    .unwrap()
}
#[test]
fn backend_contract_is_complete_unique_and_serializable() {
    let value = contract();
    value.validate().unwrap();
    assert_eq!(
        value.support(VideoOperation::ClipTrim),
        MutationSupport::SafeRoundtrip
    );

    let encoded = serde_json::to_vec(&value).unwrap();
    let decoded: BackendContract = serde_json::from_slice(&encoded).unwrap();
    assert_eq!(decoded, value);
}

#[test]
fn backend_contract_rejects_missing_and_duplicate_operations() {
    let mut missing = contract();
    missing.operations.pop();
    assert!(missing.validate().is_err());

    let mut duplicate = contract();
    duplicate.operations[1].operation = duplicate.operations[0].operation;
    assert!(duplicate.validate().is_err());
}
#[test]
fn backend_contract_rejects_unbounded_or_ambiguous_identity() {
    let mut value = contract();
    value.identity.backend_id = "../native".into();
    assert!(value.validate().is_err());

    let mut value = contract();
    value.operations[0].reason = Some(String::new());
    assert!(value.validate().is_err());
}

fn loss(impact: ProjectionLossImpact) -> ProjectionLoss {
    ProjectionLoss {
        kind: ProjectionLossKind::OpaqueNativeObject,
        impact,
        code: "native.opaque".into(),
        semantic_path: Some("sequence/main/clip/c1".into()),
        detail: "native object is preserved outside the portable model".into(),
    }
}
#[test]
fn exact_projection_cannot_hide_losses() {
    let report = ProjectionReport {
        project: Project::new(Profile::default()).unwrap(),
        fidelity: ProjectionFidelity::Exact,
        losses: vec![loss(ProjectionLossImpact::Advisory)],
    };
    assert!(report.validate().is_err());
}

#[test]
fn read_only_loss_requires_lossy_fidelity() {
    let mut report = ProjectionReport {
        project: Project::new(Profile::default()).unwrap(),
        fidelity: ProjectionFidelity::SemanticallyEquivalent,
        losses: vec![loss(ProjectionLossImpact::ReadOnly)],
    };
    assert!(report.validate().is_err());

    report.fidelity = ProjectionFidelity::LossyReadOnly;
    report.validate().unwrap();
}
#[test]
fn projection_contract_denies_unknown_fields() {
    let mut value = serde_json::to_value(contract()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("authority".into(), serde_json::Value::Bool(true));
    assert!(serde_json::from_value::<BackendContract>(value).is_err());
}

#[test]
fn backend_contract_rejects_adapter_path_traversal_and_control_text() {
    let mut value = contract();
    value.identity.adapter_id = "../fixture".into();
    assert!(value.validate().is_err());

    let mut value = contract();
    value.identity.adapter_id = "fixture//1".into();
    assert!(value.validate().is_err());

    let mut value = contract();
    value.identity.backend_version = Some("1.0\nforged".into());
    assert!(value.validate().is_err());

    let mut value = contract();
    value.operations[0].reason = Some("unsafe\rmetadata".into());
    assert!(value.validate().is_err());

    let mut report = ProjectionReport {
        project: Project::new(Profile::default()).unwrap(),
        fidelity: ProjectionFidelity::SemanticallyEquivalent,
        losses: vec![loss(ProjectionLossImpact::Advisory)],
    };
    report.losses[0].detail = "unsafe\u{0007}detail".into();
    assert!(report.validate().is_err());
}

#[derive(Clone)]
struct ForeignNative {
    semantic: Project,
}

struct ForeignProjection;

impl SemanticVideoProjection<ForeignNative> for ForeignProjection {
    fn contract(&self, _native: &ForeignNative) -> semwright_video_domain::Result<BackendContract> {
        BackendContract::from_support(
            BackendIdentity {
                backend_id: "foreign-editor".into(),
                backend_version: Some("42".into()),
                adapter_id: "foreign-semantic/1".into(),
            },
            ProjectionFidelity::Exact,
            |operation| {
                if operation == VideoOperation::ClipTrim {
                    (MutationSupport::SafeRoundtrip, None)
                } else {
                    (
                        MutationSupport::Unsupported,
                        Some("fixture backend only demonstrates trim".into()),
                    )
                }
            },
        )
    }

    fn project(&self, native: &ForeignNative) -> semwright_video_domain::Result<ProjectionReport> {
        let report = ProjectionReport {
            project: native.semantic.clone(),
            fidelity: ProjectionFidelity::Exact,
            losses: vec![],
        };
        report.validate()?;
        Ok(report)
    }
}

#[test]
fn foreign_backend_can_implement_contract_without_native_format_types() {
    let native = ForeignNative {
        semantic: Project::new(Profile::default()).unwrap(),
    };
    let adapter = ForeignProjection;
    let contract = adapter.contract(&native).unwrap();
    let report = adapter.project(&native).unwrap();

    assert_eq!(contract.identity.backend_id, "foreign-editor");
    assert_eq!(
        contract.support(VideoOperation::ClipTrim),
        MutationSupport::SafeRoundtrip
    );
    assert_eq!(
        contract.support(VideoOperation::ClipSplit),
        MutationSupport::Unsupported
    );
    assert_eq!(report.project, native.semantic);
    assert_eq!(report.fidelity, ProjectionFidelity::Exact);
}

#[test]
fn checked_in_fuzz_seeds_are_valid_semantic_contracts() {
    let contract: BackendContract = serde_json::from_slice(include_bytes!(
        "../../../fuzz/corpus/video_domain_contract/contract.json"
    ))
    .unwrap();
    contract.validate().unwrap();

    let report: ProjectionReport = serde_json::from_slice(include_bytes!(
        "../../../fuzz/corpus/video_domain_contract/projection.json"
    ))
    .unwrap();
    report.validate().unwrap();
}
