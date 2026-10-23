//! Portable, closed native-video observation data. No files, process, backend,
//! provider, Store or approval authority is available here. Consumers must bind
//! the original receipt and reopen every retained artifact independently.
use crate::{Error, Result};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The report profile describes observed data, never source admission authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservationProfile {
    MotionCanvasFfv1,
    MltVisualFfv1,
    MltH264AacMaster,
}
impl ObservationProfile {
    pub fn wire_name(self) -> &'static str {
        match self {
            Self::MotionCanvasFfv1 => "motion-canvas-ffv1-rec709-v1",
            Self::MltVisualFfv1 => "mlt-ffv1-visual-rec709-v1",
            Self::MltH264AacMaster => "mlt-h264-aac-master-rec709-v1",
        }
    }
    pub fn codec(self) -> &'static str {
        if self == Self::MltH264AacMaster {
            "h264"
        } else {
            "ffv1"
        }
    }
    pub fn audio_streams(self) -> u64 {
        u64::from(self == Self::MltH264AacMaster)
    }
    pub fn is_native_mlt(self) -> bool {
        self != Self::MotionCanvasFfv1
    }
}
pub const MLT_SOURCE_POLICY: &str = "mlt-render-project-bytes-v1";
pub const MLT_RECIPE: &str = "mlt-native-processing-no-additional-transfer-conversion-v1";
pub const READBACK_FILTER_LEFT: &str = "zscale=pin=709:tin=709:min=709:rin=limited:p=709:t=709:m=gbr:r=full:dither=none:agamma=0:chromalin=left,format=rgba";

/// The source pix_fmt and chroma location must come from the actual decoder.
/// 4:4:4 has no chroma subsampling. Other admitted 8-bit formats require an
/// observed left chroma site and set that input location explicitly in zscale.
pub fn readback_filter(
    profile: ObservationProfile,
    pixel_format: &str,
    chroma_location: Option<&str>,
) -> Result<&'static str> {
    match (profile, pixel_format) {
        (ObservationProfile::MotionCanvasFfv1, "yuv444p")
        | (ObservationProfile::MltVisualFfv1, "yuv444p") => Ok(READBACK_FILTER),
        (ObservationProfile::MltVisualFfv1, "yuv420p" | "yuv422p")
        | (ObservationProfile::MltH264AacMaster, "yuv420p")
            if chroma_location == Some("left") =>
        {
            Ok(READBACK_FILTER_LEFT)
        }
        _ => Err(Error::unsupported(
            "Observed pixel format or chroma location is outside this readback profile",
        )),
    }
}
fn closed_extended(v: &Value, base: &[&str], extra: &[&str], extended: bool) -> Result<()> {
    if extended {
        let mut fields = base.to_vec();
        fields.extend_from_slice(extra);
        closed(v, &fields)
    } else {
        closed(v, base)
    }
}
fn optional_chroma(v: &Value) -> Result<Option<&str>> {
    match v.get("chroma_location") {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if !value.is_empty() && value.len() <= 32 => Ok(Some(value)),
        _ => Err(Error::invalid(
            "Observed chroma location must be string or null",
        )),
    }
}
/// This is the sole known non-color H.264 frame side datum admitted here.
/// Its presence is retained in the report, never silently discarded.
pub fn validate_side_data_types(
    value: &Value,
    profile: ObservationProfile,
    stream: bool,
) -> Result<()> {
    let values = value
        .as_array()
        .filter(|v| v.len() <= 4)
        .ok_or_else(|| Error::invalid("Observed side-data type array required"))?;
    for item in values {
        if stream
            || profile != ObservationProfile::MltH264AacMaster
            || item.as_str() != Some("H.26[45] User Data Unregistered SEI message")
        {
            return Err(Error::unsupported(
                "Unreviewed color/presentation side data",
            ));
        }
    }
    Ok(())
}
fn validate_native_input_sources(value: &Value) -> Result<()> {
    let sources = value
        .as_array()
        .filter(|v| v.len() <= 4096)
        .ok_or_else(|| Error::invalid("Original staged source descriptor list required"))?;
    let mut total = 0u64;
    for source in sources {
        closed(source, &["root", "path", "sha256", "bytes"])?;
        if !matches!(text(source, "root")?, "project" | "media" | "output") {
            return Err(Error::invalid("Original staged source root"));
        }
        relative(source, "path")?;
        digest_field(source, "sha256")?;
        let bytes = uint(source, "bytes")?;
        if bytes == 0 || bytes > 256 * 1024 * 1024 {
            return Err(Error::limit("Original staged source size"));
        }
        total = total
            .checked_add(bytes)
            .filter(|v| *v <= MAX_STAGED_SOURCE_BYTES)
            .ok_or_else(|| Error::limit("Original staged source aggregate"))?;
    }
    Ok(())
}
fn validate_audio_stream(audio: &Value, video_stream: u64) -> Result<()> {
    closed(
        audio,
        &[
            "index",
            "codec",
            "sample_format",
            "sample_rate",
            "channels",
            "channel_layout",
            "time_base_num",
            "time_base_den",
            "start_pts",
            "duration_ts",
            "frame_count",
            "decoded_sample_count",
            "frames",
        ],
    )?;
    let index = uint(audio, "index")?;
    if index > 15
        || index == video_stream
        || text(audio, "codec")? != "aac"
        || text(audio, "sample_format")? != "fltp"
        || uint(audio, "sample_rate")? != 48_000
        || uint(audio, "channels")? != 2
        || text(audio, "channel_layout")? != "stereo"
        || uint(audio, "time_base_num")? != 1
        || uint(audio, "time_base_den")? != 48_000
    {
        return Err(Error::unsupported(
            "Observed master audio is not AAC 48 kHz stereo",
        ));
    }
    for field in ["start_pts", "duration_ts"] {
        if !audio[field].is_null() && audio[field].as_i64().is_none() {
            return Err(Error::invalid(
                "Audio stream clock is not observed integer/null",
            ));
        }
    }
    if audio["duration_ts"].as_i64().is_some_and(|v| v <= 0) {
        return Err(Error::invalid("Observed audio duration"));
    }
    let frames = audio["frames"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 36_000)
        .ok_or_else(|| Error::invalid("Complete audio-frame roster missing"))?;
    if uint(audio, "frame_count")? != frames.len() as u64 {
        return Err(Error::invalid("Audio frame count"));
    }
    let mut total = 0u64;
    let mut previous = None;
    for (i, frame) in frames.iter().enumerate() {
        closed(
            frame,
            &[
                "index",
                "pts",
                "duration",
                "nb_samples",
                "sample_format",
                "channels",
                "channel_layout",
            ],
        )?;
        let pts = frame["pts"]
            .as_i64()
            .ok_or_else(|| Error::invalid("Audio PTS not observed"))?;
        if uint(frame, "index")? != i as u64
            || previous.is_some_and(|p| pts <= p)
            || frame["duration"].as_i64().is_none_or(|v| v <= 0)
            || text(frame, "sample_format")? != "fltp"
            || uint(frame, "channels")? != 2
            || text(frame, "channel_layout")? != "stereo"
        {
            return Err(Error::invalid("Observed audio frame differs"));
        }
        let samples = uint(frame, "nb_samples")?;
        if samples == 0 || samples > 4096 {
            return Err(Error::limit("Decoded AAC frame sample count"));
        }
        total = total
            .checked_add(samples)
            .ok_or_else(|| Error::limit("Audio sample count overflow"))?;
        previous = Some(pts);
    }
    if uint(audio, "decoded_sample_count")? != total {
        return Err(Error::invalid(
            "Decoded audio sample aggregate was replaced",
        ));
    }
    Ok(())
}

