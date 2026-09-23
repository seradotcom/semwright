#![no_main]

use libfuzzer_sys::fuzz_target;
use semwright_video_domain::{
    edit::{self, Edit},
    model::{
        Clip, Editability, MediaAsset, Profile, Project, Resource, Timeline, Track,
    },
    time::FrameRange,
};

fn fixture() -> Project {
    let mut project = Project::new(Profile::default()).expect("valid profile");
    project.assets.insert(
        "a".into(),
        MediaAsset {
            id: "a".into(),
            name: "asset".into(),
            kind: "video".into(),
            resource: Resource::Relative("a.mp4".into()),
            frames: Some(10_000),
            editability: Editability::Editable,
        },
    );
    project.sequences[0].tracks.push(Track {
        id: "t".into(),
        name: "track".into(),
        kind: "video".into(),
        muted: false,
        hidden: false,
        lanes: vec![Timeline {
            id: "lane".into(),
            clips: vec![Clip {
                id: "c".into(),
                name: "clip".into(),
                asset: "a".into(),
                start: 0,
                source: FrameRange::new(0, 100).expect("valid clip range"),
                effects: vec![],
                speed: (1, 1),
            }],
        }],
        effects: vec![],
        editability: Editability::Editable,
    });
    project.validate().expect("valid fixture");
    project
}

fuzz_target!(|data: &[u8]| {
    let mut project = fixture();

    for (index, chunk) in data.chunks(4).take(32).enumerate() {
        if chunk.len() < 4 {
            break;
        }
        let a = u64::from(chunk[1]);
        let b = u64::from(chunk[2]);
        let intent = match chunk[0] % 7 {
            0 => Edit::MarkerAdd {
                sequence: "sequence0".into(),
                frame: (a * 16 + b) % 2_000,
                label: format!("m-{index}"),
            },
            1 => Edit::TrackMute {
                sequence: "sequence0".into(),
                track: "t".into(),
                value: chunk[3] & 1 == 1,
            },
            2 => Edit::TrackHide {
                sequence: "sequence0".into(),
                track: "t".into(),
                value: chunk[3] & 1 == 1,
            },
            3 => {
                let start = 100 + ((a * 8 + b) % 1_000);
                let len = 1 + u64::from(chunk[3] % 32);
                Edit::Insert {
                    sequence: "sequence0".into(),
                    track: "t".into(),
                    asset: "a".into(),
                    start,
                    source: FrameRange::new(200, 200 + len).expect("bounded range"),
                    name: format!("insert-{index}"),
                    ripple: chunk[3] & 1 == 1,
                }
            }
            4 => {
                let raw = (u16::from(chunk[2]) << 8) | u16::from(chunk[3]);
                let value = -60_000 + i64::from(raw) * 84_000 / i64::from(u16::MAX);
                Edit::AudioVolume {
                    sequence: "sequence0".into(),
                    clip: "c".into(),
                    value,
                }
            }
            5 => Edit::Trim {
                sequence: "sequence0".into(),
                clip: "c".into(),
                source: FrameRange::new(a % 40, 60 + b % 40).expect("bounded trim"),
            },
            _ => Edit::Split {
                sequence: "sequence0".into(),
                clip: "c".into(),
                at: 1 + ((a + b) % 98),
            },
        };

        if let Ok(outcome) = edit::apply(&project, intent, &format!("fuzz-{index}")) {
            project = outcome.result;
            project
                .validate()
                .expect("successful semantic edit must preserve invariants");
            project
                .semantic_digest()
                .expect("successful semantic edit must remain digestible");
        }
    }
});
