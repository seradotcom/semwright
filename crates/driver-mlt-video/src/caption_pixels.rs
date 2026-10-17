//! Read-only FFV1-to-H.264 caption pixels, original-media admission.
//!
//! The caller never supplies an executable, filter, ROI, sample frame, output
//! file or host path. The original owner-root SHA is verified separately by
//! the existing Linux openat2 Root before any sealed FFmpeg process runs.
//! A CI proof of *other* media is never accepted as owner-media evidence.
use crate::{
    Error, Result,
    json::{Value, array, obj},
};

pub const MAX_CUES: usize = 8;
pub const MAX_FRAMES: u64 = 900;
pub const MAX_WIDTH: u32 = 1920;
pub const MAX_HEIGHT: u32 = 1080;
pub const MAX_SOURCE_BYTES: u64 = 268_435_456;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cue {
    pub start: u64,
    pub end: u64,
}

pub fn admit_cues(value: &Value, frame_count: u64) -> Result<Vec<Cue>> {
    if !(2..=MAX_FRAMES).contains(&frame_count) {
        return Err(Error::limit("Caption observation frame budget"));
    }
    let values = value.as_array()?;
    if values.is_empty() || values.len() > MAX_CUES {
        return Err(Error::limit(
            "Read-only caption observation supports 1..8 cues",
        ));
    }
    let mut cues: Vec<Cue> = Vec::with_capacity(values.len());
    for item in values {
        item.strict(
            &["start_frame", "end_frame_exclusive"],
            &["start_frame", "end_frame_exclusive"],
        )?;
        let start = item.uint("start_frame")?;
        let end = item.uint("end_frame_exclusive")?;
        // The previous inactive frame is mandatory for independent mask
        // attribution. A cue at frame 0 cannot claim this bounded contract.
        if start == 0
            || start >= end
            || end > frame_count
            || cues.last().is_some_and(|previous| previous.end >= start)
        {
            return Err(Error::invalid(
                "Native caption cues must have an inactive prior frame, be ordered, nonoverlapping and frame-bound",
            ));
        }
        cues.push(Cue { start, end });
    }
    Ok(cues)
}

fn rgba_delta(a: &[u8], b: &[u8], index: usize) -> u16 {
    (0..3)
        .map(|component| a[index + component].abs_diff(b[index + component]) as u16)
        .max()
        .unwrap_or(0)
}

fn dimensions(width: u32, height: u32, frames: &[&[u8]]) -> Result<(usize, usize)> {
    if !(2..=MAX_WIDTH).contains(&width)
        || !(2..=MAX_HEIGHT).contains(&height)
        || u64::from(width) * u64::from(height) > 2_073_600
    {
        return Err(Error::limit(
            "Caption observer RGB geometry outside bounded profile",
        ));
    }
    let len = usize::try_from(u64::from(width) * u64::from(height) * 3)
        .map_err(|_| Error::limit("Caption RGB size overflow"))?;
    if frames.iter().any(|frame| frame.len() != len) {
        return Err(Error::invalid(
            "Decoded original native RGB is not exact-size",
        ));
    }
    Ok((width as usize, height as usize))
}

#[derive(Clone, Debug)]
pub struct FrameMask {
    pub frame_index: u64,
    pub glyph_mask_pixels: u64,
    pub lossy_master_error_milli: u64,
    pub inactive_reference_error_milli: u64,
    pub static_control_error_milli: u64,
}

impl FrameMask {
    pub fn json(&self) -> Value {
        obj([
            ("frame_index", self.frame_index.into()),
            ("glyph_mask_pixels", self.glyph_mask_pixels.into()),
            (
                "lossy_master_error_milli",
                self.lossy_master_error_milli.into(),
            ),
            (
                "inactive_reference_error_milli",
                self.inactive_reference_error_milli.into(),
            ),
            (
                "static_control_error_milli",
                self.static_control_error_milli.into(),
            ),
        ])
    }
}

/// Compare original lossless FFV1 pixels one frame before an authored cue
/// with the cue-active frame, then check actual H.264 decoder readback.
/// The upper half of the FFV1 pair must be stationary so background motion
/// cannot be silently mistaken for proof of caption glyphs. This rejects
/// scene transitions instead of guessing a visual difference is a subtitle.
pub fn verify_mask(
    inactive: &[u8],
    active: &[u8],
    lossy: &[u8],
    width: u32,
    height: u32,
    frame_index: u64,
) -> Result<FrameMask> {
    let (w, h) = dimensions(width, height, &[inactive, active, lossy])?;
    // Static source background above caption safe region is prerequisite.
    // Moving or scene-changing videos must use a stronger object observer.
    let control_rows = h / 2;
    let mut control_error: u64 = 0;
    let mut control_count: u64 = 0;
    for i in (0..control_rows * w * 3).step_by(12) {
        control_error += u64::from(rgba_delta(inactive, active, i));
        control_count += 1;
    }
    let control_milli = control_error.saturating_mul(1000) / control_count.max(1);
    if control_milli > 750 {
        return Err(Error::unsupported(
            "Caption source background is not stationary outside the safe-area mask",
        ));
    }

    // Original caption grammar places text in the lower image area.
    let x0 = w * 6 / 100;
    let x1 = w * 94 / 100;
    let y0 = h * 55 / 100;
    let y1 = h * 98 / 100;
    let mut glyphs = 0u64;
    let mut sum_lossy_error = 0u64;
    let mut sum_inactive_error = 0u64;
    let mut glyph_contrast = 0u64;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = (y * w + x) * 3;
            let contrast = u64::from(rgba_delta(inactive, active, i));
            if contrast < 48 {
                continue;
            }
            glyphs += 1;
            glyph_contrast += contrast;
            sum_lossy_error += u64::from(rgba_delta(lossy, active, i));
            sum_inactive_error += u64::from(rgba_delta(lossy, inactive, i));
        }
    }
    let maximum = ((x1 - x0) * (y1 - y0) / 3).min(100_000) as u64;
    let minimum = 50u64.max(u64::from(width) * u64::from(height) / 5000);
    if glyphs < minimum
        || glyphs > maximum
        || glyph_contrast < glyphs * 52
        || sum_inactive_error <= glyphs * 20
        || sum_lossy_error.saturating_mul(3) >= sum_inactive_error.saturating_mul(2)
        || sum_lossy_error > glyphs * 48
    {
        return Err(Error::new(
            "Conflict",
            "Actual original FFV1-to-H264 native caption glyph mask is not independently verified",
        ));
    }
    Ok(FrameMask {
        frame_index,
        glyph_mask_pixels: glyphs,
        lossy_master_error_milli: sum_lossy_error.saturating_mul(1000) / glyphs,
        inactive_reference_error_milli: sum_inactive_error.saturating_mul(1000) / glyphs,
        static_control_error_milli: control_milli,
    })
}

