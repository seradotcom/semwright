use semwright_audio_domain::{
    model::{AudioProfile, AudioProject},
    presets::{self, SfxPreset},
    time::SampleRate,
};
use semwright_faust_audio::faust::compile_project_synth;
use std::{path::PathBuf, process::Command};

fn faust_bin() -> PathBuf {
    std::env::var_os("SEMWRIGHT_FAUST_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/usr/bin/faust"))
}

fn compile_real(source: &str, name: &str) {
    let faust = faust_bin();
    assert!(
        faust.is_file(),
        "real Faust compiler not found at {}",
        faust.display()
    );
    let temp = tempfile::tempdir().unwrap();
    let dsp = temp.path().join(format!("{name}.dsp"));
    let cpp = temp.path().join(format!("{name}.cpp"));
    std::fs::write(&dsp, source).unwrap();
    let output = Command::new(&faust)
        .arg(&dsp)
        .arg("-o")
        .arg(&cpp)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "Faust rejected {name}: {}\nSOURCE:\n{source}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(cpp.is_file() && cpp.metadata().unwrap().len() > 0);
}

#[test]
#[ignore = "requires a real Faust compiler and standard libraries"]
fn real_faust_accepts_every_semwright_sfx_preset() {
    for preset in SfxPreset::ALL {
        let mut project = AudioProject::new(AudioProfile {
            sample_rate: SampleRate(48_000),
            channels: 2,
            ..AudioProfile::default()
        })
        .unwrap();
        let synth = presets::synth_for(*preset, "sfx", SampleRate(48_000), 48_000, 42).unwrap();
        project.synths.insert("sfx".into(), synth);
        project.validate().unwrap();
        let program = compile_project_synth(&project, "sfx", 48_000, 2).unwrap();
        compile_real(&program.source, preset.as_str());
    }
}

#[test]
#[ignore = "native helper diagnostic only; does not replace Broker conformance"]
fn fixed_interpreter_renders_every_preset_without_the_host_harness() {
    use std::io::Write;
    use std::process::Stdio;
    let helper = PathBuf::from(
        std::env::var_os("SEMWRIGHT_TEST_FAUST_HELPER")
            .expect("pinned native helper must be provided"),
    );
    for preset in SfxPreset::ALL {
        let mut project = AudioProject::new(AudioProfile::default()).unwrap();
        project.synths.insert(
            "sfx".into(),
            presets::synth_for(*preset, "sfx", SampleRate(48000), 48000, 42).unwrap(),
        );
        let program = compile_project_synth(&project, "sfx", 48000, 2).unwrap();
        let temp = tempfile::tempdir().unwrap();
        let mut child = Command::new(&helper)
            .args(["render", "/usr/share/faust"])
            .arg(temp.path())
            .args(["48000", "48000", "wav", "16"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(program.source.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "native helper failed for {}: {}",
            preset.as_str(),
            String::from_utf8_lossy(&output.stderr)
        );
        let receipt: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(receipt["frames"], 48000);
        assert!(temp.path().join("render.wav").metadata().unwrap().len() > 192000);
    }
}
