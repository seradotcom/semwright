//! Internal EI/libei sender transport for an already-granted RemoteDesktop fd.
use futures_util::StreamExt;
use reis::{
    ei,
    enumflags2::BitFlags,
    event::{Device, DeviceCapability, EiEvent},
};
use semwright_types::{Error, ErrorCode, Result};
use std::{os::unix::net::UnixStream, sync::OnceLock, time::Instant};
use tokio::sync::{mpsc, oneshot, watch};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Requested {
    pub keyboard: bool,
    pub pointer: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub text: bool,
    pub pointer: bool,
    pub button: bool,
    pub scroll: bool,
}

impl Capabilities {
    #[must_use]
    pub fn satisfies(self, requested: Requested) -> bool {
        (!requested.keyboard || self.text)
            && (!requested.pointer || (self.pointer && self.button && self.scroll))
    }
}
enum Command {
    KeySym(u32, oneshot::Sender<Result<()>>),
    Text(String, oneshot::Sender<Result<()>>),
    Motion(f32, f32, oneshot::Sender<Result<()>>),
    Button(u32, oneshot::Sender<Result<()>>),
    Scroll(f32, f32, oneshot::Sender<Result<()>>),
    Stop(oneshot::Sender<Result<()>>),
}

#[derive(Clone)]
pub struct EisClient {
    commands: mpsc::Sender<Command>,
    capabilities: watch::Receiver<Capabilities>,
    closed: watch::Receiver<bool>,
}

impl EisClient {
    pub async fn connect(stream: UnixStream, requested: Requested) -> Result<Self> {
        let (tx, rx) = mpsc::channel(64);
        let (caps_tx, caps_rx) = watch::channel(Capabilities::default());
        let (closed_tx, closed_rx) = watch::channel(false);
        let (setup_tx, setup_rx) = oneshot::channel();
        std::thread::Builder::new()
            .name("semwright-eis".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("EIS runtime");
                runtime.block_on(async move {
                    let setup = async {
                        let context = ei::Context::new(stream).map_err(|e| {
                            Error::new(ErrorCode::BackendFailed, format!("EIS socket: {e}"))
                        })?;
                        let (connection, events) = context
                            .handshake_tokio("semwright", ei::handshake::ContextType::Sender)
                            .await
                            .map_err(|e| {
                                Error::new(ErrorCode::BackendFailed, format!("EIS handshake: {e}"))
                            })?;
                        Ok::<_, Error>((connection, events))
                    }
                    .await;
                    match setup {
                        Ok((connection, events)) => {
                            let _ = setup_tx.send(Ok(()));
                            run(connection, events, rx, caps_tx, closed_tx, requested).await;
                        }
                        Err(error) => {
                            let _ = setup_tx.send(Err(error));
                            closed_tx.send_replace(true);
                        }
                    }
                });
            })
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "Cannot start EIS worker"))?;
        setup_rx
            .await
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "EIS setup task stopped"))??;
        let client = Self {
            commands: tx,
            capabilities: caps_rx,
            closed: closed_rx,
        };
        client.wait_ready(requested).await?;
        Ok(client)
    }

    async fn wait_ready(&self, requested: Requested) -> Result<()> {
        let mut capabilities = self.capabilities.clone();
        let mut closed = self.closed.clone();
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                if *closed.borrow() {
                    return Err(Error::new(
                        ErrorCode::BackendFailed,
                        "EIS disconnected during setup",
                    ));
                }
                if capabilities.borrow().satisfies(requested) {
                    return Ok(());
                }
                tokio::select! {
                    changed = capabilities.changed() => changed.map_err(|_| {
                        Error::new(ErrorCode::BackendFailed, "EIS capability channel closed")
                    })?,
                    changed = closed.changed() => changed.map_err(|_| {
                        Error::new(ErrorCode::BackendFailed, "EIS lifecycle channel closed")
                    })?,
                }
            }
        })
        .await
        .map_err(|_| Error::new(ErrorCode::Timeout, "EIS device negotiation timed out"))?
    }

    #[must_use]
    pub fn capabilities(&self) -> Capabilities {
        *self.capabilities.borrow()
    }

    #[must_use]
    pub fn is_closed(&self) -> bool {
        *self.closed.borrow()
    }

    pub async fn wait_closed(&self) {
        let mut closed = self.closed.clone();
        while !*closed.borrow() {
            if closed.changed().await.is_err() {
                break;
            }
        }
    }

    pub async fn keysym(&self, keysym: u32) -> Result<()> {
        self.send(|reply| Command::KeySym(keysym, reply)).await
    }

    pub async fn type_text(&self, text: &str) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        self.send(|reply| Command::Text(text.to_owned(), reply))
            .await
    }

    pub async fn motion(&self, dx: f64, dy: f64) -> Result<()> {
        let (dx, dy) = (f32_checked(dx)?, f32_checked(dy)?);
        self.send(|reply| Command::Motion(dx, dy, reply)).await
    }

    pub async fn click(&self, button: u32) -> Result<()> {
        self.send(|reply| Command::Button(button, reply)).await
    }

    pub async fn scroll(&self, dx: f64, dy: f64) -> Result<()> {
        let (dx, dy) = (f32_checked(dx)?, f32_checked(dy)?);
        self.send(|reply| Command::Scroll(dx, dy, reply)).await
    }

    pub async fn stop(&self) -> Result<()> {
        self.send(Command::Stop).await
    }
    async fn send(&self, make: impl FnOnce(oneshot::Sender<Result<()>>) -> Command) -> Result<()> {
        if *self.closed.borrow() {
            return Err(Error::new(
                ErrorCode::BackendFailed,
                "EIS transport is closed",
            ));
        }
        let (tx, rx) = oneshot::channel();
        self.commands
            .send(make(tx))
            .await
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "EIS transport task stopped"))?;
        rx.await
            .map_err(|_| Error::new(ErrorCode::BackendFailed, "EIS response was lost"))?
    }
}

