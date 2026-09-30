//! Technical AV sync marker detection over bounded ffprobe frame metadata.
//! The driver never accepts a filter graph or argv from a capability payload.
use crate::{Error, Result};

pub const MAX_SYNC_CUES: usize = 16;
pub const MIN_WINDOW_US: u64 = 10_000;
pub const MAX_WINDOW_US: u64 = 500_000;
pub const MAX_FULL_SCAN_US: u64 = 60_000_000;
pub const MAX_SYNC_METADATA_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_SYNC_ROWS: usize = 8192;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CueWindow {
    pub id: String,
    pub expected_us: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detection {
    pub cue_id: String,
    pub presentation_time_us: u64,
    pub uncertainty_us: u64,
    pub confidence: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeResult {
    pub flashes: Vec<Detection>,
    pub impulses: Vec<Detection>,
    pub missing_video: Vec<String>,
    pub missing_audio: Vec<String>,
    pub exhaustive_video: bool,
    pub exhaustive_audio: bool,
}

#[derive(Clone, Copy)]
enum Metric {
    Luma,
    PeakDb,
}

fn number(value: &serde_json::Value) -> Option<f64> {
    match value {
        serde_json::Value::Number(value) => value.as_f64(),
        serde_json::Value::String(value) => match value.as_str() {
            "-inf" | "-Infinity" => Some(-200.0),
            "inf" | "+inf" | "Infinity" | "+Infinity" => None,
            value => value.parse::<f64>().ok(),
        },
        _ => None,
    }
    .filter(|value| value.is_finite())
}

fn time_us(frame: &serde_json::Value) -> Option<u64> {
    let value = frame
        .get("best_effort_timestamp_time")
        .or_else(|| frame.get("pts_time"))
        .and_then(number)?;
    if !(0.0..=600.0).contains(&value) {
        return None;
    }
    let micros = (value * 1_000_000.0).round();
    (micros.is_finite() && (0.0..=600_000_000.0).contains(&micros)).then_some(micros as u64)
}

fn rows(bytes: &[u8], tag: &str) -> Result<Vec<(u64, f64)>> {
    if bytes.is_empty() || bytes.len() > MAX_SYNC_METADATA_BYTES {
        return Err(Error::limit(
            "Sync probe metadata exceeds bounded full-scan budget",
        ));
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| Error::invalid("Malformed ffprobe sync JSON"))?;
    let frames = value
        .get("frames")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| Error::invalid("ffprobe sync result omitted frames"))?;
    if frames.len() > MAX_SYNC_ROWS {
        return Err(Error::limit("Sync probe returned too many frame records"));
    }
    let mut result = Vec::with_capacity(frames.len());
    for frame in frames {
        let Some(time) = time_us(frame) else {
            continue;
        };
        let Some(metric) = frame
            .get("tags")
            .and_then(|tags| tags.get(tag))
            .and_then(number)
        else {
            continue;
        };
        result.push((time, metric));
    }
    result.sort_by_key(|(time, _)| *time);
    result.dedup_by_key(|(time, _)| *time);
    Ok(result)
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.total_cmp(b));
    let middle = values.len() / 2;
    Some(if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}

fn uncertainty(rows: &[(u64, f64)], window_us: u64) -> u64 {
    let mut gaps = rows
        .windows(2)
        .filter_map(|pair| pair[1].0.checked_sub(pair[0].0))
        .filter(|gap| *gap > 0)
        .collect::<Vec<_>>();
    if gaps.is_empty() {
        return window_us;
    }
    gaps.sort_unstable();
    (gaps[gaps.len() / 2] / 2).max(1)
}

fn detection(
    cue: &CueWindow,
    bytes: &[u8],
    tag: &str,
    metric: Metric,
    window_us: u64,
) -> Result<Option<Detection>> {
    let all_rows = rows(bytes, tag)?;
    let start = cue.expected_us.saturating_sub(window_us);
    let end = cue
        .expected_us
        .checked_add(window_us)
        .ok_or_else(|| Error::limit("Sync detection window overflow"))?
        .min(600_000_000);
    let rows = all_rows
        .into_iter()
        .filter(|(time, _)| (start..=end).contains(time))
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return Ok(None);
    }
    let baseline = median(rows.iter().map(|(_, value)| *value).collect())
        .ok_or_else(|| Error::invalid("Sync metric baseline is absent"))?;
    let &(time, maximum) = rows
        .iter()
        .max_by(|left, right| {
            left.1
                .total_cmp(&right.1)
                .then_with(|| right.0.cmp(&left.0))
        })
        .ok_or_else(|| Error::invalid("Sync metric maximum is absent"))?;
    let delta = maximum - baseline;
    let confidence = match metric {
        Metric::Luma => {
            if maximum < 64.0 || delta < 16.0 {
                return Ok(None);
            }
            ((delta / 64.0) * 10_000.0).round().clamp(0.0, 10_000.0) as u16
        }
        Metric::PeakDb => {
            if maximum < -24.0 || delta < 12.0 {
                return Ok(None);
            }
            ((delta / 18.0) * 10_000.0).round().clamp(0.0, 10_000.0) as u16
        }
    };
    Ok(Some(Detection {
        cue_id: cue.id.clone(),
        presentation_time_us: time,
        uncertainty_us: uncertainty(&rows, window_us),
        confidence,
    }))
}

