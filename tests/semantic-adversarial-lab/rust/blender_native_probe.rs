//! Independent G Blender native adversarial probe.
//! Runs only on disposable hosted runners; product Blender processes execute through Driver Host.
use semwright_backend_api::Provider;
use semwright_core::{Broker, NoApprover, audit::Audit};
use semwright_driver_blender::authoring::NativeSnapshot;
use semwright_driver_host::DriverProvider;
use semwright_driver_sdk::{
    ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, DriverToolMount, Manifest,
    Transport,
};
use semwright_policy::{FilesystemGrant, Policy, PolicyConfig, Profile};
use semwright_semantic_composition::{Verdict, VerificationReport};
use semwright_types::{Envelope, ExecuteRequest, unique_id};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};
use std::{
    collections::{BTreeSet, HashSet},
    fs, io,
    os::unix::fs::{PermissionsExt, symlink},
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_util::sync::CancellationToken;

const SOURCE: &str = env!("G_LAB_COMPILED_SOURCE_SHA");
type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

fn file_sha(path: &Path) -> AnyResult<String> {
    Ok(format!("{:x}", Sha256::digest(fs::read(path)?)))
}

fn boxed(message: impl Into<String>) -> Box<dyn std::error::Error> {
    Box::new(io::Error::other(message.into()))
}

struct NativeFixture {
    broker: Arc<Broker>,
    provider: Arc<DriverProvider>,
    _state: tempfile::TempDir,
    session: String,
}

impl NativeFixture {
    async fn start(
        workspace: &Path,
        blender_root: &Path,
        driver_binary: &Path,
        sandbox_helper: &Path,
        allow: bool,
    ) -> AnyResult<Self> {
        let state = tempfile::tempdir()?;
        fs::set_permissions(state.path(), fs::Permissions::from_mode(0o700))?;
        let package = state.path().join("package");
        fs::create_dir(&package)?;
        let executable = package.join("driver");
        fs::copy(driver_binary, &executable)?;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700))?;

        let runner = package.join("blender-session-runner");
        fs::copy(driver_binary.with_file_name("semwright-blender-session-runner"), &runner)?;
        fs::set_permissions(&runner, fs::Permissions::from_mode(0o700))?;
        let scratch = state.path().join("scratch");
        fs::create_dir(&scratch)?;
        fs::set_permissions(&scratch, fs::Permissions::from_mode(0o700))?;
        let runtime_sha = file_sha(&executable)?;
        let blender_root = fs::canonicalize(blender_root)?;
        let blender = blender_root.join("blender");
        if !blender.is_file() {
            return Err(boxed("pinned Blender binary missing"));
        }
        let blender_sha = file_sha(&blender)?;
        let manifest = Manifest {
            manifest_version: 1,
            protocol: 8,
            transport: Transport::StdioV1,
            id: "blender".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            publisher: "semwright-g-adversarial".into(),
            sha256: runtime_sha,
            executable,
            application: ApplicationMatch {
                desktop_id: Some("org.blender.Blender".into()),
                process_names: vec!["blender".into()],
                supported_versions: vec!["4.5.14".into()],
            },
            mounts: vec![
                DriverMount { root: "font-config".into(), read_only: true, execute: false },
                DriverMount { root: "scratch".into(), read_only: false, execute: false },
                DriverMount {
                    root: "workspace".into(),
                    read_only: false,
                    execute: false,
                },
                DriverMount {
                    root: "blender-runtime".into(),
                    read_only: true,
                    execute: false,
                },
            ],
            system_config: vec![],
            tools: vec![DriverToolMount {
                name: "blender-session-runner".into(),
                root: "blender-session-runner-executable".into(),
                sha256: file_sha(&runner)?,
                mounts: vec!["workspace".into(), "blender-runtime".into(), "scratch".into(), "font-config".into()],
                system_config: vec![],
                dependencies: vec!["blender".into()],
            }, DriverToolMount {
                name: "blender".into(),
                root: "blender-executable".into(),
                sha256: blender_sha,
                mounts: vec![], system_config: vec![], dependencies: vec![],
            }],
            secrets: vec![],
            network: false,
            loopback_port: None,
            resources: DriverResources {
                open_files: 256,
                processes: 64,
                cpu_seconds: 300,
                operation_cpu_seconds: 0,
                address_space_bytes: 4_294_967_296,
                file_size_bytes: 1_073_741_824,
            },
            request_timeout_ms: 300_000,
            interfaces: DriverInterfaces {
                cooperative_cancellation: true,
                progress: true,
                health: true,
                host_tools: true,
                ..Default::default()
            },
        };
        let grants = vec![
            FilesystemGrant { name: "scratch".into(), path: fs::canonicalize(&scratch)?, read: true, write: true },
            FilesystemGrant { name: "blender-session-runner-executable".into(), path: fs::canonicalize(&runner)?, read: true, write: false },
            FilesystemGrant {
                name: "workspace".into(),
                path: fs::canonicalize(workspace)?,
                read: true,
                write: true,
            },
            FilesystemGrant {
                name: "blender-runtime".into(),
                path: blender_root,
                read: true,
                write: false,
            },
            FilesystemGrant {
                name: "font-config".into(),
                path: fs::canonicalize("/etc/fonts")?,
                read: true,
                write: false,
            },
            FilesystemGrant {
                name: "blender-executable".into(),
                path: fs::canonicalize(&blender)?,
                read: true,
                write: false,
            },
        ];
        let provider =
            DriverProvider::connect(manifest, state.path(), sandbox_helper, &grants, false).await?;
        let broker = Broker::new(
            Policy::new(PolicyConfig {
                profile: Profile::Workspace,
                allow: if allow {
                    ["driver:blender".into()].into()
                } else {
                    BTreeSet::new()
                },
                ..Default::default()
            })?,
            vec![],
            Audit::open(&state.path().join("audit"), 65_536, 2)?,
            Arc::new(NoApprover),
            None,
            json!({"g_adversarial_blender":true}),
            false,
        )?;
        broker.mount_provider(provider.clone()).await?;
        Ok(Self {
            broker,
            provider,
            _state: state,
            session: unique_id(),
        })
    }

    async fn raw_with_token(
        &self,
        session: &str,
        request_id: &str,
        command: &str,
        args: Value,
        cancellation: CancellationToken,
    ) -> Envelope {
        self.broker
            .clone()
            .execute(
                session.into(),
                request_id.into(),
                ExecuteRequest {
                    command: format!("driver.blender.{command}"),
                    args,
                    dry_run: false,
                    backend: None,
                },
                cancellation,
            )
            .await
    }

    async fn raw(&self, session: &str, command: &str, args: Value) -> Envelope {
        self.raw_with_token(
            session,
            &unique_id(),
            command,
            args,
            CancellationToken::new(),
        )
        .await
    }

    async fn call_with_request(
        &self,
        request_id: &str,
        command: &str,
        args: Value,
    ) -> AnyResult<Value> {
        let out = self
            .raw_with_token(
                &self.session,
                request_id,
                command,
                args,
                CancellationToken::new(),
            )
            .await;
        if !out.ok {
            return Err(boxed(format!("product baseline call failed: {out:?}")));
        }
        out.data.ok_or_else(|| boxed("product result data missing"))
    }

    async fn call(&self, command: &str, args: Value) -> AnyResult<Value> {
        let out = self.raw(&self.session, command, args).await;
        if !out.ok {
            return Err(boxed(format!("product baseline call failed: {out:?}")));
        }
        out.data.ok_or_else(|| boxed("product result data missing"))
    }

    async fn shutdown(&self) -> AnyResult<()> {
        self.provider.shutdown().await?;
        Ok(())
    }
}

