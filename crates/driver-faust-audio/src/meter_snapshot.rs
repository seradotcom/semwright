//! Fixed Host snapshot entrypoint for the existing pinned audio meter.
//! It does not grant paths, discover executables, retry, or start another scheduler.
pub(crate) const TOOL_NAME: &str = "audio-meter-snapshot";

pub fn run() -> semwright_types::Result<()> {
    #[cfg(target_os = "linux")]
    {
        linux::run()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Err(semwright_types::Error::new(
            semwright_types::ErrorCode::Unsupported,
            "Host audio snapshots require the Linux isolated profile",
        ))
    }
}
#[cfg(target_os = "linux")]
mod linux {
    use crate::analysis_runtime::{
        MAX_INPUT_BYTES, METER_NAME, snapshot_input_bounded, validate_digest, validate_name,
    };
    use semwright_driver_sdk::{validate_materialized_tool_argument, workspace_mount};
    use semwright_types::{Error, ErrorCode, Result};
    use std::{
        ffi::OsString,
        fs,
        os::unix::process::CommandExt,
        path::{Path, PathBuf},
        process::Command,
    };
    use tempfile::TempDir;
    const ADDRESS_SPACE_BYTES: u64 = 2 * 1024 * 1024 * 1024;
    const CPU_SECONDS: u64 = 30;
    #[derive(Debug)]
    struct Arguments {
        input_root: PathBuf,
        meter: PathBuf,
        file_name: String,
        expected_sha256: String,
        expected_bytes: u64,
        layout: String,
    }
    impl Arguments {
        fn parse(args: Vec<OsString>) -> Result<Self> {
            if args.len() != 8 || args[0] != "--input-root" || args[2] != "--meter" {
                return Err(Error::invalid("Invalid fixed audio snapshot arguments"));
            }
            let input_root = PathBuf::from(&args[1]);
            let meter = PathBuf::from(&args[3]);
            if !input_root.is_absolute() || !meter.is_absolute() {
                return Err(Error::invalid("Host audio paths must be absolute"));
            }
            let field = |index: usize, prefix: &str| -> Result<String> {
                args[index]
                    .to_str()
                    .and_then(|value| value.strip_prefix(prefix))
                    .map(str::to_owned)
                    .ok_or_else(|| Error::invalid("Invalid fixed audio snapshot field"))
            };
            let file_name = field(4, "--file-name=")?;
            let expected_sha256 = field(5, "--expected-sha256=")?;
            let size_text = field(6, "--expected-bytes=")?;
            let layout = field(7, "--layout=")?;
            validate_name(&file_name)?;
            validate_digest(&expected_sha256)?;
            let expected_bytes = size_text
                .parse::<u64>()
                .ok()
                .filter(|value| *value > 0 && *value <= MAX_INPUT_BYTES)
                .filter(|value| value.to_string() == size_text)
                .ok_or_else(|| Error::invalid("Invalid bounded audio snapshot size"))?;
            if !matches!(layout.as_str(), "mono" | "stereo") {
                return Err(Error::invalid(
                    "Audio analysis layout must be mono or stereo",
                ));
            }
            Ok(Self {
                input_root,
                meter,
                file_name,
                expected_sha256,
                expected_bytes,
                layout,
            })
        }
    }
    fn validate_root(root: &Path) -> Result<()> {
        if root != workspace_mount("analysis-input")? {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Audio snapshot root differs from the materialized input grant",
            ));
        }
        let metadata = fs::symlink_metadata(root)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::invalid(
                "Audio snapshot input must be a real directory",
            ));
        }
        Ok(())
    }
    fn checked_snapshot(args: &Arguments) -> Result<(TempDir, PathBuf)> {
        let (temp, staged, digest, bytes) = snapshot_input_bounded(
            &args.input_root.join(&args.file_name),
            &args.file_name,
            MAX_INPUT_BYTES,
        )?;
        if digest != args.expected_sha256 || bytes != args.expected_bytes {
            return Err(Error::new(
                ErrorCode::Conflict,
                "Host audio input differs from the driver-verified snapshot",
            ));
        }
        Ok((temp, staged))
    }
    fn restricted_limit(current: libc::rlimit, maximum: libc::rlim_t) -> libc::rlimit {
        let hard = current.rlim_max.min(maximum);
        libc::rlimit {
            rlim_cur: current.rlim_cur.min(hard),
            rlim_max: hard,
        }
    }
    fn restrict_meter_limits() -> Result<()> {
        for (resource, maximum) in [
            (libc::RLIMIT_AS, ADDRESS_SPACE_BYTES as libc::rlim_t),
            (libc::RLIMIT_CPU, CPU_SECONDS as libc::rlim_t),
            (libc::RLIMIT_NOFILE, 256 as libc::rlim_t),
            (libc::RLIMIT_NPROC, 16 as libc::rlim_t),
            (libc::RLIMIT_FSIZE, MAX_INPUT_BYTES as libc::rlim_t),
        ] {
            let mut current = libc::rlimit {
                rlim_cur: 0,
                rlim_max: 0,
            };
            // SAFETY: current is a live, correctly aligned rlimit output value.
            if unsafe { libc::getrlimit(resource, &mut current) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            let next = restricted_limit(current, maximum);
            // SAFETY: next is a live rlimit; both limits only decrease.
            if unsafe { libc::setrlimit(resource, &next) } != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        Ok(())
    }
    pub(super) fn run() -> Result<()> {
        // Apply the meter ceiling before argument allocation or snapshot copying.
        restrict_meter_limits()?;
        let args = Arguments::parse(std::env::args_os().skip(1).collect())?;
        validate_root(&args.input_root)?;
        validate_materialized_tool_argument(METER_NAME, &args.meter)?;
        let (_temp, staged) = checked_snapshot(&args)?;
        // The Host owns this same PID, its timeout and process-group cleanup.
        // The snapshot stays in this sandbox across exec. No child is spawned.
        // Successful exec never returns; an exec failure drops the TempDir.
        let error = Command::new(&args.meter)
            .arg("analyze")
            .arg(staged)
            .arg(&args.layout)
            .env_clear()
            .exec();
        Err(error.into())
    }
    #[cfg(test)]
    mod tests {
        use super::*;
        use sha2::{Digest, Sha256};
        fn args() -> Vec<OsString> {
            [
                "--input-root".to_owned(),
                "/workspace/analysis-input".to_owned(),
                "--meter".to_owned(),
                "/plugin/tools/audio-meter".to_owned(),
                "--file-name=voice.wav".to_owned(),
                format!("--expected-sha256={}", "ab".repeat(32)),
                "--expected-bytes=800".to_owned(),
                "--layout=mono".to_owned(),
            ]
            .into_iter()
            .map(OsString::from)
            .collect()
        }
        #[test]
        fn fixed_arguments_preserve_existing_wav_and_flac_basenames() {
            let parsed = Arguments::parse(args()).expect("fixed WAV request");
            assert_eq!(parsed.file_name, "voice.wav");
            let mut values = args();
            values[4] = "--file-name=-voice.flac".into();
            values[7] = "--layout=stereo".into();
            assert_eq!(
                Arguments::parse(values)
                    .expect("basename is not an option")
                    .file_name,
                "-voice.flac"
            );
        }
        #[test]
        fn fixed_arguments_reject_extra_missing_or_reordered_fields() {
            let mut extra = args();
            extra.push("--script=other.py".into());
            assert!(Arguments::parse(extra).is_err());
            let mut missing = args();
            missing.pop();
            assert!(Arguments::parse(missing).is_err());
            let mut reordered = args();
            reordered.swap(4, 5);
            assert!(Arguments::parse(reordered).is_err());
            let mut duplicate = args();
            duplicate[7] = "--expected-bytes=800".into();
            assert!(Arguments::parse(duplicate).is_err());
        }
        #[test]
        fn fixed_arguments_reject_escaped_names_unbounded_sizes_and_unpinned_shapes() {
            for (index, replacement) in [
                (1, "relative-root"),
                (3, "audio-meter"),
                (4, "--file-name=../voice.wav"),
                (4, "--file-name=.voice.wav"),
                (4, "--file-name=voice.wav/other"),
                (4, "--file-name=voice.mp3"),
                (5, "--expected-sha256=AB"),
                (6, "--expected-bytes=0"),
                (6, "--expected-bytes=536870913"),
                (6, "--expected-bytes=0800"),
                (7, "--layout=surround"),
            ] {
                let mut values = args();
                values[index] = replacement.into();
                assert!(Arguments::parse(values).is_err(), "{replacement}");
            }
        }
        fn fixture(root: &Path, bytes: &[u8]) -> Arguments {
            Arguments {
                input_root: root.to_owned(),
                meter: PathBuf::from("/plugin/tools/audio-meter"),
                file_name: "voice.wav".into(),
                expected_sha256: hex::encode(Sha256::digest(bytes)),
                expected_bytes: bytes.len() as u64,
                layout: "mono".into(),
            }
        }
        #[test]
        fn host_snapshot_is_independent_and_bound_to_the_verified_bytes() {
            let input = tempfile::tempdir().expect("input directory");
            let source = input.path().join("voice.wav");
            fs::write(&source, b"original input").expect("input bytes");
            let request = fixture(input.path(), b"original input");
            let (snapshot, staged) = checked_snapshot(&request).expect("matching snapshot");
            fs::write(&source, b"replacement bytes").expect("replace original");
            assert_eq!(
                fs::read(&staged).expect("retained snapshot"),
                b"original input"
            );
            let error = checked_snapshot(&request).expect_err("changed Host input");
            assert_eq!(error.code, ErrorCode::Conflict);
            drop(snapshot);
            assert!(!staged.exists());
        }
        #[test]
        fn host_snapshot_rejects_equal_size_digest_substitution_and_size_mismatch() {
            let input = tempfile::tempdir().expect("input directory");
            fs::write(input.path().join("voice.wav"), b"changed!").expect("input bytes");
            let request = fixture(input.path(), b"original");
            assert_eq!(
                checked_snapshot(&request)
                    .expect_err("same size, other digest")
                    .code,
                ErrorCode::Conflict
            );
            let mut request = fixture(input.path(), b"changed!");
            request.expected_bytes += 1;
            assert_eq!(
                checked_snapshot(&request).expect_err("size differs").code,
                ErrorCode::Conflict
            );
        }
        #[test]
        fn host_snapshot_rejects_symlinks_and_hardlinks_before_meter_exec() {
            use std::os::unix::fs::symlink;
            let input = tempfile::tempdir().expect("input directory");
            let original = input.path().join("original.wav");
            let source = input.path().join("voice.wav");
            fs::write(&original, b"original input").expect("input bytes");
            symlink(&original, &source).expect("symlink");
            let request = fixture(input.path(), b"original input");
            assert!(checked_snapshot(&request).is_err());
            fs::remove_file(&source).expect("remove symlink");
            fs::hard_link(&original, &source).expect("hardlink");
            assert_eq!(
                checked_snapshot(&request)
                    .expect_err("hardlinked source")
                    .code,
                ErrorCode::PermissionDenied
            );
        }
        #[test]
        fn limits_never_raise_an_existing_soft_or_hard_ceiling() {
            let current = libc::rlimit {
                rlim_cur: 7,
                rlim_max: 11,
            };
            let reduced = restricted_limit(current, 9);
            assert_eq!((reduced.rlim_cur, reduced.rlim_max), (7, 9));
            let current = libc::rlimit {
                rlim_cur: 7,
                rlim_max: 11,
            };
            let tighter = restricted_limit(current, 5);
            assert_eq!((tighter.rlim_cur, tighter.rlim_max), (5, 5));
        }
        #[test]
        fn actual_meter_limits_are_lowered_only_in_a_test_child() {
            const CHILD: &str = "SEMWRIGHT_AUDIO_METER_RLIMIT_TEST_CHILD";
            if std::env::var_os(CHILD).is_none() {
                let status = Command::new(std::env::current_exe().expect("test executable"))
                    .args(["--exact", "meter_snapshot::linux::tests::actual_meter_limits_are_lowered_only_in_a_test_child", "--nocapture"])
                    .env(CHILD, "1").status().expect("isolated limit test child");
                assert!(status.success());
                return;
            }
            restrict_meter_limits().expect("decrease process limits");
            for (resource, maximum) in [
                (libc::RLIMIT_AS, ADDRESS_SPACE_BYTES as libc::rlim_t),
                (libc::RLIMIT_CPU, CPU_SECONDS as libc::rlim_t),
            ] {
                let mut observed = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                // SAFETY: observed is a live, correctly aligned output value.
                let status = unsafe { libc::getrlimit(resource, &mut observed) };
                assert_eq!(status, 0);
                assert!(observed.rlim_cur <= maximum);
                assert!(observed.rlim_max <= maximum);
                assert!(observed.rlim_cur <= observed.rlim_max);
            }
        }
    }
}
