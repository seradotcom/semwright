//! Development microbenchmarks. No measurements are asserted or supplied in this archive.
use semwright_backend_api::{Backend, Context};
use semwright_backends::fake::FakeDesktop;
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_policy::{Policy, PolicyConfig, Profile};
use semwright_protocol::{ClientMessage, decode, encode};
use semwright_registry::Registry;
use semwright_types::{ExecuteRequest, Selector, UiNode, unique_id};
use serde_json::json;
use std::{hint::black_box, sync::Arc, time::Instant};
use tokio_util::sync::CancellationToken;
fn report(name: &str, count: usize, start: Instant) {
    let elapsed = start.elapsed().as_nanos();
    println!(
        "{}",
        json!({"benchmark":name,"iterations":count,"total_ns":elapsed,"mean_ns":elapsed/(count as u128),"method":"single-process arithmetic mean; not a statistical performance claim"})
    );
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let n = 1000usize;
    let nodes: Vec<UiNode> = (0..500)
        .map(|i| UiNode {
            reference: format!("ui:{i:032x}"),
            role: "button".into(),
            name: format!("Export {i}"),
            description: String::new(),
            help: String::new(),
            accessibility_id: String::new(),
            framework: String::new(),
            attributes: Default::default(),
            relations: vec![],
            facets: Default::default(),
            states: vec!["enabled".into()],
            actions: vec!["click".into()],
            app: "fixture".into(),
            parent_ref: None,
            bounds: None,
            children_count: 0,
        })
        .collect();
    let exact: Selector =
        serde_json::from_value(json!({"name":{"op":"exact","value":"Export 249"}}))?;
    let ranked: Selector = serde_json::from_value(json!({"query":"export 249"}))?;
    for (name, selector) in [
        ("selector_exact_500", exact),
        ("selector_ranked_500", ranked),
    ] {
        let start = Instant::now();
        for _ in 0..n {
            black_box(selector.select(black_box(&nodes))?);
        }
        report(name, n, start);
    }
    let registry = Registry::builtin()?;
    let policy = Policy::new(PolicyConfig {
        profile: Profile::Desktop,
        ..Default::default()
    })?;
    let descriptor = registry.describe("ui.invoke")?;
    let args = json!({"ref":"ui:00000000000000000000000000000001"});
    let start = Instant::now();
    for _ in 0..n {
        black_box(policy.check(descriptor, black_box(&args), None));
    }
    report("policy", n, start);
    let hello = ClientMessage::Hello {
        version: 1,
        session: None,
    };
    let start = Instant::now();
    for _ in 0..n {
        let bytes = encode(black_box(&hello))?;
        black_box(decode::<ClientMessage>(&bytes)?);
    }
    report("protocol_roundtrip", n, start);
    let start = Instant::now();
    for _ in 0..n {
        black_box(registry.search(black_box("ui"), 20));
    }
    report("registry_search", n, start);
    let fake = Arc::new(FakeDesktop::new());
    let context = Context {
        session: unique_id(),
        request_id: semwright_types::unique_id(),
        cancellation: CancellationToken::new(),
    };
    let start = Instant::now();
    for _ in 0..n {
        black_box(
            fake.execute(
                &context,
                "ui.snapshot",
                &json!({"actionable":true,"max_nodes":20}),
            )
            .await?,
        );
    }
    report("fake_actionable_snapshot", n, start);
    let dir = tempfile::tempdir()?;
    let audit = Audit::open(&dir.path().join("audit"), 65536, 2)?;
    let broker = Broker::new(
        policy,
        vec![fake],
        audit,
        Arc::new(NoApprover),
        None,
        json!({}),
        true,
    )?;
    let session = unique_id();
    let start = Instant::now();
    let actions = 100;
    for _ in 0..actions {
        let found = broker
            .clone()
            .execute(
                session.clone(),
                unique_id(),
                ExecuteRequest {
                    command: "ui.find".into(),
                    args: json!({"selector":{"name":{"op":"exact","value":"Export"}}}),
                    dry_run: false,
                    backend: None,
                },
                CancellationToken::new(),
            )
            .await;
        if !found.ok {
            return Err("fake discovery failed".into());
        }
        let value = found.data.ok_or("missing discovery data")?;
        let action = broker
            .clone()
            .execute(
                session.clone(),
                unique_id(),
                ExecuteRequest {
                    command: "ui.invoke".into(),
                    args: json!({"ref":value["nodes"][0]["ref"],"action":"click"}),
                    dry_run: false,
                    backend: None,
                },
                CancellationToken::new(),
            )
            .await;
        if !action.ok {
            return Err("fake action failed".into());
        }
        black_box(action);
    }
    report("fake_broker_find_invoke_with_audit", actions, start);
    broker.shutdown().await;
    Ok(())
}