pub const SOURCE_POLICY: &str = "motion-canvas-srgb-rgb8-opaque-v1";
pub const RECIPE: &str = "srgb-to-rec709-relative-linear-v1";
pub const ENCODE_FILTER: &str = "format=gbrp,zscale=pin=709:tin=iec61966-2-1:min=gbr:rin=full:p=709:t=709:m=709:r=limited:dither=none:agamma=0,format=yuv444p";
pub const READBACK_FILTER: &str = "zscale=pin=709:tin=709:min=709:rin=limited:p=709:t=709:m=gbr:r=full:dither=none:agamma=0,format=rgba";
pub const REPORT_LIMIT: usize = 8 * 1024 * 1024;
pub const PNG_LIMIT: usize = 32 * 1024 * 1024;
// Wire limit inherited from the selected complete PNG decoder; the driver
// tests equality against its real decoder. This module never decodes images.
pub const PNG_DECODE_LIMIT: usize = 16 * 1024 * 1024;
pub const MAX_JSON: usize = 1_048_576;
pub const MAX_STAGED_SOURCE_BYTES: u64 = 1024 * 1024 * 1024;
pub const MAX_VIDEO_ARTIFACT_BYTES: u64 = 1024 * 1024 * 1024;
// New observation-specific limit, pending owner review. Does not raise existing
// one-GiB staging or one-GiB artifact limits.
pub const RETAINED_PNG_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug)]
pub struct ObservationBudget {
    pub rgba_bytes_per_frame: u64,
    pub png_bytes_upper_bound: u64,
    pub retained_png_bytes_upper_bound: u64,
    pub peak_free_bytes: u64,
}
pub fn observation_budget(width: u32, height: u32, count: u64) -> Result<ObservationBudget> {
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || u64::from(width) * u64::from(height) > 8_847_360
        || count == 0
        || count > 36_000
    {
        return Err(Error::invalid(
            "Complete frame observation profile is outside bounds",
        ));
    }
    // RGBA8, one filter byte/row, generous deflate expansion plus per-row IDAT
    // overhead and 64KiB fixed PNG headers. Reviewed FFmpeg PNG encoder + zlib
    // use only Z_NO_FLUSH/Z_FINISH. No ICC/EXIF/other frame side data admitted.
    let row = u64::from(width) * 4 + 1;
    let png = (row + row.div_ceil(4) + 64) * u64::from(height) + 65_536;
    let retained = png
        .checked_mul(count)
        .ok_or_else(|| Error::limit("PNG bound overflow"))?;
    if png > PNG_DECODE_LIMIT as u64
        || retained > RETAINED_PNG_LIMIT
        || count * 2048 + 65_536 > MAX_JSON as u64
    {
        return Err(Error::limit(
            "Complete decoded PNG retention exceeds the closed profile",
        ));
    }
    // Spool and published copy may coexist. Include one extra detection frame,
    // full existing input and two existing artifact copies, reports and reserve.
    let peak = retained * 2
        + png
        + MAX_STAGED_SOURCE_BYTES
        + MAX_VIDEO_ARTIFACT_BYTES * 2
        + (REPORT_LIMIT as u64) * 2
        + 64 * 1024 * 1024;
    Ok(ObservationBudget {
        rgba_bytes_per_frame: u64::from(width) * u64::from(height) * 4,
        png_bytes_upper_bound: png,
        retained_png_bytes_upper_bound: retained,
        peak_free_bytes: peak,
    })
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str> {
    v.get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty() && v.len() <= 256)
        .ok_or_else(|| {
            Error::new(
                "BackendFailed",
                format!("Decoded observation omitted {key}"),
            )
        })
}
fn uint(v: &Value, key: &str) -> Result<u64> {
    v.get(key).and_then(Value::as_u64).ok_or_else(|| {
        Error::new(
            "BackendFailed",
            format!("Decoded observation omitted {key}"),
        )
    })
}
fn rational(value: &str) -> Result<(i64, i64)> {
    let (n, d) = value
        .split_once('/')
        .ok_or_else(|| Error::invalid("Observed rational format"))?;
    let n: i64 = n
        .parse()
        .map_err(|_| Error::invalid("Observed rational numerator"))?;
    let d: i64 = d
        .parse()
        .map_err(|_| Error::invalid("Observed rational denominator"))?;
    if n <= 0 || d <= 0 || n > 1_000_000_000 || d > 1_000_000_000 {
        return Err(Error::invalid("Observed rational bounds"));
    }
    Ok((n, d))
}
pub fn clock_observation(frames: &[Frame], fps_num: u32, fps_den: u32) -> Result<Value> {
    let first = frames
        .first()
        .ok_or_else(|| Error::invalid("Clock requires observed frames"))?;
    let tn = i128::from(first.time_base_num);
    let td = i128::from(first.time_base_den);
    let denominator = td * i128::from(fps_num);
    let mut endpoint = i64::MIN;
    let mut exact_pts = true;
    let mut exact_durations = true;
    for frame in frames {
        endpoint = endpoint.max(
            frame
                .pts
                .checked_add(frame.duration)
                .ok_or_else(|| Error::limit("Observed endpoint overflow"))?,
        );
        exact_pts &= frame.residual_num == 0;
        exact_durations &=
            i128::from(frame.duration) * tn * i128::from(fps_num) == i128::from(fps_den) * td;
    }
    let residual = i128::from(endpoint) * tn * i128::from(fps_num)
        - (frames.len() as i128) * i128::from(fps_den) * td;
    Ok(
        json!({"exact_pts":exact_pts,"exact_durations":exact_durations,"exact_endpoint":residual==0,
        "endpoint_pts":endpoint,"endpoint_basis":"max-observed-frame-pts-plus-duration",
        "canonical_endpoint_residual_num":residual.to_string(),
        "canonical_endpoint_residual_den":denominator.to_string()}),
    )
}
#[derive(Debug)]
pub struct Frame {
    pub index: u64,
    pub pts: i64,
    pub duration: i64,
    pub presentation: Value,
    pub time_base_num: i64,
    pub time_base_den: i64,
    pub residual_num: i128,
    pub residual_den: i128,
    pub color: Value,
}
#[derive(Debug)]
pub struct DecodedPng {
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
    pub rgba_sha256: String,
    pub alpha_min: u8,
    pub alpha_max: u8,
}
pub fn rgba_roster_digest(frames: &[DecodedPng]) -> Result<String> {
    let mut h = Sha256::new();
    h.update(b"motionwright.native-rgba-sequence/1\0");
    for (index, frame) in frames.iter().enumerate() {
        let raw = hex::decode(&frame.rgba_sha256).map_err(|_| Error::invalid("RGBA digest"))?;
        if raw.len() != 32 {
            return Err(Error::invalid("RGBA digest length"));
        }
        h.update((index as u64).to_be_bytes());
        h.update(&raw);
    }
    Ok(hex::encode(h.finalize()))
}

