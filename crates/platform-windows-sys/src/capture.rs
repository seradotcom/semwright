use semwright_types::{Error, ErrorCode, Result};
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
    sync::mpsc::{self, SyncSender},
    thread,
    time::{Duration, Instant},
};
use windows::{
    Foundation::TypedEventHandler,
    Graphics::{
        Capture::{
            Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
            GraphicsCapturePicker, GraphicsCaptureSession,
        },
        DirectX::{Direct3D11::IDirect3DDevice, DirectXPixelFormat},
        SizeInt32,
    },
    Win32::{
        Foundation::{HMODULE, HWND},
        Graphics::{
            Direct3D::{D3D_DRIVER_TYPE, D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP},
            Direct3D11::{
                D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAP_READ,
                D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_STAGING, D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext,
                ID3D11Texture2D,
            },
            Dxgi::{
                Common::{
                    DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_SAMPLE_DESC,
                },
                IDXGIAdapter, IDXGIDevice,
            },
        },
        System::WinRT::{
            Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess},
            Graphics::Capture::IGraphicsCaptureItemInterop,
            RO_INIT_SINGLETHREADED, RoInitialize, RoUninitialize,
        },
        UI::{
            Shell::IInitializeWithWindow,
            WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
                TranslateMessage, WM_QUIT, WS_OVERLAPPED,
            },
        },
    },
    core::{IInspectable, Interface, factory, w},
};

const MAX_DIMENSION: u32 = 4096;
const MAX_PIXELS: u64 = 4_194_304;
const MAX_ROW_PITCH: usize = MAX_DIMENSION as usize * 16;
const MAX_PNG_BYTES: usize = 8 * 1024 * 1024;
const PICKER_POLL: Duration = Duration::from_millis(15);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureArtifact {
    pub path: PathBuf,
    pub bytes: usize,
    pub width: u32,
    pub height: u32,
}

struct RoApartment;

impl RoApartment {
    fn sta() -> Result<Self> {
        // SAFETY: this dedicated picker thread owns a single WinRT STA until Drop.
        unsafe { RoInitialize(RO_INIT_SINGLETHREADED) }.map_err(|_| {
            Error::new(
                ErrorCode::Unavailable,
                "Windows capture picker STA initialization failed",
            )
        })?;
        Ok(Self)
    }
}

impl Drop for RoApartment {
    fn drop(&mut self) {
        // SAFETY: balances the successful RoInitialize on this same thread.
        unsafe { RoUninitialize() };
    }
}

struct PickerOwner(HWND);

impl PickerOwner {
    fn new() -> Result<Self> {
        // A hidden top-level STATIC window gives the desktop picker a Semwright-owned HWND.
        // It is never used to infer or steal authority from another foreground application.
        // SAFETY: STATIC is a predefined process-independent class; all arguments are
        // immediate values and no borrowed caller pointer is retained by this hidden owner.
        let hwnd = unsafe {
            CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                w!("Semwright Screen Capture"),
                WS_OVERLAPPED,
                -32_000,
                -32_000,
                1,
                1,
                None,
                None,
                None,
                None,
            )
        }
        .map_err(|_| {
            Error::new(
                ErrorCode::Unavailable,
                "Windows capture picker owner window could not be created",
            )
        })?;
        Ok(Self(hwnd))
    }
}