fn f32_checked(value: f64) -> Result<f32> {
    if !value.is_finite() || value < f32::MIN as f64 || value > f32::MAX as f64 {
        return Err(Error::invalid("EIS coordinate is outside finite f32 range"));
    }
    Ok(value as f32)
}

fn monotonic_micros() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_micros() as u64
}
#[derive(Clone)]
struct LiveDevice {
    device: Device,
    resumed: bool,
}

fn current_capabilities(devices: &[LiveDevice]) -> Capabilities {
    let mut result = Capabilities::default();
    for live in devices.iter().filter(|d| d.resumed) {
        result.text |= live.device.has_capability(DeviceCapability::Text);
        result.pointer |= live.device.has_capability(DeviceCapability::Pointer);
        result.button |= live.device.has_capability(DeviceCapability::Button);
        result.scroll |= live.device.has_capability(DeviceCapability::Scroll);
    }
    result
}

fn find_device(devices: &[LiveDevice], capability: DeviceCapability) -> Result<&Device> {
    devices
        .iter()
        .find(|d| d.resumed && d.device.has_capability(capability))
        .map(|d| &d.device)
        .ok_or_else(|| Error::new(ErrorCode::Unsupported, "EIS capability is not available"))
}

fn frame(connection: &reis::event::Connection, device: &Device) -> Result<()> {
    device
        .device()
        .frame(connection.serial(), monotonic_micros());
    connection
        .flush()
        .map_err(|e| Error::new(ErrorCode::BackendFailed, format!("EIS flush: {e}")))
}

fn set_resumed(devices: &mut [LiveDevice], device: &Device, resumed: bool) {
    if let Some(live) = devices.iter_mut().find(|d| d.device == *device) {
        live.resumed = resumed;
    }
}

fn bind_requested(seat: &reis::event::Seat, requested: Requested) {
    let mut capabilities = BitFlags::empty();
    if requested.keyboard {
        capabilities.insert(DeviceCapability::Text);
    }
    if requested.pointer {
        capabilities.insert(DeviceCapability::Pointer);
        capabilities.insert(DeviceCapability::Button);
        capabilities.insert(DeviceCapability::Scroll);
    }
    seat.bind_capabilities(capabilities);
}

async fn run(
    connection: reis::event::Connection,
    mut events: reis::tokio::EiConvertEventStream,
    mut commands: mpsc::Receiver<Command>,
    capabilities: watch::Sender<Capabilities>,
    closed: watch::Sender<bool>,
    requested: Requested,
) {
    let mut devices: Vec<LiveDevice> = vec![];
    let mut sequence = 1u32;
    loop {
        tokio::select! {
            event = events.next() => {
                let Some(event) = event else { break };
                let Ok(event) = event else { break };
                match event {
                    EiEvent::Disconnected(_) => break,
                    EiEvent::SeatAdded(event) => {
                        bind_requested(&event.seat, requested);
                        let _ = connection.flush();
                    }
                    EiEvent::DeviceAdded(event) => {
                        if event.device.device().version() >= 3 {
                            event.device.device().ready();
                            let _ = connection.flush();
                        }
                        devices.push(LiveDevice { device: event.device, resumed: false });
                    }
                    EiEvent::DeviceResumed(event) => {
                        event.device.device().start_emulating(connection.serial(), sequence);
                        sequence = sequence.wrapping_add(1).max(1);
                        let _ = connection.flush();
                        set_resumed(&mut devices, &event.device, true);
                    }
                    EiEvent::DevicePaused(event) => {
                        set_resumed(&mut devices, &event.device, false);
                    }
                    EiEvent::DeviceRemoved(event) => {
                        devices.retain(|d| d.device != event.device);
                    }
                    _ => {}
                }
                capabilities.send_replace(current_capabilities(&devices));
            }
            command = commands.recv() => {
                let Some(command) = command else { break };
                if handle_command(&connection, &devices, command) {
                    break;
                }
            }
        }
    }
    connection.connection().disconnect();
    let _ = connection.flush();
    closed.send_replace(true);
}

