#![allow(dead_code)] // Shared test helpers are intentionally not all used by every test crate.
use semwright_mlt_video::{
    adapters,
    edit::{self, Edit},
    fs::PrivateDir,
    model::*,
    time::FrameRange,
};
use std::path::Path;

pub fn fixture(path: &str) -> Vec<u8> {
    std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(path),
    )
    .unwrap()
}
pub fn sample() -> Project {
    adapters::load(&fixture("mlt/simple.mlt")).unwrap()
}
pub fn apply(p: &Project, edit: Edit) -> Project {
    edit::plan(p, &edit::revision(p).unwrap(), edit, "test-plan", false)
        .unwrap()
        .result
}
pub fn temp() -> PrivateDir {
    let base = std::fs::canonicalize(Path::new("/tmp")).unwrap();
    PrivateDir::new(&base).unwrap()
}
pub fn color(id: &str) -> MediaAsset {
    MediaAsset {
        id: id.into(),
        name: id.into(),
        kind: "color".into(),
        resource: Resource::Color("red".into()),
        frames: Some(100_000),
        service: "color".into(),
        original: None,
        proxy: None,
        opaque: false,
    }
}
pub fn track(id: &str) -> Track {
    Track {
        id: id.into(),
        name: id.into(),
        kind: "av".into(),
        muted: false,
        hidden: false,
        lanes: vec![Timeline {
            id: format!("lane_{id}"),
            clips: vec![],
        }],
        effects: vec![],
        opaque: false,
    }
}
pub fn clip(id: &str, asset: &str, start: u64, length: u64) -> Clip {
    Clip {
        id: id.into(),
        name: id.into(),
        asset: asset.into(),
        start,
        source: FrameRange::new(0, length).unwrap(),
        effects: vec![],
        binding: None,
        speed: (1, 1),
    }
}
pub fn small() -> Project {
    let mut p = Project::new(Profile::default()).unwrap();
    p.assets.insert("a".into(), color("a"));
    let mut t = track("t");
    t.lanes[0].clips.push(clip("c", "a", 0, 100));
    p.sequences[0].tracks.push(t);
    p.validate().unwrap();
    p
}
#[cfg(feature = "test-tools")]
pub fn fake() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_BIN_EXE_fake-melt"))
}
