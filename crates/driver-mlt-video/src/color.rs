//! Proposed closed color/complete-readback contract for the existing frames.encode.
//! Producer facts are not Host/Store authority. PTS are never replaced by a canonical clock.
use crate::{Error, Result, hash::sha256};
use serde_json::{Value, json};

// One portable parser shared with applications. Linux Root/runtime and PNG
// decoding remain in this crate; the opaque parsed type grants no authority.
pub use semwright_video_domain::native_observation::{
    CompleteObservation, DecodedPng, ENCODE_FILTER, Frame, MLT_RECIPE, MLT_SOURCE_POLICY,
    ObservationBudget, ObservationProfile, PNG_DECODE_LIMIT, PNG_LIMIT, READBACK_FILTER, RECIPE,
    REPORT_LIMIT, RETAINED_PNG_LIMIT, SOURCE_POLICY, clock_observation, observation_budget,
    parse_complete_observation, readback_filter, rgba_roster_digest, validate_side_data_types,
};

pub fn require_srgb_plan(plan: &serde_json::Map<String, Value>) -> Result<()> {
    if plan.get("color_space").and_then(Value::as_str) != Some("srgb")
        || plan.get("alpha").and_then(Value::as_bool) != Some(false)
    {
        return Err(Error::new(
            "Unsupported",
            "Frame encode requires source-bound opaque sRGB; P3, alpha and unknown color are unsupported",
        ));
    }
    Ok(())
}
const PNG_CRC_TABLE: [u32; 256] = {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < table.len() {
        let mut value = index as u32;
        let mut bit = 0;
        while bit < 8 {
            value = if value & 1 == 0 {
                value >> 1
            } else {
                (value >> 1) ^ 0xedb8_8320
            };
            bit += 1;
        }
        table[index] = value;
        index += 1;
    }
    table
};

fn png_chunk_crc(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for byte in bytes {
        let index = ((crc ^ u32::from(*byte)) & 0xff) as usize;
        crc = PNG_CRC_TABLE[index] ^ (crc >> 8);
    }
    !crc
}

/// Check every chunk checksum and the terminal boundary before pixel admission.
/// The Motion Canvas verifier remains the only decoder of pixel data.
fn inspect_png_chunks(
    bytes: &[u8],
    mut inspect: impl FnMut(&[u8], &[u8]) -> Result<()>,
) -> Result<()> {
    if bytes.len() < 8 || bytes.len() > PNG_LIMIT || &bytes[..8] != b"\x89PNG\r\n\x1a\n" {
        return Err(Error::invalid("PNG signature or byte budget"));
    }
    let mut offset = 8usize;
    while offset < bytes.len() {
        if bytes.len() - offset < 12 {
            return Err(Error::invalid("Truncated PNG chunk"));
        }
        let n = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
        let end = offset
            .checked_add(n)
            .and_then(|v| v.checked_add(12))
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| Error::invalid("PNG chunk length"))?;
        let kind = &bytes[offset + 4..offset + 8];
        let payload = &bytes[offset + 8..end - 4];
        if (offset == 8 && (kind != b"IHDR" || n != 13)) || (offset != 8 && kind == b"IHDR") {
            return Err(Error::invalid("PNG header chunk identity"));
        }
        let expected = u32::from_be_bytes(bytes[end - 4..end].try_into().unwrap());
        if png_chunk_crc(&bytes[offset + 4..end - 4]) != expected {
            return Err(Error::invalid("PNG chunk checksum"));
        }
        inspect(kind, payload)?;
        if kind == b"IEND" {
            if !payload.is_empty() {
                return Err(Error::invalid("PNG end marker payload"));
            }
            if end != bytes.len() {
                return Err(Error::invalid("PNG trailing data"));
            }
            return Ok(());
        }
        offset = end;
    }
    Err(Error::invalid("PNG end marker missing"))
}

