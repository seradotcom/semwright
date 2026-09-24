use clap::{CommandFactory, Parser};
use semwright_cli::*;
use semwright_driver_host::conformance as driver_conformance;
use semwright_driver_registry::{
    Index as DriverIndex, InstallRoots, create_package as create_driver_package,
    inspect_package as inspect_driver_package, install_from_index as install_driver_from_index,
    remove_installed as remove_installed_driver,
};
use semwright_driver_sdk::Manifest as DriverManifest;
use semwright_federation::{
    StdioUpstreamConfig, default_upstream_registry_path, doctor_stdio, load_upstream_registry,
    new_upstream, save_upstream_registry,
};
use semwright_protocol::{self as ipc, ClientMessage, ServerMessage};
use semwright_types::provider::canonical_slug;
use semwright_types::*;
use serde_json::json;
use std::{path::PathBuf, time::Duration};

fn load_driver_manifest(path: &std::path::Path) -> Result<DriverManifest> {
    let manifest: DriverManifest = serde_json::from_str(&read_file(path, 1_048_576)?)?;
    manifest.validate()?;
    Ok(manifest)
}

fn driver_view(manifest: &DriverManifest) -> Result<serde_json::Value> {
    let identity = manifest.identity()?;
    Ok(json!({
        "identity": identity,
        "manifest_version": manifest.manifest_version,
        "protocol": manifest.protocol,
        "publisher": manifest.publisher,
        "application": manifest.application,
        "transport": manifest.transport,
        "executable": manifest.executable,
        "sha256": manifest.sha256,
        "network": manifest.network,
        "mounts": manifest.mounts,
        "system_config": manifest.system_config,
        "resources": manifest.resources,
        "interfaces": manifest.interfaces,
        "request_timeout_ms": manifest.request_timeout_ms,
        "executable_exists": manifest.executable.is_file(),
        "policy_grants_changed": false
    }))
}