impl Drop for PickerOwner {
    fn drop(&mut self) {
        // SAFETY: the HWND was created by PickerOwner on this picker thread.
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapturedBgra {
    pub width: u32,
    pub height: u32,
    /// Compact BGRA8 rows. There is no driver row padding in this buffer.
    pub bytes: Vec<u8>,
}

enum FrameSignal {
    Frame(Direct3D11CaptureFrame),
    Closed,
}

struct CaptureLease {
    item: GraphicsCaptureItem,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    frame_token: i64,
    closed_token: i64,
}

impl Drop for CaptureLease {
    fn drop(&mut self) {
        let _ = self.pool.RemoveFrameArrived(self.frame_token);
        let _ = self.item.RemoveClosed(self.closed_token);
        let _ = self.session.Close();
        let _ = self.pool.Close();
    }
}

/// Public documented desktop interop for an explicitly resolved HWND.
///
/// This helper does not itself grant capture authority. The caller must have selected/revalidated
/// the target and policy must have allowed capture before invoking it.
pub fn item_for_window(hwnd: HWND) -> Result<GraphicsCaptureItem> {
    let interop = factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().map_err(|_| {
        Error::new(
            ErrorCode::Unavailable,
            "Windows.Graphics.Capture activation factory unavailable",
        )
    })?;
    // SAFETY: hwnd is a caller-revalidated live window handle. CreateForWindow is the
    // documented desktop interop API and returns an owned WinRT GraphicsCaptureItem.
    unsafe { interop.CreateForWindow::<GraphicsCaptureItem>(hwnd) }.map_err(|_| {
        Error::new(
            ErrorCode::ConsentRequired,
            "Windows.Graphics.Capture item could not be created",
        )
    })
}

pub fn capture_supported() -> bool {
    windows::Graphics::Capture::GraphicsCaptureSession::IsSupported().unwrap_or(false)
}

fn pick_item<F>(timeout: Duration, cancelled: &F) -> Result<GraphicsCaptureItem>
where
    F: Fn() -> bool,
{
    if timeout.is_zero() {
        return Err(Error::new(
            ErrorCode::Timeout,
            "Windows capture picker deadline already expired",
        ));
    }
    let _apartment = RoApartment::sta()?;
    let owner = PickerOwner::new()?;
    let picker = GraphicsCapturePicker::new().map_err(|_| {
        Error::new(
            ErrorCode::Unavailable,
            "Windows GraphicsCapturePicker is unavailable",
        )
    })?;
    let initialize: IInitializeWithWindow = picker.cast().map_err(|_| {
        Error::new(
            ErrorCode::Unavailable,
            "Windows capture picker cannot bind to a desktop owner window",
        )
    })?;
    // SAFETY: owner is a live Semwright-owned top-level HWND on this STA thread.
    unsafe { initialize.Initialize(owner.0) }.map_err(|_| {
        Error::new(
            ErrorCode::Unavailable,
            "Windows capture picker owner initialization failed",
        )
    })?;
    let operation = picker.PickSingleItemAsync().map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows capture picker could not start",
        )
    })?;
    let deadline = Instant::now() + timeout;
    loop {
        if cancelled() {
            let _ = operation.Cancel();
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Windows capture picker cancelled",
            ));
        }
        if Instant::now() >= deadline {
            let _ = operation.Cancel();
            return Err(Error::new(
                ErrorCode::Timeout,
                "Windows capture picker timed out",
            ));
        }
        let status = operation.Status().map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Windows capture picker status unavailable",
            )
        })?;
        match status.0 {
            0 => {}
            1 => {
                return operation.GetResults().map_err(|_| {
                    Error::new(
                        ErrorCode::BackendFailed,
                        "Windows capture picker did not return a selected item",
                    )
                });
            }
            2 => {
                return Err(Error::new(
                    ErrorCode::Cancelled,
                    "Windows capture picker was cancelled by the user",
                ));
            }
            _ => {
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    "Windows capture picker failed",
                ));
            }
        }
        let mut message = MSG::default();
        // The WinRT picker is owned by this STA. Pump only this thread's messages while waiting.
        // SAFETY: message is writable storage owned by this STA thread; no HWND filter is used.
        while unsafe { PeekMessageW(&mut message, None, 0, 0, PM_REMOVE) }.as_bool() {
            if message.message == WM_QUIT {
                let _ = operation.Cancel();
                return Err(Error::new(
                    ErrorCode::Cancelled,
                    "Windows capture picker owner thread exited",
                ));
            }
            // SAFETY: message was initialized by PeekMessageW on this thread.
            unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        thread::sleep(PICKER_POLL.min(deadline.saturating_duration_since(Instant::now())));
    }
}

