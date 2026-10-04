//! owned-file canonical-library contracts. No app, daemon, Host or IPC.
#![cfg(target_os = "linux")]
use semwright_effect_conformance::{composition::*, *};
use semwright_native_sdk::{composition_report, effects_readback::*, report_validation};
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

struct Fixture {
    _temp: tempfile::TempDir,
    spec_path: PathBuf,
    root: PathBuf,
    spec: ProtectedSpec,
}
impl Fixture {
    fn new(bytes: &[u8], mime: &str, selector: Selector, predicate: Predicate) -> Self {
        let temp = tempfile::tempdir().unwrap();
        let protected = temp.path().join("protected");
        let root = temp.path().join("admitted");
        let app = temp.path().join("application");
        for p in [&protected, &root, &app] {
            fs::create_dir(p).unwrap();
        }
        fs::write(root.join("artifact"), bytes).unwrap();
        let input = SpecificationInput {
            owner: Owner {
                session: "owned-test".into(),
                principal: PrincipalBinding::Named("test-author".into()),
            },
            request_id: "request_one".into(),
            source_digest: Digest::of_bytes(b"declared-source"),
            runtime_digest: Digest::of_bytes(b"declared-runtime"),
            declared_producer_execution_status: ExecutionStatus::Unknown,
            application_roots: vec![app.to_str().unwrap().into()],
            artifacts: vec![ArtifactBinding {
                slot: "output".into(),
                path: "artifact".into(),
                sha256: Digest::of_bytes(bytes),
                bytes: bytes.len() as u64,
                mime_type: mime.into(),
            }],
            checks: vec![PropertyCheck {
                id: "property_one".into(),
                artifact_slot: "output".into(),
                selector,
                predicate,
            }],
        };
        Self {
            spec_path: protected.join("spec.json"),
            root,
            spec: prepare_spec(input).unwrap(),
            _temp: temp,
        }
    }
    fn json(value: i32, expected: i32) -> Self {
        Self::new(
            format!("{{\"value\":{value}}}").as_bytes(),
            "application/json",
            Selector::Json {
                pointer: "/value".into(),
                scalar: ScalarKind::Number {
                    units: "number".into(),
                },
            },
            Predicate::Equals {
                expected: ObservedValue::Number {
                    value: expected as f64,
                    units: "number".into(),
                },
            },
        )
    }
    fn write(&self) -> Digest {
        let bytes = serde_json::to_vec(&self.spec).unwrap();
        fs::write(&self.spec_path, &bytes).unwrap();
        fs::set_permissions(&self.spec_path, fs::Permissions::from_mode(0o600)).unwrap();
        Digest::of_bytes(&bytes)
    }
    fn run(&self) -> Result<VerifiedRun> {
        verify(&self.spec_path, &self.write(), &self.root)
    }
    fn second(&mut self, bytes: &[u8], expected: i32) {
        fs::write(self.root.join("second"), bytes).unwrap();
        self.spec.definition.artifacts.push(ArtifactBinding {
            slot: "second".into(),
            path: "second".into(),
            sha256: Digest::of_bytes(bytes),
            bytes: bytes.len() as u64,
            mime_type: "application/json".into(),
        });
        self.spec.definition.checks.push(PropertyCheck {
            id: "second_rule".into(),
            artifact_slot: "second".into(),
            selector: Selector::Json {
                pointer: "/value".into(),
                scalar: ScalarKind::Number {
                    units: "number".into(),
                },
            },
            predicate: Predicate::Equals {
                expected: ObservedValue::Number {
                    value: expected as f64,
                    units: "number".into(),
                },
            },
        });
        self.spec = prepare_spec(self.spec.definition.clone()).unwrap();
    }
}

