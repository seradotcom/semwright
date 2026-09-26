//! Internal EI/libei sender transport for an already-granted RemoteDesktop fd.
use crate::eis_xkb::{KeyboardMap, ModifierState, Stroke};
use futures_util::StreamExt;
use reis::{
    ei,
    enumflags2::BitFlags,
    event::{Device, DeviceCapability, EiEvent},
};
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use semwright_types::{Error, ErrorCode, Result};
use std::{
    os::unix::net::UnixStream,
    sync::OnceLock,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot, watch};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Requested {
    pub keyboard: bool,
    pub pointer: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub keyboard: bool,
    pub text: bool,
    pub pointer: bool,
    pub button: bool,
    pub scroll: bool,
}

impl Capabilities {
    #[must_use]
    pub fn satisfies(self, requested: Requested) -> bool {
        (!requested.keyboard || self.text || self.keyboard)
            && (!requested.pointer || (self.pointer && self.button && self.scroll))
    }
}
enum Command {
    KeySym(u32, oneshot::Sender<Result<()>>),
    Text(String, CancellationToken, oneshot::Sender<Result<()>>),
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
                        Ok::<_, Error>((context, connection, events))
                    }
                    .await;
                    match setup {
                        Ok((context, connection, events)) => {
                            let _ = setup_tx.send(Ok(()));
                            run(
                                context, connection, events, rx, caps_tx, closed_tx, requested,
                            )
                            .await;
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
        self.type_text_cancellable(text, CancellationToken::new())
            .await
    }

    pub async fn type_text_cancellable(
        &self,
        text: &str,
        cancellation: CancellationToken,
    ) -> Result<()> {
        if text.is_empty() {
            return Ok(());
        }
        if cancellation.is_cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "EIS text input cancelled before dispatch",
            ));
        }
        self.send(|reply| Command::Text(text.to_owned(), cancellation, reply))
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
    modifiers: ModifierState,
}

fn current_capabilities(devices: &[LiveDevice]) -> Capabilities {
    let mut result = Capabilities::default();
    for live in devices.iter().filter(|d| d.resumed) {
        result.keyboard |= live.device.has_capability(DeviceCapability::Keyboard);
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

fn find_native_text_device(devices: &[LiveDevice]) -> Option<&Device> {
    devices
        .iter()
        .find(|d| d.resumed && d.device.has_capability(DeviceCapability::Text))
        .map(|d| &d.device)
}

fn find_keyboard_device(devices: &[LiveDevice]) -> Result<&LiveDevice> {
    devices
        .iter()
        .find(|d| d.resumed && d.device.has_capability(DeviceCapability::Keyboard))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::Unsupported,
                "EIS keyboard capability is not available",
            )
        })
}

const EIS_FLUSH_BACKPRESSURE_TIMEOUT: Duration = Duration::from_millis(500);
const EIS_FLUSH_POLL_SLICE: Duration = Duration::from_millis(10);
const EIS_KEYCODE_TEXT_BATCH_SIZE: usize = 8;
const EIS_KEYCODE_TEXT_BATCH_INTERVAL: Duration = Duration::from_millis(8);

fn frame(
    context: &ei::Context,
    connection: &reis::event::Connection,
    device: &Device,
) -> Result<()> {
    device
        .device()
        .frame(connection.serial(), monotonic_micros());
    flush_with_backpressure(context, connection)
}

fn drain_incoming(context: &ei::Context) -> Result<()> {
    // Nonblocking read only moves socket bytes into reis' internal buffer. The existing
    // EiConvertEventStream remains responsible for parsing and applying those events.
    context.read().map(|_| ()).map_err(|error| {
        Error::new(
            ErrorCode::BackendFailed,
            format!("EIS inbound drain: {error}"),
        )
    })
}

