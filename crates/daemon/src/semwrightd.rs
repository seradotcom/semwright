use clap::Parser;
use semwright_core::{Approver, Broker, NoApprover, audit::Audit};
use semwright_daemon::{config, console::Console, server};
use semwright_driver_host::DriverProvider;
use semwright_federation::{
    ExternalMcpProvider, default_upstream_registry_path, load_upstream_registry,
};
use semwright_platform_common::{artifact::ArtifactHandoff, filesystem::Filesystem};
use semwright_plugin_host::Host;
use semwright_policy::Policy;
use semwright_protocol::{current_uid, private_directory, runtime_directory};
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
}
fn main() {
    let args = Args::parse();
    if current_uid() == 0 && !args.fake {
        eprintln!("PermissionDenied: semwrightd must run as your login user, not root");
        std::process::exit(3);
    }
    // SAFETY: umask is set once before the asynchronous runtime or worker threads are created.
    unsafe {
        libc::umask(0o077);
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
    let runtime = runtime_directory()?;
    let socket = args.socket.unwrap_or_else(|| {
        runtime.join(if args.fake {
            "fake.sock"
        } else {
            "broker.sock"
        })
    });
    let config = config::load(args.config.as_deref())?;
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
    if let Some(parent) = socket.parent() {
        private_directory(parent)?;
        protected.push(parent.to_path_buf());
    }
    config::confine_grants(&config, &protected)?;
    let audit_dir = state.join("audit");
    let audit = Audit::open(&audit_dir, config.audit_max_bytes, config.audit_retention)?;
    let policy = Policy::new(config.policy.clone())?;

    let platform = semwright_platform_host::bootstrap(
        args.fake,
        &runtime,
        &state,
        config.applications.clone(),
        config.browser.clone(),
        config.blender_socket.clone(),
    )
    .await?;
    let mut backends = platform.backends;
    let environment = platform.environment;
    // Holds Linux D-Bus ownership (and any future native connection lifetimes)
    // until after the shared broker has shut down.
    let platform_keepalive = platform.keepalive;
    if !config.policy.filesystem.is_empty() {
        backends.push(Arc::new(Filesystem::new(&config.policy.filesystem)?));
        backends.push(Arc::new(ArtifactHandoff::new(&config.policy.filesystem)?));
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
            config.policy.filesystem.clone(),
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
        let manifest = config::driver_manifest(path)?;
        let provider = DriverProvider::connect(
            manifest,
            &state.join("drivers"),
            &sandbox_helper,
            &config.policy.filesystem,
            config.driver_network,
        )
        .await?;
        broker.mount_provider(provider).await?;
    }
    if args.fake && !upstreams.is_empty() {
        return Err(Error::new(
            ErrorCode::PolicyDenied,
            "Fake mode cannot launch trusted MCP upstream processes",
        ));
    }
    for upstream in upstreams {
        let provider = ExternalMcpProvider::connect_trusted_stdio(upstream).await?;
        broker.mount_provider(provider).await?;
    }
    let stop = CancellationToken::new();
    let signal = stop.clone();
    tokio::spawn(async move {
        let mut terminate =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(_) => {
                    signal.cancel();
                    return;
                }
            };
        tokio::select! {_ = tokio::signal::ctrl_c()=>(),_ = terminate.recv()=>()};
        signal.cancel();
    });
    tracing::info!(
        fake = args.fake,
        "broker ready; authorization defaults deny side effects"
    );
    let result = server::serve(&socket, broker, stop).await;
    drop(platform_keepalive);
    result
}