async fn manage_driver(cli: &Cli, command: &DriverCommand) -> Result<()> {
    match command {
        DriverCommand::Validate { manifest } => {
            let manifest = load_driver_manifest(manifest)?;
            print_result(
                &json!({"valid":true,"driver":driver_view(&manifest)?,"executed":false}),
                cli.json,
            )
        }
        DriverCommand::Inspect { manifest } => {
            let manifest = load_driver_manifest(manifest)?;
            print_result(&driver_view(&manifest)?, cli.json)
        }
        DriverCommand::Conformance { manifest } => {
            let manifest = load_driver_manifest(manifest)?;
            let helper = std::env::current_exe()?
                .parent()
                .ok_or_else(|| Error::unavailable("Cannot locate sandbox helper directory"))?
                .join("semwright-sandbox");
            let runtime = semwright_protocol::runtime_directory()?;
            let state = runtime.join(format!("driver-conformance-{}", unique_id()));
            semwright_protocol::private_directory(&state)?;
            let result = driver_conformance(manifest, &state, &helper, &[], false).await;
            let _ = std::fs::remove_dir_all(&state);
            print_result(&serde_json::to_value(result?)?, cli.json)
        }
        DriverCommand::Scaffold {
            name,
            output,
            sdk_path,
        } => {
            if !canonical_slug(name) {
                return Err(Error::invalid(
                    "Driver name must be a canonical lowercase slug",
                ));
            }
            let sdk = sdk_path.canonicalize()?;
            if !sdk.join("Cargo.toml").is_file() {
                return Err(Error::invalid(
                    "--sdk-path must be the semwright-driver-sdk crate directory",
                ));
            }
            let crates = sdk
                .parent()
                .ok_or_else(|| Error::invalid("SDK path has no crates directory"))?;
            std::fs::create_dir(output)?;
            std::fs::create_dir(output.join("src"))?;
            let package = format!("semwright-{}-driver", name);
            let cargo = format!(
                "[package]\nname = {}\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nsemwright-driver-sdk = {{ path = {} }}\nsemwright-types = {{ path = {} }}\nasync-trait = \"0.1\"\nserde_json = \"1\"\ntokio = {{ version = \"1\", features = [\"macros\",\"rt-multi-thread\"] }}\n",
                serde_json::to_string(&package)?,
                serde_json::to_string(&sdk)?,
                serde_json::to_string(&crates.join("types"))?
            );
            create(&output.join("Cargo.toml"), &cargo)?;
            let main = format!(
                r#"use async_trait::async_trait;
use semwright_driver_sdk::{{Capability, Driver, descriptor_digest, serve}};
use semwright_types::{{CommandDescriptor, Error, ErrorCode, Idempotency, Result, Risk}};
use serde_json::{{Value, json}};

fn capability() -> Capability {{
    Capability {{
        descriptor: CommandDescriptor {{
            name: "driver.{name}.ping".into(),
            version: "1".into(),
            description: "Example read-only driver operation".into(),
            input_schema: json!({{"type":"object","additionalProperties":false}}),
            output_schema: json!({{"type":"object","properties":{{"ok":{{"const":true}}}},"required":["ok"],"additionalProperties":false}}),
            requires: vec!["driver:{name}".into()],
            risk: Risk::ReadOnly,
            idempotency: Idempotency::ReadOnly,
            timeout_ms: 2000,
            dry_run: true,
            interactive_consent: false,
            backends: vec!["driver:{name}".into()],
        }},
        aliases: vec!["ping".into()],
        tags: vec!["example".into()],
        object_types: vec![],
    }}
}}
struct Example;
#[async_trait]
impl Driver for Example {{
    fn id(&self) -> &str {{ "{name}" }}
    fn version(&self) -> &str {{ env!("CARGO_PKG_VERSION") }}
    async fn capabilities(&mut self) -> Result<Vec<Capability>> {{ Ok(vec![capability()]) }}
    async fn execute(&mut self, command: &str, digest: &str, _args: Value) -> Result<Value> {{
        let cap = capability();
        if command != cap.descriptor.name || descriptor_digest(&cap.descriptor)? != digest {{
            return Err(Error::new(ErrorCode::StaleReference, "Capability descriptor changed"));
        }}
        Ok(json!({{"ok":true}}))
    }}
}}
#[tokio::main]
async fn main() {{
    if let Err(error) = serve(Example).await {{
        eprintln!("{{error}}");
        std::process::exit(error.exit_code());
    }}
}}
"#
            );
            create(&output.join("src/main.rs"), &main)?;
            let manifest = json!({
                "manifest_version":1,
                "protocol":1,
                "id":name,
                "version":"0.1.0",
                "publisher":"REPLACE_WITH_PUBLISHER",
                "executable":format!("/ABSOLUTE/PATH/TO/{package}"),
                "sha256":"0".repeat(64),
                "application":{"desktop_id":format!("org.example.{name}"),"process_names":[],"supported_versions":[]},
                "transport":"stdio_v1",
                "mounts":[],
                "system_config":[],
                "network":false,
                "resources":{
                    "open_files":128,
                    "processes":32,
                    "cpu_seconds":20,
                    "address_space_bytes":536870912,
                    "file_size_bytes":16777216
                },
                "request_timeout_ms":30000,
                "interfaces":{"dynamic_capabilities":false,"cooperative_cancellation":false,"events":false,"health":true}
            });
            create(
                &output.join("driver.manifest.example.json"),
                &(serde_json::to_string_pretty(&manifest)? + "\n"),
            )?;
            create(
                &output.join("README.md"),
                "# Semwright application driver\n\nBuild the binary, replace the absolute executable path and SHA-256 in driver.manifest.example.json, rename it to a protected owner manifest, then run semwright driver validate and semwright driver conformance. Scaffolding never installs or grants authority to the driver.\n",
            )?;
            print_result(
                &json!({"created":output,"installed":false,"next":"build, pin SHA-256, validate, conformance"}),
                cli.json,
            )
        }
        DriverCommand::Package { command } => match command {
            DriverPackageCommand::Create {
                manifest,
                output,
                semwright,
            } => {
                let manifest = load_driver_manifest(manifest)?;
                let requirement = semwright
                    .clone()
                    .unwrap_or_else(|| format!("={}", env!("CARGO_PKG_VERSION")));
                if cli.dry_run {
                    print_result(
                        &json!({
                            "valid": true,
                            "created": false,
                            "output": output,
                            "semwright": requirement,
                            "executed": false
                        }),
                        cli.json,
                    )
                } else {
                    let digest = create_driver_package(&manifest, &requirement, output)?;
                    print_result(
                        &json!({
                            "created": true,
                            "output": output,
                            "package_sha256": digest,
                            "semwright": requirement,
                            "executed": false
                        }),
                        cli.json,
                    )
                }
            }
            DriverPackageCommand::Inspect { package } => {
                let (metadata, executable, digest) = inspect_driver_package(package)?;
                print_result(
                    &json!({
                        "package": package,
                        "package_sha256": digest,
                        "executable_bytes": executable.len(),
                        "metadata": metadata,
                        "executed": false
                    }),
                    cli.json,
                )
            }
        },
        DriverCommand::Index { command } => match command {
            DriverIndexCommand::Validate { index } => {
                let registry = DriverIndex::load(index)?;
                print_result(
                    &json!({
                        "valid": true,
                        "index": index,
                        "entries": registry.drivers.len(),
                        "executed": false
                    }),
                    cli.json,
                )
            }
            DriverIndexCommand::Search {
                index,
                query,
                application_version,
            } => {
                let registry = DriverIndex::load(index)?;
                let query = query.to_ascii_lowercase();
                let mut rows = vec![];
                for entry in &registry.drivers {
                    if !query.is_empty()
                        && !entry.id.to_ascii_lowercase().contains(&query)
                        && !entry.publisher.to_ascii_lowercase().contains(&query)
                        && !entry.version.to_ascii_lowercase().contains(&query)
                    {
                        continue;
                    }
                    rows.push(json!({
                        "id": entry.id,
                        "version": entry.version,
                        "publisher": entry.publisher,
                        "package": entry.package,
                        "package_sha256": entry.package_sha256,
                        "package_bytes": entry.package_bytes,
                        "semwright": entry.semwright,
                        "application_versions": entry.application_versions,
                        "compatible": entry.compatible(
                            env!("CARGO_PKG_VERSION"),
                            application_version.as_deref()
                        )?,
                        "application_version_required":
                            !entry.application_versions.is_empty() && application_version.is_none()
                    }));
                }
                print_result(
                    &json!({
                        "index": index,
                        "query": query,
                        "application_version": application_version,
                        "drivers": rows,
                        "executed": false
                    }),
                    cli.json,
                )
            }
        },
        DriverCommand::Install {
            index,
            id,
            version,
            application_version,
            data_dir,
            config_dir,
        } => {
            let registry = DriverIndex::load(index)?;
            let entry = registry.resolve(
                id,
                version.as_deref(),
                application_version.as_deref(),
                env!("CARGO_PKG_VERSION"),
            )?;
            let defaults = InstallRoots::defaults()?;
            let roots = InstallRoots {
                data: data_dir.clone().unwrap_or(defaults.data),
                config: config_dir.clone().unwrap_or(defaults.config),
            };
            if cli.dry_run {
                print_result(
                    &json!({
                        "resolved": entry,
                        "installed": false,
                        "dry_run": true,
                        "data_root": roots.data,
                        "config_root": roots.config,
                        "policy_grants_changed": false,
                        "executed": false
                    }),
                    cli.json,
                )
            } else {
                let receipt = install_driver_from_index(
                    index,
                    entry,
                    application_version.as_deref(),
                    &roots,
                )?;
                print_result(
                    &json!({
                        "installed": true,
                        "receipt": receipt,
                        "policy_grants_changed": false,
                        "executed": false,
                        "restart_required": true
                    }),
                    cli.json,
                )
            }
        }
        DriverCommand::Update {
            index,
            id,
            application_version,
            data_dir,
            config_dir,
        } => {
            let registry = DriverIndex::load(index)?;
            let entry = registry.resolve(
                id,
                None,
                application_version.as_deref(),
                env!("CARGO_PKG_VERSION"),
            )?;
            let defaults = InstallRoots::defaults()?;
            let roots = InstallRoots {
                data: data_dir.clone().unwrap_or(defaults.data),
                config: config_dir.clone().unwrap_or(defaults.config),
            };
            if cli.dry_run {
                print_result(
                    &json!({
                        "resolved": entry,
                        "updated": false,
                        "dry_run": true,
                        "policy_grants_changed": false,
                        "executed": false
                    }),
                    cli.json,
                )
            } else {
                let receipt = install_driver_from_index(
                    index,
                    entry,
                    application_version.as_deref(),
                    &roots,
                )?;
                print_result(
                    &json!({
                        "updated": true,
                        "receipt": receipt,
                        "policy_grants_changed": false,
                        "executed": false,
                        "restart_required": true
                    }),
                    cli.json,
                )
            }
        }
        DriverCommand::Remove {
            id,
            version,
            data_dir,
            config_dir,
        } => {
            let defaults = InstallRoots::defaults()?;
            let roots = InstallRoots {
                data: data_dir.clone().unwrap_or(defaults.data),
                config: config_dir.clone().unwrap_or(defaults.config),
            };
            if cli.dry_run {
                print_result(
                    &json!({
                        "id": id,
                        "version": version,
                        "removed": false,
                        "dry_run": true,
                        "policy_grants_changed": false,
                        "executed": false
                    }),
                    cli.json,
                )
            } else {
                remove_installed_driver(id, version, &roots)?;
                print_result(
                    &json!({
                        "id": id,
                        "version": version,
                        "removed": true,
                        "policy_grants_changed": false,
                        "executed": false,
                        "restart_required": true
                    }),
                    cli.json,
                )
            }
        }
    }
}

