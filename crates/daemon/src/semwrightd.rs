#[cfg(feature = "private-driver-startup-stderr")]
mod driver_startup_stderr_custody;
#[cfg(feature = "private-host-stdin-custody")]
mod host_stdin_custody;
#[cfg(any(
    feature = "private-host-stdin-custody",
    feature = "private-driver-startup-stderr"
))]
mod private_custody;
use clap::Parser;
use semwright_core::{Approver, Broker, NoApprover, audit::Audit};
use semwright_daemon::console::Console;
use semwright_daemon::{config, server};
use semwright_driver_host::DriverProvider;
use semwright_federation::{
    ExternalMcpProvider, default_upstream_registry_path, load_upstream_registry,
};
use semwright_platform_common::{artifact::ArtifactHandoff, filesystem::Filesystem};
use semwright_plugin_host::Host;
use semwright_policy::Policy;
#[cfg(unix)]
use semwright_protocol::{current_uid, private_directory};
use semwright_protocol::{default_endpoint, runtime_directory};
use semwright_types::*;
use std::{collections::BTreeSet, path::PathBuf, sync::Arc};
use tokio_util::sync::CancellationToken;
#[derive(Parser)]
#[command(
    version,
    about = "Semwright local semantic capability broker. Observe-only unless owner configuration grants more."
)]
struct Args {
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    socket: Option<PathBuf>,
    /// Owner-managed MCP definitions. Definitions never grant policy authority.
    #[arg(long, env = "SEMWRIGHT_MCP_UPSTREAMS")]
    mcp_upstreams: Option<PathBuf>,
    /// Instantiate only deterministic fixtures. Never routes to the live desktop.
    #[arg(long)]
    fake: bool,
    /// Ask the human on this process's controlling terminal. No agent approval endpoint exists.
    #[arg(long)]
    approval_console: bool,
    #[arg(long,default_value="pretty",value_parser=["pretty","json"])]
    log_format: String,
    #[cfg(feature = "private-host-stdin-custody")]
    #[command(flatten)]
    stdin_custody: host_stdin_custody::Options,
    #[cfg(feature = "private-driver-startup-stderr")]
    #[command(flatten)]
    startup_stderr: driver_startup_stderr_custody::Options,
}
fn main() {
    let args = Args::parse();
    #[cfg(unix)]
    if current_uid() == 0 && !args.fake {
        eprintln!("PermissionDenied: semwrightd must run as your login user, not root");
        std::process::exit(3);
    }
    #[cfg(unix)]
    {
        // SAFETY: umask is set once before the asynchronous runtime or worker threads are created.
        unsafe {
            libc::umask(0o077);
        }
    }
    if args.log_format == "json" {
        tracing_subscriber::fmt()
            .json()
            .with_writer(std::io::stderr)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_writer(std::io::stderr)
            .init();
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("Internal: unable to initialize runtime");
            std::process::exit(1);
        }
    };
    if let Err(error) = semwright_platform_host::run(runtime, run(args)) {
        eprintln!("{error}");
        std::process::exit(error.exit_code());
    }
}
async fn run(args: Args) -> Result<()> {
    #[cfg(target_os = "windows")]
    if args.approval_console {
        Console::validate()?;
    }
    let runtime = runtime_directory()?;
    let socket = match args.socket {
        Some(path) => path,
        None => default_endpoint(if args.fake { "fake" } else { "broker" })?,
    };
    let config = config::load(args.config.as_deref())?;
    #[cfg(feature = "private-host-stdin-custody")]
    let mut stdin_custody =
        host_stdin_custody::Bootstrap::prepare(&args.stdin_custody, args.fake, &config.drivers)?;
    #[cfg(feature = "private-driver-startup-stderr")]
    let mut startup_stderr = driver_startup_stderr_custody::Bootstrap::prepare(
        &args.startup_stderr,
        args.fake,
        &config.drivers,
        |path| {
            #[cfg(feature = "private-host-stdin-custody")]
            if let Some(custody) = &stdin_custody {
                return custody.retained_manifest(path);
            }
            config::retained_driver_manifest(path)
        },
    )?;
    // One completion path for every startup/serve Result after custody setup.
    // This does not retry or rewrite any native error; it lets private capture
    // finish before main may exit the process on an error.
    let result = async {
        let upstream_registry_path = if args.fake && args.mcp_upstreams.is_none() {
            None
        } else {
            Some(
                args.mcp_upstreams
                    .clone()
                    .map(Ok)
                    .unwrap_or_else(default_upstream_registry_path)?,
            )
        };
        let registry = match &upstream_registry_path {
            Some(path) => load_upstream_registry(path)?,
            None => Default::default(),
        };
        let mut upstreams = config
            .trusted_mcp_stdio_upstreams
            .iter()
            .filter(|entry| entry.enabled)
            .cloned()
            .collect::<Vec<_>>();
        upstreams.extend(registry.upstreams.into_iter().filter(|entry| entry.enabled));
        let mut upstream_slugs = BTreeSet::new();
        if upstreams
            .iter()
            .any(|entry| !upstream_slugs.insert(entry.slug.clone()))
        {
            return Err(Error::new(
                ErrorCode::Conflict,
                "MCP upstream slug is defined more than once across owner configuration",
            ));
        }
        #[cfg(unix)]
        if args.fake
            && current_uid() == 0
            && (!config.policy.filesystem.is_empty()
                || !config.plugins.is_empty()
                || !config.drivers.is_empty())
        {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Root fake mode cannot expose filesystem grants or plugins",
            ));
        }
        let state = config::state_directory(args.fake, &runtime)?;
        let mut protected = vec![runtime.clone(), state.clone()];
        if let Some(path) = &args.config {
            protected.push(path.clone());
        }
        if let Some(path) = &upstream_registry_path
            && path.exists()
        {
            protected.push(path.clone());
        }
        for path in config.plugins.iter().chain(&config.drivers) {
            if path.exists() {
                protected.push(path.clone());
            }
        }
        #[cfg(unix)]
        if let Some(parent) = socket.parent() {
            private_directory(parent)?;
            protected.push(parent.to_path_buf());
        }
        #[cfg(feature = "private-host-stdin-custody")]
        if let Some(custody) = &stdin_custody {
            protected.push(custody.directory().to_path_buf());
        }
        #[cfg(feature = "private-driver-startup-stderr")]
        if let Some(custody) = &startup_stderr {
            protected.push(custody.directory().to_path_buf());
        }
        config::confine_grants(&config, &protected)?;
        // Driver secret/tool authorities may be individual files. Generic filesystem,
        // browser-upload, artifact and plugin roots are directory-scoped and must not
        // attempt to open those file-backed grants as directories.
        let directory_filesystem_grants = config::directory_filesystem_grants(&config)?;
        let audit_dir = state.join("audit");
        let audit = Audit::open(&audit_dir, config.audit_max_bytes, config.audit_retention)?;
        let policy = Policy::new(config.policy.clone())?;

        let platform = semwright_platform_host::bootstrap(
            args.fake,
            &runtime,
            &state,
            config.applications.clone(),
            config.browser.clone(),
            directory_filesystem_grants.clone(),
            config.blender_socket.clone(),
        )
        .await?;
        let mut backends = platform.backends;
        let environment = platform.environment;
        // Holds Linux D-Bus ownership (and any future native connection lifetimes)
        // until after the shared broker has shut down.
        let platform_keepalive = platform.keepalive;
        if !directory_filesystem_grants.is_empty() {
            backends.push(Arc::new(Filesystem::new(&directory_filesystem_grants)?));
            backends.push(Arc::new(ArtifactHandoff::new(
                &directory_filesystem_grants,
            )?));
        }
        let sandbox_helper = std::env::current_exe()?
            .parent()
            .ok_or_else(|| Error::unavailable("Cannot locate sandbox helper directory"))?
            .join("semwright-sandbox");
        let host = if args.fake {
            None
        } else {
            Some(Arc::new(Host::new(
                state.join("plugins"),
                sandbox_helper.clone(),
                directory_filesystem_grants.clone(),
                config.plugin_network,
            )?))
        };
        let approver: Arc<dyn Approver> = if args.approval_console {
            Arc::new(Console)
        } else {
            Arc::new(NoApprover)
        };
        let broker = Broker::new(
            policy,
            backends,
            audit,
            approver,
            host,
            environment,
            args.fake,
        )?;
        broker.configure_project_graphs(
            &state.join("projects"),
            semwright_platform_services::current_user_principal()?,
        )?;
        for path in &config.plugins {
            broker.install_manifest(config::manifest(path)?)?;
        }
        if args.fake && !config.drivers.is_empty() {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Fake mode cannot launch application drivers",
            ));
        }
        for path in &config.drivers {
            #[cfg(feature = "private-driver-startup-stderr")]
            let retained = startup_stderr
                .as_ref()
                .map(|custody| custody.retained_manifest(path))
                .transpose()?;
            #[cfg(not(feature = "private-driver-startup-stderr"))]
            let retained: Option<config::RetainedDriverManifest> = None;
            let retained = match retained {
                Some(retained) => retained,
                None => {
                    #[cfg(not(feature = "private-host-stdin-custody"))]
                    let retained = config::retained_driver_manifest(path)?;
                    #[cfg(feature = "private-host-stdin-custody")]
                    let retained = match &stdin_custody {
                        Some(custody) => custody.retained_manifest(path)?,
                        None => config::retained_driver_manifest(path)?,
                    };
                    retained
                }
            };
            #[cfg(feature = "private-host-stdin-custody")]
            let custody_sink = stdin_custody
                .as_ref()
                .and_then(|custody| custody.sink_for(retained.parsed()));
            #[cfg(not(feature = "private-host-stdin-custody"))]
            let custody_sink = None;
            #[cfg(feature = "private-driver-startup-stderr")]
            let startup_sink = startup_stderr
                .as_ref()
                .and_then(|custody| custody.sink_for(retained.parsed()));
            #[cfg(not(feature = "private-driver-startup-stderr"))]
            let startup_sink: Option<
                semwright_driver_host::DriverStartupStderrCustody,
            > = None;
            #[cfg(feature = "private-driver-startup-stderr")]
            let observes_startup = startup_sink.is_some();
            let connected = match startup_sink {
                Some(sink) => {
                    DriverProvider::connect_with_gpu_and_retained_manifest_observed(
                        retained.into_parts(),
                        &state.join("drivers"),
                        &sandbox_helper,
                        &config.policy.filesystem,
                        config.driver_network,
                        config.driver_nvidia_gpu,
                        (custody_sink, sink),
                    )
                    .await
                }
                None => {
                    DriverProvider::connect_with_gpu_and_retained_manifest(
                        retained.into_parts(),
                        &state.join("drivers"),
                        &sandbox_helper,
                        &config.policy.filesystem,
                        config.driver_network,
                        config.driver_nvidia_gpu,
                        custody_sink,
                    )
                    .await
                }
            };
            #[cfg(feature = "private-driver-startup-stderr")]
            if observes_startup && let Some(custody) = &startup_stderr {
                custody.connection_result(&connected);
            }
            let provider = connected?;
            broker.mount_driver_provider(provider).await?;
        }

        if args.fake && !upstreams.is_empty() {
            return Err(Error::new(
                ErrorCode::PolicyDenied,
                "Fake mode cannot launch trusted MCP upstream processes",
            ));
        }
        for upstream in upstreams {
            let provider = ExternalMcpProvider::connect_sandboxed_stdio(
                upstream,
                &state.join("mcp-upstreams"),
                &sandbox_helper,
                config.mcp_network,
            )
            .await?;
            broker.mount_provider(provider).await?;
        }
        // Learned workflows may depend on configured plugins, drivers, or federated MCPs.
        // Restore them only after all owner-configured providers are present in the catalog.
        let workflow_restore = broker.configure_workflows(&state.join("workflows"))?;
        tracing::info!(
            workflow_restore = %workflow_restore,
            "workflow store loaded"
        );
        let stop = CancellationToken::new();
        let signal = stop.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                let mut terminate =
                    match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                    {
                        Ok(signal) => signal,
                        Err(_) => {
                            signal.cancel();
                            return;
                        }
                    };
                tokio::select! {_ = tokio::signal::ctrl_c()=>(),_ = terminate.recv()=>()};
            }
            #[cfg(target_os = "windows")]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            signal.cancel();
        });
        tracing::info!(
            fake = args.fake,
            "broker ready; authorization defaults deny side effects"
        );
        let result = server::serve(&socket, broker, stop).await;
        #[cfg(feature = "private-host-stdin-custody")]
        if let Some(custody) = &mut stdin_custody {
            custody.finish().await;
        }
        drop(platform_keepalive);
        result
    }
    .await;
    #[cfg(feature = "private-driver-startup-stderr")]
    if let Some(custody) = &mut startup_stderr {
        custody.finish().await;
    }
    result
}