#[test]
fn json_readback_is_canonical_and_only_proves_immutable_property() {
    let mut f = Fixture::json(7, 7);
    f.spec.definition.declared_producer_execution_status = ExecutionStatus::Failed;
    f.spec = prepare_spec(f.spec.definition.clone()).unwrap();
    let run = f.run().unwrap();
    let r = run.result();
    assert_eq!(r.verdict, Verdict::Pass);
    assert_eq!(r.evaluation.report.verdict().unwrap(), Verdict::Pass);
    assert_eq!(r.scope, SCOPE);
    assert!(!r.execution_authority);
    assert_eq!(
        r.declared_producer_execution_status,
        ExecutionStatus::Failed
    );
    assert_eq!(
        r.evaluation.report.execution_status,
        ExecutionStatus::Completed
    );
    assert_eq!(r.evaluation.report.support_level, SupportLevel::ReadOnly);
    let base = &r.evaluation.report.validation.base.0[0];
    assert_eq!(base.key.provider, "semwright.native-artifact-reader");
    assert!(base.provider_session.starts_with("readback:"));
    assert_eq!(
        base.revision,
        Revision::Fingerprint(Digest::of_bytes(br#"{"value":7}"#))
    );
    let measurements = report_validation::private_measurements(&run);
    assert_eq!(measurements.len(), 1);
    assert_eq!(
        measurements[0].observed,
        Some(ObservedValue::Number {
            value: 7.0,
            units: "number".into()
        })
    );
    assert_eq!(
        serde_json::to_value(&measurements[0].observation).unwrap(),
        serde_json::to_value(&r.evaluation.report.validation.checks[0].evidence[0]).unwrap()
    );
}
#[test]
fn wrong_value_fails_and_readback_never_copies_the_expected_value() {
    let f = Fixture::json(7, 99);
    let run = f.run().unwrap();
    assert_eq!(run.result().verdict, Verdict::Fail);
    assert_eq!(
        run.result().private_measurements[0].observed,
        Some(ObservedValue::Number {
            value: 7.0,
            units: "number".into()
        })
    );
}
#[test]
fn csv_quoted_fields_and_independent_numeric_units() {
    let f = Fixture::new(
        b"cell,value,note\r\nA1,33,\"comma, and \"\"quote\"\"\"\r\n",
        "text/csv",
        Selector::Csv {
            row: 0,
            column: "note".into(),
            scalar: ScalarKind::Text,
        },
        Predicate::Equals {
            expected: ObservedValue::Text {
                value: "comma, and \"quote\"".into(),
            },
        },
    );
    assert_eq!(f.run().unwrap().result().verdict, Verdict::Pass);
    let n = Fixture::new(
        b"cell,value\nA1,33\n",
        "text/csv",
        Selector::Csv {
            row: 0,
            column: "value".into(),
            scalar: ScalarKind::Number {
                units: "number".into(),
            },
        },
        Predicate::Within {
            expected: 33.0,
            tolerance: 0.0,
            units: "metre".into(),
        },
    );
    let run = n.run().unwrap();
    assert_eq!(run.result().verdict, Verdict::Unknown);
    assert!(run.result().private_measurements[0].observed.is_none());
}
#[test]
fn missing_property_and_type_mismatch_are_unknown_not_false() {
    for pointer in ["/missing", "/value"] {
        let f = Fixture::new(
            br#"{"value":true}"#,
            "application/json",
            Selector::Json {
                pointer: pointer.into(),
                scalar: ScalarKind::Number {
                    units: "number".into(),
                },
            },
            Predicate::Equals {
                expected: ObservedValue::Number {
                    value: 3.0,
                    units: "number".into(),
                },
            },
        );
        let run = f.run().unwrap();
        assert_eq!(run.result().verdict, Verdict::Unknown);
        assert!(run.result().private_measurements[0].observation.is_none());
    }
}
#[test]
fn stable_malformed_bytes_are_unknown_with_error_and_actual_fingerprint() {
    for (bytes, mime, selector) in [
        (
            br#"{"value":1,"value":2}"#.to_vec(),
            "application/json",
            Selector::Json {
                pointer: "/value".into(),
                scalar: ScalarKind::Text,
            },
        ),
        (
            b"a,a\nx,y\n".to_vec(),
            "text/csv",
            Selector::Csv {
                row: 0,
                column: "a".into(),
                scalar: ScalarKind::Text,
            },
        ),
        (
            b"a,b\nx\n".to_vec(),
            "text/csv",
            Selector::Csv {
                row: 0,
                column: "a".into(),
                scalar: ScalarKind::Text,
            },
        ),
        (
            b"a\n\"unfinished".to_vec(),
            "text/csv",
            Selector::Csv {
                row: 0,
                column: "a".into(),
                scalar: ScalarKind::Text,
            },
        ),
    ] {
        let f = Fixture::new(
            &bytes,
            mime,
            selector,
            Predicate::Equals {
                expected: ObservedValue::Text { value: "x".into() },
            },
        );
        let run = f.run().unwrap();
        assert_eq!(run.result().verdict, Verdict::Unknown);
        assert!(matches!(run.result().inspection_state, HelperState::Error));
        assert_eq!(
            run.result().evaluation.report.validation.base.0[0].revision,
            Revision::Fingerprint(Digest::of_bytes(&bytes))
        );
    }
}
#[test]
fn lossy_large_integer_and_nonfinite_values_never_pass() {
    for number in [
        "9007199254740993",
        "9007199254740993.0",
        "1e400",
        "-9007199254740993",
    ] {
        let bytes = format!("{{\"value\":{number}}}");
        let f = Fixture::new(
            bytes.as_bytes(),
            "application/json",
            Selector::Json {
                pointer: "/value".into(),
                scalar: ScalarKind::Number {
                    units: "number".into(),
                },
            },
            Predicate::Equals {
                expected: ObservedValue::Number {
                    value: 1.0,
                    units: "number".into(),
                },
            },
        );
        let run = f.run().unwrap();
        assert_eq!(run.result().verdict, Verdict::Unknown);
        assert!(matches!(run.result().inspection_state, HelperState::Error));
    }
}
#[test]
fn independent_fail_survives_missing_artifact_and_invalid_digest() {
    for missing in [true, false] {
        let mut f = Fixture::json(7, 99);
        f.second(br#"{"value":1}"#, 1);
        if missing {
            fs::remove_file(f.root.join("second")).unwrap();
        } else {
            fs::write(f.root.join("second"), br#"{"value":2}"#).unwrap();
        }
        let run = f.run().unwrap();
        let r = run.result();
        assert_eq!(r.verdict, Verdict::Fail);
        assert_eq!(
            r.evaluation.report.validation.checks[0].verdict,
            Verdict::Fail
        );
        assert_eq!(
            r.evaluation.report.validation.checks[1].verdict,
            Verdict::Unknown
        );
        assert_eq!(
            r.evaluation.report.validation.base.0.len(),
            if missing { 1 } else { 2 }
        );
        assert!(r.private_measurements[1].observed.is_none());
    }
}
#[test]
fn no_read_snapshot_yields_error_without_a_fabricated_report() {
    let f = Fixture::json(1, 1);
    fs::remove_file(f.root.join("artifact")).unwrap();
    assert!(
        f.run()
            .unwrap_err()
            .to_string()
            .contains("no artifact snapshot")
    );
}
#[test]
fn protected_spec_plan_runtime_source_and_digest_substitutions_stop_before_artifact_io() {
    for field in ["plan", "source", "runtime", "rule", "postbase"] {
        let mut f = Fixture::json(1, 1);
        match field {
            "plan" => f.spec.plan.digest = Digest::of_bytes(b"substitution"),
            "source" => f.spec.definition.source_digest = Digest::of_bytes(b"other"),
            "runtime" => f.spec.definition.runtime_digest = Digest::of_bytes(b"other"),
            "rule" => f.spec.contract.rules[0].artifact = Some(Digest::of_bytes(b"other")),
            _ => f.spec.plan.body.base.0[0].provider_session = "driver:source".into(),
        };
        let hash = f.write();
        let err = verify(&f.spec_path, &hash, &f.root.join("nonexistent"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("binding differs"), "{err}");
    }
    let f = Fixture::json(1, 1);
    f.write();
    assert!(verify(&f.spec_path, &Digest::of_bytes(b"wrong"), &f.root).is_err());
}
#[test]
fn symlinks_overlaps_modes_and_path_escapes_are_refused() {
    let f = Fixture::json(1, 1);
    let hash = f.write();
    fs::set_permissions(&f.spec_path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(verify(&f.spec_path, &hash, &f.root).is_err());
    fs::set_permissions(&f.spec_path, fs::Permissions::from_mode(0o600)).unwrap();
    let link = f.spec_path.with_file_name("linked");
    symlink(&f.spec_path, &link).unwrap();
    assert!(verify(&link, &hash, &f.root).is_err());
    let mut f = Fixture::json(1, 1);
    f.spec.definition.application_roots = vec![f.root.to_str().unwrap().into()];
    f.spec = prepare_spec(f.spec.definition.clone()).unwrap();
    assert!(f.run().is_err());
    let mut f = Fixture::json(1, 1);
    f.spec.definition.artifacts[0].path = "../protected/spec.json".into();
    assert!(prepare_spec(f.spec.definition).is_err());
    let mut f = Fixture::json(1, 99);
    f.second(br#"{"value":1}"#, 1);
    let outside = f.root.parent().unwrap().join("outside");
    fs::write(&outside, br#"{"value":1}"#).unwrap();
    fs::remove_file(f.root.join("second")).unwrap();
    symlink(outside, f.root.join("second")).unwrap();
    let run = f.run().unwrap();
    assert_eq!(run.result().verdict, Verdict::Fail);
    assert_eq!(run.result().evaluation.report.validation.base.0.len(), 1);
}
#[test]
fn scalar_profile_rejects_global_predicates_and_duplicate_rule_mapping() {
    let f = Fixture::json(1, 1);
    for p in [
        Predicate::Preserved,
        Predicate::Reopened,
        Predicate::Relation {
            target: "x".into(),
            binding: "y".into(),
        },
        Predicate::Membership {
            expected: Default::default(),
        },
        Predicate::Artifact {
            digest: Digest::of_bytes(b"a"),
            bytes: 1,
            media_type: "text/plain".into(),
        },
    ] {
        let mut d = f.spec.definition.clone();
        d.checks[0].predicate = p;
        assert!(prepare_spec(d).is_err());
    }
    let mut d = f.spec.definition.clone();
    d.checks.push(d.checks[0].clone());
    assert!(prepare_spec(d).is_err());
}
#[test]
fn imported_pass_needs_fresh_private_run_and_full_observation_equality() {
    let f = Fixture::json(1, 1);
    let run = f.run().unwrap();
    let bytes = serde_json::to_vec(&run.result().evaluation.report).unwrap();
    report_validation::require_exact_report(&bytes, &run).unwrap();
    let claim = composition_report::inspect_claim(&bytes).unwrap();
    assert_eq!(claim.claimed_verdict, Verdict::Pass);
    assert!(!claim.execution_authority);
    assert!(claim.requires_independent_reverification);
    let other = Fixture::json(2, 1).run().unwrap();
    assert!(report_validation::require_exact_report(&bytes, &other).is_err());
    let mut value: Value = serde_json::from_slice(&bytes).unwrap();
    value["validation"]["checks"][0]["evidence"][0]["method"] = "self-certified".into();
    assert!(
        report_validation::require_exact_report(&serde_json::to_vec(&value).unwrap(), &run)
            .is_err()
    );
}
#[test]
fn public_input_is_bounded_and_cannot_inject_context_or_observations() {
    for good in [b"".as_slice(), b"  ", b"{}", b" \n{}\n"] {
        validate_public_input(good).unwrap();
    }
    for bad in [
        b"null".as_slice(),
        b"{\"observations\":[]}",
        b"{\"execution_authority\":true}",
        b"{\"context\":{}}",
        &[b' '; 65],
    ] {
        assert!(validate_public_input(bad).is_err());
    }
}
#[test]
fn generated_schemas_preserve_canonical_types_and_closed_input() {
    let spec = serde_json::to_value(schemars::schema_for!(ProtectedSpec)).unwrap();
    assert_eq!(spec["additionalProperties"], false);
    assert!(spec["properties"]["plan"].is_object());
    let result = serde_json::to_value(schemars::schema_for!(VerificationResult)).unwrap();
    assert!(result["properties"]["evaluation"].is_object());
    assert!(!result.to_string().contains("native_execution_verified"));
}

#[test]
fn shipped_owned_fixtures_are_checked_by_the_same_file_reader() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/native/effects-fixtures");
    let json = fs::read(root.join("scalar.json")).unwrap();
    let csv = fs::read(root.join("table.csv")).unwrap();
    let j = Fixture::new(
        &json,
        "application/json",
        Selector::Json {
            pointer: "/value".into(),
            scalar: ScalarKind::Number {
                units: "number".into(),
            },
        },
        Predicate::Equals {
            expected: ObservedValue::Number {
                value: 7.0,
                units: "number".into(),
            },
        },
    );
    let c = Fixture::new(
        &csv,
        "text/csv",
        Selector::Csv {
            row: 0,
            column: "value".into(),
            scalar: ScalarKind::Number {
                units: "number".into(),
            },
        },
        Predicate::Equals {
            expected: ObservedValue::Number {
                value: 33.0,
                units: "number".into(),
            },
        },
    );
    assert_eq!(j.run().unwrap().result().verdict, Verdict::Pass);
    assert_eq!(c.run().unwrap().result().verdict, Verdict::Pass);
}

#[test]
fn cli_runs_only_protected_readback_and_refuses_observation_injection() {
    use std::io::Write;
    use std::process::{Command, Stdio};
    let f = Fixture::json(7, 7);
    let hash = f.write();
    let binary = env!("CARGO_BIN_EXE_semwright-native-effects");
    let args = [
        "--spec",
        f.spec_path.to_str().unwrap(),
        "--spec-sha256",
        hash.as_str(),
        "--artifact-root",
        f.root.to_str().unwrap(),
    ];
    let result = Command::new(binary).args(args).output().unwrap();
    assert!(result.status.success());
    let value: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(value["verdict"], "PASS");
    assert_eq!(value["scope"], SCOPE);
    assert_eq!(value["execution_authority"], false);
    let mut child = Command::new(binary)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(br#"{"observations":[]}"#)
        .unwrap();
    let rejected = child.wait_with_output().unwrap();
    assert_eq!(rejected.status.code(), Some(2));
    assert!(rejected.stdout.is_empty());
    let error: Value = serde_json::from_slice(&rejected.stderr).unwrap();
    assert_eq!(error["state"], "ERROR");
    assert!(error["canonical_report"].is_null());
    for which in ["spec", "result"] {
        let out = Command::new(binary)
            .args(["--schema", which])
            .output()
            .unwrap();
        assert!(out.status.success());
        let parsed: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(parsed["additionalProperties"], false);
    }
}

#[test]
fn quoted_bare_cr_in_unselected_field_blocks_pass_but_crlf_is_valid() {
    for (bytes, expected) in [
        (b"value,note\n7,\"a\rb\"\n".as_slice(), Verdict::Unknown),
        (b"value,note\n7,\"a\r\nb\"\n".as_slice(), Verdict::Pass),
    ] {
        let f = Fixture::new(
            bytes,
            "text/csv",
            Selector::Csv {
                row: 0,
                column: "value".into(),
                scalar: ScalarKind::Number {
                    units: "number".into(),
                },
            },
            Predicate::Equals {
                expected: ObservedValue::Number {
                    value: 7.0,
                    units: "number".into(),
                },
            },
        );
        let run = f.run().unwrap();
        assert_eq!(run.result().verdict, expected);
        if expected == Verdict::Unknown {
            assert!(matches!(run.result().inspection_state, HelperState::Error));
        }
    }
}
