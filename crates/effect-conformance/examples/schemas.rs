fn main() {
    let schemas = serde_json::json!({
        "contract": schemars::schema_for!(semwright_effect_conformance::EffectContract),
        "readback_workflow": schemars::schema_for!(semwright_effect_conformance::ReadbackWorkflow),
        "observation": schemars::schema_for!(semwright_effect_conformance::AdapterObservation),
        "evaluation": schemars::schema_for!(semwright_effect_conformance::EffectEvaluation),
        "workflow_quality": schemars::schema_for!(semwright_effect_conformance::WorkflowQuality),
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&schemas).expect("schema serialization")
    );
}