#[cfg(test)]
mod stdin_custody_cli_tests {
    use super::*;

    #[test]
    fn ordinary_cli_does_not_enable_custody() {
        let args = Args::try_parse_from(["semwrightd"]).unwrap();
        assert!(!args.fake);
        #[cfg(feature = "private-host-stdin-custody")]
        assert!(args.stdin_custody.disabled());
        #[cfg(feature = "private-driver-startup-stderr")]
        assert!(args.startup_stderr.disabled());
    }

    #[cfg(not(feature = "private-driver-startup-stderr"))]
    #[test]
    fn startup_stderr_flags_are_absent_from_default_binary() {
        for flag in [
            "--driver-startup-stderr-dir",
            "--driver-startup-stderr-provider",
        ] {
            let error = Args::try_parse_from(["semwrightd", flag, "unused"])
                .err()
                .unwrap();
            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        }
    }

    #[cfg(feature = "private-driver-startup-stderr")]
    #[test]
    fn startup_stderr_cli_requires_explicit_selection_and_rejects_fake() {
        let args = Args::try_parse_from([
            "semwrightd",
            "--driver-startup-stderr-dir",
            "unused",
            "--driver-startup-stderr-provider",
            "driver:fixture",
        ])
        .unwrap();
        assert!(!args.startup_stderr.disabled());
        assert!(
            driver_startup_stderr_custody::Bootstrap::prepare(
                &args.startup_stderr,
                true,
                &[],
                |_| panic!("fake custody cannot load manifests")
            )
            .is_err()
        );
    }

    #[cfg(not(feature = "private-host-stdin-custody"))]
    #[test]
    fn diagnostic_flags_are_absent_from_the_default_binary() {
        for flag in [
            "--host-stdin-custody-dir",
            "--host-stdin-custody-provider",
            "--host-stdin-custody-tool",
        ] {
            let error = Args::try_parse_from(["semwrightd", flag, "unused"])
                .err()
                .unwrap();
            assert_eq!(error.kind(), clap::error::ErrorKind::UnknownArgument);
        }
    }

    #[cfg(feature = "private-host-stdin-custody")]
    #[test]
    fn diagnostic_binary_requires_explicit_cli_selection() {
        let args = Args::try_parse_from([
            "semwrightd",
            "--host-stdin-custody-dir",
            "unused",
            "--host-stdin-custody-provider",
            "driver:fixture",
            "--host-stdin-custody-tool",
            "node",
        ])
        .unwrap();
        assert!(!args.stdin_custody.disabled());
        assert!(host_stdin_custody::Bootstrap::prepare(&args.stdin_custody, true, &[]).is_err());
    }
}
