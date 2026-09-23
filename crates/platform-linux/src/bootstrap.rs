//! Connection ownership is part of the platform host lifetime.
use crate::{
    atspi::Atspi,
    bridge::{Gnome, Kwin},
    clipboard::Clipboard,
    hyprland::Hyprland,
    portal::Portal,
    sway::Sway,
    system::System,
    x11::X11,
};
use semwright_adapters::{
    blender::Blender,
    chromium::{BrowserConfig, Chromium},
};
use semwright_backend_api::Backend;
use semwright_platform_api::DesktopHost;
use semwright_platform_common::Application;
use semwright_types::{Error, ErrorCode, Result};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};

pub async fn bootstrap(
    runtime: &Path,
    state: &Path,
    applications: BTreeMap<String, Application>,
    browser: BrowserConfig,
    blender_socket: Option<PathBuf>,
) -> Result<DesktopHost> {
    let dbus = match zbus::Connection::session().await {
        Ok(connection) => {
            connection.request_name("org.semwright.Broker").await.map_err(|_|Error::new(ErrorCode::Conflict,"Another real broker owns org.semwright.Broker, or the session bus denied the name"))?;
            Some(connection)
        }
        Err(_) => None,
    };
    let mut environment = crate::environment();
    environment["broker_session_bus_owned"] = serde_json::json!(dbus.is_some());
    let mut backends: Vec<Arc<dyn Backend>> = vec![
        Arc::new(Atspi::default()),
        Arc::new(Sway::default()),
        Arc::new(Hyprland::default()),
        Arc::new(X11::default()),
        Arc::new(Gnome::new(dbus.clone())),
        Arc::new(Clipboard::default()),
        Arc::new(System::new(applications)?),
    ];
    if let Some(connection) = &dbus {
        match Kwin::attach(connection).await {
            Ok(kwin) => backends.push(Arc::new(kwin)),
            Err(_) => environment["kwin_mailbox"] = serde_json::json!("unavailable"),
        }
    }
    backends.push(Arc::new(Portal::new(
        runtime.join("artifacts"),
        state.join("portal"),
    )?));
    let blender_socket = blender_socket.unwrap_or_else(|| {
        runtime
            .parent()
            .unwrap_or(runtime)
            .join("semwright-blender/bridge.sock")
    });
    backends.push(Arc::new(Blender::new(blender_socket)));
    backends.push(Arc::new(Chromium::new(browser, runtime.join("browser"))?));
    Ok(DesktopHost {
        backends,
        environment,
        keepalive: Box::new(dbus),
    })
}
