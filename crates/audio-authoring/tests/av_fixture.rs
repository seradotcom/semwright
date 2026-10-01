use semwright_audio_domain::model::AudioProject;
use semwright_media_time::{CueGraph, Rate, Round};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Crossfade {
    overlap_frames: u64,
    left_fade_out_frames: u64,
    right_fade_in_frames: u64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Resampling {
    source_rate: u32,
    source_frames: u64,
    target_rate: u32,
    expected_target_frames: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AvSync {
    video_rate_num: u32,
    video_rate_den: u32,
    video_frame: i64,
    audio_rate: u32,
    expected_audio_frame: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    schema_version: u32,
    project: AudioProject,
    cues: CueGraph,
    crossfade: Crossfade,
    resampling: Resampling,
    av_sync: AvSync,
}

#[test]
fn technical_av_fixture_uses_real_audio_and_media_time_contracts() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../fixtures/audio/av-technical-fixture.json"
    ))
    .unwrap();
    assert_eq!(fixture.schema_version, 1);
    fixture.project.validate().unwrap();
    let resolved = fixture.cues.resolve().unwrap();
    assert_eq!(resolved.len(), 3);
    assert_eq!(fixture.project.stems[0].id, "silence");
    assert_eq!(fixture.project.tempo_changes.len(), 1);
    assert_eq!(fixture.project.midi_phrases.len(), 1);
    assert_eq!(
        fixture.crossfade.overlap_frames,
        fixture.crossfade.left_fade_out_frames
    );
    assert_eq!(
        fixture.crossfade.overlap_frames,
        fixture.crossfade.right_fade_in_frames
    );

    let source_rate = Rate::new(fixture.resampling.source_rate, 1).unwrap();
    let target_rate = Rate::new(fixture.resampling.target_rate, 1).unwrap();
    let duration = source_rate
        .at(i64::try_from(fixture.resampling.source_frames).unwrap())
        .unwrap();
    assert_eq!(
        target_rate
            .quantize(duration, Round::NearestAway)
            .unwrap()
            .index,
        fixture.resampling.expected_target_frames
    );

    let video_rate = Rate::new(
        fixture.av_sync.video_rate_num,
        fixture.av_sync.video_rate_den,
    )
    .unwrap();
    let audio_rate = Rate::new(fixture.av_sync.audio_rate, 1).unwrap();
    let time = video_rate.at(fixture.av_sync.video_frame).unwrap();
    let audio = audio_rate.quantize(time, Round::NearestAway).unwrap();
    assert_eq!(audio.index, fixture.av_sync.expected_audio_frame);
    assert_eq!(audio.error.num, 0);
}
