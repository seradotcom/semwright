use crate::model::{MAX_KEYFRAMES, MotionTimeline};
use thiserror::Error;

#[derive(Debug, Error, PartialEq)]
pub enum MotionError {
    #[error("invalid duration")]
    Duration,
    #[error("invalid keyframe")]
    Keyframe,
    #[error("too many keyframes")]
    Limit,
}

pub fn validate(t: &MotionTimeline) -> Result<(), MotionError> {
    if !t.duration.is_finite() || t.duration <= 0.0 || t.duration > 3600.0 {
        return Err(MotionError::Duration);
    }
    let mut count = 0;
    for track in &t.tracks {
        let mut last = -1.0;
        for k in &track.keyframes {
            count += 1;
            if count > MAX_KEYFRAMES {
                return Err(MotionError::Limit);
            }
            if !k.timeline_position.is_finite()
                || k.timeline_position < 0.0
                || k.timeline_position > t.duration
                || k.timeline_position < last
            {
                return Err(MotionError::Keyframe);
            }
            last = k.timeline_position;
        }
    }
    Ok(())
}