/// Closed parsed data only. Construction is private, with no Deserialize or
/// authority constructor. A consumer still reopens artifacts through the
/// original Host/Store receipt and verifies every retained PNG.
#[derive(Debug)]
pub struct CompleteObservation {
    document: Value,
    exact_clock: bool,
    profile: ObservationProfile,
}
impl CompleteObservation {
    pub fn document(&self) -> &Value {
        &self.document
    }
    pub fn is_opaque_rec709_ffv1_without_audio_exact_clock(&self) -> bool {
        self.profile != ObservationProfile::MltH264AacMaster && self.exact_clock
    }
    pub fn is_opaque_rec709_h264_aac_with_exact_video_clock(&self) -> bool {
        self.profile == ObservationProfile::MltH264AacMaster && self.exact_clock
    }
    pub fn profile(&self) -> ObservationProfile {
        self.profile
    }
}
fn closed(v: &Value, fields: &[&str]) -> Result<()> {
    let object = v
        .as_object()
        .ok_or_else(|| Error::invalid("Observation object required"))?;
    if object.len() != fields.len() || fields.iter().any(|key| !object.contains_key(*key)) {
        return Err(Error::invalid(
            "Observation fields differ from the closed schema",
        ));
    }
    Ok(())
}
fn digest_field(v: &Value, key: &str) -> Result<()> {
    if !valid_digest(text(v, key)?) {
        return Err(Error::invalid("Observation digest malformed"));
    }
    Ok(())
}
fn relative(v: &Value, key: &str) -> Result<String> {
    let path = v
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid("Observation path missing"))?;
    // Syntax validation only; opening through the authorized pinned Root remains
    // mandatory in the runtime/consumer. No filesystem authority is granted.
    if path.is_empty()
        || path.len() > 4096
        || path.starts_with('/')
        || path.contains('\\')
        || path.contains(':')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
    {
        return Err(Error::invalid(
            "Observation path is not bounded relative syntax",
        ));
    }
    Ok(path.to_owned())
}
fn expected_color(v: &Value) -> Result<()> {
    closed(v, &["matrix", "range", "primaries", "transfer"])?;
    if ["matrix", "range", "primaries", "transfer"]
        .iter()
        .any(|key| v.get(*key).and_then(Value::as_u64) != Some(1))
    {
        return Err(Error::new(
            "Unsupported",
            "Observed limited Rec.709 color required",
        ));
    }
    Ok(())
}
fn validate_presentation(v: &Value, stream: bool) -> Result<()> {
    if stream {
        closed(v, &["sample_aspect_ratio", "field_order"])?;
        if !v["field_order"].is_null()
            && !matches!(v["field_order"].as_str(), Some("unknown" | "progressive"))
        {
            return Err(Error::new("Unsupported", "Interlaced stream observation"));
        }
    } else {
        closed(
            v,
            &[
                "sample_aspect_ratio",
                "interlaced_frame",
                "top_field_first",
                "repeat_pict",
                "crop_top",
                "crop_bottom",
                "crop_left",
                "crop_right",
            ],
        )?;
        if uint(v, "interlaced_frame")? != 0
            || uint(v, "repeat_pict")? != 0
            || uint(v, "top_field_first")? > 1
            || ["crop_top", "crop_bottom", "crop_left", "crop_right"]
                .iter()
                .any(|key| v[*key].as_u64() != Some(0))
        {
            return Err(Error::new(
                "Unsupported",
                "Non-progressive or cropped frame observation",
            ));
        }
    }
    if text(v, "sample_aspect_ratio")? != "1:1" {
        return Err(Error::new(
            "Unsupported",
            "Non-square or unknown sample aspect ratio",
        ));
    }
    Ok(())
}
pub fn parse_complete_observation(bytes: &[u8]) -> Result<CompleteObservation> {
    if bytes.is_empty() || bytes.len() > MAX_JSON {
        return Err(Error::limit("Closed observation parser byte budget"));
    }
    validate_closed_wire(bytes)?;
    let v: Value = serde_json::from_slice(bytes).map_err(|_| Error::invalid("Observation JSON"))?;
    validate_structure_budget(&v)?;
    let profile = match text(&v, "schema")? {
        "semwright.mlt-complete-frame-observation/1" => ObservationProfile::MotionCanvasFfv1,
        "semwright.mlt-complete-frame-observation/2" => match text(&v, "observation_profile")? {
            "mlt-ffv1-visual-rec709-v1" => ObservationProfile::MltVisualFfv1,
            "mlt-h264-aac-master-rec709-v1" => ObservationProfile::MltH264AacMaster,
            _ => return Err(Error::unsupported("Unknown native observation profile")),
        },
        _ => return Err(Error::unsupported("Unknown observation schema")),
    };
    let extended = profile.is_native_mlt();
    closed_extended(
        &v,
        &[
            "schema",
            "source_binding",
            "artifact",
            "source_policy",
            "color_recipe",
            "encoder_filter",
            "decoder",
            "probe",
            "stream",
            "video_streams",
            "audio_streams",
            "canonical_fps_num",
            "canonical_fps_den",
            "frame_count",
            "first_frame",
            "end_frame_exclusive",
            "exact_clock",
            "clock_observation",
            "frames",
            "frame_path_prefix",
            "rgba_conversion",
            "rgba_filter",
            "rgba_transfer",
            "rgba_bytes_per_frame",
            "rgba_sequence_sha256",
            "rgba_sequence_algorithm",
            "retained_png_bytes",
            "alpha_min",
            "alpha_max",
            "all_pixels_opaque",
            "authority_scope",
        ],
        &[
            "observation_profile",
            "native_input_sources",
            "audio_stream",
            "clock_scope",
        ],
        extended,
    )?;
    if (!extended
        && (text(&v, "source_policy")? != SOURCE_POLICY
            || text(&v, "color_recipe")? != RECIPE
            || text(&v, "encoder_filter")? != ENCODE_FILTER))
        || (extended
            && (text(&v, "source_policy")? != MLT_SOURCE_POLICY
                || text(&v, "color_recipe")? != MLT_RECIPE
                || !v["encoder_filter"].is_null()))
        || text(&v, "rgba_conversion")?
            != "ffmpeg-explicit-bt709-limited-to-bt709-rgb-full-no-transfer-change"
        || text(&v, "rgba_transfer")? != "bt709-nonlinear"
        || text(&v, "rgba_sequence_algorithm")?
            != "motionwright.native-rgba-sequence/1-nul+index-u64be+rgba-sha256-bytes"
        || text(&v, "authority_scope")?
            != "observed data only; original Host/Store/provider-generation/job/source closure is required externally"
    {
        return Err(Error::new(
            "Unsupported",
            "Complete observation profile differs",
        ));
    }
    let source = &v["source_binding"];
    closed(
        source,
        &[
            "source_kind",
            "root",
            "path",
            "sha256",
            "bytes",
            "source_first_frame",
        ],
    )?;
    if text(source, "source_kind")?
        != if extended {
            "mlt_render_project_bytes"
        } else {
            "motion_canvas_frame_manifest_bytes"
        }
        || !matches!(text(source, "root")?, "project" | "media" | "output")
        || uint(source, "bytes")? == 0
        || uint(source, "bytes")? > 8 * 1024 * 1024
        || uint(source, "source_first_frame")? > 100_000_000
    {
        return Err(Error::invalid("Original source-manifest binding differs"));
    }
    relative(source, "path")?;
    digest_field(source, "sha256")?;
    if extended {
        if v["clock_scope"] != "decoded-video-frames" {
            return Err(Error::invalid(
                "Master clock scope must be explicit video-only",
            ));
        }
        if text(source, "root")? != "output" || uint(source, "source_first_frame")? != 0 {
            return Err(Error::invalid("Native XML source binding differs"));
        }
        validate_native_input_sources(&v["native_input_sources"])?;
    }
    let artifact = &v["artifact"];
    closed(artifact, &["root", "path", "sha256", "bytes"])?;
    let artifact_path = relative(artifact, "path")?;
    if text(artifact, "root")? != "output"
        || !artifact_path.ends_with(if profile == ObservationProfile::MltH264AacMaster {
            ".mp4"
        } else {
            ".mkv"
        })
        || uint(artifact, "bytes")? == 0
        || uint(artifact, "bytes")? > MAX_VIDEO_ARTIFACT_BYTES
        || v["frame_path_prefix"].as_str() != Some(format!("{artifact_path}.").as_str())
    {
        return Err(Error::invalid("Exact decoded-artifact binding differs"));
    }
    digest_field(artifact, "sha256")?;
    if extended
        && source["path"].as_str() != Some(format!("{artifact_path}.native-input.mlt").as_str())
    {
        return Err(Error::invalid(
            "Native input descriptor does not derive from the exact output",
        ));
    }
    let decoder = &v["decoder"];
    closed(decoder, &["name", "version", "executable_sha256"])?;
    if text(decoder, "name")? != "FFmpeg"
        || !text(decoder, "version")?.starts_with("ffmpeg version ")
    {
        return Err(Error::invalid("Decoder identity is not observed FFmpeg"));
    }
    digest_field(decoder, "executable_sha256")?;
    let probe = &v["probe"];
    closed(
        probe,
        &["name", "version", "executable_sha256", "libraries"],
    )?;
    if text(probe, "name")? != "ffprobe" {
        return Err(Error::invalid("Probe identity"));
    }
    text(probe, "version")?;
    digest_field(probe, "executable_sha256")?;
    let libraries = probe["libraries"]
        .as_array()
        .filter(|a| !a.is_empty() && a.len() <= 16)
        .ok_or_else(|| Error::invalid("Observed libraries missing"))?;
    let mut names = std::collections::BTreeSet::new();
    for library in libraries {
        closed(library, &["name", "major", "minor", "micro"])?;
        if !names.insert(text(library, "name")?) {
            return Err(Error::invalid("Duplicate observed library"));
        }
        for key in ["major", "minor", "micro"] {
            if uint(library, key)? > u32::MAX as u64 {
                return Err(Error::invalid("Observed library version"));
            }
        }
    }
    let stream = &v["stream"];
    closed_extended(
        stream,
        &[
            "index",
            "codec",
            "pixel_format",
            "width",
            "height",
            "time_base_num",
            "time_base_den",
            "fps_num",
            "fps_den",
            "r_frame_rate_num",
            "r_frame_rate_den",
            "color",
            "presentation",
            "start_pts",
            "duration_ts",
        ],
        &["chroma_location", "side_data_types"],
        extended,
    )?;
    let width =
        u32::try_from(uint(stream, "width")?).map_err(|_| Error::invalid("Observed width"))?;
    let height =
        u32::try_from(uint(stream, "height")?).map_err(|_| Error::invalid("Observed height"))?;
    let count = uint(&v, "frame_count")?;
    let budget = observation_budget(width, height, count)?;
    if text(stream, "codec")? != profile.codec()
        || uint(stream, "index")? > 15
        || uint(&v, "video_streams")? != 1
        || uint(&v, "audio_streams")? != profile.audio_streams()
        || uint(&v, "first_frame")? != 0
        || uint(&v, "end_frame_exclusive")? != count
        || uint(&v, "rgba_bytes_per_frame")? != budget.rgba_bytes_per_frame
        || uint(&v, "alpha_min")? != 255
        || uint(&v, "alpha_max")? != 255
        || v["all_pixels_opaque"].as_bool() != Some(true)
    {
        return Err(Error::new(
            "Unsupported",
            "Complete observed stream is outside the closed profile",
        ));
    }
    let pixel_format = text(stream, "pixel_format")?;
    let chroma = if extended {
        optional_chroma(stream)?
    } else {
        None
    };
    if v["rgba_filter"].as_str() != Some(readback_filter(profile, pixel_format, chroma)?) {
        return Err(Error::invalid(
            "RGBA conversion does not match actual observed chroma sampling",
        ));
    }
    if extended {
        validate_side_data_types(&stream["side_data_types"], profile, true)?;
        if profile == ObservationProfile::MltH264AacMaster {
            validate_audio_stream(&v["audio_stream"], uint(stream, "index")?)?;
        } else if !v["audio_stream"].is_null() {
            return Err(Error::invalid("Unexpected audio observation"));
        }
    }
    expected_color(&stream["color"])?;
    validate_presentation(&stream["presentation"], true)?;
    for key in ["start_pts", "duration_ts"] {
        if !stream[key].is_null() && stream[key].as_i64().is_none() {
            return Err(Error::invalid("Observed stream clock optional integer"));
        }
    }
    if stream["duration_ts"].as_i64().is_some_and(|v| v <= 0) {
        return Err(Error::invalid("Observed stream duration"));
    }
    let (tn, td) = rational(&format!(
        "{}/{}",
        uint(stream, "time_base_num")?,
        uint(stream, "time_base_den")?
    ))?;
    let fpsn = uint(&v, "canonical_fps_num")?;
    let fpsd = uint(&v, "canonical_fps_den")?;
    if fpsn == 0 || fpsn > 120_000 || fpsd == 0 || fpsd > 1001 {
        return Err(Error::invalid("Canonical comparison rate bounds"));
    }
    for (n, d) in [
        ("fps_num", "fps_den"),
        ("r_frame_rate_num", "r_frame_rate_den"),
    ] {
        let (on, od) = rational(&format!("{}/{}", uint(stream, n)?, uint(stream, d)?))?;
        if i128::from(on) * i128::from(fpsd) != i128::from(od) * i128::from(fpsn) {
            return Err(Error::new(
                "Unsupported",
                "Observed fps differs; no clock inference",
            ));
        }
    }
    let frames = v["frames"]
        .as_array()
        .filter(|a| a.len() as u64 == count)
        .ok_or_else(|| Error::invalid("Incomplete retained frame roster"))?;
    let mut parsed_frames = Vec::with_capacity(frames.len());
    let mut clock_frames = Vec::with_capacity(frames.len());
    let mut actual_total = 0u64;
    let mut exact = true;
    let mut previous = None;
    for (index, f) in frames.iter().enumerate() {
        closed_extended(
            f,
            &[
                "index",
                "pts",
                "duration",
                "presentation",
                "time_base_num",
                "time_base_den",
                "canonical_residual_num",
                "canonical_residual_den",
                "width",
                "height",
                "pixel_format",
                "color",
                "file",
                "png_sha256",
                "png_bytes",
                "rgba_sha256",
                "alpha_min",
                "alpha_max",
            ],
            &["chroma_location", "side_data_types"],
            extended,
        )?;
        let pts = f["pts"]
            .as_i64()
            .ok_or_else(|| Error::invalid("Actual integer PTS missing"))?;
        if uint(f, "index")? != index as u64
            || uint(f, "width")? != u64::from(width)
            || uint(f, "height")? != u64::from(height)
            || text(f, "pixel_format")? != pixel_format
            || uint(f, "time_base_num")? != tn as u64
            || uint(f, "time_base_den")? != td as u64
            || text(f, "file")? != format!("decoded-{index:06}.png")
            || uint(f, "alpha_min")? != 255
            || uint(f, "alpha_max")? != 255
            || previous.is_some_and(|p| pts <= p)
        {
            return Err(Error::invalid(
                "Retained frame geometry/path/clock/opacity differs",
            ));
        }
        previous = Some(pts);
        let duration = f["duration"]
            .as_i64()
            .filter(|v| *v > 0)
            .ok_or_else(|| Error::invalid("Actual frame duration missing"))?;
        validate_presentation(&f["presentation"], false)?;
        expected_color(&f["color"])?;
        if extended {
            if pixel_format != "yuv444p" && optional_chroma(f)? != chroma {
                return Err(Error::invalid("Observed frame chroma sampling changed"));
            }
            validate_side_data_types(&f["side_data_types"], profile, false)?;
        }
        let numerator = i128::from(pts) * i128::from(tn) * i128::from(fpsn)
            - (index as i128) * i128::from(fpsd) * i128::from(td);
        let denominator = i128::from(td) * i128::from(fpsn);
        if text(f, "canonical_residual_num")? != numerator.to_string()
            || text(f, "canonical_residual_den")? != denominator.to_string()
        {
            return Err(Error::invalid(
                "Observed PTS residual was rounded or replaced",
            ));
        }
        exact &= numerator == 0;
        clock_frames.push(Frame {
            index: index as u64,
            pts,
            duration,
            presentation: f["presentation"].clone(),
            time_base_num: tn,
            time_base_den: td,
            residual_num: numerator,
            residual_den: denominator,
            color: f["color"].clone(),
        });
        let bytes = uint(f, "png_bytes")?;
        if bytes <= 8 || bytes > budget.png_bytes_upper_bound || bytes > PNG_DECODE_LIMIT as u64 {
            return Err(Error::limit(
                "Retained PNG bytes outside complete-decoder bounds",
            ));
        }
        digest_field(f, "png_sha256")?;
        digest_field(f, "rgba_sha256")?;
        actual_total = actual_total
            .checked_add(bytes)
            .ok_or_else(|| Error::limit("Retained PNG sum"))?;
        parsed_frames.push(DecodedPng {
            file: text(f, "file")?.into(),
            bytes,
            sha256: text(f, "png_sha256")?.into(),
            rgba_sha256: text(f, "rgba_sha256")?.into(),
            alpha_min: 255,
            alpha_max: 255,
        });
    }
    let actual_clock = clock_observation(&clock_frames, fpsn as u32, fpsd as u32)?;
    if v["clock_observation"] != actual_clock {
        return Err(Error::invalid(
            "Observed clock endpoint or duration exactness was replaced",
        ));
    }
    exact &= actual_clock["exact_durations"] == true && actual_clock["exact_endpoint"] == true;
    if actual_total != uint(&v, "retained_png_bytes")?
        || actual_total > RETAINED_PNG_LIMIT
        || v["exact_clock"].as_bool() != Some(exact)
        || text(&v, "rgba_sequence_sha256")? != rgba_roster_digest(&parsed_frames)?
    {
        return Err(Error::invalid(
            "Complete decoded aggregate or exact-clock claim differs",
        ));
    }
    Ok(CompleteObservation {
        document: v,
        exact_clock: exact,
        profile,
    })
}

