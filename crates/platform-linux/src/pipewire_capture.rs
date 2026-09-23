//! Bounded PipeWire frame capture for an already-authorized portal remote.
//!
//! Authorization remains owned by the desktop portal. This module consumes
//! only the restricted fd returned by OpenPipeWireRemote.
use pipewire as pw;
use pw::{properties::properties, spa};
use semwright_types::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    io::Write,
    os::{fd::OwnedFd, unix::fs::OpenOptionsExt},
    path::Path,
    rc::Rc,
    time::{Duration, Instant},
};
use tokio_util::sync::CancellationToken;

const MAX_DIMENSION: u32 = 16_384;
const MAX_FRAME_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamTarget {
    /// ScreenCast v6+ pipewire-serial / PipeWire object.serial.
    Serial(u64),
    /// Compatibility fallback for portals older than v6. Node ids may be reused.
    LegacyNode(u32),
}

impl StreamTarget {
    pub fn validate(self) -> Result<Self> {
        match self {
            Self::Serial(0) | Self::LegacyNode(0) => {
                Err(Error::invalid("PipeWire stream target is invalid"))
            }
            _ => Ok(self),
        }
    }

    #[must_use]
    pub fn stable_identity(self) -> bool {
        matches!(self, Self::Serial(_))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PixelFormat {
    Rgba,
    Bgra,
    Rgbx,
    Bgrx,
    Rgb,
    Bgr,
}

impl PixelFormat {
    fn from_spa(format: spa::param::video::VideoFormat) -> Result<Self> {
        use spa::param::video::VideoFormat as F;
        if format == F::RGBA {
            Ok(Self::Rgba)
        } else if format == F::BGRA {
            Ok(Self::Bgra)
        } else if format == F::RGBx {
            Ok(Self::Rgbx)
        } else if format == F::BGRx {
            Ok(Self::Bgrx)
        } else if format == F::RGB {
            Ok(Self::Rgb)
        } else if format == F::BGR {
            Ok(Self::Bgr)
        } else {
            Err(Error::new(
                ErrorCode::Unavailable,
                format!("Unsupported PipeWire raw-video format: {format:?}"),
            ))
        }
    }

    #[must_use]
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            Self::Rgba | Self::Bgra | Self::Rgbx | Self::Bgrx => 4,
            Self::Rgb | Self::Bgr => 3,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub row_bytes: u32,
    pub format: PixelFormat,
    pub data: Vec<u8>,
}
fn checked_frame_layout(
    width: u32,
    height: u32,
    stride: i32,
    format: PixelFormat,
) -> Result<(usize, usize)> {
    if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "PipeWire frame dimensions are outside the capture budget",
        ));
    }
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(format.bytes_per_pixel()))
        .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Frame row size overflow"))?;
    let source_stride = if stride == 0 {
        row_bytes
    } else {
        usize::try_from(stride)
            .ok()
            .filter(|value| *value >= row_bytes)
            .ok_or_else(|| Error::invalid("PipeWire frame stride is invalid"))?
    };
    let rows = usize::try_from(height)
        .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Frame row count overflow"))?;
    let output = row_bytes
        .checked_mul(rows)
        .filter(|bytes| *bytes <= MAX_FRAME_BYTES)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::ResourceExhausted,
                "PipeWire frame exceeds the capture byte budget",
            )
        })?;
    Ok((source_stride, output))
}

fn copy_mapped_frame(
    mapped: &[u8],
    offset: u32,
    chunk_size: u32,
    stride: i32,
    width: u32,
    height: u32,
    format: PixelFormat,
) -> Result<CapturedFrame> {
    let (source_stride, output_len) = checked_frame_layout(width, height, stride, format)?;
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|w| w.checked_mul(format.bytes_per_pixel()))
        .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Frame row size overflow"))?;
    let offset = usize::try_from(offset)
        .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Frame offset overflow"))?;
    let chunk_size = usize::try_from(chunk_size)
        .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Frame size overflow"))?;
    let rows = usize::try_from(height)
        .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Frame row count overflow"))?;
    let required = source_stride
        .checked_mul(rows.saturating_sub(1))
        .and_then(|prefix| prefix.checked_add(row_bytes))
        .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "Frame layout overflow"))?;
    if required > chunk_size {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "PipeWire buffer is shorter than the negotiated frame layout",
        ));
    }
    let end = offset
        .checked_add(required)
        .filter(|end| *end <= mapped.len())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::BackendFailed,
                "PipeWire buffer bounds are invalid",
            )
        })?;
    let source = &mapped[offset..end];
    let mut output = Vec::with_capacity(output_len);
    for row in 0..rows {
        let start = row * source_stride;
        output.extend_from_slice(&source[start..start + row_bytes]);
    }
    Ok(CapturedFrame {
        width,
        height,
        row_bytes: u32::try_from(row_bytes)
            .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Frame row size overflow"))?,
        format,
        data: output,
    })
}

