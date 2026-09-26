//! Prevent semantic audio data from being silently omitted by compiler v2.
//!
//! The managed model can describe a mix for later assembly, but the observed
//! Motion Canvas compiler emits only `audio.first()` and no volume control.
//! Until an evidenced mapping exists, unsupported mixes must not compile as if
//! they had been honored. This guard does not implement a mixer or an exporter.
use crate::{Error, ErrorCode, Result, model::Project};

pub fn validate(project: &Project) -> Result<()> {
    if project.audio.len() > 1 {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "MOTION_CANVAS_AUDIO_MIX_UNSUPPORTED: compiler v2 can represent only one project audio track; assemble multiple tracks through a verified audio/video provider",
        ));
    }
    if project
        .audio
        .first()
        .is_some_and(|track| track.volume != 1.0)
    {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "MOTION_CANVAS_AUDIO_GAIN_UNSUPPORTED: compiler v2 does not emit volume; only unity gain is representable",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Asset, AssetKind, AudioTrack};

    fn fixture(volume: f64) -> Project {
        let mut project = Project::empty("audio-contract".into());
        project.generation = "0".repeat(32);
        project.assets.push(Asset {
            id: "narration".into(),
            kind: AssetKind::Audio,
            path: "assets/narration.wav".into(),
            sha256: "a".repeat(64),
            bytes: 44,
            dimensions: None,
            duration_ms: Some(1000),
            provenance: None,
            license: None,
        });
        project.audio.push(AudioTrack {
            id: "voice".into(),
            asset: "narration".into(),
            offset_ms: 200,
            volume,
        });
        project
    }

    #[test]
    fn no_audio_and_single_unity_gain_track_are_representable() {
        validate(&Project::empty("silent".into())).unwrap();
        let project = fixture(1.0);
        validate(&project).unwrap();
        let generated = crate::compiler::compile(&project).unwrap();
        let metadata: serde_json::Value =
            serde_json::from_slice(&generated.files["src/project.meta"]).unwrap();
        assert_eq!(metadata["shared"]["audioOffset"], serde_json::json!(0.2));
    }

    #[test]
    fn two_tracks_are_not_silently_reduced_to_the_first() {
        let mut project = fixture(1.0);
        project.audio.push(AudioTrack {
            id: "music".into(),
            asset: "narration".into(),
            offset_ms: 0,
            volume: 1.0,
        });
        crate::validate::project_valid(&project).unwrap();
        assert!(validate(&project).is_err());
        assert!(crate::compiler::compile(&project).is_err());
    }

    #[test]
    fn zero_attenuated_and_nonfinite_gain_are_not_ignored() {
        for volume in [0.0, 0.5, 0.999, f64::NAN, f64::INFINITY] {
            let project = fixture(volume);
            assert!(validate(&project).is_err());
        }
    }

    #[test]
    fn rejection_does_not_modify_semantic_audio() {
        let project = fixture(0.5);
        let before = serde_json::to_vec(&project).unwrap();
        assert!(crate::compiler::compile(&project).is_err());
        assert_eq!(serde_json::to_vec(&project).unwrap(), before);
    }
}