pub fn flash(cue: &CueWindow, bytes: &[u8], window_us: u64) -> Result<Option<Detection>> {
    detection(
        cue,
        bytes,
        "lavfi.signalstats.YAVG",
        Metric::Luma,
        window_us,
    )
}

pub fn impulse(cue: &CueWindow, bytes: &[u8], window_us: u64) -> Result<Option<Detection>> {
    detection(
        cue,
        bytes,
        "lavfi.astats.Overall.Peak_level",
        Metric::PeakDb,
        window_us,
    )
}

pub fn valid_cue_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 96
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(time: &str, tag: &str, value: &str) -> serde_json::Value {
        serde_json::json!({
            "best_effort_timestamp_time": time,
            "tags": { tag: value }
        })
    }

    #[test]
    fn bright_flash_is_detected_with_frame_uncertainty() {
        let tag = "lavfi.signalstats.YAVG";
        let value = serde_json::json!({"frames":[
            frame("0.966667",tag,"12.0"),
            frame("1.000000",tag,"240.0"),
            frame("1.033333",tag,"13.0")
        ]});
        let detection = flash(
            &CueWindow {
                id: "flash".into(),
                expected_us: 1_000_000,
            },
            &serde_json::to_vec(&value).unwrap(),
            100_000,
        )
        .unwrap()
        .unwrap();
        assert_eq!(detection.presentation_time_us, 1_000_000);
        assert!((16_000..=17_000).contains(&detection.uncertainty_us));
        assert_eq!(detection.confidence, 10_000);
    }

    #[test]
    fn audio_impulse_requires_level_and_contrast() {
        let tag = "lavfi.astats.Overall.Peak_level";
        let value = serde_json::json!({"frames":[
            frame("0.998667",tag,"-80.0"),
            frame("1.000000",tag,"-2.0"),
            frame("1.001333",tag,"-78.0")
        ]});
        let detection = impulse(
            &CueWindow {
                id: "impulse".into(),
                expected_us: 1_000_000,
            },
            &serde_json::to_vec(&value).unwrap(),
            100_000,
        )
        .unwrap()
        .unwrap();
        assert_eq!(detection.presentation_time_us, 1_000_000);
        assert!(detection.uncertainty_us <= 667);
        assert_eq!(detection.confidence, 10_000);
    }

    #[test]
    fn ordinary_frames_do_not_become_markers() {
        let tag = "lavfi.signalstats.YAVG";
        let value = serde_json::json!({"frames":[
            frame("1.0",tag,"80"),
            frame("1.03",tag,"83"),
            frame("1.06",tag,"81")
        ]});
        assert!(
            flash(
                &CueWindow {
                    id: "none".into(),
                    expected_us: 1_000_000,
                },
                &serde_json::to_vec(&value).unwrap(),
                100_000,
            )
            .unwrap()
            .is_none()
        );
    }

    #[test]
    fn full_scan_dataset_resolves_each_cue_inside_its_own_window() {
        let tag = "lavfi.signalstats.YAVG";
        let value = serde_json::json!({"frames":[
            frame("0.000000",tag,"10"),
            frame("1.000000",tag,"240"),
            frame("1.033333",tag,"11"),
            frame("1.966667",tag,"12"),
            frame("2.000000",tag,"220"),
            frame("2.033333",tag,"12"),
            frame("3.000000",tag,"10")
        ]});
        let bytes = serde_json::to_vec(&value).unwrap();
        let first = flash(
            &CueWindow {
                id: "one".into(),
                expected_us: 1_000_000,
            },
            &bytes,
            100_000,
        )
        .unwrap()
        .unwrap();
        let second = flash(
            &CueWindow {
                id: "two".into(),
                expected_us: 2_000_000,
            },
            &bytes,
            100_000,
        )
        .unwrap()
        .unwrap();
        assert_eq!(first.presentation_time_us, 1_000_000);
        assert_eq!(second.presentation_time_us, 2_000_000);
    }

    #[test]
    fn cue_ids_are_strict_data_not_filter_syntax() {
        assert!(valid_cue_id("cue-1"));
        assert!(!valid_cue_id("x;movie=/etc/passwd"));
    }
}

#[cfg(test)]
mod equal_peak_tests {
    use super::*;

    fn frame(time: &str, tag: &str, value: &str) -> serde_json::Value {
        serde_json::json!({
            "best_effort_timestamp_time": time,
            "tags": { tag: value }
        })
    }

    #[test]
    fn equal_peak_prefers_earliest_presentation_timestamp() {
        let tag = "lavfi.signalstats.YAVG";
        let value = serde_json::json!({"frames":[
            frame("0.966667",tag,"12"),
            frame("1.000000",tag,"240"),
            frame("1.033333",tag,"240"),
            frame("1.066667",tag,"12")
        ]});
        let detection = flash(
            &CueWindow {
                id: "flash".into(),
                expected_us: 1_000_000,
            },
            &serde_json::to_vec(&value).unwrap(),
            100_000,
        )
        .unwrap()
        .unwrap();
        assert_eq!(detection.presentation_time_us, 1_000_000);
    }
}