struct ListenerData {
    format: spa::param::video::VideoInfoRaw,
    negotiated: bool,
}

fn pw_error(context: &str, error: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::BackendFailed, format!("{context}: {error}"))
}
/// Capture one CPU-mappable raw frame from an already-authorized portal PipeWire fd.
pub fn capture_one(
    remote_fd: OwnedFd,
    target: StreamTarget,
    timeout: Duration,
    cancellation: CancellationToken,
) -> Result<CapturedFrame> {
    let target = target.validate()?;
    if timeout.is_zero() || timeout > Duration::from_secs(30) {
        return Err(Error::invalid("PipeWire capture timeout is outside bounds"));
    }

    pw::init();
    let mainloop = pw::main_loop::MainLoopRc::new(None)
        .map_err(|error| pw_error("PipeWire main loop", error))?;
    let context = pw::context::ContextRc::new(&mainloop, None)
        .map_err(|error| pw_error("PipeWire context", error))?;
    let core = context
        .connect_fd_rc(remote_fd, None)
        .map_err(|error| pw_error("PipeWire portal remote", error))?;

    let mut props = properties! {
        *pw::keys::MEDIA_TYPE => "Video",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Screen",
    };
    let legacy_node = match target {
        StreamTarget::Serial(serial) => {
            props.insert((*pw::keys::TARGET_OBJECT).as_bytes(), serial.to_string());
            None
        }
        StreamTarget::LegacyNode(node) => Some(node),
    };
    let stream = pw::stream::StreamRc::new(core, "semwright-screencast", props)
        .map_err(|error| pw_error("PipeWire stream", error))?;

    let outcome: Rc<RefCell<Option<Result<CapturedFrame>>>> = Rc::new(RefCell::new(None));
    let outcome_state = Rc::clone(&outcome);
    let outcome_process = Rc::clone(&outcome);
    let loop_state = mainloop.clone();
    let loop_process = mainloop.clone();

    let listener = stream
        .add_local_listener_with_user_data(ListenerData {
            format: Default::default(),
            negotiated: false,
        })
        .state_changed(move |_, _, _, new| {
            if let pw::stream::StreamState::Error(message) = new {
                if outcome_state.borrow().is_none() {
                    *outcome_state.borrow_mut() = Some(Err(Error::new(
                        ErrorCode::BackendFailed,
                        format!("PipeWire stream entered error state: {message}"),
                    )));
                }
                loop_state.quit();
            }
        })
        .param_changed(|_, user_data, id, param| {
            if id != spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Some(param) = param else {
                user_data.negotiated = false;
                return;
            };
            let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param)
            else {
                user_data.negotiated = false;
                return;
            };
            if media_type != spa::param::format::MediaType::Video
                || media_subtype != spa::param::format::MediaSubtype::Raw
            {
                user_data.negotiated = false;
                return;
            }
            user_data.negotiated = user_data.format.parse(param).is_ok()
                && PixelFormat::from_spa(user_data.format.format()).is_ok();
        })
        .process(move |stream, user_data| {
            if !user_data.negotiated || outcome_process.borrow().is_some() {
                return;
            }
            let Some(mut buffer) = stream.dequeue_buffer() else {
                return;
            };
            let Some(data) = buffer.datas_mut().first_mut() else {
                return;
            };
            let offset = data.chunk().offset();
            let size = data.chunk().size();
            let stride = data.chunk().stride();
            if data
                .chunk()
                .flags()
                .contains(spa::buffer::ChunkFlags::CORRUPTED)
            {
                *outcome_process.borrow_mut() = Some(Err(Error::new(
                    ErrorCode::BackendFailed,
                    "PipeWire delivered a corrupted frame",
                )));
                loop_process.quit();
                return;
            }
            let result = match data.data() {
                Some(mapped) => {
                    PixelFormat::from_spa(user_data.format.format()).and_then(|format| {
                        let dimensions = user_data.format.size();
                        copy_mapped_frame(
                            mapped,
                            offset,
                            size,
                            stride,
                            dimensions.width,
                            dimensions.height,
                            format,
                        )
                    })
                }
                None => Err(Error::new(
                    ErrorCode::Unavailable,
                    "PipeWire frame is not CPU-mappable; DMA-BUF capture is not enabled",
                )),
            };
            *outcome_process.borrow_mut() = Some(result);
            loop_process.quit();
        })
        .register()
        .map_err(|error| pw_error("PipeWire listener", error))?;

    let timeout_outcome = Rc::clone(&outcome);
    let timeout_loop = mainloop.clone();
    let deadline = Instant::now() + timeout;
    let timer = mainloop.loop_().add_timer(move |_| {
        if timeout_outcome.borrow().is_some() {
            return;
        }
        let error = if cancellation.is_cancelled() {
            Some(Error::new(
                ErrorCode::Cancelled,
                "PipeWire frame capture cancelled",
            ))
        } else if Instant::now() >= deadline {
            Some(Error::new(
                ErrorCode::Timeout,
                "PipeWire frame capture timed out",
            ))
        } else {
            None
        };
        if let Some(error) = error {
            *timeout_outcome.borrow_mut() = Some(Err(error));
            timeout_loop.quit();
        }
    });
    timer
        .update_timer(
            Some(Duration::from_millis(20)),
            Some(Duration::from_millis(20)),
        )
        .into_result()
        .map_err(|error| pw_error("PipeWire lifecycle timer", error))?;

    let format_object = spa::pod::object!(
        spa::utils::SpaTypes::ObjectParamFormat,
        spa::param::ParamType::EnumFormat,
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaType,
            Id,
            spa::param::format::MediaType::Video
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::MediaSubtype,
            Id,
            spa::param::format::MediaSubtype::Raw
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFormat,
            Choice,
            Enum,
            Id,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::BGRA,
            spa::param::video::VideoFormat::BGRx,
            spa::param::video::VideoFormat::RGBA,
            spa::param::video::VideoFormat::RGBx,
            spa::param::video::VideoFormat::BGR,
            spa::param::video::VideoFormat::RGB,
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoSize,
            Choice,
            Range,
            Rectangle,
            spa::utils::Rectangle {
                width: 1920,
                height: 1080
            },
            spa::utils::Rectangle {
                width: 1,
                height: 1
            },
            spa::utils::Rectangle {
                width: MAX_DIMENSION,
                height: MAX_DIMENSION
            }
        ),
        spa::pod::property!(
            spa::param::format::FormatProperties::VideoFramerate,
            Choice,
            Range,
            Fraction,
            spa::utils::Fraction { num: 60, denom: 1 },
            spa::utils::Fraction { num: 0, denom: 1 },
            spa::utils::Fraction { num: 240, denom: 1 }
        ),
    );
    let values = spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &spa::pod::Value::Object(format_object),
    )
    .map_err(|error| pw_error("PipeWire format serialization", error))?
    .0
    .into_inner();
    let format = spa::pod::Pod::from_bytes(&values)
        .ok_or_else(|| Error::new(ErrorCode::Internal, "Invalid generated PipeWire format pod"))?;
    let mut params = [format];
    stream
        .connect(
            spa::utils::Direction::Input,
            legacy_node,
            pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
            &mut params,
        )
        .map_err(|error| pw_error("PipeWire stream connect", error))?;

    mainloop.run();
    drop(listener);
    drop(timer);
    drop(stream);
    outcome.borrow_mut().take().unwrap_or_else(|| {
        Err(Error::new(
            ErrorCode::BackendFailed,
            "PipeWire capture stopped",
        ))
    })
}

