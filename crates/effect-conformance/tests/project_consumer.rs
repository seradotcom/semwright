mod common;
use common::*;
use semwright_effect_conformance::composition::*;
use semwright_effect_conformance::*;
use semwright_project_graph as graph;

fn receipt(evaluation: EffectEvaluation) -> graph::ExecutionReceipt {
    let report = evaluation.report;
    graph::ExecutionReceipt {
        version: 1,
        id: graph::ReceiptId::new(),
        derivation: graph::DerivationId::new(),
        project: graph::ProjectId::new(),
        owner: evaluation.owner,
        request_id: evaluation.request_id,
        operation: graph::OperationIdentity {
            capability: "contractual.effect-consumer".into(),
            descriptor: Digest::of_bytes(b"contractual-descriptor"),
            runtime: Digest::of_bytes(b"no-native-runtime"),
            plan: report.validation.plan_digest.clone(),
            parameters: Digest::of_bytes(b"fixture-parameters"),
            recipe: None,
        },
        source_base: report.validation.base.clone(),
        inputs: vec![],
        outputs: vec![graph::RevisionPin {
            asset: graph::LogicalAssetId::new(),
            revision: graph::AssetRevision::new(),
            fingerprint: graph::Fingerprint {
                bytes: Some(Digest::of_bytes(b"fixture-output")),
                projection: None,
            },
            equivalence: graph::Equivalence::ExactBytes,
        }],
        determinants: vec![graph::Determinant {
            class: graph::DependencyClass::Contract,
            key: "effects.contract".into(),
            digest: evaluation.contract_digest,
        }],
        coverage: graph::Coverage::unknown(),
        verification: report,
        completed_unix_ms: 1,
    }
}
#[test]
fn c_p0_receipt_preserves_f_required_unknown_and_fail_without_safety_promotion() {
    let adapter = graph::ReceiptAdapter::registered(
        "contractual.effect-consumer".into(),
        Digest::of_bytes(b"contractual-descriptor"),
        Digest::of_bytes(b"no-native-runtime"),
    )
    .unwrap();
    let unknown = run(&mut ModelAdapter {
        trusted: false,
        ..Default::default()
    });
    let failure = run(&mut ModelAdapter {
        mutate: |r, o| {
            if r.id == "position" {
                o.value = Some(ObservedValue::Number {
                    value: 9.0,
                    units: "metre".into(),
                });
            }
        },
        ..Default::default()
    });
    for (evaluation, expected) in [(unknown, Verdict::Unknown), (failure, Verdict::Fail)] {
        let record = receipt(evaluation);
        record.validate().unwrap();
        let admitted = adapter
            .admit(&record.owner.clone(), &record.request_id.clone(), record)
            .unwrap();
        assert_eq!(admitted.record().verification.verdict().unwrap(), expected);
        assert!(!admitted.record().coverage.cache_safe());
        let bytes = canonical_bytes(admitted.record()).unwrap();
        let decoded: graph::ExecutionReceipt = strict_decode(&bytes).unwrap();
        assert_eq!(decoded.verification.verdict().unwrap(), expected);
        let mut substituted = decoded.owner.clone();
        substituted.session = "wrong-owner".into();
        assert!(adapter.admit(&substituted, "request-1", decoded).is_err());
    }
}