fn flush_with_backpressure(
    context: &ei::Context,
    connection: &reis::event::Connection,
) -> Result<()> {
    let deadline = Instant::now() + EIS_FLUSH_BACKPRESSURE_TIMEOUT;
    loop {
        match connection.flush() {
            Ok(()) => return Ok(()),
            Err(error)
                if error == rustix::io::Errno::AGAIN || error == rustix::io::Errno::WOULDBLOCK =>
            {
                let now = Instant::now();
                if now >= deadline {
                    return Err(Error::new(
                        ErrorCode::Timeout,
                        "EIS flush backpressure timed out",
                    ));
                }
                let wait = deadline
                    .saturating_duration_since(now)
                    .min(EIS_FLUSH_POLL_SLICE);
                let timeout = Timespec {
                    tv_sec: wait.as_secs().try_into().unwrap_or(i64::MAX),
                    tv_nsec: i64::from(wait.subsec_nanos()),
                };
                let mut fds = [PollFd::new(context, PollFlags::OUT)];
                let ready = poll(&mut fds, Some(&timeout)).map_err(|poll_error| {
                    Error::new(
                        ErrorCode::BackendFailed,
                        format!("EIS writable poll: {poll_error}"),
                    )
                })?;
                if ready == 0 {
                    continue;
                }
                let revents = fds[0].revents();
                if revents.intersects(PollFlags::ERR | PollFlags::HUP | PollFlags::NVAL) {
                    return Err(Error::new(
                        ErrorCode::BackendFailed,
                        format!("EIS writable poll failed: {revents:?}"),
                    ));
                }
            }
            Err(error) => {
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    format!("EIS flush: {error}"),
                ));
            }
        }
    }
}

fn set_resumed(devices: &mut [LiveDevice], device: &Device, resumed: bool) {
    if let Some(live) = devices.iter_mut().find(|d| d.device == *device) {
        live.resumed = resumed;
        if !resumed {
            live.modifiers = ModifierState::default();
        }
    }
}

fn set_modifiers(devices: &mut [LiveDevice], event: &reis::event::KeyboardModifiers) {
    if let Some(live) = devices.iter_mut().find(|d| d.device == event.device) {
        live.modifiers = ModifierState {
            depressed: event.depressed,
            latched: event.latched,
            locked: event.locked,
            group: event.group,
        };
    }
}