fn packed_png_pixels(frame: &CapturedFrame) -> Result<(png::ColorType, Vec<u8>)> {
    let pixels = usize::try_from(frame.width)
        .ok()
        .and_then(|width| {
            usize::try_from(frame.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "PNG pixel count overflow"))?;
    let expected = usize::try_from(frame.row_bytes)
        .ok()
        .and_then(|row| {
            usize::try_from(frame.height)
                .ok()
                .and_then(|height| row.checked_mul(height))
        })
        .ok_or_else(|| Error::new(ErrorCode::ResourceExhausted, "PNG frame size overflow"))?;
    if frame.data.len() != expected {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Captured frame data length does not match its packed layout",
        ));
    }
    match frame.format {
        PixelFormat::Rgba => Ok((png::ColorType::Rgba, frame.data.clone())),
        PixelFormat::Rgb => Ok((png::ColorType::Rgb, frame.data.clone())),
        PixelFormat::Bgra | PixelFormat::Rgbx | PixelFormat::Bgrx => {
            let mut output =
                Vec::with_capacity(pixels.checked_mul(4).ok_or_else(|| {
                    Error::new(ErrorCode::ResourceExhausted, "PNG buffer overflow")
                })?);
            for pixel in frame.data.as_chunks::<4>().0 {
                match frame.format {
                    PixelFormat::Bgra => {
                        output.extend_from_slice(&[pixel[2], pixel[1], pixel[0], pixel[3]])
                    }
                    PixelFormat::Rgbx => {
                        output.extend_from_slice(&[pixel[0], pixel[1], pixel[2], 255])
                    }
                    PixelFormat::Bgrx => {
                        output.extend_from_slice(&[pixel[2], pixel[1], pixel[0], 255])
                    }
                    _ => unreachable!(),
                }
            }
            Ok((png::ColorType::Rgba, output))
        }
        PixelFormat::Bgr => {
            let mut output =
                Vec::with_capacity(pixels.checked_mul(3).ok_or_else(|| {
                    Error::new(ErrorCode::ResourceExhausted, "PNG buffer overflow")
                })?);
            for pixel in frame.data.as_chunks::<3>().0 {
                output.extend_from_slice(&[pixel[2], pixel[1], pixel[0]]);
            }
            Ok((png::ColorType::Rgb, output))
        }
    }
}