fn handle_command(
    connection: &reis::event::Connection,
    devices: &[LiveDevice],
    command: Command,
) -> bool {
    match command {
        Command::KeySym(keysym, reply) => {
            let _ = reply.send(send_keysym(connection, devices, keysym));
        }
        Command::Text(text, reply) => {
            let _ = reply.send(send_text(connection, devices, &text));
        }
        Command::Motion(dx, dy, reply) => {
            let _ = reply.send(send_motion(connection, devices, dx, dy));
        }
        Command::Button(button, reply) => {
            let _ = reply.send(send_button(connection, devices, button));
        }
        Command::Scroll(dx, dy, reply) => {
            let _ = reply.send(send_scroll(connection, devices, dx, dy));
        }
        Command::Stop(reply) => {
            let _ = reply.send(Ok(()));
            return true;
        }
    }
    false
}

fn send_keysym(
    connection: &reis::event::Connection,
    devices: &[LiveDevice],
    keysym: u32,
) -> Result<()> {
    let device = find_device(devices, DeviceCapability::Text)?;
    let text = device
        .interface::<ei::Text>()
        .ok_or_else(|| Error::new(ErrorCode::Unsupported, "EIS text interface disappeared"))?;
    text.keysym(keysym, ei::keyboard::KeyState::Press);
    frame(connection, device)?;
    text.keysym(keysym, ei::keyboard::KeyState::Released);
    frame(connection, device)
}

fn send_text(
    connection: &reis::event::Connection,
    devices: &[LiveDevice],
    text_value: &str,
) -> Result<()> {
    let device = find_device(devices, DeviceCapability::Text)?;
    let text = device
        .interface::<ei::Text>()
        .ok_or_else(|| Error::new(ErrorCode::Unsupported, "EIS text interface disappeared"))?;
    for chunk in utf8_chunks(text_value, 254)? {
        text.utf8(chunk);
        frame(connection, device)?;
    }
    Ok(())
}

fn utf8_chunks(value: &str, limit: usize) -> Result<Vec<&str>> {
    if value.is_empty() || limit == 0 {
        return Err(Error::invalid("EIS text chunk is empty"));
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < value.len() {
        let remaining = &value[start..];
        if remaining.len() <= limit {
            chunks.push(remaining);
            break;
        }
        let end = remaining
            .char_indices()
            .map(|(i, _)| i)
            .take_while(|i| *i <= limit)
            .last()
            .filter(|i| *i > 0)
            .ok_or_else(|| Error::invalid("Single UTF-8 scalar exceeds EIS text budget"))?;
        chunks.push(&remaining[..end]);
        start += end;
    }
    Ok(chunks)
}

fn send_motion(
    connection: &reis::event::Connection,
    devices: &[LiveDevice],
    dx: f32,
    dy: f32,
) -> Result<()> {
    let device = find_device(devices, DeviceCapability::Pointer)?;
    let pointer = device
        .interface::<ei::Pointer>()
        .ok_or_else(|| Error::new(ErrorCode::Unsupported, "EIS pointer interface disappeared"))?;
    pointer.motion_relative(dx, dy);
    frame(connection, device)
}
fn send_button(
    connection: &reis::event::Connection,
    devices: &[LiveDevice],
    button: u32,
) -> Result<()> {
    let device = find_device(devices, DeviceCapability::Button)?;
    let interface = device
        .interface::<ei::Button>()
        .ok_or_else(|| Error::new(ErrorCode::Unsupported, "EIS button interface disappeared"))?;
    interface.button(button, ei::button::ButtonState::Press);
    frame(connection, device)?;
    interface.button(button, ei::button::ButtonState::Released);
    frame(connection, device)
}

fn send_scroll(
    connection: &reis::event::Connection,
    devices: &[LiveDevice],
    dx: f32,
    dy: f32,
) -> Result<()> {
    let device = find_device(devices, DeviceCapability::Scroll)?;
    let scroll = device
        .interface::<ei::Scroll>()
        .ok_or_else(|| Error::new(ErrorCode::Unsupported, "EIS scroll interface disappeared"))?;
    scroll.scroll(dx, dy);
    frame(connection, device)?;
    scroll.scroll_stop(u32::from(dx != 0.0), u32::from(dy != 0.0), 0);
    frame(connection, device)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_requirements_are_explicit() {
        let pointer = Capabilities {
            pointer: true,
            button: true,
            scroll: true,
            ..Default::default()
        };
        assert!(pointer.satisfies(Requested {
            keyboard: false,
            pointer: true
        }));
        assert!(!pointer.satisfies(Requested {
            keyboard: true,
            pointer: true
        }));
    }

    #[test]
    fn unicode_chunking_preserves_text_and_protocol_budget() {
        let value = "é".repeat(300);
        let chunks = utf8_chunks(&value, 254).unwrap();
        assert_eq!(chunks.concat(), value);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.len() <= 254 && !chunk.is_empty())
        );
    }

    #[test]
    fn non_finite_pointer_values_are_rejected() {
        assert!(f32_checked(f64::NAN).is_err());
        assert!(f32_checked(f64::INFINITY).is_err());
    }
}