fn validate_size(width: i32, height: i32) -> Result<(u32, u32)> {
    let width = u32::try_from(width)
        .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Capture width is invalid"))?;
    let height = u32::try_from(height)
        .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Capture height is invalid"))?;
    if width == 0
        || height == 0
        || width > MAX_DIMENSION
        || height > MAX_DIMENSION
        || u64::from(width) * u64::from(height) > MAX_PIXELS
    {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Capture dimensions exceed the bounded pixel budget",
        ));
    }
    Ok((width, height))
}

fn create_device_with(driver: D3D_DRIVER_TYPE) -> Result<(ID3D11Device, ID3D11DeviceContext)> {
    let mut device = None;
    let mut context = None;
    // SAFETY: all out-pointers refer to stack-owned Options, no adapter/software module is used,
    // and the returned COM interfaces own their references.
    unsafe {
        D3D11CreateDevice(
            None::<&IDXGIAdapter>,
            driver,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )
        .map_err(|_| Error::unavailable("D3D11 device creation failed"))?;
    }
    let device = device.ok_or_else(|| {
        Error::new(
            ErrorCode::BackendFailed,
            "D3D11 reported success without returning a device",
        )
    })?;
    let context = context.ok_or_else(|| {
        Error::new(
            ErrorCode::BackendFailed,
            "D3D11 reported success without returning an immediate context",
        )
    })?;
    Ok((device, context))
}

fn create_device() -> Result<(ID3D11Device, ID3D11DeviceContext, IDirect3DDevice)> {
    let (device, context) = create_device_with(D3D_DRIVER_TYPE_HARDWARE)
        .or_else(|_| create_device_with(D3D_DRIVER_TYPE_WARP))?;
    let dxgi: IDXGIDevice = device
        .cast()
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "D3D11 device is not DXGI"))?;
    // SAFETY: dxgi is an owned live IDXGIDevice created above.
    let inspectable = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "D3D11 device could not be wrapped for Windows.Graphics.Capture",
        )
    })?;
    let winrt: IDirect3DDevice = inspectable.cast().map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "WinRT Direct3D device projection failed",
        )
    })?;
    Ok((device, context, winrt))
}

fn send_frame(sender: &SyncSender<FrameSignal>, frame: Direct3D11CaptureFrame) {
    let _ = sender.try_send(FrameSignal::Frame(frame));
}

fn compact_bgra_rows(source: &[u8], row_pitch: usize, width: u32, height: u32) -> Result<Vec<u8>> {
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(4))
        .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Capture row size overflow"))?;
    let rows = usize::try_from(height)
        .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Capture height overflow"))?;
    if row_pitch < row_bytes || row_pitch > MAX_ROW_PITCH {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "D3D11 capture row pitch is outside the bounded contract",
        ));
    }
    let required = row_pitch
        .checked_mul(rows)
        .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Capture map size overflow"))?;
    if source.len() < required {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "D3D11 mapped capture buffer is shorter than its row pitch",
        ));
    }
    let capacity = row_bytes
        .checked_mul(rows)
        .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Capture buffer size overflow"))?;
    let mut out = Vec::with_capacity(capacity);
    for row in 0..rows {
        let start = row * row_pitch;
        out.extend_from_slice(&source[start..start + row_bytes]);
    }
    Ok(out)
}

