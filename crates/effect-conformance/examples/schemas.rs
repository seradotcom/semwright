fn main() {
    let schema = schemars::schema_for!(semwright_effect_conformance::EffectContract);
    println!(
        "{}",
        serde_json::to_string_pretty(&schema).expect("schema serialization")
    );
}