pub fn inspect_source_png(bytes: &[u8], width: u32, height: u32) -> Result<()> {
    if bytes.len() < 33
        || bytes.len() > PNG_LIMIT
        || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
        || &bytes[12..16] != b"IHDR"
        || bytes[24] != 8
        || !matches!(bytes[25], 2 | 6)
    {
        return Err(Error::new(
            "Unsupported",
            "Only original RGB8/RGBA8 Motion PNG is admitted",
        ));
    }
    let mut color_chunks = std::collections::BTreeSet::new();
    let mut density_seen = false;
    inspect_png_chunks(bytes, |kind, payload| {
        if kind == b"pHYs" {
            if density_seen
                || payload.len() != 9
                || payload[8] > 1
                || u32::from_be_bytes(payload[..4].try_into().unwrap()) == 0
                || payload[..4] != payload[4..8]
            {
                return Err(Error::new(
                    "Unsupported",
                    "Source PNG pixel density is not a unique square declaration",
                ));
            }
            density_seen = true;
        }
        if matches!(kind, b"iCCP" | b"cICP" | b"gAMA" | b"cHRM" | b"sRGB")
            && !color_chunks.insert(kind.to_vec())
        {
            return Err(Error::new(
                "Unsupported",
                "Duplicate source PNG color declaration",
            ));
        }
        let allowed = match kind {
            b"iCCP" => false,
            b"cICP" => payload == [1, 13, 0, 1],
            b"gAMA" => payload == 45_455u32.to_be_bytes(),
            b"cHRM" => {
                payload
                    == [
                        31_270u32, 32_900, 64_000, 33_000, 30_000, 60_000, 15_000, 6_000,
                    ]
                    .iter()
                    .flat_map(|v| v.to_be_bytes())
                    .collect::<Vec<_>>()
            }
            b"sRGB" => payload.len() == 1 && payload[0] <= 3,
            _ => true,
        };
        if !allowed {
            return Err(Error::new(
                "Unsupported",
                "Source PNG profile conflicts with explicit sRGB admission",
            ));
        }
        Ok(())
    })?;
    let observed = semwright_driver_motion_canvas::security::inspect_png(bytes)
        .map_err(|_| Error::invalid("Original Motion PNG failed complete decoding"))?;
    if observed.width != width
        || observed.height != height
        || observed.min_alpha != 255
        || observed.max_alpha != 255
    {
        return Err(Error::new(
            "Unsupported",
            "Original Motion PNG geometry or opacity differs",
        ));
    }
    Ok(())
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

fn observed_chroma(value: &Value) -> Result<Value> {
    match value.get("chroma_location") {
        None => Ok(Value::Null),
        Some(Value::String(v)) if !v.is_empty() && v.len() <= 32 => Ok(json!(v)),
        _ => Err(Error::invalid(
            "Actual chroma-location field has wrong type",
        )),
    }
}
fn observed_side_data(value: &Value, profile: ObservationProfile, stream: bool) -> Result<Value> {
    let items = match value.get("side_data_list") {
        None => Vec::new(),
        Some(Value::Array(values)) if values.len() <= 4 => values
            .iter()
            .map(|v| {
                let object = v
                    .as_object()
                    .filter(|v| v.len() == 1 && v.contains_key("side_data_type"))
                    .ok_or_else(|| {
                        Error::invalid("Observed side-data entry must expose its actual type")
                    })?;
                let name = object["side_data_type"]
                    .as_str()
                    .filter(|v| !v.is_empty() && v.len() <= 128)
                    .ok_or_else(|| Error::invalid("Observed side-data type missing"))?;
                Ok(json!(name))
            })
            .collect::<Result<Vec<_>>>()?,
        _ => return Err(Error::invalid("Observed side-data must be an array")),
    };
    let v = json!(items);
    validate_side_data_types(&v, profile, stream)?;
    Ok(v)
}
fn observed_audio(stream: &Value, frames: &[&Value], video_index: u64) -> Result<Value> {
    let index = uint(stream, "index")?;
    if index > 15
        || index == video_index
        || text(stream, "codec_type")? != "audio"
        || text(stream, "codec_name")? != "aac"
        || text(stream, "sample_fmt")? != "fltp"
        || text(stream, "sample_rate")? != "48000"
        || uint(stream, "channels")? != 2
        || text(stream, "channel_layout")? != "stereo"
    {
        return Err(Error::unsupported("Observed master audio profile differs"));
    }
    let (tn, td) = rational(text(stream, "time_base")?)?;
    if (tn, td) != (1, 48_000) {
        return Err(Error::unsupported("Observed AAC time base differs"));
    }
    let start = optional_integer(stream, "start_pts")?;
    let duration = optional_integer(stream, "duration_ts")?;
    if duration.is_some_and(|v| v <= 0) || frames.is_empty() || frames.len() > 36_000 {
        return Err(Error::invalid("Complete decoded AAC observations missing"));
    }
    let mut records = Vec::with_capacity(frames.len());
    let mut previous = None;
    let mut samples = 0u64;
    for (i, f) in frames.iter().enumerate() {
        if uint(f, "stream_index")? != index
            || text(f, "sample_fmt")? != "fltp"
            || uint(f, "channels")? != 2
            || text(f, "channel_layout")? != "stereo"
        {
            return Err(Error::unsupported("Decoded AAC stream/layout changed"));
        }
        let pts = f["pts"]
            .as_i64()
            .ok_or_else(|| Error::unsupported("AAC PTS not observed"))?;
        let duration = f["duration"]
            .as_i64()
            .filter(|v| *v > 0)
            .ok_or_else(|| Error::unsupported("AAC frame duration not observed"))?;
        if previous.is_some_and(|p| pts <= p) {
            return Err(Error::unsupported("AAC PTS not monotonic"));
        }
        let count = uint(f, "nb_samples")?;
        if count == 0 || count > 4096 {
            return Err(Error::limit("AAC sample count bounds"));
        }
        samples = samples
            .checked_add(count)
            .ok_or_else(|| Error::limit("AAC sample sum"))?;
        // Audio sample_rate is a stream observation: ffprobe 6.1 does not print
        // it per audio frame. Do not invent a per-frame field from the stream.
        records.push(
            json!({"index":i,"pts":pts,"duration":duration,"nb_samples":count,
            "sample_format":text(f,"sample_fmt")?,"channels":uint(f,"channels")?,
            "channel_layout":text(f,"channel_layout")?}),
        );
        previous = Some(pts);
    }
    Ok(
        json!({"index":index,"codec":text(stream,"codec_name")?,"sample_format":text(stream,"sample_fmt")?,
        "sample_rate":48_000,"channels":uint(stream,"channels")?,"channel_layout":text(stream,"channel_layout")?,
        "time_base_num":tn,"time_base_den":td,"start_pts":start,"duration_ts":duration,
        "frame_count":records.len(),"decoded_sample_count":samples,"frames":records}),
    )
}

fn color(value: &Value, profile: ObservationProfile, stream: bool) -> Result<Value> {
    if text(value, "color_space")? != "bt709"
        || text(value, "color_range")? != "tv"
        || text(value, "color_primaries")? != "bt709"
        || text(value, "color_transfer")? != "bt709"
    {
        return Err(Error::new(
            "Unsupported",
            "Observed frame color is not declared limited BT.709",
        ));
    }
    observed_side_data(value, profile, stream)?;
    Ok(json!({"matrix":1,"range":1,"primaries":1,"transfer":1}))
}
fn optional_integer(value: &Value, key: &str) -> Result<Option<i64>> {
    match value.get(key) {
        None => Ok(None),
        Some(Value::String(v)) if v == "N/A" => Ok(None),
        Some(v) => v
            .as_i64()
            .map(Some)
            .ok_or_else(|| Error::invalid("Observed optional integer type")),
    }
}
fn observed_presentation(value: &Value, stream: bool) -> Result<Value> {
    // ffprobe obtains SAR through av_guess_sample_aspect_ratio from actual
    // stream/frame metadata. It is not copied from the requested dimensions.
    if text(value, "sample_aspect_ratio")? != "1:1" {
        return Err(Error::new(
            "Unsupported",
            "Observed square pixels are required",
        ));
    }
    if stream {
        let field_order = value.get("field_order").cloned().unwrap_or(Value::Null);
        if !field_order.is_null()
            && !matches!(field_order.as_str(), Some("unknown" | "progressive"))
        {
            return Err(Error::new(
                "Unsupported",
                "Observed interlaced stream is unsupported",
            ));
        }
        return Ok(json!({"sample_aspect_ratio":"1:1","field_order":field_order}));
    }
    if uint(value, "interlaced_frame")? != 0
        || uint(value, "repeat_pict")? != 0
        || uint(value, "top_field_first")? > 1
        || ["crop_top", "crop_bottom", "crop_left", "crop_right"]
            .iter()
            .any(|key| value.get(*key).and_then(Value::as_u64) != Some(0))
    {
        return Err(Error::new(
            "Unsupported",
            "Decoded frame is not an uncropped progressive image",
        ));
    }
    Ok(json!({"sample_aspect_ratio":"1:1","interlaced_frame":0,
        "top_field_first":uint(value,"top_field_first")?,"repeat_pict":0,
        "crop_top":0,"crop_bottom":0,"crop_left":0,"crop_right":0}))
}
#[derive(Debug)]
pub struct Probe {
    pub profile: ObservationProfile,
    pub frame_metadata: Vec<Value>,
    pub audio_stream: Value,
    pub frames: Vec<Frame>,
    pub stream: Value,
    pub exact_clock: bool,
    pub clock_observation: Value,
    pub ffprobe_version: String,
    pub libraries: Value,
}
pub fn parse_probe(
    bytes: &[u8],
    width: u32,
    height: u32,
    count: u64,
    fps_num: u32,
    fps_den: u32,
) -> Result<Probe> {
    parse_probe_profile(
        ObservationProfile::MotionCanvasFfv1,
        bytes,
        width,
        height,
        count,
        fps_num,
        fps_den,
    )
}

pub fn parse_probe_profile(
    profile: ObservationProfile,
    bytes: &[u8],
    width: u32,
    height: u32,
    count: u64,
    fps_num: u32,
    fps_den: u32,
) -> Result<Probe> {
    if bytes.len() > REPORT_LIMIT
        || fps_num == 0
        || fps_den == 0
        || fps_num > 120_000
        || fps_den > 1001
    {
        return Err(Error::limit("Complete frame observation bounds"));
    }
    // Existing bounded duplicate-key JSON parser runs before serde extraction.
    crate::json::parse(bytes)?;
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|_| Error::invalid("Malformed complete ffprobe observation"))?;
    let streams = value
        .get("streams")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::invalid("Missing observed streams"))?;
    if streams.len() as u64 != 1 + profile.audio_streams() {
        return Err(Error::unsupported(
            "Observed video/audio stream count differs",
        ));
    }
    let videos = streams
        .iter()
        .filter(|s| s["codec_type"] == "video")
        .collect::<Vec<_>>();
    let audios = streams
        .iter()
        .filter(|s| s["codec_type"] == "audio")
        .collect::<Vec<_>>();
    if videos.len() != 1 || audios.len() as u64 != profile.audio_streams() {
        return Err(Error::unsupported("Unexpected observed stream types"));
    }
    let stream = videos[0];
    let pixel_format = text(stream, "pix_fmt")?;
    let chroma = observed_chroma(stream)?;
    readback_filter(profile, pixel_format, chroma.as_str())?;
    if text(stream, "codec_name")? != profile.codec()
        || uint(stream, "width")? != u64::from(width)
        || uint(stream, "height")? != u64::from(height)
    {
        return Err(Error::new(
            "BackendFailed",
            "Observed stream differs from selected profile",
        ));
    }
    let stream_index = uint(stream, "index")?;
    let stream_color = color(stream, profile, true)?;
    let stream_side_data = observed_side_data(stream, profile, true)?;
    let stream_presentation = observed_presentation(stream, true)?;
    let start_pts = optional_integer(stream, "start_pts")?;
    let duration_ts = optional_integer(stream, "duration_ts")?;
    if duration_ts.is_some_and(|v| v <= 0) {
        return Err(Error::invalid("Invalid observed stream duration"));
    }
    let (tn, td) = rational(text(stream, "time_base")?)?;
    let (rn, rd) = rational(text(stream, "r_frame_rate")?)?;
    let (an, ad) = rational(text(stream, "avg_frame_rate")?)?;
    if i128::from(rn) * i128::from(fps_den) != i128::from(rd) * i128::from(fps_num)
        || i128::from(an) * i128::from(fps_den) != i128::from(ad) * i128::from(fps_num)
    {
        return Err(Error::new(
            "Unsupported",
            "Observed frame rates differ; no inferred rate",
        ));
    }
    let all_input = value
        .get("frames")
        .and_then(Value::as_array)
        .ok_or_else(|| Error::invalid("Missing complete decoded frame records"))?;
    let mut input = Vec::new();
    let mut audio_input = Vec::new();
    for frame in all_input {
        match text(frame, "media_type")? {
            "video" => input.push(frame),
            "audio" if profile.audio_streams() == 1 => audio_input.push(frame),
            _ => return Err(Error::unsupported("Unexpected decoded frame type")),
        }
    }
    if count == 0 || count > 36_000 || input.len() as u64 != count {
        return Err(Error::new(
            "BackendFailed",
            "Complete decoded video count differs",
        ));
    }
    let audio_stream = if profile.audio_streams() == 1 {
        observed_audio(audios[0], &audio_input, uint(stream, "index")?)?
    } else {
        Value::Null
    };
    let mut frames = Vec::with_capacity(input.len());
    let mut frame_metadata = Vec::with_capacity(input.len());
    let mut exact_clock = true;
    let mut previous = None;
    for (index, f) in input.iter().enumerate() {
        if text(f, "media_type")? != "video"
            || uint(f, "stream_index")? != stream_index
            || text(f, "pix_fmt")? != pixel_format
            || uint(f, "width")? != u64::from(width)
            || uint(f, "height")? != u64::from(height)
        {
            return Err(Error::new(
                "BackendFailed",
                "Decoded frame geometry/format/stream changed",
            ));
        }
        let pts = f
            .get("pts")
            .and_then(Value::as_i64)
            .ok_or_else(|| Error::new("Unsupported", "Observed frame has no integer PTS"))?;
        if previous.is_some_and(|p| pts <= p) {
            return Err(Error::new(
                "Unsupported",
                "Observed PTS are not strictly increasing",
            ));
        }
        previous = Some(pts);
        let duration = f
            .get("duration")
            .and_then(Value::as_i64)
            .filter(|v| *v > 0)
            .ok_or_else(|| Error::new("Unsupported", "Decoded frame duration is not observed"))?;
        let presentation = observed_presentation(f, false)?;
        let frame_chroma = observed_chroma(f)?;
        if pixel_format != "yuv444p" && frame_chroma != chroma {
            return Err(Error::unsupported("Decoded chroma location changed"));
        }
        let side_data = observed_side_data(f, profile, false)?;
        frame_metadata.push(json!({"chroma_location":frame_chroma,"side_data_types":side_data}));
        let residual_num = i128::from(pts) * i128::from(tn) * i128::from(fps_num)
            - (index as i128) * i128::from(fps_den) * i128::from(td);
        let residual_den = i128::from(td) * i128::from(fps_num);
        exact_clock &= residual_num == 0;
        frames.push(Frame {
            index: index as u64,
            pts,
            duration,
            presentation,
            time_base_num: tn,
            time_base_den: td,
            residual_num,
            residual_den,
            color: color(f, profile, false)?,
        });
    }
    let clock_observation = clock_observation(&frames, fps_num, fps_den)?;
    exact_clock &=
        clock_observation["exact_durations"] == true && clock_observation["exact_endpoint"] == true;
    let program = value
        .get("program_version")
        .ok_or_else(|| Error::invalid("ffprobe version missing"))?;
    let version = text(program, "version")?.to_owned();
    let libraries = value
        .get("library_versions")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty() && v.len() <= 16)
        .ok_or_else(|| Error::invalid("ffprobe library observations missing"))?;
    let mut names = std::collections::BTreeSet::new();
    let libs = libraries
        .iter()
        .map(|lib| {
            let name = text(lib, "name")?;
            if !names.insert(name) {
                return Err(Error::invalid("Duplicate decoder library"));
            }
            Ok(json!({"name":name,"major":uint(lib,"major")?,
                  "minor":uint(lib,"minor")?,"micro":uint(lib,"micro")?}))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut observed_stream = json!({"index":stream_index,"codec":profile.codec(),"pixel_format":pixel_format,
            "chroma_location":chroma,"side_data_types":stream_side_data,
            "width":uint(stream,"width")?,"height":uint(stream,"height")?,
            "time_base_num":tn,"time_base_den":td,"fps_num":an,"fps_den":ad,
            "r_frame_rate_num":rn,"r_frame_rate_den":rd,"color":stream_color,
            "start_pts":start_pts,"duration_ts":duration_ts,"presentation":stream_presentation});
    if !profile.is_native_mlt() {
        let object = observed_stream
            .as_object_mut()
            .ok_or_else(|| Error::invalid("Observed stream object"))?;
        object.remove("chroma_location");
        object.remove("side_data_types");
    }
    Ok(Probe {
        profile,
        frame_metadata,
        audio_stream,
        frames,
        exact_clock,
        clock_observation,
        ffprobe_version: version,
        libraries: json!(libs),
        stream: observed_stream,
    })
}

pub fn inspect_decoded_png(
    bytes: &[u8],
    index: u64,
    width: u32,
    height: u32,
) -> Result<DecodedPng> {
    if bytes.len() < 33 || bytes.len() > PNG_LIMIT || bytes[24] != 8 || bytes[25] != 6 {
        return Err(Error::invalid("Decoded PNG must be RGBA8 within budget"));
    }
    inspect_png_chunks(bytes, |_, _| Ok(()))?;
    let p = semwright_driver_motion_canvas::security::inspect_png(bytes)
        .map_err(|_| Error::new("BackendFailed", "Complete decoded PNG verification failed"))?;
    if p.width != width || p.height != height || p.min_alpha != 255 || p.max_alpha != 255 {
        return Err(Error::new(
            "BackendFailed",
            "Decoded RGBA geometry or opacity differs",
        ));
    }
    Ok(DecodedPng {
        file: format!("decoded-{index:06}.png"),
        bytes: bytes.len() as u64,
        sha256: sha256(bytes),
        rgba_sha256: p.pixel_sha256,
        alpha_min: p.min_alpha,
        alpha_max: p.max_alpha,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Sha256;
    fn fixture(pts: &[i64], time_base: &str) -> Vec<u8> {
        let color = json!({"color_range":"tv","color_space":"bt709","color_transfer":"bt709","color_primaries":"bt709"});
        let mut stream = color.clone();
        stream.as_object_mut().unwrap().extend(json!({"index":0,"codec_type":"video","codec_name":"ffv1","width":1920,"height":1080,"pix_fmt":"yuv444p","r_frame_rate":"30/1","avg_frame_rate":"30/1","time_base":time_base,"sample_aspect_ratio":"1:1","field_order":"progressive"}).as_object().unwrap().clone());
        let frames = pts.iter().map(|pts| {
            let mut f=color.clone();
            f.as_object_mut().unwrap().extend(json!({"media_type":"video","stream_index":0,"pts":pts,"duration":if time_base=="1/1000"{33}else{1},"width":1920,"height":1080,"pix_fmt":"yuv444p","sample_aspect_ratio":"1:1","interlaced_frame":0,"top_field_first":0,"repeat_pict":0,"crop_top":0,"crop_bottom":0,"crop_left":0,"crop_right":0}).as_object().unwrap().clone());f
        }).collect::<Vec<_>>();
        serde_json::to_vec(&json!({"streams":[stream],"frames":frames,
            "program_version":{"version":"fixture-no-native-execution"},
            "library_versions":[{"name":"libavcodec","major":60,"minor":31,"micro":102}]}))
        .unwrap()
    }
    #[test]
    fn portable_observation_limits_match_runtime_and_decoder() {
        use semwright_video_domain::native_observation as wire;
        assert_eq!(
            wire::PNG_DECODE_LIMIT,
            semwright_driver_motion_canvas::model::MAX_ASSET_BYTES
        );
        assert_eq!(wire::MAX_JSON, crate::json::MAX_JSON);
        assert_eq!(
            wire::MAX_STAGED_SOURCE_BYTES,
            crate::jobs::MAX_JOB_INPUT_BYTES
        );
        assert_eq!(
            wire::MAX_VIDEO_ARTIFACT_BYTES,
            crate::jobs::MAX_ARTIFACT_BYTES
        );
    }
    #[test]
    fn explicit_srgb_requires_opaque_and_rejects_p3_or_missing() {
        assert!(
            require_srgb_plan(
                json!({"color_space":"srgb","alpha":false})
                    .as_object()
                    .unwrap()
            )
            .is_ok()
        );
        for v in [
            json!({"color_space":"display_p3","alpha":false}),
            json!({"color_space":"srgb","alpha":true}),
            json!({}),
        ] {
            assert!(require_srgb_plan(v.as_object().unwrap()).is_err());
        }
    }
    #[test]
    fn exact_pts_are_read_and_quantized_matroska_is_not_rewritten() {
        let exact = parse_probe(&fixture(&[0, 1, 2], "1/30"), 1920, 1080, 3, 30, 1).unwrap();
        assert!(exact.exact_clock);
        let observed = parse_probe(&fixture(&[0, 33, 67], "1/1000"), 1920, 1080, 3, 30, 1).unwrap();
        assert!(!observed.exact_clock);
        assert_eq!(observed.frames[1].pts, 33);
        assert_eq!(
            (
                observed.frames[1].residual_num,
                observed.frames[1].residual_den
            ),
            (-10, 30_000)
        );
        assert_eq!(
            (
                observed.frames[2].residual_num,
                observed.frames[2].residual_den
            ),
            (10, 30_000)
        );
    }
    #[test]
    fn a_single_exact_pts_cannot_hide_quantized_duration_or_endpoint() {
        let observed = parse_probe(&fixture(&[0], "1/1000"), 1920, 1080, 1, 30, 1).unwrap();
        assert_eq!(observed.clock_observation["exact_pts"], true);
        assert_eq!(observed.clock_observation["exact_durations"], false);
        assert_eq!(observed.clock_observation["endpoint_pts"], 33);
        assert!(!observed.exact_clock);
        let two = parse_probe(&fixture(&[0, 33], "1/1000"), 1920, 1080, 2, 30, 1).unwrap();
        assert_eq!(two.clock_observation["endpoint_pts"], 66);
        assert_eq!(
            two.clock_observation["canonical_endpoint_residual_num"],
            "-20"
        );
    }
    #[test]
    fn missing_sample_aspect_ratio_is_never_inferred() {
        for section in ["streams", "frames"] {
            let mut value: Value = serde_json::from_slice(&fixture(&[0], "1/30")).unwrap();
            value[section][0]
                .as_object_mut()
                .unwrap()
                .remove("sample_aspect_ratio");
            let error = parse_probe(&serde_json::to_vec(&value).unwrap(), 1920, 1080, 1, 30, 1)
                .unwrap_err();
            assert_eq!(error.code, "BackendFailed");
            assert_eq!(
                error.message,
                "Decoded observation omitted sample_aspect_ratio"
            );
        }
    }

    #[test]
    fn geometry_and_duration_must_be_observed_not_defaulted() {
        for (field, bad) in [
            ("duration", json!(null)),
            ("duration", json!(0)),
            ("sample_aspect_ratio", json!("N/A")),
            ("sample_aspect_ratio", json!("4:3")),
            ("interlaced_frame", json!(1)),
            ("repeat_pict", json!(1)),
            ("crop_left", json!(1)),
        ] {
            let mut v: Value = serde_json::from_slice(&fixture(&[0], "1/30")).unwrap();
            v["frames"][0][field] = bad;
            assert!(parse_probe(&serde_json::to_vec(&v).unwrap(), 1920, 1080, 1, 30, 1).is_err());
        }
    }
    #[test]
    fn every_observed_frame_color_is_required() {
        let mut v: Value = serde_json::from_slice(&fixture(&[0, 1], "1/30")).unwrap();
        v["frames"][1]["color_transfer"] = json!("unknown");
        assert!(parse_probe(&serde_json::to_vec(&v).unwrap(), 1920, 1080, 2, 30, 1).is_err());
    }
    #[test]
    fn missing_duplicate_pts_and_incomplete_frames_fail() {
        assert!(parse_probe(&fixture(&[0, 0], "1/30"), 1920, 1080, 2, 30, 1).is_err());
        assert!(parse_probe(&fixture(&[0], "1/30"), 1920, 1080, 2, 30, 1).is_err());
        let mut v: Value = serde_json::from_slice(&fixture(&[0, 1], "1/30")).unwrap();
        v["frames"][1].as_object_mut().unwrap().remove("pts");
        assert!(parse_probe(&serde_json::to_vec(&v).unwrap(), 1920, 1080, 2, 30, 1).is_err());
    }
    #[test]
    fn stream_audio_and_side_data_cannot_hide() {
        let mut v: Value = serde_json::from_slice(&fixture(&[0], "1/30")).unwrap();
        v["streams"]
            .as_array_mut()
            .unwrap()
            .push(json!({"codec_type":"audio"}));
        assert!(parse_probe(&serde_json::to_vec(&v).unwrap(), 1920, 1080, 1, 30, 1).is_err());
        let mut v: Value = serde_json::from_slice(&fixture(&[0], "1/30")).unwrap();
        v["frames"][0]["side_data_list"] = json!([{"side_data_type":"ICC profile"}]);
        assert!(parse_probe(&serde_json::to_vec(&v).unwrap(), 1920, 1080, 1, 30, 1).is_err());
    }
    #[test]
    fn present_side_data_must_be_an_empty_array() {
        for bad in [json!(null), json!(""), json!({}), json!([{}]), json!(false)] {
            for section in ["streams", "frames"] {
                let mut v: Value = serde_json::from_slice(&fixture(&[0], "1/30")).unwrap();
                v[section][0]["side_data_list"] = bad.clone();
                assert!(
                    parse_probe(&serde_json::to_vec(&v).unwrap(), 1920, 1080, 1, 30, 1).is_err()
                );
            }
        }
        let mut v: Value = serde_json::from_slice(&fixture(&[0], "1/30")).unwrap();
        v["streams"][0]["side_data_list"] = json!([]);
        v["frames"][0]["side_data_list"] = json!([]);
        assert!(parse_probe(&serde_json::to_vec(&v).unwrap(), 1920, 1080, 1, 30, 1).is_ok());
    }
    #[test]
    fn film180_bound_fits_proposed_two_gib_without_raising_existing_limits() {
        let b = observation_budget(1920, 1080, 180).unwrap();
        assert_eq!(b.rgba_bytes_per_frame * 180, 1_492_992_000);
        assert!(b.retained_png_bytes_upper_bound < RETAINED_PNG_LIMIT);
        assert!(b.retained_png_bytes_upper_bound > crate::jobs::MAX_ARTIFACT_BYTES);
        assert_eq!(crate::jobs::MAX_JOB_INPUT_BYTES, 1_073_741_824);
        assert_eq!(crate::jobs::MAX_ARTIFACT_BYTES, 1_073_741_824);
        assert!(observation_budget(1920, 1080, 36_000).is_err());
    }
    #[test]
    fn rgba_digest_is_explicit_roster_not_raw_concatenation() {
        let p = DecodedPng {
            file: "decoded-000000.png".into(),
            bytes: 100,
            sha256: "b".repeat(64),
            rgba_sha256: "a".repeat(64),
            alpha_min: 255,
            alpha_max: 255,
        };
        let got = rgba_roster_digest(&[p]).unwrap();
        let mut expected = Sha256::new();
        expected.update(b"motionwright.native-rgba-sequence/1\0");
        expected.update(&0u64.to_be_bytes());
        expected.update(&[0xaa; 32]);
        assert_eq!(got, expected.finish());
        assert_ne!(got, sha256(&[0xaa; 32]));
    }
}

/// Every destination is derived from one fresh output path. Existing or unknown
/// attempts remain untouched; the caller never retries publication in place.
pub fn preflight_observation_output(
    output: &crate::fs::Root,
    path: &str,
    width: u32,
    height: u32,
    count: u64,
    native: bool,
) -> Result<()> {
    crate::fs::validate_relative(path)?;
    let budget = observation_budget(width, height, count)?;
    let mut destinations = vec![path.to_owned(), format!("{path}.color-observation.json")];
    if native {
        destinations.push(format!("{path}.native-input.mlt"));
    }
    destinations.extend((0..count).map(|i| format!("{path}.decoded-{i:06}.png")));
    for destination in destinations {
        if output.exists(&destination)? {
            return Err(Error::new(
                "Conflict",
                "Observed output or derived evidence already exists",
            ));
        }
    }
    let extra = if native {
        2 * crate::xml::MAX_XML as u64
    } else {
        0
    };
    if output.available_bytes_for(path)? < budget.peak_free_bytes + extra {
        return Err(Error::limit(
            "Complete frame evidence lacks measured free-space budget",
        ));
    }
    Ok(())
}
/// Publish the one complete observation through the existing Root. The final
/// video is published by the caller only after this report and all PNGs pass.
/// A report by itself is never a successful production receipt.
pub fn build_observation_report(
    observed: &crate::runtime::EncodedFramesObservation,
    output_path: &str,
    source_binding: Value,
    native_input_sources: Option<&[crate::fs::Artifact]>,
    width: u32,
    height: u32,
    fps_num: u32,
    fps_den: u32,
) -> Result<Vec<u8>> {
    let profile = observed.probe.profile;
    let extended = profile.is_native_mlt();
    if observed.frames.len() != observed.probe.frames.len()
        || observed.frames.len() != observed.probe.frame_metadata.len()
        || native_input_sources.is_some() != extended
    {
        return Err(Error::invalid("Measured frame/source roster disagreement"));
    }
    let budget = observation_budget(width, height, observed.frames.len() as u64)?;
    let mut frame_records = Vec::with_capacity(observed.frames.len());
    for (index, (decoded, clock)) in observed
        .frames
        .iter()
        .zip(&observed.probe.frames)
        .enumerate()
    {
        if decoded.file != format!("decoded-{index:06}.png") || clock.index != index as u64 {
            return Err(Error::invalid("Measured frame index differs"));
        }
        let mut frame = json!({
            "index":clock.index,"pts":clock.pts,"duration":clock.duration,"presentation":clock.presentation,
            "time_base_num":clock.time_base_num,"time_base_den":clock.time_base_den,
            "canonical_residual_num":clock.residual_num.to_string(),
            "canonical_residual_den":clock.residual_den.to_string(),
            "width":width,"height":height,"pixel_format":observed.probe.stream["pixel_format"],"color":clock.color,
            "file":decoded.file,"png_sha256":decoded.sha256,"png_bytes":decoded.bytes,
            "rgba_sha256":decoded.rgba_sha256,"alpha_min":decoded.alpha_min,"alpha_max":decoded.alpha_max,
        });
        if extended {
            frame["chroma_location"] =
                observed.probe.frame_metadata[index]["chroma_location"].clone();
            frame["side_data_types"] =
                observed.probe.frame_metadata[index]["side_data_types"].clone();
        }
        frame_records.push(frame);
    }
    let filter = readback_filter(
        profile,
        observed.probe.stream["pixel_format"]
            .as_str()
            .ok_or_else(|| Error::invalid("Observed pixel format"))?,
        observed.probe.stream["chroma_location"].as_str(),
    )?;
    let mut report = json!({
        "schema":if extended {"semwright.mlt-complete-frame-observation/2"}else{"semwright.mlt-complete-frame-observation/1"},
        "source_binding":source_binding,
        "artifact":{"root":"output","path":output_path,"sha256":observed.video_sha256,"bytes":observed.video_bytes},
        "source_policy":if extended {MLT_SOURCE_POLICY}else{SOURCE_POLICY},
        "color_recipe":if extended {MLT_RECIPE}else{RECIPE},
        "encoder_filter":if extended {Value::Null}else{json!(ENCODE_FILTER)},
        "decoder":{"name":"FFmpeg","version":observed.ffmpeg_version,"executable_sha256":observed.ffmpeg_sha256},
        "probe":{"name":"ffprobe","version":observed.probe.ffprobe_version,
            "executable_sha256":observed.ffprobe_sha256,"libraries":observed.probe.libraries},
        "stream":observed.probe.stream,"video_streams":1,"audio_streams":profile.audio_streams(),
        "canonical_fps_num":fps_num,"canonical_fps_den":fps_den,
        "frame_count":observed.frames.len(),"first_frame":0,"end_frame_exclusive":observed.frames.len(),
        "exact_clock":observed.probe.exact_clock,"clock_observation":observed.probe.clock_observation,
        "frames":frame_records,"frame_path_prefix":format!("{output_path}."),
        "rgba_conversion":"ffmpeg-explicit-bt709-limited-to-bt709-rgb-full-no-transfer-change",
        "rgba_filter":filter,"rgba_transfer":"bt709-nonlinear","rgba_bytes_per_frame":budget.rgba_bytes_per_frame,
        "rgba_sequence_sha256":observed.rgba_roster_sha256,
        "rgba_sequence_algorithm":"motionwright.native-rgba-sequence/1-nul+index-u64be+rgba-sha256-bytes",
        "retained_png_bytes":observed.retained_png_bytes,
        "alpha_min":255,"alpha_max":255,"all_pixels_opaque":true,
        "authority_scope":"observed data only; original Host/Store/provider-generation/job/source closure is required externally",
    });
    if let Some(sources) = native_input_sources {
        report["observation_profile"] = json!(profile.wire_name());
        report["clock_scope"] = json!("decoded-video-frames");
        report["native_input_sources"] = json!(
            sources
                .iter()
                .map(|s| json!({
                    "root":s.root,"path":s.path,"sha256":s.sha256,"bytes":s.bytes
                }))
                .collect::<Vec<_>>()
        );
        report["audio_stream"] = observed.probe.audio_stream.clone();
    }
    let bytes = serde_json::to_vec(&report)
        .map_err(|_| Error::invalid("Complete observation serialization"))?;
    parse_complete_observation(&bytes)?;
    Ok(bytes)
}

/// The same pure report builder and verified publisher serve AppRuntime and
/// the canonical Host render job. No new capability or authority is created.
pub(crate) fn publish_observation(
    observed: &crate::runtime::EncodedFramesObservation,
    work: &crate::fs::Root,
    output: &crate::fs::Root,
    output_path: &str,
    source_binding: Value,
    native_input_sources: Option<&[crate::fs::Artifact]>,
    dimensions: (u32, u32),
    fps_num: u32,
    fps_den: u32,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<crate::fs::Artifact> {
    let (width, height) = dimensions;
    let bytes = build_observation_report(
        observed,
        output_path,
        source_binding,
        native_input_sources,
        width,
        height,
        fps_num,
        fps_den,
    )?;
    publish_complete_observation(&bytes, work, output, output_path, cancel)
}

/// Called only with bytes bound to the original Runtime observation (or the
/// terminal Host job descriptor). Parsing describes data, not admission.
/// Each staged PNG is reopened and fully decoded before exclusive publication.
pub fn publish_complete_observation(
    bytes: &[u8],
    work: &crate::fs::Root,
    output: &crate::fs::Root,
    output_path: &str,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<crate::fs::Artifact> {
    publish_complete_observation_checked(bytes, work, output, output_path, || {
        if cancel.load(std::sync::atomic::Ordering::Acquire) {
            Err(Error::new("Cancelled", "Observation publication cancelled"))
        } else {
            Ok(())
        }
    })
}

/// The Host uses its existing token; AppRuntime retains its AtomicBool wrapper.
/// Both use one complete PNG decoding and verified publication path.
pub fn publish_complete_observation_checked<F: FnMut() -> Result<()>>(
    bytes: &[u8],
    work: &crate::fs::Root,
    output: &crate::fs::Root,
    output_path: &str,
    mut check: F,
) -> Result<crate::fs::Artifact> {
    check()?;
    let parsed = parse_complete_observation(bytes)?;
    let report = parsed.document();
    if report["artifact"]["path"].as_str() != Some(output_path) {
        return Err(Error::invalid(
            "Observed artifact publication binding differs",
        ));
    }
    let width = u32::try_from(uint(&report["stream"], "width")?)
        .map_err(|_| Error::invalid("Observation width"))?;
    let height = u32::try_from(uint(&report["stream"], "height")?)
        .map_err(|_| Error::invalid("Observation height"))?;
    let frames = report["frames"]
        .as_array()
        .ok_or_else(|| Error::invalid("Complete observation frames"))?;
    for (index, frame) in frames.iter().enumerate() {
        check()?;
        let basename = format!("decoded-{index:06}.png");
        if frame["file"].as_str() != Some(basename.as_str()) {
            return Err(Error::invalid("Observed frame path differs"));
        }
        let png_bytes = work.read(&basename, PNG_LIMIT)?;
        let actual = inspect_decoded_png(&png_bytes, index as u64, width, height)?;
        if frame["png_sha256"].as_str() != Some(actual.sha256.as_str())
            || frame["png_bytes"].as_u64() != Some(actual.bytes)
            || frame["rgba_sha256"].as_str() != Some(actual.rgba_sha256.as_str())
            || frame["alpha_min"].as_u64() != Some(u64::from(actual.alpha_min))
            || frame["alpha_max"].as_u64() != Some(u64::from(actual.alpha_max))
        {
            return Err(Error::new(
                "StaleReference",
                "Staged decoded PNG differs from original observation",
            ));
        }
        let png = output.publish_verified(
            "output",
            &format!("{output_path}.{basename}"),
            std::io::Cursor::new(&png_bytes),
            PNG_LIMIT as u64,
            &actual.sha256,
            actual.bytes,
        )?;
        if output.digest(&png.path, PNG_LIMIT as u64)? != png.sha256 {
            return Err(Error::new(
                "StaleReference",
                "Retained decoded PNG changed before final publication",
            ));
        }
    }
    check()?;
    let observation = output.write_new(
        "output",
        &format!("{output_path}.color-observation.json"),
        bytes,
    )?;
    if output.digest(&observation.path, REPORT_LIMIT as u64)? != observation.sha256 {
        return Err(Error::new(
            "StaleReference",
            "Retained observation changed before final video publication",
        ));
    }
    Ok(observation)
}

/// The final artifact is the success linearization point. This readback runs
/// after it without cancellation; an uncertain committed mutation is UNKNOWN
/// and must be inspected, never retried by the caller.
pub fn verify_published_video(
    output: &crate::fs::Root,
    artifact: &crate::fs::Artifact,
) -> Result<()> {
    let observed = crate::hash::reader_hash(
        output
            .read_file(&artifact.path, artifact.bytes)
            .map_err(|_| unknown_publication())?,
        artifact.bytes,
    )
    .map_err(|_| unknown_publication())?;
    if observed.0 != artifact.sha256 || observed.1 != artifact.bytes {
        return Err(unknown_publication());
    }
    Ok(())
}
fn unknown_publication() -> Error {
    let mut error = Error::new(
        "BackendFailed",
        "Video was published but final readback identity is uncertain; inspect without retry",
    );
    error.outcome_known = false;
    error
}

/// Revalidate the actual staged XML and media against their original identities.
/// AppRuntime and the Host request share this authority and confined FD reads.
pub(crate) fn verify_staged_native_inputs(
    work: &crate::fs::Root,
    project_sha256: &str,
    project_bytes: u64,
    staged_sources: &[(crate::fs::Artifact, String)],
) -> Result<()> {
    let project = work.read("project.mlt", crate::xml::MAX_XML)?;
    if project.len() as u64 != project_bytes || sha256(&project) != project_sha256 {
        return Err(Error::new("StaleReference", "Original native XML changed"));
    }
    for (source, name) in staged_sources {
        let actual = crate::hash::reader_hash(
            work.read_file(name, crate::jobs::MAX_MEDIA_BYTES)?,
            crate::jobs::MAX_MEDIA_BYTES,
        )?;
        if actual.0 != source.sha256 || actual.1 != source.bytes {
            return Err(Error::new(
                "StaleReference",
                "Original staged native source changed",
            ));
        }
    }
    Ok(())
}

/// Private files inside one existing Host scratch job. They are not new commands.
pub const NATIVE_OBSERVATION_REQUEST_FILE: &str = "native-observation-request.json";
pub const NATIVE_OBSERVATION_FILE: &str = "native-complete-observation.json";
pub const NATIVE_OBSERVATION_REQUEST_LIMIT: usize = crate::json::MAX_JSON;

/// Source data bound by the Host before render. This is not a Host receipt and
/// cannot admit an output independently of the original job and Root grants.
#[derive(Clone)]
pub struct NativeObservationRequest {
    pub output_path: String,
    pub project_sha256: String,
    pub project_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub frames: u64,
    staged_sources: Vec<(crate::fs::Artifact, String)>,
}
impl NativeObservationRequest {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > NATIVE_OBSERVATION_REQUEST_LIMIT {
            return Err(Error::limit(
                "Native observation request exceeds byte budget",
            ));
        }
        // Reuse the driver's bounded, duplicate-rejecting JSON parser.
        let doc = crate::json::parse(bytes)?;
        let fields = [
            "schema",
            "output_path",
            "project_sha256",
            "project_bytes",
            "width",
            "height",
            "fps_num",
            "fps_den",
            "frames",
            "staged_sources",
        ];
        doc.strict(&fields, &fields)?;
        if doc.str("schema")? != "semwright.mlt-native-observation-request/1" {
            return Err(Error::invalid("Native observation request schema"));
        }
        let output_path = doc.str("output_path")?.to_owned();
        crate::fs::validate_relative(&output_path)?;
        if !output_path.ends_with(".mkv") {
            return Err(Error::invalid(
                "Host visual observation requires the selected MKV output",
            ));
        }
        let project_sha256 = doc.str("project_sha256")?.to_owned();
        require_request_digest(&project_sha256)?;
        let project_bytes = doc.uint("project_bytes")?;
        if project_bytes == 0 || project_bytes > crate::xml::MAX_XML as u64 {
            return Err(Error::limit("Original native XML size"));
        }
        let as_u32 = |key| {
            u32::try_from(doc.uint(key)?)
                .map_err(|_| Error::invalid("Native observation geometry/rate"))
        };
        let width = as_u32("width")?;
        let height = as_u32("height")?;
        let fps_num = as_u32("fps_num")?;
        let fps_den = as_u32("fps_den")?;
        crate::time::FrameRate {
            num: fps_num,
            den: fps_den,
        }
        .validate()?;
        let frames = doc.uint("frames")?;
        observation_budget(width, height, frames)?;
        let inputs = doc.get("staged_sources")?.as_array()?;
        if inputs.len() > 4096 {
            return Err(Error::limit("Native observation source count"));
        }
        let mut staged_sources = Vec::with_capacity(inputs.len());
        let mut names = std::collections::BTreeSet::new();
        let mut total = 0u64;
        for input in inputs {
            let fields = ["root", "path", "sha256", "bytes", "staged_name"];
            input.strict(&fields, &fields)?;
            let root = input.str("root")?.to_owned();
            if !matches!(root.as_str(), "project" | "media" | "output") {
                return Err(Error::invalid("Native source root"));
            }
            let path = input.str("path")?.to_owned();
            crate::fs::validate_relative(&path)?;
            let sha256 = input.str("sha256")?.to_owned();
            require_request_digest(&sha256)?;
            let bytes = input.uint("bytes")?;
            if bytes == 0 || bytes > crate::jobs::MAX_MEDIA_BYTES {
                return Err(Error::limit("Native staged source size"));
            }
            total = total
                .checked_add(bytes)
                .ok_or_else(|| Error::limit("Native staged source total overflow"))?;
            if total > crate::jobs::MAX_JOB_INPUT_BYTES {
                return Err(Error::limit(
                    "Native staged sources exceed original job limit",
                ));
            }
            let staged_name = input.str("staged_name")?.to_owned();
            let name = staged_name.as_bytes();
            if name.len() != 34
                || !staged_name.starts_with("asset-")
                || !staged_name.ends_with(".bin")
                || !name[6..30]
                    .iter()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
                || !names.insert(staged_name.clone())
            {
                return Err(Error::invalid(
                    "Native staged filename differs from Host derivation",
                ));
            }
            staged_sources.push((
                crate::fs::Artifact {
                    root,
                    path,
                    sha256,
                    bytes,
                },
                staged_name,
            ));
        }
        Ok(Self {
            output_path,
            project_sha256,
            project_bytes,
            width,
            height,
            fps_num,
            fps_den,
            frames,
            staged_sources,
        })
    }

    pub fn sources(&self) -> Vec<crate::fs::Artifact> {
        self.staged_sources.iter().map(|(a, _)| a.clone()).collect()
    }

    pub fn source_binding(&self) -> Value {
        json!({"source_kind":"mlt_render_project_bytes","root":"output",
            "path":format!("{}.native-input.mlt",self.output_path),
            "sha256":self.project_sha256,"bytes":self.project_bytes,"source_first_frame":0})
    }

    pub fn verify_staged(&self, work: &crate::fs::Root) -> Result<()> {
        verify_staged_native_inputs(
            work,
            &self.project_sha256,
            self.project_bytes,
            &self.staged_sources,
        )
    }

    pub fn verify_report(&self, observation: &CompleteObservation) -> Result<()> {
        let doc = observation.document();
        let sources = self
            .sources()
            .iter()
            .map(|s| json!({"root":s.root,"path":s.path,"sha256":s.sha256,"bytes":s.bytes}))
            .collect::<Vec<_>>();
        if observation.profile() != ObservationProfile::MltVisualFfv1
            || doc["artifact"]["path"].as_str() != Some(self.output_path.as_str())
            || doc["source_binding"] != self.source_binding()
            || doc["native_input_sources"] != json!(sources)
            || doc["stream"]["width"].as_u64() != Some(u64::from(self.width))
            || doc["stream"]["height"].as_u64() != Some(u64::from(self.height))
            || doc["canonical_fps_num"].as_u64() != Some(u64::from(self.fps_num))
            || doc["canonical_fps_den"].as_u64() != Some(u64::from(self.fps_den))
            || doc["frame_count"].as_u64() != Some(self.frames)
        {
            return Err(Error::new(
                "StaleReference",
                "Native observation is not bound to this original Host request",
            ));
        }
        Ok(())
    }
}

fn require_request_digest(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Error::invalid("Native request SHA-256"));
    }
    Ok(())
}

/// Constructed only by the original Host staging path, before its render job.
pub fn native_observation_request(
    output_path: &str,
    project: &[u8],
    expected: &crate::model::Profile,
    frames: u64,
    staged_sources: &[(crate::fs::Artifact, String)],
) -> Result<Vec<u8>> {
    expected.validate()?;
    let inputs = staged_sources
        .iter()
        .map(|(s, name)| {
            json!({
                "root":s.root,"path":s.path,"sha256":s.sha256,"bytes":s.bytes,"staged_name":name,
            })
        })
        .collect::<Vec<_>>();
    let bytes = serde_json::to_vec(&json!({
        "schema":"semwright.mlt-native-observation-request/1","output_path":output_path,
        "project_sha256":sha256(project),"project_bytes":project.len(),
        "width":expected.width,"height":expected.height,"fps_num":expected.fps.num,
        "fps_den":expected.fps.den,"frames":frames,"staged_sources":inputs,
    }))
    .map_err(|_| Error::invalid("Native observation request serialization"))?;
    NativeObservationRequest::parse(&bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod png_completion_tests {
    use super::{inspect_decoded_png, inspect_source_png};

    fn opaque_rgba_png() -> Vec<u8> {
        // One 1x1 RGBA8 pixel [17, 34, 51, 255], with filter byte 0.
        // The zlib stream contains a final stored block of five bytes.
        let mut bytes = hex::decode(
            "89504e470d0a1a0a\
             0000000d49484452000000010000000108060000001f15c489",
        )
        .unwrap();
        let data = hex::decode("7801010500faff00112233ff02140166").unwrap();

        bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
        bytes.extend_from_slice(b"IDAT");
        bytes.extend_from_slice(&data);

        // Fixture construction deliberately does not call the production CRC helper.
        let mut crc = u32::MAX;
        for byte in b"IDAT".iter().chain(&data) {
            crc ^= u32::from(*byte);
            for _ in 0..8 {
                let polynomial = if crc & 1 == 0 { 0 } else { 0xedb8_8320 };
                crc = (crc >> 1) ^ polynomial;
            }
        }
        bytes.extend_from_slice(&(!crc).to_be_bytes());
        bytes.extend_from_slice(&[0, 0, 0, 0, b'I', b'E', b'N', b'D', 0xae, 0x42, 0x60, 0x82]);

        assert!(
            inspect_source_png(&bytes, 1, 1).is_ok(),
            "the complete source PNG fixture must be admitted",
        );
        assert!(
            inspect_decoded_png(&bytes, 0, 1, 1).is_ok(),
            "the complete decoded PNG fixture must be admitted",
        );
        bytes
    }

    fn density_payload(x: u32, y: u32, unit: u8) -> Vec<u8> {
        let mut payload = Vec::with_capacity(9);
        payload.extend_from_slice(&x.to_be_bytes());
        payload.extend_from_slice(&y.to_be_bytes());
        payload.push(unit);
        payload
    }

    fn with_phys_chunks(payloads: &[Vec<u8>]) -> Vec<u8> {
        let original = opaque_rgba_png();
        let mut bytes = original[..33].to_vec();
        for payload in payloads {
            bytes.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_be_bytes());
            bytes.extend_from_slice(b"pHYs");
            bytes.extend_from_slice(payload);
            // Keep the fixture CRC independent of the production chunk checker.
            let mut crc = u32::MAX;
            for byte in b"pHYs".iter().chain(payload) {
                crc ^= u32::from(*byte);
                for _ in 0..8 {
                    let polynomial = if crc & 1 == 0 { 0 } else { 0xedb8_8320 };
                    crc = (crc >> 1) ^ polynomial;
                }
            }
            bytes.extend_from_slice(&(!crc).to_be_bytes());
        }
        bytes.extend_from_slice(&original[33..]);
        bytes
    }

    #[test]
    fn source_png_admits_absent_or_square_density() {
        assert!(inspect_source_png(&with_phys_chunks(&[]), 1, 1).is_ok());
        for (x, y, unit) in [(1, 1, 0), (1, 1, 1), (3780, 3780, 0), (3780, 3780, 1)] {
            let png = with_phys_chunks(&[density_payload(x, y, unit)]);
            assert!(
                inspect_source_png(&png, 1, 1).is_ok(),
                "square pHYs must remain admitted: {x}, {y}, {unit}"
            );
        }
    }

    #[test]
    fn source_png_rejects_non_square_unknown_or_malformed_density() {
        let mut oversized = density_payload(1, 1, 0);
        oversized.push(0);
        for payload in [
            density_payload(2, 1, 0),
            density_payload(1, 2, 1),
            density_payload(0, 0, 0),
            density_payload(0, 1, 1),
            density_payload(1, 0, 1),
            density_payload(1, 1, 2),
            Vec::new(),
            vec![0; 8],
            oversized,
        ] {
            let png = with_phys_chunks(&[payload]);
            let error = inspect_source_png(&png, 1, 1).unwrap_err();
            assert_eq!(error.code, "Unsupported");
            assert_eq!(
                error.message,
                "Source PNG pixel density is not a unique square declaration"
            );
        }
    }

    #[test]
    fn source_png_rejects_duplicate_square_density() {
        let payload = density_payload(3780, 3780, 1);
        let png = with_phys_chunks(&[payload.clone(), payload]);
        let error = inspect_source_png(&png, 1, 1).unwrap_err();
        assert_eq!(error.code, "Unsupported");
        assert_eq!(
            error.message,
            "Source PNG pixel density is not a unique square declaration"
        );
    }

    fn assert_both_reject(bytes: &[u8]) {
        let source_rejected = inspect_source_png(bytes, 1, 1).is_err();
        let decoded_rejected = inspect_decoded_png(bytes, 0, 1, 1).is_err();
        assert!(
            source_rejected && decoded_rejected,
            "source rejected={source_rejected}, decoded rejected={decoded_rejected}",
        );
    }

    #[test]
    fn png_completion_rejects_missing_iend() {
        let mut bytes = opaque_rgba_png();
        bytes.truncate(bytes.len() - 12);
        assert_both_reject(&bytes);
    }

    #[test]
    fn png_completion_rejects_corrupt_iend_crc() {
        let mut bytes = opaque_rgba_png();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        assert_both_reject(&bytes);
    }

    #[test]
    fn png_completion_rejects_bytes_after_iend() {
        let mut bytes = opaque_rgba_png();
        bytes.extend_from_slice(b"unobserved trailing bytes");
        assert_both_reject(&bytes);
    }
}