fn read_frame(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    frame: &Direct3D11CaptureFrame,
) -> Result<CapturedBgra> {
    let content = frame.ContentSize().map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows capture frame content size unavailable",
        )
    })?;
    let (width, height) = validate_size(content.Width, content.Height)?;
    let surface = frame.Surface().map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows capture frame surface unavailable",
        )
    })?;
    let access: IDirect3DDxgiInterfaceAccess = surface.cast().map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows capture surface has no DXGI interface access",
        )
    })?;
    // SAFETY: the WinRT surface owns a D3D11 texture for the lifetime of frame.
    let source: ID3D11Texture2D = unsafe { access.GetInterface() }.map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows capture surface is not a D3D11 texture",
        )
    })?;
    let mut source_desc = D3D11_TEXTURE2D_DESC::default();
    // SAFETY: source_desc is writable and source is a live COM interface.
    unsafe { source.GetDesc(&mut source_desc) };
    if source_desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM
        && source_desc.Format != DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
    {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "Windows capture returned an unexpected pixel format",
        ));
    }
    if width > source_desc.Width || height > source_desc.Height {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Windows capture content size exceeds the backing texture",
        ));
    }
    validate_size(source_desc.Width as i32, source_desc.Height as i32)?;

    let staging_desc = D3D11_TEXTURE2D_DESC {
        Width: source_desc.Width,
        Height: source_desc.Height,
        MipLevels: 1,
        ArraySize: 1,
        Format: source_desc.Format,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
        MiscFlags: 0,
    };
    let mut staging = None;
    // SAFETY: descriptor is fully initialized, initial data is absent, and the out-pointer is
    // stack-owned.
    unsafe { device.CreateTexture2D(&staging_desc, None, Some(&mut staging)) }.map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "D3D11 capture staging texture creation failed",
        )
    })?;
    let staging = staging.ok_or_else(|| {
        Error::new(
            ErrorCode::BackendFailed,
            "D3D11 capture staging texture was not returned",
        )
    })?;

    // SAFETY: source/staging are live resources on the same D3D11 device and have compatible
    // dimensions/format.
    unsafe { context.CopyResource(&staging, &source) };

    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: the staging resource was created CPU-readable and mapped is writable.
    unsafe { context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)) }.map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "D3D11 capture staging texture map failed",
        )
    })?;

    let row_pitch = mapped.RowPitch as usize;
    let mapped_len = row_pitch
        .checked_mul(source_desc.Height as usize)
        .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Capture map size overflow"));
    let bytes = match mapped_len {
        Ok(mapped_len) if mapped_len <= MAX_ROW_PITCH * MAX_DIMENSION as usize => {
            // SAFETY: D3D11 Map guarantees at least RowPitch * texture-height accessible bytes
            // until Unmap. Bounds are validated before constructing the slice.
            let source_bytes =
                unsafe { std::slice::from_raw_parts(mapped.pData.cast::<u8>(), mapped_len) };
            compact_bgra_rows(source_bytes, row_pitch, width, height)
        }
        Ok(_) => Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Capture mapped buffer exceeds the bounded contract",
        )),
        Err(error) => Err(error),
    };
    // SAFETY: this exactly balances the successful Map above.
    unsafe { context.Unmap(&staging, 0) };
    Ok(CapturedBgra {
        width,
        height,
        bytes: bytes?,
    })
}