fn upstream_registry_path(cli: &Cli) -> Result<PathBuf> {
    cli.mcp_upstreams
        .clone()
        .map(Ok)
        .unwrap_or_else(default_upstream_registry_path)
}
fn upstream_view(upstream: &StdioUpstreamConfig) -> serde_json::Value {
    json!({
        "slug":upstream.slug,
        "enabled":upstream.enabled,
        "program":upstream.program,
        "sha256":upstream.sha256,
        "arg_count":upstream.args.len(),
        "expected_name":upstream.expected_name,
        "expected_version":upstream.expected_version,
        "request_timeout_ms":upstream.request_timeout_ms,
        "required_policy_scope":format!("external-mcp:{}",upstream.slug),
    })
}
async fn manage_upstream(cli: &Cli, command: &McpUpstream) -> Result<()> {
    let path = upstream_registry_path(cli)?;
    let mut registry = load_upstream_registry(&path)?;
    let result = match command {
        McpUpstream::List => json!({
            "registry":path,
            "upstreams":registry.upstreams.iter().map(upstream_view).collect::<Vec<_>>(),
            "policy_grants_changed":false
        }),
        McpUpstream::Inspect { slug } => {
            let upstream = registry
                .find(slug)
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "MCP upstream not found"))?;
            json!({"registry":path,"upstream":upstream_view(upstream),"policy_grants_changed":false})
        }
        McpUpstream::Add {
            slug,
            program,
            args,
            sha256,
            expected_name,
            expected_version,
            request_timeout_ms,
            disabled,
            replace,
        } => {
            let program = std::fs::canonicalize(program)?;
            let upstream = new_upstream(
                slug.clone(),
                program,
                sha256.clone(),
                args.clone(),
                expected_name.clone(),
                expected_version.clone(),
                *request_timeout_ms,
                !*disabled,
            )?;
            let view = upstream_view(&upstream);
            registry.add(upstream, *replace)?;
            if !cli.dry_run {
                save_upstream_registry(&path, &registry)?;
            }
            json!({
                "registry":path,
                "upstream":view,
                "saved":!cli.dry_run,
                "dry_run":cli.dry_run,
                "policy_grants_changed":false,
                "restart_required":!cli.dry_run
            })
        }
        McpUpstream::Enable { slug } => {
            registry.set_enabled(slug, true)?;
            if !cli.dry_run {
                save_upstream_registry(&path, &registry)?;
            }
            json!({
                "registry":path,"slug":slug,"enabled":true,"policy_grants_changed":false,
                "dry_run":cli.dry_run,"saved":!cli.dry_run,"restart_required":!cli.dry_run
            })
        }
        McpUpstream::Disable { slug } => {
            registry.set_enabled(slug, false)?;
            if !cli.dry_run {
                save_upstream_registry(&path, &registry)?;
            }
            json!({
                "registry":path,"slug":slug,"enabled":false,"policy_grants_changed":false,
                "dry_run":cli.dry_run,"saved":!cli.dry_run,"restart_required":!cli.dry_run
            })
        }
        McpUpstream::Remove { slug } => {
            registry.remove(slug)?;
            if !cli.dry_run {
                save_upstream_registry(&path, &registry)?;
            }
            json!({
                "registry":path,"slug":slug,"removed":true,"policy_grants_changed":false,
                "dry_run":cli.dry_run,"saved":!cli.dry_run,"restart_required":!cli.dry_run
            })
        }
        McpUpstream::Doctor { slug } => {
            let upstream = registry
                .find(slug)
                .cloned()
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "MCP upstream not found"))?;
            if cli.dry_run {
                upstream.validate()?;
                json!({
                    "registry":path,
                    "slug":slug,
                    "dry_run":true,
                    "executable_valid":true,
                    "launched":false,
                    "required_policy_scope":format!("external-mcp:{slug}"),
                    "policy_grants_changed":false
                })
            } else {
                let doctor = doctor_stdio(upstream).await?;
                json!({
                    "registry":path,
                    "slug":slug,
                    "healthy":true,
                    "provider":doctor.provider,
                    "namespace":doctor.namespace,
                    "server_version":doctor.server_version,
                    "capabilities":doctor.capabilities,
                    "dry_run":false,
                    "launched":true,
                    "policy_grants_changed":false
                })
            }
        }
    };
    print_result(&result, cli.json)
}

