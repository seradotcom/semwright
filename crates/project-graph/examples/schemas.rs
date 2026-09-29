use semwright_project_graph::*;
fn main() {
    let schemas = serde_json::json!({
        "schema_version": SCHEMA_VERSION,
        "rebuild_proposal": schemars::schema_for!(RebuildProposal),
        "rebuild_request": schemars::schema_for!(RebuildRequest),
        "rebuild_reservation": schemars::schema_for!(RebuildReservation),
        "manifest": schemars::schema_for!(PortableManifest),
        "impact": schemars::schema_for!(ImpactReport),
        "query": schemars::schema_for!(AssetQuery),
        "receipt": schemars::schema_for!(ExecutionReceipt),
        "revision_candidate": schemars::schema_for!(RevisionCandidate),
        "external_intent": schemars::schema_for!(ExternalIntent),
        "asset": schemars::schema_for!(Asset),
        "edge": schemars::schema_for!(Edge),
        "knowledge": schemars::schema_for!(Knowledge),
        "revision": schemars::schema_for!(RevisionRecord)
    });
    println!(
        "{}",
        serde_json::to_string_pretty(&schemas).expect("schemas serialize")
    );
}