/// Capture one bounded BGRA8 frame from an already-authorized capture item.
///
/// The function owns the frame/session lifetimes, removes event handlers on every exit path and
/// times out rather than blocking the broker indefinitely.
fn capture_item_once_cancellable<F>(
    item: &GraphicsCaptureItem,
    timeout: Duration,
    cancelled: &F,
) -> Result<CapturedBgra>
where
    F: Fn() -> bool,
{
    if !capture_supported() {
        return Err(Error::unavailable(
            "Windows.Graphics.Capture is not supported on this device",
        ));
    }
    let size = item.Size().map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows capture item size unavailable",
        )
    })?;
    let _ = validate_size(size.Width, size.Height)?;
    let (device, context, winrt_device) = create_device()?;
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
        &winrt_device,
        DirectXPixelFormat::B8G8R8A8UIntNormalized,
        1,
        SizeInt32 {
            Width: size.Width,
            Height: size.Height,
        },
    )
    .map_err(|_| Error::unavailable("Windows capture frame pool creation failed"))?;

    let (tx, rx) = mpsc::sync_channel::<FrameSignal>(2);
    let frame_tx = tx.clone();
    let frame_handler =
        TypedEventHandler::<Direct3D11CaptureFramePool, IInspectable>::new(move |sender, _| {
            if let Some(sender) = sender.as_ref()
                && let Ok(frame) = sender.TryGetNextFrame()
            {
                send_frame(&frame_tx, frame);
            }
            Ok(())
        });
    let frame_token = pool
        .FrameArrived(&frame_handler)
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "FrameArrived registration failed"))?;

    let closed_tx = tx.clone();
    let closed_handler =
        TypedEventHandler::<GraphicsCaptureItem, IInspectable>::new(move |_, _| {
            let _ = closed_tx.try_send(FrameSignal::Closed);
            Ok(())
        });
    let closed_token = item.Closed(&closed_handler).map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Capture Closed registration failed",
        )
    })?;

    let session = pool.CreateCaptureSession(item).map_err(|_| {
        Error::new(
            ErrorCode::BackendFailed,
            "Windows capture session creation failed",
        )
    })?;
    let _ = session.SetIsCursorCaptureEnabled(false);
    session
        .StartCapture()
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "Windows capture could not start"))?;

    let _lease = CaptureLease {
        item: item.clone(),
        pool,
        session,
        frame_token,
        closed_token,
    };
    let deadline = Instant::now() + timeout;
    loop {
        if cancelled() {
            return Err(Error::new(
                ErrorCode::Cancelled,
                "Windows capture cancelled while waiting for a frame",
            ));
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(Error::new(
                ErrorCode::Timeout,
                "Windows capture timed out waiting for the first frame",
            ));
        }
        match rx.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(FrameSignal::Frame(frame)) => return read_frame(&device, &context, &frame),
            Ok(FrameSignal::Closed) => {
                return Err(Error::new(
                    ErrorCode::Unavailable,
                    "Windows capture target closed before a frame arrived",
                ));
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(Error::new(
                    ErrorCode::BackendFailed,
                    "Windows capture frame channel disconnected",
                ));
            }
        }
    }
}

pub fn capture_item_once(item: &GraphicsCaptureItem, timeout: Duration) -> Result<CapturedBgra> {
    capture_item_once_cancellable(item, timeout, &|| false)
}

fn bgra_to_rgba(frame: &CapturedBgra) -> Result<Vec<u8>> {
    let expected = usize::try_from(frame.width)
        .ok()
        .and_then(|width| width.checked_mul(frame.height as usize))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "Capture pixel buffer overflow",
            )
        })?;
    if frame.bytes.len() != expected {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Windows capture BGRA buffer length does not match dimensions",
        ));
    }
    let mut rgba = frame.bytes.clone();
    let (pixels, remainder) = rgba.as_chunks_mut::<4>();
    debug_assert!(remainder.is_empty());
    for pixel in pixels {
        pixel.swap(0, 2);
    }
    Ok(rgba)
}

fn encode_png(frame: &CapturedBgra) -> Result<Vec<u8>> {
    let rgba = bgra_to_rgba(frame)?;
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, frame.width, frame.height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Windows capture PNG header failed",
            )
        })?;
        writer.write_image_data(&rgba).map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Windows capture PNG encoding failed",
            )
        })?;
    }
    if encoded.is_empty() || encoded.len() > MAX_PNG_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Windows capture PNG exceeds the bounded artifact budget",
        ));
    }
    Ok(encoded)
}

fn write_artifact(root: &Path, frame: &CapturedBgra, encoded: &[u8]) -> Result<CaptureArtifact> {
    crate::paths::ensure_owner_only_directory(root)?;
    let path = root.join(format!("capture-{}.png", uuid::Uuid::new_v4().simple()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|_| {
            Error::new(
                ErrorCode::BackendFailed,
                "Windows capture artifact could not be created",
            )
        })?;
    let write_result = (|| -> std::io::Result<()> {
        file.write_all(encoded)?;
        file.sync_all()
    })();
    if write_result.is_err() {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Windows capture artifact write failed",
        ));
    }
    Ok(CaptureArtifact {
        path,
        bytes: encoded.len(),
        width: frame.width,
        height: frame.height,
    })
}