/// Strict control for an inactive frame. Motion Canvas must actually hide
/// the glyph after its half-open cue. No OCR or source text is exposed.
pub fn verify_inactive(
    original_before: &[u8],
    original_after: &[u8],
    lossy_after: &[u8],
    width: u32,
    height: u32,
) -> Result<()> {
    let _ = dimensions(
        width,
        height,
        &[original_before, original_after, lossy_after],
    )?;
    let mut source_delta = 0u64;
    let mut master_delta = 0u64;
    let mut count = 0u64;
    for index in (0..original_before.len()).step_by(12) {
        source_delta += u64::from(rgba_delta(original_before, original_after, index));
        master_delta += u64::from(rgba_delta(original_after, lossy_after, index));
        count += 1;
    }
    if source_delta.saturating_mul(1000) > count * 2500
        || master_delta.saturating_mul(1000) > count * 25_000
    {
        return Err(Error::new(
            "Conflict",
            "Native caption did not return to independently observed stationary inactive pixels",
        ));
    }
    Ok(())
}

/// Exact source-free PASS payload, never actual pixels/text/private paths.
pub fn proof(source_sha: &str, master_sha: &str, total_frames: u64, frames: &[FrameMask]) -> Value {
    obj([
        ("status", "PASS".into()),
        (
            "evidence_scope",
            "original-confined-ffv1-h264-decoded-caption-glyphs-not-human-reading".into(),
        ),
        ("source_sha256", source_sha.to_owned().into()),
        ("master_sha256", master_sha.to_owned().into()),
        ("frame_count", total_frames.into()),
        ("sampled_frames", array(frames.iter().map(FrameMask::json))),
        ("human_readability", "NOT_RUN".into()),
        ("speech_identity", "NOT_VERIFIED".into()),
        ("publication_authorized", false.into()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::parse;
    #[test]
    fn cadence_admission_rejects_ambiguous_borders_and_overlap() {
        let accept = parse(br#"[{"start_frame":15,"end_frame_exclusive":16},{"start_frame":32,"end_frame_exclusive":33}]"#).unwrap();
        assert_eq!(admit_cues(&accept, 33).unwrap().len(), 2);
        for forged in [
            r#"[{"start_frame":0,"end_frame_exclusive":1}]"#,
            r#"[{"start_frame":32,"end_frame_exclusive":34}]"#,
            r#"[{"start_frame":2,"end_frame_exclusive":2}]"#,
            r#"[{"start_frame":2,"end_frame_exclusive":3},{"start_frame":3,"end_frame_exclusive":4}]"#,
            r#"[{"start_frame":15,"end_frame_exclusive":16,"unexpected":"a"}]"#,
            r#"[{"start_frame":2,"end_frame_exclusive":3},{"start_frame":1,"end_frame_exclusive":2}]"#,
            r#"[]"#,
        ] {
            assert!(
                admit_cues(&parse(forged.as_bytes()).unwrap(), 33).is_err(),
                "{forged}"
            );
        }
    }
    fn image(width: u32, height: u32, value: u8) -> Vec<u8> {
        vec![value; width as usize * height as usize * 3]
    }
    #[test]
    fn proof_requires_real_source_glyphs_and_retained_lossy_mask() {
        let (w, h) = (1280, 720);
        let inactive = image(w, h, 255);
        let mut active = inactive.clone();
        let mut lossy = image(w, h, 253);
        for y in 480..510 {
            for x in 290..330 {
                let i = ((y * w + x) * 3) as usize;
                active[i..i + 3].fill(0);
                lossy[i..i + 3].fill(4);
            }
        }
        let pass = verify_mask(&inactive, &active, &lossy, w, h, 15).unwrap();
        assert_eq!(pass.glyph_mask_pixels, 1200);
        assert_eq!(pass.frame_index, 15);
        assert_eq!(
            proof(&"a".repeat(64), &"b".repeat(64), 33, &[pass])
                .str("status")
                .unwrap(),
            "PASS"
        );
        assert!(verify_mask(&inactive, &inactive, &lossy, w, h, 15).is_err());
        assert!(verify_mask(&inactive, &active, &inactive, w, h, 15).is_err());
        active[0..3].fill(1);
        // A single outlier does not constitute broad background motion; but
        // explicit active glyphs still must originate in actual FFV1.
        let mut moving = inactive.clone();
        moving[..h as usize * w as usize * 3 / 2].fill(12);
        assert!(verify_mask(&inactive, &moving, &lossy, w, h, 15).is_err());
        assert!(verify_mask(&inactive, &active[..10], &lossy, w, h, 15).is_err());
    }
}
