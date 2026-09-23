//! Native small system surface. No arbitrary executable/argv supplied by the agent.
use async_trait::async_trait;
use semwright_backend_api::{Backend, Context, feature};
pub use semwright_platform_common::Application;
use semwright_types::*;
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::fs::MetadataExt,
    },
    process::Stdio,
};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};
use zbus::{
    Connection, Proxy,
    zvariant::{OwnedObjectPath, OwnedValue},
};
struct Process {
    child: Child,
    pidfd: OwnedFd,
    pid: u32,
    name: String,
}
pub struct System {
    applications: BTreeMap<String, Application>,
    processes: Mutex<BTreeMap<String, Process>>,
}
fn bus<T>(r: zbus::Result<T>) -> Result<T> {
    r.map_err(|_| {
        Error::new(
            ErrorCode::Unavailable,
            "Native system D-Bus service is unavailable",
        )
    })
}
impl System {
    pub fn new(applications: BTreeMap<String, Application>) -> Result<Self> {
        for app in applications.values() {
            if !app.executable.is_absolute() || app.args.len() > 64 {
                return Err(Error::invalid(
                    "Application launch configuration requires an absolute executable and at most 64 arguments",
                ));
            }
            if app
                .executable
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| matches!(s, "sh" | "bash" | "zsh" | "sudo" | "pkexec" | "su"))
            {
                return Err(Error::invalid(
                    "Shell and privilege helpers are not application-launch targets",
                ));
            }
            if app.args.iter().any(|s| s.len() > 8192) {
                return Err(Error::invalid("Configured application argument too long"));
            }
        }
        Ok(Self {
            applications,
            processes: Mutex::new(BTreeMap::new()),
        })
    }
    async fn launch(&self, ctx: &Context, key: &str) -> Result<Value> {
        let app = self.applications.get(key).ok_or_else(|| {
            Error::new(
                ErrorCode::PolicyDenied,
                "Application key is not on the owner-controlled launch allowlist",
            )
        })?;
        let mut command = Command::new(&app.executable);
        command
            .args(&app.args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin");
        // These are desktop routing values, not an inherited arbitrary environment.
        for k in [
            "HOME",
            "LANG",
            "DISPLAY",
            "XAUTHORITY",
            "DBUS_SESSION_BUS_ADDRESS",
            "XDG_RUNTIME_DIR",
            "WAYLAND_DISPLAY",
            "XDG_CURRENT_DESKTOP",
        ] {
            if let Some(v) = std::env::var_os(k) {
                command.env(k, v);
            }
        }
        if let Some(cwd) = &app.cwd {
            command.current_dir(cwd);
        }
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(false);
        ctx.check_cancelled()?;
        let mut child = command.spawn()?;
        let pid = child.id().ok_or_else(|| {
            Error::new(ErrorCode::BackendFailed, "Launched process has no PID").uncertain()
        })?;
        // SAFETY: pidfd_open takes an integer PID and flags; returned fd pins the
        // process identity even if the numeric PID is later reused.
        let fd = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
        if fd < 0 {
            let _ = child.kill().await;
            return Err(Error::unavailable(
                "pidfd_open is required for owned-process lifecycle safety",
            ));
        }
        let id = unique_id();
        // SAFETY: successful pidfd_open returns a newly-owned descriptor.
        let pidfd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        self.processes.lock().await.insert(
            id.clone(),
            Process {
                child,
                pidfd,
                pid,
                name: key.into(),
            },
        );
        Ok(
            json!({"launched":true,"process_ref":target_marker(NativeTarget{kind:"process".into(),identity:id,revision:0,fingerprint:pid.to_string(),app:key.into()}),"pid":pid}),
        )
    }
    async fn list(&self) -> Result<Value> {
        let mut processes = vec![];
        // SAFETY: getuid has no pointer arguments or other preconditions.
        let uid = unsafe { libc::getuid() };
        for entry in std::fs::read_dir("/proc")? {
            let Ok(entry) = entry else { continue };
            let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                continue;
            };
            let Ok(meta) = entry.metadata() else { continue };
            if meta.uid() != uid {
                continue;
            }
            let name = std::fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
            processes.push(json!({"pid":pid,"name":name.trim_end().chars().take(256).collect::<String>(),"uid":uid}));
            if processes.len() >= 4096 {
                break;
            }
        }
        processes.sort_by_key(|p| p["pid"].as_u64());
        Ok(json!({"processes":processes,"scope":"current_uid","command_lines_included":false}))
    }
}
#[async_trait]
impl Backend for System {
    fn name(&self) -> &'static str {
        "system"
    }
    fn supports(&self, c: &str) -> bool {
        matches!(
            c,
            "app.launch"
                | "process.list"
                | "process.signal"
                | "notifications.send"
                | "network.status"
                | "systemd.user.status"
        )
    }
    fn operation_feature(&self, command: &str) -> Option<String> {
        self.supports(command).then(|| command.to_owned())
    }
    async fn probe(&self) -> Vec<Feature> {
        async fn service_present(system: bool, name: &str) -> bool {
            tokio::time::timeout(std::time::Duration::from_millis(700), async {
                let connection = if system {
                    Connection::system().await?
                } else {
                    Connection::session().await?
                };
                let proxy = Proxy::new(
                    &connection,
                    "org.freedesktop.DBus",
                    "/org/freedesktop/DBus",
                    "org.freedesktop.DBus",
                )
                .await?;
                proxy.call::<_, _, bool>("NameHasOwner", &(name,)).await
            })
            .await
            .is_ok_and(|result| result.unwrap_or(false))
        }
        let (notifications, network, systemd) = tokio::join!(
            service_present(false, "org.freedesktop.Notifications"),
            service_present(true, "org.freedesktop.NetworkManager"),
            service_present(false, "org.freedesktop.systemd1"),
        );
        vec![
            feature(
                self.name(),
                "process.list",
                std::path::Path::new("/proc/self/status").is_file(),
                "Current-UID process metadata only",
                "A mounted proc filesystem is required",
            ),
            feature(
                self.name(),
                "app.launch",
                !self.applications.is_empty(),
                "Only explicitly configured launch keys are available",
                "Configure an application executable and fixed arguments in owner configuration",
            ),
            feature(
                self.name(),
                "process.signal",
                !self.processes.lock().await.is_empty(),
                "Only retained broker-launched process references can be signalled",
                "Launch a permitted application first",
            ),
            feature(
                self.name(),
                "notifications.send",
                notifications,
                "Notification bus service ownership was checked",
                "Start a notification service in this session",
            ),
            feature(
                self.name(),
                "network.status",
                network,
                "NetworkManager system-bus ownership was checked",
                "NetworkManager is optional and is not inferred from /proc availability",
            ),
            feature(
                self.name(),
                "systemd.user.status",
                systemd,
                "User systemd session-bus ownership was checked",
                "A systemd user manager must be available",
            ),
        ]
    }
    async fn execute(&self, ctx: &Context, c: &str, args: &Value) -> Result<Value> {
        ctx.check_cancelled()?;
        match c {
            "app.launch" => self.launch(ctx, arg_str(args, "application")?).await,
            "process.list" => self.list().await,
            "process.signal" => {
                let target = native_target(args)?;
                let mut processes = self.processes.lock().await;
                let process = processes.get_mut(&target.identity).ok_or_else(|| {
                    Error::new(
                        ErrorCode::PolicyDenied,
                        "Only broker-launched processes can be signalled",
                    )
                })?;
                if process.child.try_wait()?.is_some() {
                    return Err(Error::new(
                        ErrorCode::StaleReference,
                        "Owned process already exited",
                    ));
                }
                let signal = match arg_str(args, "signal")? {
                    "term" => libc::SIGTERM,
                    "int" => libc::SIGINT,
                    _ => return Err(Error::invalid("Only TERM and INT are exposed")),
                };
                // SAFETY: pidfd pins the exact retained child; null siginfo asks the
                // kernel to synthesize normal sender metadata. No user pointers.
                let result = unsafe {
                    libc::syscall(
                        libc::SYS_pidfd_send_signal,
                        process.pidfd.as_raw_fd(),
                        signal,
                        std::ptr::null::<libc::siginfo_t>(),
                        0,
                    )
                };
                if result != 0 {
                    return Err(std::io::Error::last_os_error().into());
                }
                Ok(json!({"signalled":true}))
            }
            "notifications.send" => {
                let conn = bus(Connection::session().await)?;
                let p = bus(Proxy::new(
                    &conn,
                    "org.freedesktop.Notifications",
                    "/org/freedesktop/Notifications",
                    "org.freedesktop.Notifications",
                )
                .await)?;
                let id: u32 = bus(p
                    .call(
                        "Notify",
                        &(
                            "Semwright",
                            0u32,
                            "",
                            arg_str(args, "summary")?,
                            args["body"].as_str().unwrap_or(""),
                            Vec::<String>::new(),
                            HashMap::<String, OwnedValue>::new(),
                            5000i32,
                        ),
                    )
                    .await)?;
                Ok(json!({"notification_id":id}))
            }
            "network.status" => {
                let conn = bus(Connection::system().await)?;
                let p = bus(Proxy::new(
                    &conn,
                    "org.freedesktop.NetworkManager",
                    "/org/freedesktop/NetworkManager",
                    "org.freedesktop.NetworkManager",
                )
                .await)?;
                let state: u32 = bus(p.get_property("State").await)?;
                let connectivity: u32 = bus(p.get_property("Connectivity").await)?;
                Ok(json!({"state":state,"connectivity":connectivity,"configuration_changed":false}))
            }
            "systemd.user.status" => {
                let conn = bus(Connection::session().await)?;
                let manager = bus(Proxy::new(
                    &conn,
                    "org.freedesktop.systemd1",
                    "/org/freedesktop/systemd1",
                    "org.freedesktop.systemd1.Manager",
                )
                .await)?;
                let path: OwnedObjectPath =
                    bus(manager.call("GetUnit", &(arg_str(args, "unit")?,)).await)?;
                let unit = bus(Proxy::new(
                    &conn,
                    "org.freedesktop.systemd1",
                    path.as_str(),
                    "org.freedesktop.systemd1.Unit",
                )
                .await)?;
                let active: String = bus(unit.get_property("ActiveState").await)?;
                let sub: String = bus(unit.get_property("SubState").await)?;
                Ok(json!({"active_state":active,"sub_state":sub}))
            }
            _ => Err(Error::new(
                ErrorCode::Unsupported,
                "Unknown native system command",
            )),
        }
    }
    async fn validate(&self, t: &NativeTarget) -> Result<()> {
        let mut guard = self.processes.lock().await;
        let p = guard
            .get_mut(&t.identity)
            .ok_or_else(|| Error::new(ErrorCode::StaleReference, "Unknown owned process"))?;
        if p.pid.to_string() != t.fingerprint || p.name != t.app || p.child.try_wait()?.is_some() {
            return Err(Error::new(
                ErrorCode::StaleReference,
                "Owned process exited or identity changed",
            ));
        }
        Ok(())
    }
}
