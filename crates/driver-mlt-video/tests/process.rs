#![cfg(feature = "test-tools")]
mod common;
use semwright_mlt_video::{
    fs::Root,
    jobs,
    runtime::{self, MediaInfo, ProcessSpec, RenderProfile, ServiceCatalog},
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    sync::{Arc, atomic::AtomicBool},
    time::{Duration, Instant},
};
fn spec(mode: &str, dir: &std::path::Path) -> ProcessSpec {
    ProcessSpec {
        executable: common::fake(),
        args: vec![mode.into()],
        cwd: dir.into(),
        timeout: Duration::from_secs(3),
        cpu_seconds: 5,
        environment: BTreeMap::new(),
    }
}
#[test]
fn process_success_exit_code() {
    let d = common::temp();
    assert!(
        runtime::run(&spec("ok", d.path()), &AtomicBool::new(false))
            .unwrap()
            .success()
    );
}
#[test]
fn process_nonzero_not_success() {
    let d = common::temp();
    let r = runtime::run(&spec("fail", d.path()), &AtomicBool::new(false)).unwrap();
    assert_eq!(r.exit_code, Some(7));
    assert!(!r.success());
}
#[test]
fn process_wall_timeout() {
    let d = common::temp();
    let mut s = spec("sleep", d.path());
    s.timeout = Duration::from_millis(50);
    let begin = Instant::now();
    let r = runtime::run(&s, &AtomicBool::new(false)).unwrap();
    assert!(r.timed_out);
    assert!(begin.elapsed() < Duration::from_secs(3));
}
#[test]
fn process_cancel_before_spawn() {
    let d = common::temp();
    assert_eq!(
        runtime::run(&spec("sleep", d.path()), &AtomicBool::new(true))
            .unwrap_err()
            .code,
        "Cancelled"
    );
}
#[test]
fn process_cancel_running() {
    let d = common::temp();
    let cancel = Arc::new(AtomicBool::new(false));
    let other = cancel.clone();
    let thread = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        other.store(true, std::sync::atomic::Ordering::Release);
    });
    let r = runtime::run(&spec("sleep", d.path()), &cancel).unwrap();
    thread.join().unwrap();
    assert!(r.cancelled);
}
#[test]
fn process_stdout_flood_is_bounded() {
    let d = common::temp();
    let r = runtime::run(&spec("flood", d.path()), &AtomicBool::new(false)).unwrap();
    assert!(r.output_exceeded);
    assert!(r.stdout.len() <= 16384);
}
#[test]
fn process_stderr_flood_is_bounded() {
    let d = common::temp();
    let r = runtime::run(&spec("stderr-flood", d.path()), &AtomicBool::new(false)).unwrap();
    assert!(r.output_exceeded);
    assert!(r.stderr.len() <= 16384);
}
#[test]
fn process_argv_is_literal() {
    let d = common::temp();
    let mut s = spec("echo", d.path());
    let names = [
        "a b.mp4",
        "a;b.mp4",
        "$(touch pwned).mp4",
        "`touch pwned`",
        "quote\".mp4",
        "--help.mp4",
        "línea\n.mp4",
    ];
    s.args.extend(names.iter().map(OsString::from));
    let r = runtime::run(&s, &AtomicBool::new(false)).unwrap();
    let got = semwright_mlt_video::json::parse(&r.stdout).unwrap();
    assert_eq!(got.as_array().unwrap().len(), names.len());
    for (v, n) in got.as_array().unwrap().iter().zip(names) {
        assert_eq!(v.string().unwrap(), n);
    }
    assert!(!d.path().join("pwned").exists());
}
#[test]
fn process_environment_is_explicit() {
    let d = common::temp();
    let r = runtime::run(&spec("env", d.path()), &AtomicBool::new(false)).unwrap();
    assert_eq!(r.stdout, b"absent");
}
#[test]
fn process_cwd_is_private() {
    let d = common::temp();
    let r = runtime::run(&spec("cwd", d.path()), &AtomicBool::new(false)).unwrap();
    assert_eq!(
        String::from_utf8(r.stdout).unwrap(),
        d.path().to_str().unwrap()
    );
}
#[test]
fn process_relative_executable_denied() {
    let d = common::temp();
    let mut s = spec("ok", d.path());
    s.executable = "fake-melt".into();
    assert!(runtime::run(&s, &AtomicBool::new(false)).is_err());
}
#[test]
fn process_failed_partial_not_success() {
    let d = common::temp();
    let r = runtime::run(&spec("partial", d.path()), &AtomicBool::new(false)).unwrap();
    assert!(!r.success());
    assert_eq!(
        std::fs::read(d.path().join("partial.mkv")).unwrap(),
        b"NOT A MEDIA FILE"
    );
}
#[test]
fn process_cancellation_kills_same_group_descendant() {
    let d = common::temp();
    let mut s = spec("descendant", d.path());
    s.timeout = Duration::from_millis(250);
    let r = runtime::run(&s, &AtomicBool::new(false)).unwrap();
    assert!(r.timed_out);
    let pid = std::fs::read_to_string(d.path().join("descendant.pid")).unwrap();
    let stat = std::fs::read_to_string(format!("/proc/{}/stat", pid.trim()));
    assert!(
        stat.is_err()
            || stat
                .unwrap()
                .split(')')
                .nth(1)
                .is_some_and(|s| s.trim_start().starts_with('Z')),
        "Descendant is still running"
    );
}
#[test]
fn service_discovery_excludes_untrusted_tokens() {
    let set = ServiceCatalog::parse_list(
        "---\nfilters:\n - volume\n - brightness\n - bad service\n - $(bad)\n...",
    )
    .unwrap();
    assert_eq!(set.len(), 2);
    assert!(set.contains("volume"));
}
#[test]
fn profile_unavailable_without_codec_probe() {
    let catalog = ServiceCatalog::default();
    for p in RenderProfile::all() {
        assert!(!p.available(&catalog));
    }
}
#[test]
fn media_probe_rational_duration() {
    let v=br#"{"streams":[{"codec_type":"video","codec_name":"ffv1","width":160,"height":90,"duration_ts":50,"time_base":"1/25","nb_frames":"50"}],"format":{}}"#;
    let m = MediaInfo::parse(v).unwrap();
    assert_eq!((m.duration_num, m.duration_den), (50, 25));
    assert_eq!(m.frames, Some(50));
    assert!(m.video);
    assert!(!m.audio);
}
#[test]
fn native_project_cannot_reach_render_runtime() {
    let d = common::temp();
    let p =
        semwright_mlt_video::adapters::load(&common::fixture("kdenlive/simple.kdenlive")).unwrap();
    let roots = BTreeMap::from([(
        "output".into(),
        Arc::new(Root::open(d.path(), true, true).unwrap()),
    )]);
    assert!(
        jobs::render_plan(
            &p,
            &p.sequences[0].id,
            "revision",
            &RenderProfile::get("lossless").unwrap(),
            "test.mkv",
            None,
            &roots
        )
        .is_err()
    );
}

#[test]
fn probe_duration_capacity_is_rational_not_nominal_fps() {
    let info = semwright_mlt_video::runtime::MediaInfo {
        duration_num: 1001,
        duration_den: 1000,
        ..Default::default()
    };
    let rate = semwright_mlt_video::time::FrameRate::new(30000, 1001).unwrap();
    assert_eq!(info.frame_capacity(rate).unwrap(), Some(30));
}
#[test]
fn probe_without_duration_is_not_a_source_handle_guarantee() {
    let info = semwright_mlt_video::runtime::MediaInfo::default();
    let rate = semwright_mlt_video::time::FrameRate::new(25, 1).unwrap();
    assert_eq!(info.frame_capacity(rate).unwrap(), None);
}