struct PageEvidence {
    members: Vec<String>,
    first_cursor: Option<String>,
    exhaustive: bool,
    stable_total: bool,
    final_page: bool,
}

async fn enumerate_domain(
    fixture: &NativeFixture,
    island: &str,
    domain: &str,
) -> AnyResult<PageEvidence> {
    let mut cursor: Option<String> = None;
    let mut first_cursor = None;
    let mut members = Vec::new();
    let mut expected_total = None;
    let mut exhaustive = true;
    let mut stable_total = true;
    let mut final_page = false;
    loop {
        let mut args = json!({"island":island,"domain":domain,"limit":1});
        if let Some(token) = &cursor {
            args["cursor"] = token.clone().into();
        }
        let page = fixture.call("composition.inspect.page", args).await?;
        exhaustive &= page["exhaustive"] == true;
        let total = page["total"]
            .as_u64()
            .ok_or_else(|| boxed("page total missing"))?;
        if let Some(previous) = expected_total {
            stable_total &= previous == total;
        } else {
            expected_total = Some(total);
        }
        let items = page["items"]
            .as_array()
            .ok_or_else(|| boxed("page items missing"))?;
        for item in items {
            members.push(
                item["member"]
                    .as_str()
                    .ok_or_else(|| boxed("page member missing"))?
                    .to_owned(),
            );
        }
        cursor = page["next_cursor"].as_str().map(str::to_owned);
        if first_cursor.is_none() {
            first_cursor = cursor.clone();
        }
        if cursor.is_none() {
            final_page = page["final_page"] == true;
            break;
        }
    }
    if expected_total != Some(members.len() as u64) {
        stable_total = false;
    }
    Ok(PageEvidence {
        members,
        first_cursor,
        exhaustive,
        stable_total,
        final_page,
    })
}