/// Encode a packed captured frame to a new mode-0600 PNG artifact.
pub fn write_png_create_new(frame: &CapturedFrame, destination: &Path) -> Result<()> {
    let (color, pixels) = packed_png_pixels(frame)?;
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, frame.width, frame.height);
        encoder.set_color(color);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|error| pw_error("PNG header", error))?;
        writer
            .write_image_data(&pixels)
            .map_err(|error| pw_error("PNG image", error))?;
        writer
            .finish()
            .map_err(|error| pw_error("PNG finalize", error))?;
    }
    if encoded.len() > MAX_FRAME_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Encoded PNG exceeds the artifact budget",
        ));
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(destination)?;
    if let Err(error) = file.write_all(&encoded).and_then(|_| file.sync_all()) {
        let _ = std::fs::remove_file(destination);
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_identity_prefers_serial_and_rejects_zero() {
        assert!(StreamTarget::Serial(42).stable_identity());
        assert!(!StreamTarget::LegacyNode(42).stable_identity());
        assert!(StreamTarget::Serial(0).validate().is_err());
        assert!(StreamTarget::LegacyNode(0).validate().is_err());
    }

    #[test]
    fn packed_frame_copy_removes_stride_padding() {
        let source = [1, 2, 3, 4, 5, 6, 0, 0, 7, 8, 9, 10, 11, 12, 0, 0];
        let frame = copy_mapped_frame(&source, 0, 16, 8, 2, 2, PixelFormat::Rgb).unwrap();
        assert_eq!(frame.row_bytes, 6);
        assert_eq!(frame.data, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn mapped_frame_bounds_fail_closed() {
        assert!(copy_mapped_frame(&[0; 8], 0, 8, 8, 2, 2, PixelFormat::Rgb).is_err());
        assert!(copy_mapped_frame(&[0; 64], 61, 4, 0, 1, 1, PixelFormat::Rgba).is_err());
        assert!(checked_frame_layout(MAX_DIMENSION + 1, 1, 0, PixelFormat::Rgba).is_err());
        assert!(checked_frame_layout(1, 1, -1, PixelFormat::Rgba).is_err());
    }

    #[test]
    fn packed_format_sizes_are_explicit() {
        assert_eq!(PixelFormat::Rgba.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Bgra.bytes_per_pixel(), 4);
        assert_eq!(PixelFormat::Rgb.bytes_per_pixel(), 3);
        assert_eq!(PixelFormat::Bgr.bytes_per_pixel(), 3);
    }

    #[test]
    fn png_conversion_normalizes_bgr_and_x_channels() {
        let bgr = CapturedFrame {
            width: 1,
            height: 1,
            row_bytes: 3,
            format: PixelFormat::Bgr,
            data: vec![3, 2, 1],
        };
        let (kind, bytes) = packed_png_pixels(&bgr).unwrap();
        assert_eq!(kind, png::ColorType::Rgb);
        assert_eq!(bytes, vec![1, 2, 3]);

        let rgbx = CapturedFrame {
            width: 1,
            height: 1,
            row_bytes: 4,
            format: PixelFormat::Rgbx,
            data: vec![1, 2, 3, 99],
        };
        let (kind, bytes) = packed_png_pixels(&rgbx).unwrap();
        assert_eq!(kind, png::ColorType::Rgba);
        assert_eq!(bytes, vec![1, 2, 3, 255]);
    }

    #[test]
    fn png_artifact_is_private_and_well_formed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frame.png");
        let frame = CapturedFrame {
            width: 2,
            height: 1,
            row_bytes: 8,
            format: PixelFormat::Rgba,
            data: vec![255, 0, 0, 255, 0, 0, 0, 255],
        };
        write_png_create_new(&frame, &path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(write_png_create_new(&frame, &path).is_err());
    }
}