async fn local(cli: &Cli) -> Result<bool> {
    match &cli.command {
        Command::Completions { shell } => {
            clap_complete::generate(
                *shell,
                &mut Cli::command(),
                "semwright",
                &mut std::io::stdout(),
            );
        }
        Command::Man => {
            clap_mangen::Man::new(Cli::command()).render(&mut std::io::stdout())?;
        }
        Command::Mcp {
            command: Mcp::Upstream { command },
        } => {
            manage_upstream(cli, command).await?;
        }
        Command::Config { .. } => {
            let paths = semwright_platform_services::paths()?;
            let endpoint = match &cli.socket {
                Some(path) => path.clone(),
                None => ipc::default_endpoint("broker")?,
            };
            print_result(
                &json!({"runtime":paths.runtime,"socket":endpoint,"config":paths.config.join("daemon.toml"),"state":paths.state}),
                cli.json,
            )?;
        }
        Command::Recipe {
            command: Recipe::Scaffold { name, output },
        } => {
            if name.is_empty()
                || name.len() > 64
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            {
                return Err(Error::invalid(
                    "Recipe name must be lowercase letters, digits or hyphens",
                ));
            }
            let template = include_str!("../../../recipes/fake-export.yaml").replacen(
                "name: fake-export",
                &format!("name: {name}"),
                1,
            );
            create(output, &template)?;
            print_result(&json!({"created":output,"installed":false}), cli.json)?;
        }
        Command::Driver { command } => {
            manage_driver(cli, command).await?;
        }
        Command::Plugin {
            command: Plugin::Scaffold { output, sdk_path },
        } => {
            let sdk = sdk_path.canonicalize()?;
            if !sdk.join("Cargo.toml").is_file() {
                return Err(Error::invalid(
                    "--sdk-path must be the semwright-plugin-sdk crate directory",
                ));
            }
            let parent = sdk
                .parent()
                .ok_or_else(|| Error::invalid("SDK path has no crates directory"))?;
            std::fs::create_dir(output)?;
            std::fs::create_dir(output.join("src"))?;
            // JSON strings are also valid TOML basic strings, preventing path interpolation.
            let manifest = format!(
                "[package]\nname = \"semwright-textstats-local\"\nversion = \"0.1.0\"\nedition = \"2024\"\n[dependencies]\nsemwright-plugin-sdk = {{ path = {} }}\nsemwright-types = {{ path = {} }}\nserde_json = \"1\"\ntokio = {{ version = \"1\", features = [\"macros\", \"rt\"] }}\n",
                serde_json::to_string(&sdk)?,
                serde_json::to_string(&parent.join("types"))?
            );
            create(&output.join("Cargo.toml"), &manifest)?;
            create(
                &output.join("src/main.rs"),
                include_str!("../../../adapters/example-plugin/src/main.rs"),
            )?;
            create(
                &output.join("README.md"),
                "# Local textstats plugin\n\nBuild this project, then generate a manifest with scripts/plugin-manifest.py from the Semwright repository. Review the executable digest and permissions before an operator installs it. No code is installed by scaffolding.\n",
            )?;
            print_result(
                &json!({"created":output,"installed":false,"next":"Build and generate a reviewed digest-bound manifest"}),
                cli.json,
            )?;
        }
        _ => return Ok(false),
    }
    Ok(true)
}
async fn run(cli: &Cli) -> Result<i32> {
    if local(cli).await? {
        return Ok(0);
    }
    let socket = cli
        .socket
        .clone()
        .map(Ok)
        .unwrap_or_else(ipc::default_socket)?;
    let ticket = cli
        .session_file
        .clone()
        .unwrap_or(socket.with_file_name("cli.session"));
    let mut client = ipc::connect_persistent(&socket, &ticket).await?;
    if let Command::Watch { after } = &cli.command {
        ipc::write_frame(
            &mut client.stream,
            &ClientMessage::Subscribe { after: *after },
        )
        .await?;
        // Never cancel a partially consumed framed read just to send a heartbeat.
        let (mut read, mut write) = client.stream.into_split();
        let heartbeat = tokio::spawn(async move {
            let mut timer = tokio::time::interval(Duration::from_secs(30));
            loop {
                timer.tick().await;
                if ipc::write_frame(&mut write, &ClientMessage::Ping {})
                    .await
                    .is_err()
                {
                    break;
                }
            }
        });
        let result = loop {
            let message = tokio::select! {_=tokio::signal::ctrl_c()=>break Ok(0),message=ipc::read_frame::<_,ServerMessage>(&mut read)=>message};
            match message {
                Ok(ServerMessage::Event { sequence, event }) => {
                    if let Err(e) = print_result(&json!({"sequence":sequence,"event":event}), true)
                    {
                        break Err(e);
                    }
                }
                Ok(ServerMessage::Pong) => (),
                Ok(ServerMessage::Error { error }) => break Err(error),
                Ok(_) => {
                    break Err(Error::new(
                        ErrorCode::ProtocolMismatch,
                        "Unexpected event-stream frame",
                    ));
                }
                Err(e) => break Err(e),
            }
        };
        heartbeat.abort();
        return result;
    }
    if matches!(
        &cli.command,
        Command::Recipe {
            command: Recipe::Test { .. }
        }
    ) {
        let check = client
            .execute(
                unique_id(),
                ExecuteRequest {
                    command: "doctor".into(),
                    args: json!({}),
                    dry_run: false,
                    backend: None,
                },
            )
            .await?;
        if check
            .data
            .as_ref()
            .and_then(|d| d.get("fake"))
            .and_then(|v| v.as_bool())
            != Some(true)
        {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "recipe test requires a broker started with --fake; refusing to run on a live desktop",
            ));
        }
    }
    let request = request(cli)?
        .ok_or_else(|| Error::new(ErrorCode::Internal, "Command has no request mapping"))?;
    let id = unique_id();
    let started = std::time::Instant::now();
    let result = tokio::select! {
        value=client.execute(id.clone(),request.clone())=>value,
        _=tokio::signal::ctrl_c()=>{let _=tokio::time::timeout(Duration::from_secs(1),client.cancel(id.clone())).await;Err(Error::new(ErrorCode::Cancelled,"Cancellation sent; a dispatched action may already have taken effect. Do not retry blindly.").uncertain())},
    };
    let envelope = match result {
        Ok(e) => e,
        Err(e) => Envelope::finish(
            id,
            request.command,
            "transport".into(),
            started.elapsed(),
            request.dry_run,
            Err(e.uncertain()),
        ),
    };
    let code = envelope.error.as_ref().map_or(0, Error::exit_code);
    print_result(&serde_json::to_value(envelope)?, cli.json)?;
    Ok(code)
}
#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let code = match run(&cli).await {
        Ok(code) => code,
        Err(error) => {
            if cli.json {
                let _ = print_result(&json!({"ok":false,"error":error}), true);
            } else {
                eprintln!("{error}");
            }
            error.exit_code()
        }
    };
    std::process::exit(code);
}
