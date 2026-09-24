use semwright_video_domain::{
    model::{Clip, Editability, MediaAsset, Project, Resource, Timeline, Track},
    render::{RenderCapability, RenderIntent, RenderPreset, RenderRange, RenderSupport},
    time::FrameRange,
};

fn project() -> Project {
    let mut project = Project::new(Default::default()).unwrap();
    project.assets.insert(
        "asset0".into(),
        MediaAsset {
            id: "asset0".into(),
            name: "source".into(),
            kind: "video".into(),
            resource: Resource::Relative("source.mov".into()),
            frames: Some(200),
            editability: Editability::Editable,
        },
    );
    project.sequences[0].tracks.push(Track {
        id: "track0".into(),
        name: "Video 1".into(),
        kind: "video".into(),
        muted: false,
        hidden: false,
        lanes: vec![Timeline {
            id: "lane0".into(),
            clips: vec![Clip {
                id: "clip0".into(),
                name: "clip".into(),
                asset: "asset0".into(),
                start: 0,
                source: FrameRange::new(0, 100).unwrap(),
                effects: vec![],
                speed: (1, 1),
            }],
        }],
        effects: vec![],
        editability: Editability::Editable,
    });
    project
}

fn preset() -> RenderPreset {
    RenderPreset {
        id: "h264-1080p".into(),
        width: Some(1920),
        height: Some(1080),
        video_codec: Some("h264".into()),
        audio_codec: Some("aac".into()),
        container: "mp4".into(),
        extension: "mp4".into(),
    }
}

#[test]
fn full_sequence_render_resolves_semantically() {
    let project = project();
    let intent = RenderIntent::full("sequence0", preset());
    let range = intent.validate_against(&project).unwrap();
    assert_eq!(range, FrameRange::new(0, 100).unwrap());
    assert_eq!(intent.expected_frames(&project).unwrap(), 100);
}

#[test]
fn bounded_render_range_must_fit_sequence() {
    let project = project();
    let mut intent = RenderIntent::full("sequence0", preset());
    intent.range = RenderRange::Frames(FrameRange::new(25, 75).unwrap());
    assert_eq!(intent.expected_frames(&project).unwrap(), 50);

    intent.range = RenderRange::Frames(FrameRange::new(90, 110).unwrap());
    assert_eq!(
        intent.validate_against(&project).unwrap_err().code,
        "InvalidArgument"
    );
}

#[test]
fn render_preset_rejects_backend_or_path_like_tokens() {
    let mut bad_codec = preset();
    bad_codec.video_codec = Some("libx264 --preset fast".into());
    assert_eq!(bad_codec.validate().unwrap_err().code, "InvalidArgument");

    let mut bad_extension = preset();
    bad_extension.extension = "../mp4".into();
    assert_eq!(
        bad_extension.validate().unwrap_err().code,
        "InvalidArgument"
    );

    for ambiguous in [".mp4", "mp4.", "..", "h264..main"] {
        let mut bad = preset();
        bad.id = ambiguous.into();
        assert_eq!(bad.validate().unwrap_err().code, "InvalidArgument");
    }
}
#[test]
fn audio_only_render_is_valid_without_dimensions() {
    let preset = RenderPreset {
        id: "audio-wav".into(),
        width: None,
        height: None,
        video_codec: None,
        audio_codec: Some("pcm_s16le".into()),
        container: "wav".into(),
        extension: "wav".into(),
    };
    preset.validate().unwrap();
}

#[test]
fn dimensions_require_video_and_complete_pair() {
    let mut incomplete_dimensions = preset();
    incomplete_dimensions.height = None;
    assert_eq!(
        incomplete_dimensions.validate().unwrap_err().code,
        "InvalidArgument"
    );

    let mut dimensions_without_video = preset();
    dimensions_without_video.video_codec = None;
    assert_eq!(
        dimensions_without_video.validate().unwrap_err().code,
        "InvalidArgument"
    );
}

#[test]
fn render_intent_digest_is_deterministic_and_content_sensitive() {
    let project = project();
    let a = RenderIntent::full("sequence0", preset());
    let mut b = a.clone();
    assert_eq!(
        a.semantic_digest(&project).unwrap(),
        b.semantic_digest(&project).unwrap()
    );
    b.range = RenderRange::Frames(FrameRange::new(0, 50).unwrap());
    assert_ne!(
        a.semantic_digest(&project).unwrap(),
        b.semantic_digest(&project).unwrap()
    );
}

#[test]
fn render_support_is_capability_not_authority() {
    let available = RenderCapability {
        preset: preset(),
        support: RenderSupport::Available,
    };
    available.validate().unwrap();
    assert!(available.support.allows_render());

    let unavailable = RenderCapability {
        preset: preset(),
        support: RenderSupport::Unavailable {
            reasons: vec!["encoder_unavailable".into()],
        },
    };
    unavailable.validate().unwrap();
    assert!(!unavailable.support.allows_render());

    let invalid = RenderCapability {
        preset: preset(),
        support: RenderSupport::Unavailable { reasons: vec![] },
    };
    assert_eq!(invalid.validate().unwrap_err().code, "InvalidArgument");

    let control_text = RenderCapability {
        preset: preset(),
        support: RenderSupport::Unsupported {
            reason: "backend\nfailed".into(),
        },
    };
    assert_eq!(control_text.validate().unwrap_err().code, "InvalidArgument");
}

#[test]
fn render_intent_roundtrips_as_backend_neutral_json() {
    let intent = RenderIntent::full("sequence0", preset());
    let json = serde_json::to_string(&intent).unwrap();
    assert!(!json.contains("melt"));
    assert!(!json.contains("libx264"));
    let decoded: RenderIntent = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded, intent);
}

#[test]
fn render_contract_rejects_unknown_struct_fields() {
    let mut value = serde_json::to_value(RenderIntent::full("sequence0", preset())).unwrap();
    value.as_object_mut().unwrap().insert(
        "native_encoder".into(),
        serde_json::Value::String("libx264".into()),
    );
    assert!(serde_json::from_value::<RenderIntent>(value).is_err());

    let mut value = serde_json::to_value(preset()).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("backend_flag".into(), serde_json::Value::Bool(true));
    assert!(serde_json::from_value::<RenderPreset>(value).is_err());
}