#[cfg(test)]
mod complete_tests {
    use super::*;
    fn document(quantized: bool) -> Value {
        let tb = if quantized { 1000 } else { 30 };
        let pts = if quantized { 33 } else { 1 };
        let rgba = DecodedPng {
            file: "decoded-000000.png".into(),
            bytes: 100,
            sha256: "b".repeat(64),
            rgba_sha256: "a".repeat(64),
            alpha_min: 255,
            alpha_max: 255,
        };
        let roster = rgba_roster_digest(&[
            rgba,
            DecodedPng {
                file: "decoded-000001.png".into(),
                bytes: 100,
                sha256: "b".repeat(64),
                rgba_sha256: "a".repeat(64),
                alpha_min: 255,
                alpha_max: 255,
            },
        ])
        .unwrap();
        let frame = |i: u64, p: i64| {
            json!({"index":i,"pts":p,"duration":pts,"presentation":{"sample_aspect_ratio":"1:1","interlaced_frame":0,"top_field_first":0,"repeat_pict":0,"crop_top":0,"crop_bottom":0,"crop_left":0,"crop_right":0},"time_base_num":1,"time_base_den":tb,
            "canonical_residual_num":(i128::from(p)*30-i128::from(i)*i128::from(tb)).to_string(),
            "canonical_residual_den":(tb*30).to_string(),
            "width":1920,"height":1080,"pixel_format":"yuv444p",
            "color":{"matrix":1,"range":1,"primaries":1,"transfer":1},
            "file":format!("decoded-{i:06}.png"),"png_sha256":"b".repeat(64),"png_bytes":100,
            "rgba_sha256":"a".repeat(64),"alpha_min":255,"alpha_max":255})
        };
        json!({
            "schema":"semwright.mlt-complete-frame-observation/1",
            "source_binding":{"source_kind":"motion_canvas_frame_manifest_bytes","root":"output","path":"manifest.json",
                "sha256":"c".repeat(64),"bytes":123,"source_first_frame":0},
            "artifact":{"root":"output","path":"native.mkv","sha256":"d".repeat(64),"bytes":456},
            "source_policy":SOURCE_POLICY,"color_recipe":RECIPE,"encoder_filter":ENCODE_FILTER,
            "decoder":{"name":"FFmpeg","version":"ffmpeg version synthetic-parser-fixture","executable_sha256":"e".repeat(64)},
            "probe":{"name":"ffprobe","version":"synthetic-parser-fixture","executable_sha256":"f".repeat(64),
                "libraries":[{"name":"libavcodec","major":60,"minor":31,"micro":102}]},
            "stream":{"index":0,"codec":"ffv1","pixel_format":"yuv444p","width":1920,"height":1080,
                "time_base_num":1,"time_base_den":tb,"fps_num":30,"fps_den":1,"r_frame_rate_num":30,"r_frame_rate_den":1,
                "color":{"matrix":1,"range":1,"primaries":1,"transfer":1},"start_pts":null,"duration_ts":null,
                "presentation":{"sample_aspect_ratio":"1:1","field_order":"progressive"}},
            "video_streams":1,"audio_streams":0,"canonical_fps_num":30,"canonical_fps_den":1,
            "frame_count":2,"first_frame":0,"end_frame_exclusive":2,"exact_clock":!quantized,
            "clock_observation":{"exact_pts":!quantized,"exact_durations":!quantized,"exact_endpoint":!quantized,
                "endpoint_pts":pts*2,"endpoint_basis":"max-observed-frame-pts-plus-duration",
                "canonical_endpoint_residual_num":(i128::from(pts)*2*30-2*i128::from(tb)).to_string(),
                "canonical_endpoint_residual_den":(tb*30).to_string()},
            "frames":[frame(0,0),frame(1,pts)],"frame_path_prefix":"native.mkv.",
            "rgba_conversion":"ffmpeg-explicit-bt709-limited-to-bt709-rgb-full-no-transfer-change",
            "rgba_filter":READBACK_FILTER,"rgba_transfer":"bt709-nonlinear","rgba_bytes_per_frame":8_294_400,
            "rgba_sequence_sha256":roster,
            "rgba_sequence_algorithm":"motionwright.native-rgba-sequence/1-nul+index-u64be+rgba-sha256-bytes",
            "retained_png_bytes":200,"alpha_min":255,"alpha_max":255,"all_pixels_opaque":true,
            "authority_scope":"observed data only; original Host/Store/provider-generation/job/source closure is required externally"
        })
    }
    fn parse(v: &Value) -> Result<CompleteObservation> {
        parse_complete_observation(&serde_json::to_vec(v).unwrap())
    }

