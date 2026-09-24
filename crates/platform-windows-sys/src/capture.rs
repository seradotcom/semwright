use semwright_types::{Error, ErrorCode, Result};
use windows::{
    Graphics::Capture::GraphicsCaptureItem,
    Win32::{Foundation::HWND, System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop},
};

/// Public, documented desktop interop for Windows.Graphics.Capture.
/// This proves target acquisition without private APIs. Pixel readback is intentionally a
/// separate step and is not advertised until the D3D11 frame path is verified natively.
pub fn item_for_window(hwnd: HWND) -> Result<GraphicsCaptureItem> {
    let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()
        .map_err(|_| {
            Error::new(
                ErrorCode::Unavailable,
                "Windows.Graphics.Capture activation factory unavailable",
            )
        })?;
    // SAFETY: `hwnd` is a caller-revalidated live window handle. CreateForWindow is the
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