fn row(case_id: &str, observed: Value) -> Value {
    json!({"case_id":case_id,"observed":observed})
}

fn blocked_row(case_id: &str, reason: &str) -> Value {
    json!({"case_id":case_id,"blocked_reason":reason})
}

async fn run() -> AnyResult<Value> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 5 {
        return Err(boxed(
            "expected driver, sandbox, blender-root, workspace, spec arguments",
        ));
    }
    let driver = fs::canonicalize(&args[0])?;
    let sandbox = fs::canonicalize(&args[1])?;
    let blender_root = fs::canonicalize(&args[2])?;
    let workspace = fs::canonicalize(&args[3])?;
    let spec: Value = serde_json::from_slice(&fs::read(&args[4])?)?;

    let sentinel = workspace.join("g-external-sentinel.txt");
    let sentinel_bytes = b"G synthetic sentinel must stay unchanged";
    fs::write(&sentinel, sentinel_bytes)?;

    let fixture = NativeFixture::start(&workspace, &blender_root, &driver, &sandbox, true).await?;
    let mut cases = Vec::new();

    let before = fixture.call("composition.inspect", json!({})).await?;
    let plan = fixture
        .call(
            "composition.plan",
            json!({"intent":{"kind":"create","spec":spec}}),
        )
        .await?;
    let after_plan = fixture.call("composition.inspect", json!({})).await?;
    cases.push(row(
        "G-BLENDER-001",
        json!({
            "plan_ref_present":plan["plan_ref"].as_str().is_some(),
            "plan_nonmutating":before==after_plan
        }),
    ));

    let denied = fixture
        .raw(
            "g-foreign-authenticated-session",
            "composition.apply",
            json!({"plan_ref":plan["plan_ref"]}),
        )
        .await;
    let after_denied = fixture.call("composition.inspect", json!({})).await?;
    cases.push(row(
        "G-BLENDER-002",
        json!({"foreign_owner_denied":!denied.ok,"state_unchanged":before==after_denied}),
    ));

    let token = CancellationToken::new();
    token.cancel();
    let cancelled = fixture
        .raw_with_token(
            &fixture.session,
            "g-blender-pre-cancelled",
            "composition.apply",
            json!({"plan_ref":plan["plan_ref"]}),
            token,
        )
        .await;
    let after_cancel = fixture.call("composition.inspect", json!({})).await?;
    cases.push(row(
        "G-BLENDER-003",
        json!({"pre_cancelled_denied":!cancelled.ok,"state_unchanged":before==after_cancel}),
    ));

    let applied = fixture
        .call_with_request(
            "g-blender-apply",
            "composition.apply",
            json!({"plan_ref":plan["plan_ref"]}),
        )
        .await?;
    let report: VerificationReport = serde_json::from_value(applied["report"].clone())?;
    let items = applied["snapshot"]["items"]
        .as_array()
        .ok_or_else(|| boxed("applied snapshot items missing"))?;
    let product = items
        .iter()
        .find(|item| item["entity"] == "product")
        .ok_or_else(|| boxed("product entity missing"))?;
    let instance = items
        .iter()
        .find(|item| item["entity"] == "instance")
        .ok_or_else(|| boxed("instance entity missing"))?;
    let island = applied["island"]
        .as_str()
        .ok_or_else(|| boxed("island missing"))?
        .to_owned();
    let product_name = product["name"]
        .as_str()
        .ok_or_else(|| boxed("product native name missing"))?
        .to_owned();
    cases.push(row(
        "G-BLENDER-004",
        json!({
            "total":applied["snapshot"]["total"],
            "verification_pass":report.verdict()?==Verdict::Pass,
            "shared_data_name":product["data_name"]==instance["data_name"],
            "shared_data_users":product["data_users"]
        }),
    ));

    let replay = fixture
        .raw(
            &fixture.session,
            "composition.apply",
            json!({"plan_ref":plan["plan_ref"]}),
        )
        .await;
    cases.push(row(
        "G-BLENDER-005",
        json!({"consumed_plan_rejected":!replay.ok}),
    ));

    let domains = [
        "objects",
        "bones",
        "actions",
        "curves",
        "keyframes",
        "properties",
    ];
    let mut all_nonempty = true;
    let mut all_exhaustive = true;
    let mut all_stable = true;
    let mut no_duplicates = true;
    let mut all_final = true;
    let mut objects_first_cursor = None;
    for domain in domains {
        let page = enumerate_domain(&fixture, &island, domain).await?;
        all_nonempty &= !page.members.is_empty();
        all_exhaustive &= page.exhaustive;
        all_stable &= page.stable_total;
        all_final &= page.final_page;
        let unique: HashSet<_> = page.members.iter().collect();
        no_duplicates &= unique.len() == page.members.len();
        if domain == "objects" {
            objects_first_cursor = page.first_cursor;
        }
    }
    cases.push(row(
        "G-BLENDER-006",
        json!({
            "domains":6,
            "all_nonempty":all_nonempty,
            "all_exhaustive":all_exhaustive,
            "stable_totals":all_stable,
            "no_duplicates":no_duplicates,
            "all_final_pages":all_final
        }),
    ));

    let consumed_cursor_rejected = if let Some(cursor) = objects_first_cursor {
        !fixture
            .raw(
                &fixture.session,
                "composition.inspect.page",
                json!({"island":island,"domain":"objects","limit":1,"cursor":cursor}),
            )
            .await
            .ok
    } else {
        false
    };
    cases.push(row(
        "G-BLENDER-007",
        json!({"consumed_cursor_rejected":consumed_cursor_rejected}),
    ));

    let pre_persist_a = fixture
        .call("composition.inspect", json!({"island":island}))
        .await?;
    let pre_persist_b = fixture
        .call("composition.inspect", json!({"island":island}))
        .await?;
    let persist = fixture
        .raw(
            &fixture.session,
            "composition.persist",
            json!({"island":island,"path":"g-native.blend"}),
        )
        .await;
    let blend_path = workspace.join("g-native.blend");
    let pre_persist_stable = pre_persist_a["drift"] == false
        && pre_persist_b["drift"] == false
        && pre_persist_a["fingerprint"] == pre_persist_b["fingerprint"];
    let mut persist_diagnostic = json!({
        "pre_persist_stable":pre_persist_stable,
        "first_drift":pre_persist_a["drift"],
        "second_drift":pre_persist_b["drift"],
        "same_fingerprint":pre_persist_a["fingerprint"]==pre_persist_b["fingerprint"],
        "persist_ok":persist.ok,
        "persist_error_code":persist.error.as_ref().map(|error| format!("{:?}", error.code)),
    });

    let mut fresh: Option<NativeFixture> = None;
    if persist.ok {
        let saved = persist
            .data
            .clone()
            .ok_or_else(|| boxed("persist result data missing"))?;
        let duplicate = fixture
            .raw(
                &fixture.session,
                "composition.persist",
                json!({"island":island,"path":"g-native.blend"}),
            )
            .await;
        cases.push(row(
            "G-BLENDER-012",
            json!({
                "persisted":blend_path.is_file(),
                "sha_matches":saved["sha256"]==file_sha(&blend_path)?,
                "duplicate_rejected":!duplicate.ok
            }),
        ));

        let writer_session = applied["snapshot"]["native_session"].clone();
        let saved_fingerprint = saved["source_fingerprint"].clone();
        let reopened_fixture =
            NativeFixture::start(&workspace, &blender_root, &driver, &sandbox, true).await?;
        let reopened = reopened_fixture
            .call(
                "composition.reopen",
                json!({"island":island,"path":"g-native.blend","sha256":saved["sha256"]}),
            )
            .await?;
        let native: NativeSnapshot = serde_json::from_value(reopened.clone())?;
        cases.push(row(
            "G-BLENDER-013",
            json!({
                "fresh_native_session":writer_session!=reopened["native_session"],
                "fingerprint_matches":native.fingerprint.as_str()==saved_fingerprint.as_str().unwrap_or(""),
                "drift_false":!native.drift,
                "sentinel_preserved":fs::read(&sentinel)?==sentinel_bytes
            }),
        ));
        persist_diagnostic["reopen_ok"] = true.into();
        fresh = Some(reopened_fixture);
    } else {
        cases.push(row(
            "G-BLENDER-012",
            json!({
                "persisted":false,
                "sha_matches":false,
                "duplicate_rejected":false
            }),
        ));
        cases.push(blocked_row(
            "G-BLENDER-013",
            "clean persistence baseline failed; fresh-process reopen cannot be isolated",
        ));
        persist_diagnostic["reopen_ok"] = false.into();
    }

    let active = fresh.as_ref().unwrap_or(&fixture);
    let active_snapshot = active
        .call("composition.inspect", json!({"island":island}))
        .await?;
    let reopened_collection = active_snapshot["items"][0]["collections"][0]
        .as_str()
        .ok_or_else(|| boxed("active managed collection missing"))?
        .to_owned();
    // Hostile output rejection is isolated after the clean persistence/reopen proof so a
    // rejected export can never invalidate the persistence baseline for another case.
    let outside = workspace
        .parent()
        .ok_or_else(|| boxed("workspace parent missing"))?
        .join("g-blender-outside.glb");
    let symlink_path = workspace.join("g-symbol.glb");
    let _ = fs::remove_file(&outside);
    let _ = fs::remove_file(&symlink_path);
    symlink(&outside, &symlink_path)?;
    let symlink_export = active
        .raw(
            &active.session,
            "export.glb",
            json!({"collection":reopened_collection,"path":"g-symbol.glb","animations":true}),
        )
        .await;
    let after_symlink = active
        .call("composition.inspect", json!({"island":island}))
        .await?;
    cases.push(row(
        "G-BLENDER-011",
        json!({
            "symlink_output_rejected":!symlink_export.ok,
            "outside_absent":!outside.exists(),
            "source_drift_false":after_symlink["drift"]==false
        }),
    ));
    let _ = fs::remove_file(&symlink_path);

    // Cursor staleness is intentionally destructive to the native session's revision history.
    // Exercise it only after persistence/reopen has been independently established.
    let first_properties = active
        .call(
            "composition.inspect.page",
            json!({"island":island,"domain":"properties","limit":1}),
        )
        .await?;
    let stale_cursor = first_properties["next_cursor"]
        .as_str()
        .ok_or_else(|| boxed("properties cursor missing"))?
        .to_owned();
    active
        .call(
            "object.transform",
            json!({"name":product_name,"location":[0.01,0.0,0.4]}),
        )
        .await?;
    let stale = active
        .raw(
            &active.session,
            "composition.inspect.page",
            json!({"island":island,"domain":"properties","limit":1,"cursor":stale_cursor}),
        )
        .await;
    active
        .call(
            "object.transform",
            json!({"name":product_name,"location":[0.0,0.0,0.4]}),
        )
        .await?;
    let restored = active
        .call("composition.inspect", json!({"island":island}))
        .await?;
    cases.push(row(
        "G-BLENDER-008",
        json!({"stale_cursor_rejected":!stale.ok,"restored_drift_false":restored["drift"]==false}),
    ));

    let shared_snapshot = active
        .call("composition.inspect", json!({"island":island}))
        .await?;
    let denied_shared = active
        .raw(
            &active.session,
            "composition.plan",
            json!({"intent":{
                "kind":"material_slots",
                "island":island,
                "entity":"instance",
                "materials":["surface"],
                "expected_fingerprint":shared_snapshot["fingerprint"]
            }}),
        )
        .await;
    cases.push(row(
        "G-BLENDER-009",
        json!({"shared_mesh_material_plan_rejected":!denied_shared.ok}),
    ));

    // Run the successful GLB export last on the clean managed island so any source-state
    // drift caused by export cannot contaminate persistence, hostile-output or cursor cases.
    let pre_export = active
        .call("composition.inspect", json!({"island":island}))
        .await?;
    let export_collection = pre_export["items"][0]["collections"][0]
        .as_str()
        .ok_or_else(|| boxed("pre-export managed collection missing"))?
        .to_owned();
    let export = active
        .call(
            "export.glb",
            json!({"collection":export_collection,"path":"g-native.glb","animations":true}),
        )
        .await?;
    let glb_path = workspace.join("g-native.glb");
    let glb = fs::read(&glb_path)?;
    let after_export = active
        .call("composition.inspect", json!({"island":island}))
        .await?;
    cases.push(row(
        "G-BLENDER-010",
        json!({
            "pre_export_drift_false":pre_export["drift"]==false,
            "glb_magic":glb.starts_with(b"glTF"),
            "sha_matches":export["sha256"]==file_sha(&glb_path)?,
            "source_drift_false":after_export["drift"]==false,
            "sentinel_preserved":fs::read(&sentinel)?==sentinel_bytes
        }),
    ));

    if let Some(reopened_fixture) = fresh.as_ref() {
        reopened_fixture.shutdown().await?;
    }
    fixture.shutdown().await?;

    let denied_fixture =
        NativeFixture::start(&workspace, &blender_root, &driver, &sandbox, false).await?;
    let policy_denied = denied_fixture
        .raw(&denied_fixture.session, "composition.inspect", json!({}))
        .await;
    cases.push(row(
        "G-BLENDER-014",
        json!({"policy_denied":!policy_denied.ok}),
    ));
    denied_fixture.shutdown().await?;

    // Execution ordering is intentionally chosen to isolate destructive hostile cases from
    // persistence. Evidence ordering remains canonical and registry-bound.
    cases.sort_by(|left, right| {
        left["case_id"]
            .as_str()
            .unwrap_or("")
            .cmp(right["case_id"].as_str().unwrap_or(""))
    });

    Ok(json!({
        "schema_version":1,
        "source_sha":SOURCE,
        "route":"Broker -> policy -> Driver Host -> Blender",
        "sandbox_network_disabled":true,
        "sentinel_unchanged":fs::read(&sentinel)?==sentinel_bytes,
        "diagnostics":{"persistence":persist_diagnostic},
        "cases":cases
    }))
}

#[tokio::main]
async fn main() {
    match run().await {
        Ok(value) => println!(
            "{}",
            serde_json::to_string(&value).expect("serialize G receipt")
        ),
        Err(error) => {
            eprintln!("g blender-native helper: {error}");
            std::process::exit(1);
        }
    }
}
