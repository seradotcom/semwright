mod common;
use common::*;
use semwright_project_graph::*;
use std::sync::atomic::AtomicBool;
#[test]
fn ten_thousand_resources_fifty_thousand_edges_under_declared_budgets() {
    let started = std::time::Instant::now();
    let (mut graph, access) = setup();
    let mut ids = Vec::new();
    for n in 0..10_000 {
        ids.push(asset(&mut graph, &access, &format!("resource-{n}")));
    }
    for id in &ids {
        for target in &ids[..5] {
            graph
                .declare(&access, declared(id, target, Relation::References))
                .unwrap();
        }
    }
    let built_ms = started.elapsed().as_millis();
    let query_started = std::time::Instant::now();
    let report = graph
        .impact(&access, &ids[0], budget(), &AtomicBool::new(false))
        .unwrap();
    assert!(!report.truncated);
    assert!(report.known.is_empty());
    assert_eq!(report.possible.len(), 9999);
    assert!(report.unknown_frontier);
    assert!(report.visited_nodes <= 20_000);
    assert!(report.visited_edges <= 100_000);
    assert!(started.elapsed().as_secs() < 45);
    let peak_kib = std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("VmHWM:"))
                .and_then(|l| l.split_whitespace().nth(1))
                .and_then(|v| v.parse::<u64>().ok())
        });
    if let Some(peak) = peak_kib {
        assert!(peak < 512 * 1024, "declared process memory budget");
    }
    let metrics = serde_json::json!({"schema_version":1,"resources":10_000,"edges":50_000,"build_ms":built_ms,"query_ms":query_started.elapsed().as_millis(),"process_peak_kib":peak_kib,"memory_method":"Linux /proc/self/status VmHWM; null when unavailable","max_process_kib":512*1024,"max_total_seconds":45,"visited_nodes":report.visited_nodes,"visited_edges":report.visited_edges,"known":0,"possible":report.possible.len(),"unknown_frontier":true,"native":false,"fixture":"declared graph, not native provenance or productivity evidence"});
    if std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true") {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../verification/project-graph");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("scale.json"),
            serde_json::to_vec_pretty(&metrics).unwrap(),
        )
        .unwrap();
    }
}