/// Request explicit end-user selection through the system WGC picker, capture one bounded frame,
/// and persist it as a private PNG artifact. Selection and first-frame acquisition share one
/// deadline; cancellation never falls back to a non-consensual HWND capture path.
pub fn pick_and_capture_artifact<F>(
    root: &Path,
    timeout: Duration,
    cancelled: F,
) -> Result<CaptureArtifact>
where
    F: Fn() -> bool,
{
    if !capture_supported() {
        return Err(Error::unavailable(
            "Windows.Graphics.Capture is not supported on this device",
        ));
    }
    let started = Instant::now();
    let item = pick_item(timeout, &cancelled)?;
    if cancelled() {
        return Err(Error::new(
            ErrorCode::Cancelled,
            "Windows capture cancelled after item selection",
        ));
    }
    let remaining = timeout.saturating_sub(started.elapsed());
    if remaining.is_zero() {
        return Err(Error::new(
            ErrorCode::Timeout,
            "Windows capture deadline expired after item selection",
        ));
    }
    let frame = capture_item_once_cancellable(&item, remaining, &cancelled)?;
    if cancelled() {
        return Err(Error::new(
            ErrorCode::Cancelled,
            "Windows capture cancelled before artifact creation",
        ));
    }
    let encoded = encode_png(&frame)?;
    if cancelled() {
        return Err(Error::new(
            ErrorCode::Cancelled,
            "Windows capture cancelled before artifact write",
        ));
    }
    write_artifact(root, &frame, &encoded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_are_bounded() {
        assert_eq!(validate_size(1, 1).unwrap(), (1, 1));
        assert_eq!(validate_size(2048, 2048).unwrap(), (2048, 2048));
        for (w, h) in [(0, 1), (1, 0), (-1, 1), (4097, 1), (1, 4097), (4096, 4096)] {
            assert!(validate_size(w, h).is_err(), "{w}x{h}");
        }
    }

    #[test]
    fn row_pitch_is_compacted_without_copying_padding() {
        let source = [
            1, 2, 3, 4, 5, 6, 7, 8, 99, 99, 99, 99, 9, 10, 11, 12, 13, 14, 15, 16, 88, 88, 88, 88,
        ];
        let compact = compact_bgra_rows(&source, 12, 2, 2).unwrap();
        assert_eq!(
            compact,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]
        );
    }

    #[test]
    fn malformed_row_pitch_fails_closed() {
        assert!(compact_bgra_rows(&[0; 8], 3, 2, 1).is_err());
        assert!(compact_bgra_rows(&[0; 8], 8, 2, 2).is_err());
    }

    #[test]
    fn bgra_is_normalized_to_rgba_before_png_encoding() {
        let frame = CapturedBgra {
            width: 2,
            height: 1,
            bytes: vec![1, 2, 3, 4, 10, 20, 30, 40],
        };
        assert_eq!(
            bgra_to_rgba(&frame).unwrap(),
            vec![3, 2, 1, 4, 30, 20, 10, 40]
        );
        let png = encode_png(&frame).unwrap();
        assert!(png.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]));
        assert!(png.len() <= MAX_PNG_BYTES);
    }

    #[test]
    fn private_artifact_is_created_with_bounded_metadata() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().join("artifacts");
        let frame = CapturedBgra {
            width: 1,
            height: 1,
            bytes: vec![7, 8, 9, 255],
        };
        let encoded = encode_png(&frame).unwrap();
        let artifact = write_artifact(&root, &frame, &encoded).unwrap();
        assert_eq!(artifact.width, 1);
        assert_eq!(artifact.height, 1);
        assert_eq!(artifact.bytes, encoded.len());
        assert!(artifact.path.starts_with(&root));
        assert!(artifact.path.is_file());
        assert_eq!(std::fs::read(&artifact.path).unwrap(), encoded);
    }
}