fn bind_requested(seat: &reis::event::Seat, requested: Requested) {
    let mut capabilities = BitFlags::empty();
    if requested.keyboard {
        // EIS implementations may expose either keycode-based ei_keyboard, text-oriented
        // ei_text, or both. Bind both and preserve the distinction at dispatch time.
        capabilities.insert(DeviceCapability::Keyboard);
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
    context: ei::Context,
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
                        devices.push(LiveDevice {
                            device: event.device,
                            resumed: false,
                            modifiers: ModifierState::default(),
                        });
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
                    EiEvent::KeyboardModifiers(event) => {
                        set_modifiers(&mut devices, &event);
                    }
                    _ => {}
                }
                capabilities.send_replace(current_capabilities(&devices));
            }
            command = commands.recv() => {
                let Some(command) = command else { break };
                if handle_command(&context, &connection, &devices, command) {
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
    context: &ei::Context,
    connection: &reis::event::Connection,
    devices: &[LiveDevice],
    command: Command,
) -> bool {
    match command {
        Command::KeySym(keysym, reply) => {
            let _ = reply.send(send_keysym(context, connection, devices, keysym));
        }
        Command::Text(text, cancellation, reply) => {
            let _ = reply.send(send_text(
                context,
                connection,
                devices,
                &text,
                &cancellation,
            ));
        }
        Command::Motion(dx, dy, reply) => {
            let _ = reply.send(send_motion(context, connection, devices, dx, dy));
        }
        Command::Button(button, reply) => {
            let _ = reply.send(send_button(context, connection, devices, button));
        }
        Command::Scroll(dx, dy, reply) => {
            let _ = reply.send(send_scroll(context, connection, devices, dx, dy));
        }
        Command::Stop(reply) => {
            let _ = reply.send(Ok(()));
            return true;
        }
    }
    false
}

fn send_keysym(
    context: &ei::Context,
    connection: &reis::event::Connection,
    devices: &[LiveDevice],
    keysym: u32,
) -> Result<()> {
    if let Some(device) = find_native_text_device(devices) {
        let text = device
            .interface::<ei::Text>()
            .ok_or_else(|| Error::new(ErrorCode::Unsupported, "EIS text interface disappeared"))?;
        text.keysym(keysym, ei::keyboard::KeyState::Press);
        frame(context, connection, device)?;
        text.keysym(keysym, ei::keyboard::KeyState::Released);
        return frame(context, connection, device);
    }
    let live = find_keyboard_device(devices)?;
    let keymap = KeyboardMap::from_device(&live.device)?.ok_or_else(|| {
        Error::new(
            ErrorCode::Unsupported,
            "EIS keyboard has no XKB keymap for keysym translation",
        )
    })?;
    let stroke = keymap.stroke_for_keysym(keysym, live.modifiers)?;
    send_keyboard_stroke(context, connection, &live.device, &stroke)
}

fn send_text(
    context: &ei::Context,
    connection: &reis::event::Connection,
    devices: &[LiveDevice],
    text_value: &str,
    cancellation: &CancellationToken,
) -> Result<()> {
    if let Some(device) = find_native_text_device(devices) {
        let text = device
            .interface::<ei::Text>()
            .ok_or_else(|| Error::new(ErrorCode::Unsupported, "EIS text interface disappeared"))?;
        for chunk in utf8_chunks(text_value, 254)? {
            check_input_cancelled(cancellation)?;
            text.utf8(chunk);
            frame(context, connection, device)?;
        }
        return Ok(());
    }
    let live = find_keyboard_device(devices)?;
    let keymap = KeyboardMap::from_device(&live.device)?.ok_or_else(|| {
        Error::new(
            ErrorCode::Unsupported,
            "EIS keyboard has no XKB keymap for text translation",
        )
    })?;
    let mut batch_started = Instant::now();
    for (index, character) in text_value.chars().enumerate() {
        check_input_cancelled(cancellation)?;
        let stroke = keymap.stroke_for_char(character, live.modifiers)?;
        send_keyboard_stroke(context, connection, &live.device, &stroke)?;
        if (index + 1) % EIS_KEYCODE_TEXT_BATCH_SIZE == 0 {
            check_input_cancelled(cancellation)?;
            drain_incoming(context)?;
            let elapsed = batch_started.elapsed();
            if elapsed < EIS_KEYCODE_TEXT_BATCH_INTERVAL {
                std::thread::sleep(EIS_KEYCODE_TEXT_BATCH_INTERVAL - elapsed);
            }
            check_input_cancelled(cancellation)?;
            batch_started = Instant::now();
        }
    }
    drain_incoming(context)?;
    Ok(())
}

fn check_input_cancelled(cancellation: &CancellationToken) -> Result<()> {
    if cancellation.is_cancelled() {
        Err(Error::new(
            ErrorCode::Cancelled,
            "EIS text input cancelled during dispatch",
        ))
    } else {
        Ok(())
    }
}

fn send_keyboard_stroke(
    context: &ei::Context,
    connection: &reis::event::Connection,
    device: &Device,
    stroke: &Stroke,
) -> Result<()> {
    let keyboard = device
        .interface::<ei::Keyboard>()
        .ok_or_else(|| Error::new(ErrorCode::Unsupported, "EIS keyboard interface disappeared"))?;
    for modifier in &stroke.modifiers {
        keyboard.key(*modifier, ei::keyboard::KeyState::Press);
        frame(context, connection, device)?;
    }
    keyboard.key(stroke.keycode, ei::keyboard::KeyState::Press);
    frame(context, connection, device)?;
    keyboard.key(stroke.keycode, ei::keyboard::KeyState::Released);
    frame(context, connection, device)?;
    for modifier in stroke.modifiers.iter().rev() {
        keyboard.key(*modifier, ei::keyboard::KeyState::Released);
        frame(context, connection, device)?;
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
    context: &ei::Context,
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
    frame(context, connection, device)
}
fn send_button(
    context: &ei::Context,
    connection: &reis::event::Connection,
    devices: &[LiveDevice],
    button: u32,
) -> Result<()> {
    let device = find_device(devices, DeviceCapability::Button)?;
    let interface = device
        .interface::<ei::Button>()
        .ok_or_else(|| Error::new(ErrorCode::Unsupported, "EIS button interface disappeared"))?;
    interface.button(button, ei::button::ButtonState::Press);
    frame(context, connection, device)?;
    interface.button(button, ei::button::ButtonState::Released);
    frame(context, connection, device)
}

fn send_scroll(
    context: &ei::Context,
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
    frame(context, connection, device)?;
    scroll.scroll_stop(u32::from(dx != 0.0), u32::from(dy != 0.0), 0);
    frame(context, connection, device)
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
