mod common;
use common::*;
use semwright_effect_conformance::composition::*;
use semwright_effect_conformance::*;
use semwright_types::{CommandDescriptor, Idempotency, Risk};
use std::collections::BTreeMap;
fn descriptor(name: &str, risk: Risk) -> CommandDescriptor {
    CommandDescriptor {
        name: name.into(),
        version: "1".into(),
        description: "contract fixture, not a native catalog".into(),
        input_schema: serde_json::json!({"type":"object","additionalProperties":false}),
        output_schema: serde_json::json!({"type":"object"}),
        requires: vec![],
        risk,
        idempotency: Idempotency::NonIdempotent,
        timeout_ms: 1000,
        dry_run: false,
        interactive_consent: false,
        backends: vec!["contractual".into()],
    }
}
fn workflow(contract: &EffectContract, descriptors: &[CommandDescriptor]) -> ReadbackWorkflow {
    ReadbackWorkflow {
        version: 1,
        workflow: "save-observe".into(),
        mutation_command: descriptors[0].name.clone(),
        mutation_descriptor: canonical_digest(&descriptors[0]).unwrap(),
        contract_digest: contract.digest().unwrap(),
        persistence_required: false,
        mappings: contract
            .rules
            .iter()
            .map(|r| ReadbackMapping {
                rule: r.id.clone(),
                route: ReadbackRoute::NativeProperty,
                observer_commands: BTreeMap::from([(
                    descriptors[1].name.clone(),
                    canonical_digest(&descriptors[1]).unwrap(),
                )]),
                isolation_required: false,
                limitation: None,
            })
            .collect(),
    }
}
#[test]
fn static_readback_metadata_is_not_accepted_without_executing_observer() {
    let (contract, ctx) = fixture();
    let descriptors = vec![
        descriptor("test.apply", Risk::Mutating),
        descriptor("test.observe", Risk::ReadOnly),
    ];
    let wf = workflow(&contract, &descriptors);
    let mut adapter = ModelAdapter::default();
    assert_eq!(
        run_readback_conformance(&descriptors, &wf, &contract, &ctx, &mut adapter)
            .unwrap()
            .verdict()
            .unwrap(),
        Verdict::Pass
    );
    assert_eq!(adapter.calls, 2);
    let mut mutant = ModelAdapter {
        mutate: |r, o| {
            if r.id == "position" {
                o.value = Some(ObservedValue::Number {
                    value: 42.0,
                    units: "metre".into(),
                });
            }
        },
        ..Default::default()
    };
    assert_eq!(
        run_readback_conformance(&descriptors, &wf, &contract, &ctx, &mut mutant)
            .unwrap()
            .verdict()
            .unwrap(),
        Verdict::Fail
    );
}
#[test]
fn missing_drifted_and_duplicate_descriptors_cannot_pass_static_lint() {
    let (contract, _) = fixture();
    let mut descriptors = vec![
        descriptor("test.apply", Risk::Mutating),
        descriptor("test.observe", Risk::ReadOnly),
    ];
    let wf = workflow(&contract, &descriptors);
    descriptors[1].version = "2".into();
    assert!(lint_readback(&descriptors, &contract, &wf).is_err());
    descriptors.pop();
    assert!(lint_readback(&descriptors, &contract, &wf).is_err());
    descriptors.push(descriptors[0].clone());
    assert!(lint_readback(&descriptors, &contract, &wf).is_err());
}
#[test]
fn missing_observability_is_a_visible_gap_not_a_reason_to_delete_capability() {
    let (contract, ctx) = fixture();
    let descriptors = vec![
        descriptor("test.apply", Risk::Mutating),
        descriptor("test.observe", Risk::ReadOnly),
    ];
    let mut wf = workflow(&contract, &descriptors);
    wf.mappings[0].route = ReadbackRoute::Unavailable;
    wf.mappings[0].observer_commands.clear();
    wf.mappings[0].limitation =
        Some("irreversible downstream action; no independent result readback".into());
    let mut adapter = ModelAdapter::default();
    let evaluation =
        run_readback_conformance(&descriptors, &wf, &contract, &ctx, &mut adapter).unwrap();
    assert_eq!(adapter.calls, 1);
    assert_eq!(evaluation.verdict().unwrap(), Verdict::Unknown);
    assert!(
        evaluation
            .report
            .effects_unobservable
            .contains(&contract.rules[0].address)
    );
}
#[test]
fn native_reopen_cannot_be_hidden_under_read_only_validator_label() {
    let (contract, _) = fixture();
    let descriptors = vec![
        descriptor("test.apply", Risk::Mutating),
        descriptor("test.observe", Risk::ReadOnly),
    ];
    let mut wf = workflow(&contract, &descriptors);
    wf.mappings[0].route = ReadbackRoute::FileReopen;
    wf.mappings[0].isolation_required = true;
    assert!(lint_readback(&descriptors, &contract, &wf).is_err());
}
