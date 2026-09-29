use semwright_semantic_composition::*;
fn main() -> Result<()> {
    let schemas = serde_json::json!({"contract_version":CONTRACT_VERSION,"canonical":"semwright-json-v1","owner":schemars::schema_for!(Owner),"base_state_set":schemars::schema_for!(BaseStateSet),"profile":schemars::schema_for!(ProfileDescriptor),"budget":schemars::schema_for!(ConvergenceBudget),"validation":schemars::schema_for!(ValidationReport),"verification":schemars::schema_for!(VerificationReport)});
    println!(
        "{}",
        serde_json::to_string_pretty(&schemas)
            .map_err(|e| ContractError::Invalid(e.to_string()))?
    );
    Ok(())
}