    fn mlt_document(master: bool, quantized: bool) -> Value {
        let mut v = document(quantized);
        let name = if master { "master.mp4" } else { "visual.mkv" };
        v["schema"] = json!("semwright.mlt-complete-frame-observation/2");
        v["observation_profile"] = json!(if master {
            "mlt-h264-aac-master-rec709-v1"
        } else {
            "mlt-ffv1-visual-rec709-v1"
        });
        v["clock_scope"] = json!("decoded-video-frames");
        v["source_policy"] = json!(MLT_SOURCE_POLICY);
        v["color_recipe"] = json!(MLT_RECIPE);
        v["encoder_filter"] = Value::Null;
        v["source_binding"]["source_kind"] = json!("mlt_render_project_bytes");
        v["source_binding"]["path"] = json!(format!("{name}.native-input.mlt"));
        v["artifact"]["path"] = json!(name);
        v["frame_path_prefix"] = json!(format!("{name}."));
        v["native_input_sources"] = json!([{"root":"output","path":"original-video.mkv",
            "sha256":"1".repeat(64),"bytes":456}]);
        v["stream"]["codec"] = json!(if master { "h264" } else { "ffv1" });
        v["stream"]["pixel_format"] = json!(if master { "yuv420p" } else { "yuv444p" });
        v["stream"]["chroma_location"] = if master { json!("left") } else { Value::Null };
        v["stream"]["side_data_types"] = json!([]);
        for frame in v["frames"].as_array_mut().unwrap() {
            frame["pixel_format"] = json!(if master { "yuv420p" } else { "yuv444p" });
            frame["chroma_location"] = if master { json!("left") } else { Value::Null };
            frame["side_data_types"] = json!([]);
        }
        v["audio_streams"] = json!(u64::from(master));
        v["audio_stream"] = if master {
            json!({"index":1,"codec":"aac","sample_format":"fltp","sample_rate":48000,
                "channels":2,"channel_layout":"stereo","time_base_num":1,"time_base_den":48000,
                "start_pts":0,"duration_ts":2048,"frame_count":2,"decoded_sample_count":2048,
                "frames":[
                    {"index":0,"pts":0,"duration":1024,"nb_samples":1024,
                        "sample_format":"fltp","channels":2,"channel_layout":"stereo"},
                    {"index":1,"pts":1024,"duration":1024,"nb_samples":1024,
                        "sample_format":"fltp","channels":2,"channel_layout":"stereo"}]})
        } else {
            Value::Null
        };
        if master {
            v["rgba_filter"] = json!(READBACK_FILTER_LEFT);
        }
        v
    }
    #[test]
    fn native_visual_keeps_xml_and_quantized_video_clock_without_retiming() {
        let v = mlt_document(false, true);
        let observed = parse(&v).unwrap();
        assert_eq!(observed.profile(), ObservationProfile::MltVisualFfv1);
        assert_eq!(
            observed.document()["source_binding"]["path"],
            "visual.mkv.native-input.mlt"
        );
        assert_eq!(observed.document()["frames"][1]["pts"], 33);
        assert!(!observed.is_opaque_rec709_ffv1_without_audio_exact_clock());
        assert!(observed.document()["audio_stream"].is_null());
    }
    #[test]
    fn master_records_audio_separately_from_exact_video_clock() {
        let v = mlt_document(true, false);
        let observed = parse(&v).unwrap();
        assert_eq!(observed.profile(), ObservationProfile::MltH264AacMaster);
        assert!(observed.is_opaque_rec709_h264_aac_with_exact_video_clock());
        assert!(!observed.is_opaque_rec709_ffv1_without_audio_exact_clock());
        assert_eq!(
            observed.document()["audio_stream"]["frames"][1]["pts"],
            1024
        );
        assert_eq!(observed.document()["clock_scope"], "decoded-video-frames");
        // This synthetic observation deliberately does not assert that its AAC
        // duration equals the video duration. AV acceptance remains external.
        assert_eq!(
            observed.document()["audio_stream"]["decoded_sample_count"],
            2048
        );
    }
    #[test]
    fn native_xml_source_identity_and_staged_sources_cannot_be_substituted() {
        for replacement in [
            json!("caller_scene"),
            json!("motion_canvas_frame_manifest_bytes"),
        ] {
            let mut v = mlt_document(false, false);
            v["source_binding"]["source_kind"] = replacement;
            assert!(parse(&v).is_err());
        }
        let mut v = mlt_document(false, false);
        v["source_binding"]["path"] = json!("other.mlt");
        assert!(parse(&v).is_err());
        let mut v = mlt_document(false, false);
        v["native_input_sources"][0]["sha256"] = json!("bad");
        assert!(parse(&v).is_err());
        let mut v = mlt_document(false, false);
        v["native_input_sources"][0]["path"] = json!("../original.mkv");
        assert!(parse(&v).is_err());
        let mut v = mlt_document(false, false);
        v["source_binding"]["source_first_frame"] = json!(1);
        assert!(parse(&v).is_err());
    }
    #[test]
    fn actual_chroma_location_and_frame_presentation_are_not_defaulted() {
        for bad in [Value::Null, json!("unspecified"), json!("center")] {
            let mut v = mlt_document(true, false);
            v["stream"]["chroma_location"] = bad;
            assert!(parse(&v).is_err());
        }
        let mut v = mlt_document(true, false);
        v["frames"][1]["chroma_location"] = json!("center");
        assert!(parse(&v).is_err());
        let mut v = mlt_document(true, false);
        v["frames"][1]["presentation"]["sample_aspect_ratio"] = json!("N/A");
        assert!(parse(&v).is_err());
        let mut v = mlt_document(false, false);
        v["stream"]["pixel_format"] = json!("yuv422p");
        v["stream"]["chroma_location"] = json!("left");
        for f in v["frames"].as_array_mut().unwrap() {
            f["pixel_format"] = json!("yuv422p");
            f["chroma_location"] = json!("left");
        }
        v["rgba_filter"] = json!(READBACK_FILTER_LEFT);
        assert!(parse(&v).is_ok());
    }
    #[test]
    fn unregistered_h264_sei_is_retained_but_color_side_data_is_rejected() {
        let mut v = mlt_document(true, false);
        v["frames"][0]["side_data_types"] = json!(["H.26[45] User Data Unregistered SEI message"]);
        assert_eq!(
            parse(&v).unwrap().document()["frames"][0]["side_data_types"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        for bad in [
            "ICC profile",
            "Mastering display metadata",
            "Content light level metadata",
            "unknown",
        ] {
            v["frames"][0]["side_data_types"] = json!([bad]);
            assert!(parse(&v).is_err());
        }
        let mut v = mlt_document(false, false);
        v["frames"][0]["side_data_types"] = json!(["H.26[45] User Data Unregistered SEI message"]);
        assert!(parse(&v).is_err());
    }
    #[test]
    fn all_decoded_aac_frames_must_keep_observed_count_clock_and_layout() {
        for (field, bad) in [
            ("pts", Value::Null),
            ("pts", json!(0)),
            ("duration", json!(0)),
            ("channels", json!(1)),
            ("sample_format", json!("unknown")),
            ("nb_samples", json!(0)),
        ] {
            let mut v = mlt_document(true, false);
            v["audio_stream"]["frames"][1][field] = bad;
            assert!(parse(&v).is_err(), "{field}");
        }
        let mut v = mlt_document(true, false);
        v["audio_stream"]["decoded_sample_count"] = json!(2047);
        assert!(parse(&v).is_err());
        let mut v = mlt_document(true, false);
        v["audio_stream"]["index"] = json!(0);
        assert!(parse(&v).is_err());
        let mut v = mlt_document(true, false);
        v["audio_stream"]["codec"] = json!("opus");
        assert!(parse(&v).is_err());
    }
    #[test]
    fn native_wire_requires_video_clock_scope_and_rejects_duplicate_audio_fields() {
        let mut v = mlt_document(true, false);
        v.as_object_mut().unwrap().remove("clock_scope");
        assert!(parse(&v).is_err());
        let mut v = mlt_document(true, false);
        v["clock_scope"] = json!("all-audio-video");
        assert!(parse(&v).is_err());
        let valid = serde_json::to_string(&mlt_document(true, false)).unwrap();
        let duplicate = valid.replacen(
            "\"nb_samples\":1024",
            "\"nb_samples\":1024,\"nb_samples\":1024",
            1,
        );
        assert_ne!(duplicate, valid);
        assert!(parse_complete_observation(duplicate.as_bytes()).is_err());
        let mut legacy = document(false);
        legacy["observation_profile"] = json!("mlt-ffv1-visual-rec709-v1");
        assert!(parse(&legacy).is_err());
    }
    #[test]
    fn film180_complete_master_report_fits_the_existing_parser_budget() {
        let mut v = mlt_document(true, false);
        let prototype = v["frames"][0].clone();
        let mut decoded = Vec::new();
        let mut frames = Vec::new();
        for i in 0u64..180 {
            let mut frame = prototype.clone();
            frame["index"] = json!(i);
            frame["pts"] = json!(i);
            frame["file"] = json!(format!("decoded-{i:06}.png"));
            frames.push(frame);
            decoded.push(DecodedPng {
                file: format!("decoded-{i:06}.png"),
                bytes: 100,
                sha256: "b".repeat(64),
                rgba_sha256: "a".repeat(64),
                alpha_min: 255,
                alpha_max: 255,
            });
        }
        v["frames"] = json!(frames);
        v["frame_count"] = json!(180);
        v["end_frame_exclusive"] = json!(180);
        v["retained_png_bytes"] = json!(18_000);
        v["rgba_sequence_sha256"] = json!(rgba_roster_digest(&decoded).unwrap());
        v["clock_observation"]["endpoint_pts"] = json!(180);
        let audio = (0u64..283)
            .map(|i| {
                json!({"index":i,"pts":i*1024,"duration":1024,"nb_samples":1024,
            "sample_format":"fltp","channels":2,"channel_layout":"stereo"})
            })
            .collect::<Vec<_>>();
        v["audio_stream"]["frames"] = json!(audio);
        v["audio_stream"]["frame_count"] = json!(283);
        v["audio_stream"]["decoded_sample_count"] = json!(283 * 1024);
        v["audio_stream"]["duration_ts"] = json!(288000);
        let bytes = serde_json::to_vec(&v).unwrap();
        assert!(bytes.len() < MAX_JSON);
        let observed = parse_complete_observation(&bytes).unwrap();
        assert_eq!(observed.document()["frames"].as_array().unwrap().len(), 180);
        assert_eq!(
            observed.document()["audio_stream"]["frames"]
                .as_array()
                .unwrap()
                .len(),
            283
        );
    }

    #[test]
    fn portable_wire_rejects_duplicate_top_level_and_nested_fields() {
        let valid = serde_json::to_string(&document(false)).unwrap();
        for (needle, duplicate) in [
            ("\"frame_count\":2", "\"frame_count\":2,\"frame_count\":2"),
            ("\"transfer\":1", "\"transfer\":1,\"transfer\":1"),
            (
                "\"start_pts\":null",
                "\"start_pts\":null,\"start_pts\":null",
            ),
            (
                "\"path\":\"manifest.json\"",
                "\"path\":\"manifest.json\",\"path\":\"manifest.json\"",
            ),
        ] {
            assert!(valid.contains(needle));
            let bytes = valid.replacen(needle, duplicate, 1).into_bytes();
            assert!(parse_complete_observation(&bytes).is_err(), "{needle}");
        }
    }
    #[test]
    fn portable_wire_rejects_nested_unknown_and_wrong_types() {
        for (field, value) in [
            ("matrix", json!("1")),
            ("transfer", json!(null)),
            ("hidden_profile", json!("display-p3")),
        ] {
            let mut v = document(false);
            v["frames"][0]["color"][field] = value;
            assert!(parse(&v).is_err(), "{field}");
        }
        let mut v = document(false);
        v["stream"]["start_pts"] = json!("0");
        assert!(parse(&v).is_err());
    }
    #[test]
    fn portable_wire_paths_are_data_without_traversal_or_uri_syntax() {
        for path in [
            "",
            "../manifest.json",
            "a//manifest.json",
            "C:/manifest.json",
            "\\\\host\\manifest.json",
            "/manifest.json",
            "a/./manifest.json",
            "file:manifest.json",
            "a\u{0000}.json",
        ] {
            let mut v = document(false);
            v["source_binding"]["path"] = json!(path);
            assert!(parse(&v).is_err(), "{path:?}");
        }
        let mut v = document(false);
        v["source_binding"]["path"] = json!("native/source/manifest.json");
        assert!(parse(&v).is_ok());
    }
    #[test]
    fn portable_complete_profile_keeps_film180_and_rejects_excess_retention() {
        let budget = observation_budget(1920, 1080, 180).unwrap();
        assert_eq!(budget.rgba_bytes_per_frame * 180, 1_492_992_000);
        assert_eq!(budget.retained_png_bytes_upper_bound, 1_890_866_880);
        assert_eq!(budget.peak_free_bytes, 7_097_350_128);
        assert!(observation_budget(1920, 1080, 36_000).is_err());
        assert!(observation_budget(0, 1080, 180).is_err());
    }
    #[test]
    fn portable_wire_preserves_byte_structure_budgets() {
        assert!(parse_complete_observation(&[]).is_err());
        assert!(parse_complete_observation(&vec![b' '; MAX_JSON + 1]).is_err());
        let oversized = json!(vec![Value::Null; 16_384]);
        assert!(validate_structure_budget(&oversized).is_err());
        let mut deep = Value::Null;
        for _ in 0..41 {
            deep = json!([deep]);
        }
        assert!(validate_structure_budget(&deep).is_err());
    }
    #[test]
    fn public_parser_preserves_quantized_pts_without_admission() {
        let exact = parse(&document(false)).unwrap();
        assert!(exact.is_opaque_rec709_ffv1_without_audio_exact_clock());
        let observed = parse(&document(true)).unwrap();
        assert!(!observed.is_opaque_rec709_ffv1_without_audio_exact_clock());
        assert_eq!(observed.document()["frames"][1]["pts"], 33);
        assert_eq!(
            observed.document()["frames"][1]["canonical_residual_num"],
            "-10"
        );
    }
    #[test]
    fn public_parser_rejects_missing_frame_color_and_forged_exact_clock() {
        let mut v = document(false);
        v["frames"][1]["color"]["transfer"] = Value::Null;
        assert!(parse(&v).is_err());
        let mut v = document(true);
        v["exact_clock"] = json!(true);
        assert!(parse(&v).is_err());
        let mut v = document(true);
        v["frames"][1]["canonical_residual_num"] = json!("0");
        assert!(parse(&v).is_err());
        let mut v = document(false);
        v["rgba_transfer"] = json!("srgb");
        assert!(parse(&v).is_err());
    }
    #[test]
    fn public_parser_rejects_alternate_png_paths_and_roster() {
        let mut v = document(false);
        v["frame_path_prefix"] = json!("other.mkv.");
        assert!(parse(&v).is_err());
        let mut v = document(false);
        v["frames"][0]["file"] = json!("../decoded-000000.png");
        assert!(parse(&v).is_err());
        let mut v = document(false);
        v["frames"][1]["rgba_sha256"] = json!("0".repeat(64));
        assert!(parse(&v).is_err());
        let mut v = document(false);
        v["retained_png_bytes"] = json!(199);
        assert!(parse(&v).is_err());
    }
    #[test]
    fn public_parser_rejects_extra_fields_audio_and_probe_only_json() {
        let mut v = document(false);
        v["caller_approved"] = json!(true);
        assert!(parse(&v).is_err());
        let mut v = document(false);
        v["audio_streams"] = json!(1);
        assert!(parse(&v).is_err());
        assert!(parse_complete_observation(br#"{"frames":[],"streams":[]}"#).is_err());
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn validate_closed_wire(bytes: &[u8]) -> Result<()> {
    let _: wire::Document = serde_json::from_slice(bytes).map_err(|_| {
        Error::invalid("Closed observation wire has duplicate, unknown or malformed fields")
    })?;
    Ok(())
}

fn validate_structure_budget(value: &Value) -> Result<()> {
    let mut pending = vec![(value, 0usize)];
    let mut nodes = 0usize;
    while let Some((value, depth)) = pending.pop() {
        nodes += 1;
        if depth > 40 || nodes > 16_384 {
            return Err(Error::limit("Observation JSON structure budget"));
        }
        match value {
            Value::Array(values) => pending.extend(values.iter().map(|v| (v, depth + 1))),
            Value::Object(values) => pending.extend(values.values().map(|v| (v, depth + 1))),
            _ => {}
        }
    }
    Ok(())
}

// Private transport structs are duplicate-key/type guards, not authority or
// construction APIs. All values survive byte parsing unmodified in document().
#[allow(dead_code)]
mod wire {
    use super::Deserialize;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Document {
        observation_profile: Option<String>,
        clock_scope: Option<String>,
        native_input_sources: Option<Vec<Artifact>>,
        audio_stream: Option<Audio>,
        schema: String,
        source_binding: SourceBinding,
        artifact: Artifact,
        source_policy: String,
        color_recipe: String,
        encoder_filter: Option<String>,
        decoder: Decoder,
        probe: Probe,
        stream: Stream,
        video_streams: u64,
        audio_streams: u64,
        canonical_fps_num: u64,
        canonical_fps_den: u64,
        frame_count: u64,
        first_frame: u64,
        end_frame_exclusive: u64,
        exact_clock: bool,
        clock_observation: Clock,
        frames: Vec<Frame>,
        frame_path_prefix: String,
        rgba_conversion: String,
        rgba_filter: String,
        rgba_transfer: String,
        rgba_bytes_per_frame: u64,
        rgba_sequence_sha256: String,
        rgba_sequence_algorithm: String,
        retained_png_bytes: u64,
        alpha_min: u64,
        alpha_max: u64,
        all_pixels_opaque: bool,
        authority_scope: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct SourceBinding {
        source_kind: String,
        root: String,
        path: String,
        sha256: String,
        bytes: u64,
        source_first_frame: u64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Artifact {
        root: String,
        path: String,
        sha256: String,
        bytes: u64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Decoder {
        name: String,
        version: String,
        executable_sha256: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Probe {
        name: String,
        version: String,
        executable_sha256: String,
        libraries: Vec<Library>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Library {
        name: String,
        major: u64,
        minor: u64,
        micro: u64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Color {
        matrix: u64,
        range: u64,
        primaries: u64,
        transfer: u64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StreamPresentation {
        sample_aspect_ratio: String,
        field_order: Option<String>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct FramePresentation {
        sample_aspect_ratio: String,
        interlaced_frame: u64,
        top_field_first: u64,
        repeat_pict: u64,
        crop_top: u64,
        crop_bottom: u64,
        crop_left: u64,
        crop_right: u64,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Stream {
        chroma_location: Option<String>,
        side_data_types: Option<Vec<String>>,
        index: u64,
        codec: String,
        pixel_format: String,
        width: u64,
        height: u64,
        time_base_num: u64,
        time_base_den: u64,
        fps_num: u64,
        fps_den: u64,
        r_frame_rate_num: u64,
        r_frame_rate_den: u64,
        color: Color,
        presentation: StreamPresentation,
        start_pts: Option<i64>,
        duration_ts: Option<i64>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Clock {
        exact_pts: bool,
        exact_durations: bool,
        exact_endpoint: bool,
        endpoint_pts: i64,
        endpoint_basis: String,
        canonical_endpoint_residual_num: String,
        canonical_endpoint_residual_den: String,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Frame {
        chroma_location: Option<String>,
        side_data_types: Option<Vec<String>>,
        index: u64,
        pts: i64,
        duration: i64,
        presentation: FramePresentation,
        time_base_num: u64,
        time_base_den: u64,
        canonical_residual_num: String,
        canonical_residual_den: String,
        width: u64,
        height: u64,
        pixel_format: String,
        color: Color,
        file: String,
        png_sha256: String,
        png_bytes: u64,
        rgba_sha256: String,
        alpha_min: u64,
        alpha_max: u64,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Audio {
        index: u64,
        codec: String,
        sample_format: String,
        sample_rate: u64,
        channels: u64,
        channel_layout: String,
        time_base_num: u64,
        time_base_den: u64,
        start_pts: Option<i64>,
        duration_ts: Option<i64>,
        frame_count: u64,
        decoded_sample_count: u64,
        frames: Vec<AudioFrame>,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct AudioFrame {
        index: u64,
        pts: i64,
        duration: i64,
        nb_samples: u64,
        sample_format: String,
        channels: u64,
        channel_layout: String,
    }
}
