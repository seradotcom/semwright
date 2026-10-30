//! Closed raw-original custody for the already admitted video output Root.
//! A failed semantic parser never decides whether its input bytes survive.
use crate::{
    Error, Result,
    fs::{Artifact, PrivateDir, Root},
    hash::{reader_hash, valid_digest},
};
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Copy)]
pub enum VideoOriginalKind {
    MotionFrames,
    NativeRender,
    HostNativeRender,
    InputProbe,
    AvAudioDecode,
}

impl VideoOriginalKind {
    pub fn prefix(self) -> &'static str {
        match self {
            Self::MotionFrames => "ffv1",
            Self::NativeRender | Self::HostNativeRender => "native-video",
            Self::InputProbe => "native-input-probe",
            Self::AvAudioDecode => "av-audio",
        }
    }

    pub fn stages(self) -> &'static [&'static str] {
        match self {
            Self::MotionFrames => &crate::runtime::FFV1_CUSTODY_STAGES,
            Self::InputProbe => &["media-probe"],
            Self::AvAudioDecode => &["audio-extract", "media-probe"],
            Self::NativeRender => &[
                "render",
                "media-probe",
                "complete-probe",
                "decoder-version",
                "decode",
            ],
            Self::HostNativeRender => &["render", "complete-probe", "decoder-version", "decode"],
        }
    }

    fn native(self) -> bool {
        matches!(self, Self::NativeRender | Self::HostNativeRender)
    }
}

struct Member {
    from_inputs: bool,
    basename: String,
    destination: String,
    maximum: u64,
    reuse_exact_publication: bool,
    required_on_success: bool,
}

/// The member list is derived before the original native effect. There is no
/// directory scan, caller-selected basename, foreign Root, or retry.
pub struct VideoOriginals {
    output: Root,
    output_path: String,
    kind: VideoOriginalKind,
    workspace_receipt: Option<Artifact>,
    source_kind: &'static str,
    members: Vec<Member>,
    manifest_path: String,
    manifest_limit: usize,
    failure_receipt_basename: String,
    failure_receipt_limit: usize,
    process_backup_bound: u64,
    pub(crate) required_free_bytes: u64,
}

impl VideoOriginals {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        output: &Root,
        work: &Root,
        output_path: &str,
        dimensions: (u32, u32),
        frames: u64,
        video_basename: &str,
        kind: VideoOriginalKind,
        input_probe_count: usize,
    ) -> Result<Self> {
        if !output.readable || !output.writable {
            return Err(Error::new(
                "PermissionDenied",
                "Original custody requires the existing read/write output Root",
            ));
        }
        if !matches!(
            (kind, video_basename),
            (VideoOriginalKind::MotionFrames, "mezzanine.mkv")
                | (
                    VideoOriginalKind::NativeRender,
                    "partial.mkv" | "partial.mp4"
                )
                | (VideoOriginalKind::HostNativeRender, "partial.mkv")
        ) {
            return Err(Error::invalid(
                "Original video basename is not a closed native role",
            ));
        }
        if input_probe_count >= 1_000_000 || (!kind.native() && input_probe_count != 0) {
            return Err(Error::limit(
                "Original input-probe roster is outside its closed bound",
            ));
        }
        crate::fs::validate_relative(output_path)?;
        if !work.readable || !work.writable {
            return Err(Error::new(
                "PermissionDenied",
                "Original failure receipt requires the existing owned work Root",
            ));
        }
        let failure_receipt_basename =
            format!("{}-raw-originals-retention.original.json", kind.prefix());
        if work.exists(&failure_receipt_basename)? {
            return Err(Error::new(
                "Conflict",
                "Original final-retention receipt already exists",
            ));
        }
        let budget = crate::color::observation_budget(dimensions.0, dimensions.1, frames)?;
        let native_member = if kind.native() { 1usize } else { 0usize };
        let host_calls = if matches!(kind, VideoOriginalKind::HostNativeRender) {
            // P input probes + one final probe = input_probe_count. The detached
            // render is one additional original Host call, never a native probe.
            input_probe_count
                .checked_add(1)
                .ok_or_else(|| Error::limit("Original Host call count overflow"))?
        } else {
            0
        };
        if matches!(kind, VideoOriginalKind::HostNativeRender) && input_probe_count == 0 {
            return Err(Error::invalid(
                "Host visual roster must include its original final media probe",
            ));
        }
        let host_extra = if host_calls == 0 {
            0
        } else {
            host_calls
                .checked_mul(3)
                .and_then(|count| count.checked_add(3))
                .ok_or_else(|| Error::limit("Original Host member count overflow"))?
        };
        let expected_members = usize::try_from(frames)
            .ok()
            .and_then(|count| count.checked_add(3 + native_member + 3 * kind.stages().len()))
            .and_then(|count| {
                input_probe_count
                    .checked_mul(4)
                    .and_then(|probes| count.checked_add(probes))
            })
            .and_then(|count| count.checked_add(host_extra))
            .ok_or_else(|| Error::limit("Original member count overflow"))?;
        // Bound the allocation as well as the serialized manifest before any
        // member vector or per-input probe roster is materialized here.
        let process_error_bound = input_probe_count
            .checked_add(1)
            .and_then(|n| n.checked_add(host_calls))
            .and_then(|n| n.checked_mul(crate::runtime::FFV1_CUSTODY_METADATA))
            .ok_or_else(|| Error::limit("Original process error-report bound overflow"))?;
        let manifest_limit = expected_members
            .checked_mul(output_path.len() + 2048)
            .and_then(|n| n.checked_add(65_536))
            .and_then(|n| n.checked_add(process_error_bound))
            .filter(|n| *n <= crate::color::REPORT_LIMIT)
            .ok_or_else(|| Error::limit("Original member manifest exceeds its closed bound"))?;
        let failure_receipt_limit = manifest_limit
            .checked_add(crate::runtime::FFV1_CUSTODY_METADATA)
            .ok_or_else(|| Error::limit("Original final-retention receipt bound overflow"))?;
        // This is also the decoder's enforced RLIMIT_FSIZE before its spawn.
        // It includes the overflow-detection sentinel, without parsing it.
        let png_limit = budget.png_bytes_upper_bound;
        let mut members = vec![Member {
            from_inputs: false,
            basename: video_basename.to_owned(),
            destination: format!("{output_path}.raw-original.{video_basename}"),
            maximum: crate::jobs::MAX_ARTIFACT_BYTES,
            reuse_exact_publication: false,
            required_on_success: true,
        }];
        for index in 0..=frames {
            let basename = format!("decoded-{index:06}.png");
            members.push(Member {
                from_inputs: false,
                destination: format!("{output_path}.{basename}"),
                basename,
                maximum: png_limit,
                reuse_exact_publication: true,
                required_on_success: index < frames,
            });
        }
        if kind.native() {
            members.push(Member {
                from_inputs: true,
                basename: "project.mlt".into(),
                destination: format!("{output_path}.native-input.after.original.mlt"),
                maximum: crate::xml::MAX_XML as u64,
                reuse_exact_publication: false,
                required_on_success: true,
            });
        }
        if host_calls != 0 {
            // The existing color peak explicitly includes the work report and
            // normal published report (two REPORT_LIMIT copies). This separate
            // raw copy is charged by raw_bound, not by a second work allowance.
            let basename = crate::color::NATIVE_OBSERVATION_FILE;
            members.push(Member {
                from_inputs: false,
                basename: basename.into(),
                destination: format!("{output_path}.raw-original.{basename}"),
                maximum: crate::color::REPORT_LIMIT as u64,
                reuse_exact_publication: false,
                required_on_success: false,
            });
        }
        let process_members_start = members.len();
        let prefix = kind.prefix();
        for stage in kind.stages() {
            for stream in ["stdout", "stderr"] {
                let basename = format!("{prefix}-{stage}.{stream}.original.bin");
                members.push(Member {
                    from_inputs: false,
                    destination: format!("{output_path}.{basename}"),
                    basename,
                    maximum: crate::runtime::ffv1_stream_original_limit(stage) as u64,
                    reuse_exact_publication: true,
                    required_on_success: true,
                });
            }
            let basename = format!("{prefix}-{stage}.terminal.original.json");
            members.push(Member {
                from_inputs: false,
                destination: format!("{output_path}.{basename}"),
                basename,
                maximum: crate::runtime::FFV1_CUSTODY_METADATA as u64,
                reuse_exact_publication: true,
                required_on_success: true,
            });
        }
        let basename = format!("{prefix}-process-custody.original.json");
        members.push(Member {
            from_inputs: false,
            destination: format!("{output_path}.{basename}"),
            basename,
            maximum: crate::runtime::FFV1_CUSTODY_METADATA as u64,
            reuse_exact_publication: true,
            required_on_success: true,
        });
        // The original caller freezes this count from its BTreeSet of actual
        // non-color input assets before entering that same ordered probe loop.
        for slot in 0..input_probe_count {
            for (tail, maximum) in [
                (
                    "media-probe.stdout.original.bin",
                    crate::runtime::FFV1_CUSTODY_STREAM_PREFIX,
                ),
                (
                    "media-probe.stderr.original.bin",
                    crate::runtime::FFV1_CUSTODY_STREAM_PREFIX,
                ),
                (
                    "media-probe.terminal.original.json",
                    crate::runtime::FFV1_CUSTODY_METADATA,
                ),
                (
                    "process-custody.original.json",
                    crate::runtime::FFV1_CUSTODY_METADATA,
                ),
            ] {
                let basename = format!("native-input-probe-{slot:06}-{tail}");
                members.push(Member {
                    from_inputs: false,
                    destination: format!("{output_path}.{basename}"),
                    basename,
                    maximum: maximum as u64,
                    reuse_exact_publication: true,
                    required_on_success: true,
                });
            }
        }
        if host_calls != 0 {
            for basename in [
                crate::runtime::NATIVE_DIAGNOSTIC_FILE,
                crate::runtime::VALIDATION_DIAGNOSTIC_FILE,
            ] {
                members.push(Member {
                    from_inputs: false,
                    basename: basename.into(),
                    destination: format!("{output_path}.raw-original.{basename}"),
                    maximum: crate::runtime::NATIVE_DIAGNOSTIC_LIMIT as u64,
                    reuse_exact_publication: false,
                    required_on_success: false,
                });
            }
            for slot in 0..host_calls {
                for (tail, maximum) in [
                    (
                        "stdout.original.bin",
                        crate::runtime::FFV1_CUSTODY_HOST_STREAM,
                    ),
                    (
                        "stderr.original.bin",
                        crate::runtime::FFV1_CUSTODY_HOST_STREAM,
                    ),
                    (
                        "terminal.original.json",
                        crate::runtime::FFV1_CUSTODY_METADATA,
                    ),
                ] {
                    let basename = format!("native-host-call-{slot:06}.{tail}");
                    members.push(Member {
                        from_inputs: false,
                        destination: format!("{output_path}.{basename}"),
                        basename,
                        maximum: maximum as u64,
                        reuse_exact_publication: true,
                        required_on_success: true,
                    });
                }
            }
        }
        let manifest_path = format!("{output_path}.raw-originals.json");
        if members.len() != expected_members {
            return Err(Error::new(
                "Internal",
                "Original member count differs from its declared roster",
            ));
        }
        let raw_bound = members
            .iter()
            .try_fold(manifest_limit as u64, |sum, member| {
                sum.checked_add(member.maximum)
                    .ok_or_else(|| Error::limit("Original member budget overflow"))
            })?;
        // Each process member exists in BOTH owned work and output. Explicitly
        // charge its work copy, including every real input probe, rather than
        // consuming the unrelated fixed color scratch margin. This remains
        // conservative when work and output are on separate filesystems.
        let process_backup_bound =
            members[process_members_start..]
                .iter()
                .try_fold(0u64, |sum, member| {
                    sum.checked_add(member.maximum)
                        .ok_or_else(|| Error::limit("Owned process backup bound overflow"))
                })?;
        // The final receipt is one bounded, predeclared write to the same owned
        // work Root only if manifest publication/readback fails. Never recurse.
        let required_free_bytes = budget
            .peak_free_bytes
            .checked_add(raw_bound)
            .and_then(|n| n.checked_add(process_backup_bound))
            .and_then(|n| n.checked_add(failure_receipt_limit as u64))
            .ok_or_else(|| Error::limit("Original custody peak budget overflow"))?;
        for path in members
            .iter()
            .map(|member| member.destination.as_str())
            .chain([manifest_path.as_str()])
        {
            crate::fs::validate_relative(path)?;
            if output.exists(path)? {
                return Err(Error::new(
                    "Conflict",
                    "Original destination already exists; retain the original attempt",
                ));
            }
        }
        if output.available_bytes_for(output_path)? < required_free_bytes {
            return Err(Error::limit(
                "Output lacks the closed raw-original custody peak",
            ));
        }
        let source_kind = if kind.native() {
            "native-render-xml-bytes"
        } else {
            "motion-frame-manifest-bytes"
        };
        Ok(Self {
            output: output.try_clone()?,
            output_path: output_path.to_owned(),
            kind,
            workspace_receipt: None,
            source_kind,
            members,
            manifest_path,
            manifest_limit,
            failure_receipt_basename,
            failure_receipt_limit,
            process_backup_bound,
            required_free_bytes,
        })
    }

    /// Original outer AV probes and final audio extraction share one closed
    /// auxiliary owner. This does not borrow the video color/staging allowance.
    pub fn new_av_auxiliary(
        output: &Root,
        work: &Root,
        master_output_path: &str,
        maximum_bytes: u64,
    ) -> Result<Self> {
        crate::fs::validate_relative(master_output_path)?;
        if !master_output_path.ends_with(".mp4")
            || maximum_bytes == 0
            || maximum_bytes > crate::jobs::MAX_ARTIFACT_BYTES
        {
            return Err(Error::invalid(
                "AV auxiliary custody requires its original bounded MP4 role",
            ));
        }
        let output_path = format!("{master_output_path}.av-aux");
        let mut members = Vec::new();
        for (basename, maximum) in [
            ("video-probe.bin", crate::jobs::MAX_MEDIA_BYTES + 1),
            ("audio-probe.bin", crate::jobs::MAX_MEDIA_BYTES + 1),
            ("master.mp4", maximum_bytes),
        ] {
            members.push(Member {
                from_inputs: true,
                basename: basename.into(),
                destination: format!("{output_path}.raw-original.{basename}"),
                maximum,
                reuse_exact_publication: false,
                required_on_success: true,
            });
        }
        members.push(Member {
            from_inputs: false,
            basename: "decoded-audio.wav".into(),
            destination: format!("{output_path}.raw-original.decoded-audio.wav"),
            maximum: maximum_bytes,
            reuse_exact_publication: false,
            required_on_success: true,
        });
        for slot in 0..2 {
            Self::append_input_probe_members(&mut members, &output_path, slot);
        }
        let kind = VideoOriginalKind::AvAudioDecode;
        for stage in kind.stages() {
            for (tail, maximum) in [
                (
                    "stdout.original.bin",
                    crate::runtime::FFV1_CUSTODY_STREAM_PREFIX,
                ),
                (
                    "stderr.original.bin",
                    crate::runtime::FFV1_CUSTODY_STREAM_PREFIX,
                ),
                (
                    "terminal.original.json",
                    crate::runtime::FFV1_CUSTODY_METADATA,
                ),
            ] {
                let basename = format!("{}-{stage}.{tail}", kind.prefix());
                members.push(Member {
                    from_inputs: false,
                    destination: format!("{output_path}.{basename}"),
                    basename,
                    maximum: maximum as u64,
                    reuse_exact_publication: true,
                    required_on_success: true,
                });
            }
        }
        let basename = format!("{}-process-custody.original.json", kind.prefix());
        members.push(Member {
            from_inputs: false,
            destination: format!("{output_path}.{basename}"),
            basename,
            maximum: crate::runtime::FFV1_CUSTODY_METADATA as u64,
            reuse_exact_publication: true,
            required_on_success: true,
        });
        Self::fixed_roster(
            output,
            work,
            &output_path,
            kind,
            "staged-av-master-bytes",
            members,
            19,
            3,
            maximum_bytes,
        )
    }

    /// The caller derives this operation scope from its existing request digest,
    /// not a media path alone. Preserve the exact admitted argument bytes as a
    /// separate original; their file hash is not the command/args pair digest.
    pub fn new_host_asset_probe(output: &Root, work: &Root, output_path: &str) -> Result<Self> {
        let mut members = Vec::new();
        for (basename, maximum) in [
            ("probe.bin", crate::jobs::MAX_MEDIA_BYTES + 1),
            ("import-request.original.json", crate::json::MAX_JSON as u64),
        ] {
            members.push(Member {
                from_inputs: true,
                basename: basename.into(),
                destination: format!("{output_path}.raw-original.{basename}"),
                maximum,
                reuse_exact_publication: false,
                required_on_success: true,
            });
        }
        Self::append_input_probe_members(&mut members, output_path, 0);
        for (tail, maximum) in [
            (
                "stdout.original.bin",
                crate::runtime::FFV1_CUSTODY_HOST_STREAM,
            ),
            (
                "stderr.original.bin",
                crate::runtime::FFV1_CUSTODY_HOST_STREAM,
            ),
            (
                "terminal.original.json",
                crate::runtime::FFV1_CUSTODY_METADATA,
            ),
        ] {
            let basename = format!("native-host-call-000000.{tail}");
            members.push(Member {
                from_inputs: false,
                destination: format!("{output_path}.{basename}"),
                basename,
                maximum: maximum as u64,
                reuse_exact_publication: true,
                required_on_success: true,
            });
        }
        Self::fixed_roster(
            output,
            work,
            output_path,
            VideoOriginalKind::InputProbe,
            "input-media-bytes",
            members,
            9,
            2,
            0,
        )
    }

    fn append_input_probe_members(members: &mut Vec<Member>, output_path: &str, slot: usize) {
        for (tail, maximum) in [
            (
                "media-probe.stdout.original.bin",
                crate::runtime::FFV1_CUSTODY_STREAM_PREFIX,
            ),
            (
                "media-probe.stderr.original.bin",
                crate::runtime::FFV1_CUSTODY_STREAM_PREFIX,
            ),
            (
                "media-probe.terminal.original.json",
                crate::runtime::FFV1_CUSTODY_METADATA,
            ),
            (
                "process-custody.original.json",
                crate::runtime::FFV1_CUSTODY_METADATA,
            ),
        ] {
            let basename = format!("native-input-probe-{slot:06}-{tail}");
            members.push(Member {
                from_inputs: false,
                destination: format!("{output_path}.{basename}"),
                basename,
                maximum: maximum as u64,
                reuse_exact_publication: true,
                required_on_success: true,
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn fixed_roster(
        output: &Root,
        work: &Root,
        output_path: &str,
        kind: VideoOriginalKind,
        source_kind: &'static str,
        members: Vec<Member>,
        expected_members: usize,
        process_groups: usize,
        normal_publication_bound: u64,
    ) -> Result<Self> {
        if !output.readable || !output.writable || !work.readable || !work.writable {
            return Err(Error::new(
                "PermissionDenied",
                "Original auxiliary custody requires existing read/write output and owned work Roots",
            ));
        }
        crate::fs::validate_relative(output_path)?;
        if members.len() != expected_members {
            return Err(Error::new(
                "Internal",
                "Original auxiliary member count differs from its closed roster",
            ));
        }
        let failure_receipt_basename =
            format!("{}-raw-originals-retention.original.json", kind.prefix());
        if work.exists(&failure_receipt_basename)? {
            return Err(Error::new(
                "Conflict",
                "Original final-retention receipt already exists",
            ));
        }
        let process_error_bound = process_groups
            .checked_mul(crate::runtime::FFV1_CUSTODY_METADATA)
            .ok_or_else(|| Error::limit("Auxiliary process error-report bound overflow"))?;
        let manifest_limit = expected_members
            .checked_mul(output_path.len() + 2048)
            .and_then(|n| n.checked_add(65_536))
            .and_then(|n| n.checked_add(process_error_bound))
            .filter(|n| *n <= crate::color::REPORT_LIMIT)
            .ok_or_else(|| Error::limit("Auxiliary original manifest exceeds its closed bound"))?;
        let failure_receipt_limit = manifest_limit
            .checked_add(crate::runtime::FFV1_CUSTODY_METADATA)
            .ok_or_else(|| Error::limit("Auxiliary final-retention receipt bound overflow"))?;
        let raw_bound = members.iter().try_fold(0u64, |sum, member| {
            sum.checked_add(member.maximum)
                .ok_or_else(|| Error::limit("Auxiliary original member bound overflow"))
        })?;
        let process_backup_bound = members
            .iter()
            .filter(|member| member.reuse_exact_publication)
            .try_fold(0u64, |sum, member| {
                sum.checked_add(member.maximum)
                    .ok_or_else(|| Error::limit("Auxiliary process backup bound overflow"))
            })?;
        // Both complete copies are explicit: the original owned files and the
        // published raw copies. The normal decoded WAV is a third distinct
        // publication; no unrelated video 1 GiB staging margin is consumed.
        let required_free_bytes = raw_bound
            .checked_mul(2)
            .and_then(|n| n.checked_add(manifest_limit as u64))
            .and_then(|n| n.checked_add(failure_receipt_limit as u64))
            .and_then(|n| n.checked_add(normal_publication_bound))
            .ok_or_else(|| Error::limit("Auxiliary original custody peak overflow"))?;
        let manifest_path = format!("{output_path}.raw-originals.json");
        for path in members
            .iter()
            .map(|member| member.destination.as_str())
            .chain([manifest_path.as_str()])
        {
            crate::fs::validate_relative(path)?;
            if output.exists(path)? {
                return Err(Error::new(
                    "Conflict",
                    "Original auxiliary destination exists; retain the original attempt",
                ));
            }
        }
        if output.available_bytes_for(output_path)? < required_free_bytes {
            return Err(Error::limit(
                "Output lacks the closed auxiliary original custody peak",
            ));
        }
        Ok(Self {
            output: output.try_clone()?,
            output_path: output_path.to_owned(),
            kind,
            workspace_receipt: None,
            source_kind,
            members,
            manifest_path,
            manifest_limit,
            failure_receipt_basename,
            failure_receipt_limit,
            process_backup_bound,
            required_free_bytes,
        })
    }

    /// Freeze the exact relative instances and finite original/staging roster
    /// before the first staging write or native probe. This receipt observes an
    /// existing grant; it grants no path authority or process-terminal fact.
    pub fn declare_workspaces(
        &mut self,
        inputs: &PrivateDir,
        work: &PrivateDir,
        input_members: &[(String, u64)],
    ) -> Result<Artifact> {
        if self.workspace_receipt.is_some() {
            return Err(Error::new(
                "Conflict",
                "Owned workspace declaration cannot be repeated",
            ));
        }
        let input_identity = inputs.owned_output_identity()?.ok_or_else(|| {
            Error::new(
                "PermissionDenied",
                "Original inputs must already belong to the output Root",
            )
        })?;
        let work_identity = work.owned_output_identity()?.ok_or_else(|| {
            Error::new(
                "PermissionDenied",
                "Original work must already belong to the output Root",
            )
        })?;
        let (device, inode) = self.output.instance_identity()?;
        let output_identity = serde_json::json!({"root":"output","device":device,"inode":inode});
        if input_identity["output_root"] != output_identity
            || work_identity["output_root"] != output_identity
            || input_identity["relative_path"]
                != format!("{}.raw-original-inputs", self.output_path)
            || work_identity["relative_path"] != format!("{}.raw-original-work", self.output_path)
        {
            return Err(Error::new(
                "StaleReference",
                "Owned workspaces do not name the original output Root and operation",
            ));
        }
        let work_count = self
            .members
            .iter()
            .filter(|member| !member.from_inputs)
            .count()
            .checked_add(1)
            .ok_or_else(|| Error::limit("Owned work roster count overflow"))?;
        let count = input_members
            .len()
            .checked_add(work_count)
            .ok_or_else(|| Error::limit("Owned workspace roster count overflow"))?;
        // Allocation guard before materializing the closed metadata vectors.
        // The charged serialized cap below is tighter and uses these exact
        // names/roles, not a fixed 8 MiB allowance.
        let receipt_structure_limit = count
            .checked_mul(self.output_path.len() + 2048)
            .and_then(|bytes| bytes.checked_add(65_536))
            .filter(|bytes| *bytes <= crate::color::REPORT_LIMIT)
            .ok_or_else(|| Error::limit("Owned workspace declaration exceeds its closed bound"))?;
        let mut seen = std::collections::BTreeSet::new();
        let mut staged_sum = 0u64;
        let mut xml_declared = false;
        let mut request_declared = false;
        let mut import_request_declared = false;
        let auxiliary = matches!(
            self.kind,
            VideoOriginalKind::InputProbe | VideoOriginalKind::AvAudioDecode
        );
        for (basename, maximum) in input_members {
            crate::fs::validate_relative(basename)?;
            if basename.contains('/') || basename.len() > 128 || !seen.insert(basename.as_str()) {
                return Err(Error::invalid(
                    "Owned input roster must have unique direct bounded basenames",
                ));
            }
            if basename == "project.mlt" {
                if !self.kind.native() || *maximum != crate::xml::MAX_XML as u64 {
                    return Err(Error::invalid(
                        "Original XML bound does not match its closed role",
                    ));
                }
                xml_declared = true;
            } else if basename == crate::color::NATIVE_OBSERVATION_REQUEST_FILE {
                if !matches!(self.kind, VideoOriginalKind::HostNativeRender)
                    || *maximum != crate::color::NATIVE_OBSERVATION_REQUEST_LIMIT as u64
                {
                    return Err(Error::invalid(
                        "Host observation request is outside its existing role",
                    ));
                }
                request_declared = true;
            } else if basename == "import-request.original.json" {
                if !matches!(self.kind, VideoOriginalKind::InputProbe)
                    || *maximum != crate::json::MAX_JSON as u64
                {
                    return Err(Error::invalid(
                        "Import request original is outside its closed metadata role",
                    ));
                }
                import_request_declared = true;
            } else {
                let maximum_allowed = if auxiliary {
                    self.members
                        .iter()
                        .find(|member| member.from_inputs && member.basename == *basename)
                        .map(|member| member.maximum)
                        .ok_or_else(|| {
                            Error::invalid("Auxiliary input is outside the original closed roster")
                        })?
                } else if self.kind.native() {
                    crate::jobs::MAX_MEDIA_BYTES
                } else {
                    32 * 1024 * 1024
                };
                if *maximum == 0 || *maximum > maximum_allowed {
                    return Err(Error::limit(
                        "Owned staged-source member exceeds the existing input ceiling",
                    ));
                }
                staged_sum = staged_sum
                    .checked_add(*maximum)
                    .ok_or_else(|| Error::limit("Owned staged-source aggregate overflow"))?;
            }
        }
        if auxiliary
            && (input_members.len()
                != self
                    .members
                    .iter()
                    .filter(|member| member.from_inputs)
                    .count()
                || self
                    .members
                    .iter()
                    .filter(|member| member.from_inputs)
                    .any(|member| {
                        !input_members.iter().any(|(basename, maximum)| {
                            *basename == member.basename && *maximum == member.maximum
                        })
                    }))
        {
            return Err(Error::invalid(
                "Auxiliary input roster must match every original role and bound exactly",
            ));
        }
        if xml_declared != self.kind.native()
            || request_declared != matches!(self.kind, VideoOriginalKind::HostNativeRender)
            || import_request_declared != matches!(self.kind, VideoOriginalKind::InputProbe)
        {
            return Err(Error::invalid("Owned native metadata roster is incomplete"));
        }
        // The existing color peak already charges MAX_STAGED_SOURCE_BYTES =
        // MAX_JOB_INPUT_BYTES for the aggregate staging writes. Do not charge
        // it twice or let this metadata document enlarge that writer limit.
        let staged_limit = if auxiliary {
            staged_sum
        } else {
            staged_sum.min(crate::jobs::MAX_JOB_INPUT_BYTES)
        };
        // Preserve the native caller's existing two-XML allowance: staged XML
        // and its pre-render published copy. The after-XML copy is already in
        // R3 raw_bound. Host's distinct request is charged explicitly as well.
        let native_metadata_bound = if self.kind.native() {
            (crate::xml::MAX_XML as u64)
                .checked_mul(2)
                .and_then(|bytes| {
                    bytes.checked_add(if request_declared {
                        crate::color::NATIVE_OBSERVATION_REQUEST_LIMIT as u64
                    } else {
                        0
                    })
                })
                .ok_or_else(|| Error::limit("Owned native metadata bound overflow"))?
        } else {
            0
        };
        let child = |value: &serde_json::Value| {
            serde_json::json!({
                "relative_path":value["relative_path"],"device":value["device"],"inode":value["inode"],
            })
        };
        let mut work_members = self.members.iter().filter(|member| !member.from_inputs)
            .map(|member| serde_json::json!({"basename":member.basename,"maximum_bytes":member.maximum}))
            .collect::<Vec<_>>();
        work_members.push(serde_json::json!({
            "basename":self.failure_receipt_basename,"maximum_bytes":self.failure_receipt_limit,
        }));
        let mut document = serde_json::json!({
            "schema":"semwright.native-video-owned-workspaces/1",
            "output_path":self.output_path,"output_root":output_identity,
            "source_kind":self.source_kind,
            "original_kind":match self.kind {
                VideoOriginalKind::MotionFrames => "motion-frames",
                VideoOriginalKind::NativeRender => "native-render",
                VideoOriginalKind::HostNativeRender => "host-native-render",
                VideoOriginalKind::InputProbe => "input-probe",
                VideoOriginalKind::AvAudioDecode => "av-audio-decode",
            },
            "inputs":child(&input_identity),"work":child(&work_identity),
            "input_members":input_members.iter().map(|(basename, maximum)| serde_json::json!({
                "basename":basename,"maximum_bytes":maximum,
            })).collect::<Vec<_>>(),
            "work_members":work_members,"staged_source_aggregate_maximum_bytes":staged_limit,
            "existing_staged_source_peak_allowance":if auxiliary {0} else {crate::jobs::MAX_JOB_INPUT_BYTES},
            "native_metadata_bound":native_metadata_bound,
            "auxiliary_input_metadata_bound":if import_request_declared {crate::json::MAX_JSON} else {0},
            "workspace_receipt_maximum_bytes":u64::MAX,
            "raw_manifest_maximum_bytes":self.manifest_limit,
            "final_receipt_maximum_bytes":self.failure_receipt_limit,
            "required_free_bytes":u64::MAX,
            "native_effect_started":false,"process_tree_quiescence_observed":false,
            "read_authority":"not-granted-by-this-receipt",
            "disposition":"retain-only-these-owned-directories-no-retry",
            "physical_acceptance":false,
        });
        // Exact schema/roster bytes with only the eight variable u64 fields
        // at their longest representation. This is a reproducible upper bound
        // independent of future inode widths; the final document can only shrink.
        let mut ceiling = document.clone();
        for role in ["output_root", "inputs", "work"] {
            ceiling[role]["device"] = serde_json::json!(u64::MAX);
            ceiling[role]["inode"] = serde_json::json!(u64::MAX);
        }
        let receipt_limit = serde_json::to_vec(&ceiling)
            .map_err(|_| Error::invalid("Owned workspace cap serialization failed"))?
            .len();
        if receipt_limit > receipt_structure_limit || receipt_limit > crate::color::REPORT_LIMIT {
            return Err(Error::limit(
                "Owned workspace declaration exceeds its finite allocation/serialization cap",
            ));
        }
        let required = self
            .required_free_bytes
            .checked_add(receipt_limit as u64)
            .and_then(|bytes| bytes.checked_add(native_metadata_bound))
            .ok_or_else(|| Error::limit("Owned original placement bound overflow"))?;
        if self.output.available_bytes_for(&self.output_path)? < required {
            return Err(Error::limit(
                "Output lacks the original declared workspace and custody peak",
            ));
        }
        document["workspace_receipt_maximum_bytes"] = serde_json::json!(receipt_limit);
        document["required_free_bytes"] = serde_json::json!(required);
        let bytes = serde_json::to_vec(&document)
            .map_err(|_| Error::invalid("Owned workspace declaration serialization failed"))?;
        if bytes.len() > receipt_limit {
            return Err(Error::limit(
                "Owned workspace declaration exceeds its charged cap",
            ));
        }
        let path = format!("{}.raw-original-workspaces.json", self.output_path);
        let artifact =
            crate::runtime::retain_ffv1_original(&self.output, &path, &bytes, receipt_limit)?;
        self.required_free_bytes = required;
        self.workspace_receipt = Some(artifact.clone());
        Ok(artifact)
    }

    pub fn required_free_bytes(&self) -> u64 {
        self.required_free_bytes
    }

    pub fn workspace_receipt(&self) -> Option<&Artifact> {
        self.workspace_receipt.as_ref()
    }

    fn retain(&self, member: &Member, source: &Root) -> Result<Option<Artifact>> {
        let mut file = match source.read_file(&member.basename, member.maximum) {
            Ok(file) => file,
            Err(error) if error.code == "NotFound" => return Ok(None),
            Err(error) => return Err(error),
        };
        let before = file.metadata()?;
        let same = |a: &std::fs::Metadata, b: &std::fs::Metadata| {
            (
                a.dev(),
                a.ino(),
                a.len(),
                a.mtime(),
                a.mtime_nsec(),
                a.ctime(),
                a.ctime_nsec(),
            ) == (
                b.dev(),
                b.ino(),
                b.len(),
                b.mtime(),
                b.mtime_nsec(),
                b.ctime(),
                b.ctime_nsec(),
            )
        };
        let artifact =
            if member.reuse_exact_publication && self.output.exists(&member.destination)? {
                // Only exact content from this already preflighted, closed original
                // slot can be reused. A conflict is never overwritten or retried.
                let actual = reader_hash(&mut file, member.maximum)?;
                let retained = reader_hash(
                    self.output.read_file(&member.destination, member.maximum)?,
                    member.maximum,
                )?;
                if actual != retained {
                    return Err(Error::new(
                        "StaleReference",
                        "Retained original differs from its owned source",
                    ));
                }
                Artifact {
                    root: "output".into(),
                    path: member.destination.clone(),
                    sha256: actual.0,
                    bytes: actual.1,
                }
            } else {
                // Raw empty/malformed partials deliberately do not use the success
                // parser or publish_verified, which rejects zero-byte originals.
                self.output
                    .publish("output", &member.destination, &mut file, member.maximum)?
            };
        if !same(&before, &file.metadata()?) || artifact.bytes != before.len() {
            return Err(Error::new(
                "StaleReference",
                "Owned original changed during raw retention",
            ));
        }
        let actual = reader_hash(
            self.output.read_file(&artifact.path, member.maximum)?,
            member.maximum,
        )?;
        if actual != (artifact.sha256.clone(), artifact.bytes) {
            return Err(Error::new(
                "StaleReference",
                "Raw original readback differs",
            ));
        }
        Ok(Some(artifact))
    }

    /// Run after the whole original runtime/publication Result was observed,
    /// before either owned workspace can drop. No native effect is repeated.
    #[allow(clippy::too_many_arguments)]
    pub fn finish<T>(
        &self,
        original: Result<T>,
        source_sha256: Option<&str>,
        process_originals_complete: bool,
        process_secondary_errors: &[serde_json::Value],
        inputs: &mut PrivateDir,
        work: &mut PrivateDir,
        input_root: &Root,
        work_root: &Root,
    ) -> Result<T> {
        let mut complete = source_sha256.is_none_or(valid_digest)
            && (original.is_err() || source_sha256.is_some())
            && process_originals_complete
            && process_secondary_errors.is_empty();
        let persistent_owner =
            inputs.output_owned_for_originals() || work.output_owned_for_originals();
        if persistent_owner && self.workspace_receipt.is_none() {
            complete = false;
        }
        let mut records = Vec::with_capacity(self.members.len());
        let mut native_xml_absence_observed = None;
        for member in &self.members {
            let source = if member.from_inputs {
                input_root
            } else {
                work_root
            };
            let retained = self.retain(member, source);
            if member.from_inputs && member.basename == "project.mlt" {
                native_xml_absence_observed = match &retained {
                    Ok(None) => Some(true),
                    Ok(Some(_)) => Some(false),
                    Err(_) => None,
                };
            }
            let outcome = match retained {
                Ok(Some(artifact)) => serde_json::json!({
                    "state":"retained", "path":artifact.path,"sha256":artifact.sha256,"bytes":artifact.bytes,
                }),
                Ok(None) => {
                    if original.is_ok() && member.required_on_success {
                        complete = false;
                    }
                    serde_json::json!({"state":"absence-observed","required_on_success":member.required_on_success})
                }
                Err(error) => {
                    complete = false;
                    serde_json::json!({"state":"capture-failed","error":{
                        "code":error.code,"message":error.message,"outcome_known":error.outcome_known,
                    }})
                }
            };
            records.push(serde_json::json!({
                "source":if member.from_inputs {"owned-inputs"} else {"owned-work"},
                "basename":member.basename,"destination":member.destination,
                "maximum_bytes":member.maximum,"outcome":outcome,
            }));
        }
        let document = serde_json::json!({
            "schema":"semwright.native-video-raw-originals/1", "output_path":self.output_path,
            "source_sha256":source_sha256,"raw_members_complete":complete,
            "source_kind":self.source_kind,
            "source_input_state":if source_sha256.is_some() {"present-bound"} else {"not-bound-before-error"},
            "native_xml_absence_observed":native_xml_absence_observed,
            "process_originals_complete":process_originals_complete,
            "process_secondary_errors":process_secondary_errors,
            "process_backup_bound":self.process_backup_bound,
            "owned_workspaces_receipt":self.workspace_receipt.as_ref().map(|artifact| serde_json::json!({
                "root":artifact.root,"path":artifact.path,"sha256":artifact.sha256,"bytes":artifact.bytes,
            })),
            "final_retention_failure_receipt":{
                "root":"owned-work","basename":self.failure_receipt_basename,
                "maximum_bytes":self.failure_receipt_limit,"write_only_on_manifest_failure":true,
            },
            "original_operation_succeeded":original.is_ok(),
            "original_error":original.as_ref().err().map(|error| serde_json::json!({
                "code":error.code,"message":error.message,"outcome_known":error.outcome_known,
            })),
            "members":records,"native_execution_retried":false,"physical_acceptance":false,
            "on_incomplete":"retain-only-these-owned-workspaces-without-retry",
            "owned_input_path":inputs.path(),"owned_work_path":work.path(),
        });
        let retained = serde_json::to_vec(&document)
            .map_err(|_| Error::invalid("Raw-original manifest serialization failed"))
            .and_then(|bytes| {
                crate::runtime::retain_ffv1_original(
                    &self.output,
                    &self.manifest_path,
                    &bytes,
                    self.manifest_limit,
                )
            });
        complete &= retained.is_ok();
        if let Err(error) = retained {
            let receipt = serde_json::json!({
                "schema":"semwright.native-video-final-retention/1",
                "raw_manifest":document,
                "output_manifest_path":self.manifest_path,"raw_manifest_retention_complete":false,
                "secondary_error":{"code":error.code,"message":error.message,"outcome_known":error.outcome_known},
                "output_publication_retried":false,"receipt_write_attempts":1,
                "on_receipt_failure":"retain-these-owned-workspaces-no-recursive-receipt",
                "physical_acceptance":false,
            });
            // A receipt failure cannot replace the primary error or trigger a
            // retry, another Root, or recursive error receipts. Both owned
            // directories remain retained and the operation stays incomplete.
            let _receipt = serde_json::to_vec(&receipt)
                .map_err(|_| Error::invalid("Final-retention receipt serialization failed"))
                .and_then(|bytes| {
                    if bytes.len() > self.failure_receipt_limit {
                        return Err(Error::limit(
                            "Final-retention receipt exceeds its declared bound",
                        ));
                    }
                    let artifact = work_root.write_new(
                        "owned-work",
                        &self.failure_receipt_basename,
                        &bytes,
                    )?;
                    if artifact.bytes != bytes.len() as u64
                        || artifact.sha256 != crate::hash::sha256(&bytes)
                        || work_root
                            .read(&self.failure_receipt_basename, self.failure_receipt_limit)?
                            != bytes
                    {
                        let mut error = Error::new(
                            "BackendFailed",
                            "Final-retention receipt readback is uncertain",
                        );
                        error.outcome_known = false;
                        return Err(error);
                    }
                    Ok(artifact)
                });
        }
        if !complete {
            // Opt-in disposition for these two owned directories only. A
            // capture/readback failure cannot delete their sole originals.
            inputs.retain_originals_on_drop();
            work.retain_originals_on_drop();
        }
        match original {
            Err(error) => Err(error),
            Ok(value) if complete => Ok(value),
            Ok(_) => {
                let mut error = Error::new(
                    "BackendFailed",
                    "Native operation returned but raw original custody is incomplete; inspect without retry",
                );
                error.outcome_known = false;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::fs::MetadataExt, path::Path};

    fn fixture() -> (PrivateDir, PrivateDir, PrivateDir, Root, Root, Root) {
        let inputs = PrivateDir::new(Path::new("/tmp")).unwrap();
        let work = PrivateDir::new(Path::new("/tmp")).unwrap();
        let output = PrivateDir::new(Path::new("/tmp")).unwrap();
        let inputs_root = Root::open(inputs.path(), true, true).unwrap();
        let work_root = Root::open(work.path(), true, true).unwrap();
        let output_root = Root::open(output.path(), true, true).unwrap();
        (inputs, work, output, inputs_root, work_root, output_root)
    }

    fn plan(output: &Root, work: &Root) -> VideoOriginals {
        VideoOriginals::new(
            output,
            work,
            "original.mkv",
            (32, 32),
            2,
            "mezzanine.mkv",
            VideoOriginalKind::MotionFrames,
            0,
        )
        .unwrap()
    }

    fn receipt(output: &Root) -> serde_json::Value {
        serde_json::from_slice(
            &output
                .read(
                    "original.mkv.raw-originals.json",
                    crate::color::REPORT_LIMIT,
                )
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn raw_error_preserves_empty_video_malformed_png_and_overflow_sentinel() {
        let (mut inputs, mut work, _output, input_root, work_root, output_root) = fixture();
        let plan = plan(&output_root, &work_root);
        work_root
            .write_new("owned-work", "mezzanine.mkv", b"")
            .unwrap();
        work_root
            .write_new("owned-work", "decoded-000000.png", b"not-a-valid-png\0")
            .unwrap();
        work_root
            .write_new("owned-work", "decoded-000002.png", b"overflow-original\xff")
            .unwrap();
        let original: Result<()> = Err(Error::new("BackendFailed", "original-native-exit-7"));
        let error = plan
            .finish(
                original,
                Some(&"a".repeat(64)),
                true,
                &[],
                &mut inputs,
                &mut work,
                &input_root,
                &work_root,
            )
            .unwrap_err();
        assert_eq!(error.message, "original-native-exit-7");
        assert!(error.outcome_known);
        assert_eq!(
            output_root
                .read("original.mkv.raw-original.mezzanine.mkv", 0)
                .unwrap(),
            b""
        );
        assert_eq!(
            output_root
                .read("original.mkv.decoded-000000.png", 64)
                .unwrap(),
            b"not-a-valid-png\0"
        );
        assert_eq!(
            output_root
                .read("original.mkv.decoded-000002.png", 64)
                .unwrap(),
            b"overflow-original\xff"
        );
        assert_eq!(receipt(&output_root)["raw_members_complete"], true);
        let path = work.path().to_owned();
        drop(work);
        assert!(
            !path.exists(),
            "complete custody permits the ordinary owned cleanup"
        );
    }

    #[test]
    fn later_publication_unknown_retains_remaining_raw_bytes_and_reuses_exact_png() {
        let (mut inputs, mut work, _output, input_root, work_root, output_root) = fixture();
        let plan = plan(&output_root, &work_root);
        work_root
            .write_new("owned-work", "mezzanine.mkv", b"partial-video")
            .unwrap();
        work_root
            .write_new("owned-work", "decoded-000000.png", b"original-frame")
            .unwrap();
        output_root
            .write_new(
                "output",
                "original.mkv.decoded-000000.png",
                b"original-frame",
            )
            .unwrap();
        let before = output_root
            .read_file("original.mkv.decoded-000000.png", 64)
            .unwrap()
            .metadata()
            .unwrap()
            .ino();
        let mut failure = Error::new("BackendFailed", "final-publication-readback-unknown");
        failure.outcome_known = false;
        let original: Result<()> = Err(failure);
        let error = plan
            .finish(
                original,
                Some(&"b".repeat(64)),
                true,
                &[],
                &mut inputs,
                &mut work,
                &input_root,
                &work_root,
            )
            .unwrap_err();
        assert_eq!(error.message, "final-publication-readback-unknown");
        assert!(!error.outcome_known);
        assert_eq!(
            output_root
                .read("original.mkv.raw-original.mezzanine.mkv", 64)
                .unwrap(),
            b"partial-video"
        );
        let after = output_root
            .read_file("original.mkv.decoded-000000.png", 64)
            .unwrap()
            .metadata()
            .unwrap()
            .ino();
        assert_eq!(
            before, after,
            "exact already-published original is read, never replaced"
        );
    }

    #[test]
    fn conflicting_capture_keeps_original_error_existing_file_and_owned_workspaces() {
        let (mut inputs, mut work, _output, input_root, work_root, output_root) = fixture();
        let plan = plan(&output_root, &work_root);
        work_root
            .write_new("owned-work", "decoded-000000.png", b"sole-original")
            .unwrap();
        output_root
            .write_new(
                "output",
                "original.mkv.decoded-000000.png",
                b"conflicting-original",
            )
            .unwrap();
        let original: Result<()> = Err(Error::new("BackendFailed", "original-native-error"));
        let error = plan
            .finish(
                original,
                Some(&"c".repeat(64)),
                true,
                &[],
                &mut inputs,
                &mut work,
                &input_root,
                &work_root,
            )
            .unwrap_err();
        assert_eq!(error.message, "original-native-error");
        assert!(error.outcome_known);
        assert_eq!(
            output_root
                .read("original.mkv.decoded-000000.png", 64)
                .unwrap(),
            b"conflicting-original"
        );
        assert_eq!(receipt(&output_root)["raw_members_complete"], false);
        let input_path = inputs.path().to_owned();
        let work_path = work.path().to_owned();
        drop(inputs);
        drop(work);
        assert!(input_path.exists() && work_path.exists());
        assert_eq!(
            std::fs::read(work_path.join("decoded-000000.png")).unwrap(),
            b"sole-original"
        );
        // Only this test's exact, newly created owned directories are cleaned.
        std::fs::remove_dir_all(input_path).unwrap();
        std::fs::remove_dir_all(work_path).unwrap();
    }

    #[test]
    fn otherwise_successful_operation_with_missing_originals_becomes_unknown() {
        let (mut inputs, mut work, _output, input_root, work_root, output_root) = fixture();
        let plan = plan(&output_root, &work_root);
        let error = plan
            .finish(
                Ok(37),
                Some(&"d".repeat(64)),
                true,
                &[],
                &mut inputs,
                &mut work,
                &input_root,
                &work_root,
            )
            .unwrap_err();
        assert_eq!(error.code, "BackendFailed");
        assert!(!error.outcome_known);
        assert_eq!(receipt(&output_root)["raw_members_complete"], false);
        let input_path = inputs.path().to_owned();
        let work_path = work.path().to_owned();
        drop(inputs);
        drop(work);
        std::fs::remove_dir_all(input_path).unwrap();
        std::fs::remove_dir_all(work_path).unwrap();
    }

    #[test]
    fn declared_output_conflict_is_denied_before_any_original_work() {
        let (_inputs, _work, _output, _input_root, work_root, output_root) = fixture();
        output_root
            .write_new(
                "output",
                "original.mkv.decoded-000002.png",
                b"old-unknown-sentinel",
            )
            .unwrap();
        let result = VideoOriginals::new(
            &output_root,
            &work_root,
            "original.mkv",
            (32, 32),
            2,
            "mezzanine.mkv",
            VideoOriginalKind::MotionFrames,
            0,
        );
        assert!(matches!(result, Err(Error { code, .. }) if code == "Conflict"));
        assert_eq!(
            output_root
                .read("original.mkv.decoded-000002.png", 64)
                .unwrap(),
            b"old-unknown-sentinel"
        );
    }

    #[test]
    fn input_probe_failure_keeps_its_originals_and_explicitly_has_no_render_xml() {
        let (mut inputs, mut work, _output, input_root, work_root, output_root) = fixture();
        let plan = VideoOriginals::new(
            &output_root,
            &work_root,
            "original.mkv",
            (32, 32),
            2,
            "partial.mkv",
            VideoOriginalKind::NativeRender,
            1,
        )
        .unwrap();
        for (tail, bytes) in [
            (
                "media-probe.stdout.original.bin",
                b"partial-probe\xff".as_slice(),
            ),
            (
                "media-probe.stderr.original.bin",
                b"native-probe-error\0".as_slice(),
            ),
            (
                "media-probe.terminal.original.json",
                b"{\"exit\":7}".as_slice(),
            ),
            (
                "process-custody.original.json",
                b"{\"original_error\":7}".as_slice(),
            ),
        ] {
            work_root
                .write_new(
                    "owned-work",
                    &format!("native-input-probe-000000-{tail}"),
                    bytes,
                )
                .unwrap();
        }
        let original: Result<()> = Err(Error::new("BackendFailed", "original-input-probe-error"));
        let error = plan
            .finish(
                original,
                None,
                true,
                &[],
                &mut inputs,
                &mut work,
                &input_root,
                &work_root,
            )
            .unwrap_err();
        assert_eq!(error.message, "original-input-probe-error");
        assert_eq!(
            output_root
                .read(
                    "original.mkv.native-input-probe-000000-media-probe.stdout.original.bin",
                    64
                )
                .unwrap(),
            b"partial-probe\xff"
        );
        let report = receipt(&output_root);
        assert!(report["source_sha256"].is_null());
        assert_eq!(report["source_input_state"], "not-bound-before-error");
        assert_eq!(report["native_xml_absence_observed"], true);
        assert_eq!(report["raw_members_complete"], true);
        assert!(
            !output_root
                .exists("original.mkv.native-input.after.original.mlt")
                .unwrap()
        );
    }

    #[test]
    fn later_declared_input_probe_conflict_is_rejected_before_the_first_probe() {
        let (_inputs, _work, _output, _input_root, work_root, output_root) = fixture();
        let name = "original.mkv.native-input-probe-000001-media-probe.stdout.original.bin";
        output_root
            .write_new("output", name, b"prior-unknown-probe")
            .unwrap();
        let result = VideoOriginals::new(
            &output_root,
            &work_root,
            "original.mkv",
            (32, 32),
            2,
            "partial.mkv",
            VideoOriginalKind::NativeRender,
            2,
        );
        assert!(matches!(result, Err(Error { code, .. }) if code == "Conflict"));
        assert_eq!(output_root.read(name, 64).unwrap(), b"prior-unknown-probe");
    }

    #[test]
    fn final_manifest_error_retains_the_actual_secondary_error_and_primary() {
        let (mut inputs, mut work, _output, input_root, work_root, output_root) = fixture();
        let plan = plan(&output_root, &work_root);
        // Conflict appears after preflight, during the original attempt.
        output_root
            .write_new(
                "output",
                "original.mkv.raw-originals.json",
                b"prior-unknown-manifest",
            )
            .unwrap();
        work_root
            .write_new("owned-work", "mezzanine.mkv", b"sole-partial-video")
            .unwrap();
        let mut primary = Error::new("BackendFailed", "original-publication-unknown");
        primary.outcome_known = false;
        let original: Result<()> = Err(primary);
        let error = plan
            .finish(
                original,
                Some(&"e".repeat(64)),
                true,
                &[],
                &mut inputs,
                &mut work,
                &input_root,
                &work_root,
            )
            .unwrap_err();
        assert_eq!(error.code, "BackendFailed");
        assert_eq!(error.message, "original-publication-unknown");
        assert!(!error.outcome_known);
        let saved: serde_json::Value = serde_json::from_slice(
            &work_root
                .read(
                    "ffv1-raw-originals-retention.original.json",
                    plan.failure_receipt_limit,
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(saved["secondary_error"]["code"], "Conflict");
        assert!(
            !saved["secondary_error"]["message"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        assert_eq!(saved["secondary_error"]["outcome_known"], true);
        assert_eq!(
            saved["raw_manifest"]["original_error"]["message"],
            "original-publication-unknown"
        );
        assert_eq!(saved["raw_manifest_retention_complete"], false);
        assert_eq!(saved["receipt_write_attempts"], 1);
        assert_eq!(saved["output_publication_retried"], false);
        assert_eq!(
            output_root
                .read("original.mkv.raw-originals.json", 64)
                .unwrap(),
            b"prior-unknown-manifest"
        );
        let input_path = inputs.path().to_owned();
        let work_path = work.path().to_owned();
        drop(inputs);
        drop(work);
        assert!(input_path.exists() && work_path.exists());
        assert_eq!(
            std::fs::read(work_path.join("mezzanine.mkv")).unwrap(),
            b"sole-partial-video"
        );
        std::fs::remove_dir_all(input_path).unwrap();
        std::fs::remove_dir_all(work_path).unwrap();
    }

    #[test]
    fn every_input_probe_charges_both_copies_and_the_final_error_budget() {
        let (_inputs, _work, _output, _input_root, work_root, output_root) = fixture();
        let first = VideoOriginals::new(
            &output_root,
            &work_root,
            "original.mkv",
            (32, 32),
            2,
            "partial.mkv",
            VideoOriginalKind::NativeRender,
            1,
        )
        .unwrap();
        let second = VideoOriginals::new(
            &output_root,
            &work_root,
            "original.mkv",
            (32, 32),
            2,
            "partial.mkv",
            VideoOriginalKind::NativeRender,
            2,
        )
        .unwrap();
        // Two 16 KiB streams plus terminal and summary at 64 KiB each.
        const ONE_COPY: u64 = 163_840;
        assert_eq!(
            second.process_backup_bound - first.process_backup_bound,
            ONE_COPY
        );
        let manifest_delta = (second.manifest_limit - first.manifest_limit) as u64;
        assert_eq!(
            manifest_delta,
            4 * ("original.mkv".len() as u64 + 2048) + 65_536
        );
        assert_eq!(
            second.required_free_bytes - first.required_free_bytes,
            2 * ONE_COPY + 2 * manifest_delta
        );
        // The assertion covers both locations even if statvfs sees one shared
        // filesystem; no project asset cap or spare fixed margin is assumed.
    }

    #[test]
    fn final_failure_receipt_conflict_is_rejected_before_original_effects() {
        let (_inputs, _work, _output, _input_root, work_root, output_root) = fixture();
        let name = "ffv1-raw-originals-retention.original.json";
        work_root
            .write_new("owned-work", name, b"prior-final-retention-receipt")
            .unwrap();
        let plan = VideoOriginals::new(
            &output_root,
            &work_root,
            "original.mkv",
            (32, 32),
            2,
            "mezzanine.mkv",
            VideoOriginalKind::MotionFrames,
            0,
        );
        assert!(matches!(plan, Err(Error { code, .. }) if code == "Conflict"));
        assert_eq!(
            work_root.read(name, 64).unwrap(),
            b"prior-final-retention-receipt"
        );
        assert!(
            !output_root
                .exists("original.mkv.raw-originals.json")
                .unwrap()
        );
    }
}

#[cfg(test)]
mod output_workspace_contract_tests {
    use super::*;
    use std::{io::Write, os::unix::fs::PermissionsExt};

    fn motion_plan() -> (
        tempfile::TempDir,
        Root,
        PrivateDir,
        PrivateDir,
        VideoOriginals,
    ) {
        let output = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(output.path()).unwrap();
        let root = Root::open(&path, true, true).unwrap();
        let (inputs, work) = root.create_original_workspaces("case.mkv").unwrap();
        let work_root = work.owned_root(true, true).unwrap();
        let mut plan = VideoOriginals::new(
            &root,
            &work_root,
            "case.mkv",
            (32, 32),
            2,
            "mezzanine.mkv",
            VideoOriginalKind::MotionFrames,
            0,
        )
        .unwrap();
        plan.declare_workspaces(
            &inputs,
            &work,
            &[
                ("000000.png".into(), 32 * 1024 * 1024),
                ("000001.png".into(), 32 * 1024 * 1024),
            ],
        )
        .unwrap();
        (output, root, inputs, work, plan)
    }

    #[test]
    fn preeffect_roster_has_exact_relative_instances_and_minimal_serialized_cap() {
        let (_output, root, inputs, work, plan) = motion_plan();
        let artifact = plan.workspace_receipt().unwrap();
        let raw = root
            .read(&artifact.path, crate::color::REPORT_LIMIT)
            .unwrap();
        let document: serde_json::Value = serde_json::from_slice(&raw).unwrap();
        assert_eq!(
            document["inputs"]["relative_path"],
            "case.mkv.raw-original-inputs"
        );
        assert_eq!(
            document["work"]["relative_path"],
            "case.mkv.raw-original-work"
        );
        let (device, inode) = root.instance_identity().unwrap();
        assert_eq!(document["output_root"]["device"], device);
        assert_eq!(document["output_root"]["inode"], inode);
        assert_eq!(document["native_effect_started"], false);
        assert_eq!(document["process_tree_quiescence_observed"], false);
        assert_eq!(document["original_kind"], "motion-frames");
        assert_eq!(document["input_members"].as_array().unwrap().len(), 2);
        assert_eq!(
            document["staged_source_aggregate_maximum_bytes"],
            64 * 1024 * 1024u64
        );
        let members = document["work_members"].as_array().unwrap();
        assert!(
            members
                .iter()
                .any(|row| row["basename"] == "decoded-000002.png")
        );
        assert_eq!(
            members.last().unwrap()["basename"],
            "ffv1-raw-originals-retention.original.json"
        );
        let mut ceiling = document.clone();
        for role in ["output_root", "inputs", "work"] {
            ceiling[role]["device"] = serde_json::json!(u64::MAX);
            ceiling[role]["inode"] = serde_json::json!(u64::MAX);
        }
        ceiling["workspace_receipt_maximum_bytes"] = serde_json::json!(u64::MAX);
        ceiling["required_free_bytes"] = serde_json::json!(u64::MAX);
        let exact_cap = serde_json::to_vec(&ceiling).unwrap().len() as u64;
        assert_eq!(document["workspace_receipt_maximum_bytes"], exact_cap);
        assert!((raw.len() as u64) <= exact_cap && exact_cap < 65_536);
        assert!(
            !inputs
                .owned_root(true, false)
                .unwrap()
                .exists("000000.png")
                .unwrap()
        );
        assert!(
            !work
                .owned_root(true, false)
                .unwrap()
                .exists("mezzanine.mkv")
                .unwrap()
        );
        assert_eq!(artifact.sha256, crate::hash::sha256(&raw));
    }

    #[test]
    fn initial_host_handoff_requires_the_original_receipt_and_child_instances() {
        let (_output, root, inputs, work, plan) = motion_plan();
        let identity = plan.workspace_receipt().unwrap().sha256.clone();
        assert!(
            root.reopen_original_workspaces("case.mkv", &"0".repeat(64))
                .is_err()
        );
        let (again_inputs, again_work) = root
            .reopen_original_workspaces("case.mkv", &identity)
            .unwrap();
        assert_eq!(
            again_inputs.owned_output_identity().unwrap(),
            inputs.owned_output_identity().unwrap()
        );
        assert_eq!(
            again_work.owned_output_identity().unwrap(),
            work.owned_output_identity().unwrap()
        );
        drop(again_inputs);
        drop(again_work);
        let old_path = work.path().with_extension("retained-original");
        std::fs::rename(work.path(), &old_path).unwrap();
        std::fs::create_dir(work.path()).unwrap();
        std::fs::set_permissions(work.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            root.reopen_original_workspaces("case.mkv", &identity)
                .is_err()
        );
        assert!(
            old_path.exists(),
            "mismatched owner remains retained, not replaced or removed"
        );
    }

    #[test]
    fn staging_error_before_finalizer_retains_the_declared_partial_input() {
        let (_output, root, inputs, work, plan) = motion_plan();
        let original: Result<()> = (|| {
            let mut file = inputs.create("000000.png")?;
            file.write_all(b"partial-source-before-parse-error")?;
            file.sync_all()?;
            Err(Error::new("BackendFailed", "original-staging-failure"))
        })();
        assert_eq!(original.unwrap_err().message, "original-staging-failure");
        drop(inputs);
        drop(work);
        drop(plan);
        assert_eq!(
            root.read("case.mkv.raw-original-inputs/000000.png", 64)
                .unwrap(),
            b"partial-source-before-parse-error"
        );
        assert!(
            root.exists("case.mkv.raw-original-workspaces.json")
                .unwrap()
        );
        assert!(
            !root.exists("case.mkv.raw-originals.json").unwrap(),
            "no synthetic finalizer result is invented"
        );
    }

    #[test]
    fn manifest_failure_keeps_primary_and_final_receipt_in_persistent_work() {
        let (_output, root, mut inputs, mut work, plan) = motion_plan();
        let input_root = inputs.owned_root(true, false).unwrap();
        let work_root = work.owned_root(true, true).unwrap();
        work_root
            .write_new("owned-work", "mezzanine.mkv", b"partial-video")
            .unwrap();
        root.write_new(
            "output",
            "case.mkv.raw-originals.json",
            b"prior-unknown-manifest",
        )
        .unwrap();
        let mut primary = Error::new("BackendFailed", "original-unknown-native-error");
        primary.outcome_known = false;
        let error = plan
            .finish::<()>(
                Err(primary),
                Some(&"a".repeat(64)),
                false,
                &[],
                &mut inputs,
                &mut work,
                &input_root,
                &work_root,
            )
            .unwrap_err();
        assert_eq!(error.message, "original-unknown-native-error");
        assert!(!error.outcome_known);
        drop(inputs);
        drop(work);
        drop(plan);
        assert_eq!(
            root.read("case.mkv.raw-originals.json", 64).unwrap(),
            b"prior-unknown-manifest"
        );
        assert_eq!(
            root.read("case.mkv.raw-original-work/mezzanine.mkv", 64)
                .unwrap(),
            b"partial-video"
        );
        let bytes = root
            .read(
                "case.mkv.raw-original-work/ffv1-raw-originals-retention.original.json",
                crate::color::REPORT_LIMIT + crate::runtime::FFV1_CUSTODY_METADATA,
            )
            .unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(receipt["secondary_error"]["code"], "Conflict");
        assert_eq!(
            receipt["raw_manifest"]["original_error"]["message"],
            "original-unknown-native-error"
        );
        assert_eq!(receipt["receipt_write_attempts"], 1);
        assert_eq!(receipt["output_publication_retried"], false);
    }

    #[test]
    fn host_visual_roster_separates_real_native_probes_from_host_envelopes() {
        let output = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(output.path()).unwrap();
        let root = Root::open(&path, true, true).unwrap();
        let (inputs, work) = root.create_original_workspaces("visual.mkv").unwrap();
        let work_root = work.owned_root(true, true).unwrap();
        // No media asset: K=1 is the original final probe; two Host calls are
        // the detached render and that probe. No process is executed here.
        let mut plan = VideoOriginals::new(
            &root,
            &work_root,
            "visual.mkv",
            (32, 32),
            2,
            "partial.mkv",
            VideoOriginalKind::HostNativeRender,
            1,
        )
        .unwrap();
        let before = plan.required_free_bytes();
        let artifact = plan
            .declare_workspaces(
                &inputs,
                &work,
                &[
                    ("project.mlt".into(), crate::xml::MAX_XML as u64),
                    (
                        crate::color::NATIVE_OBSERVATION_REQUEST_FILE.into(),
                        crate::color::NATIVE_OBSERVATION_REQUEST_LIMIT as u64,
                    ),
                ],
            )
            .unwrap();
        let receipt: serde_json::Value = serde_json::from_slice(
            &root
                .read(&artifact.path, crate::color::REPORT_LIMIT)
                .unwrap(),
        )
        .unwrap();
        let members = receipt["work_members"].as_array().unwrap();
        assert!(
            !members
                .iter()
                .any(|row| row["basename"] == "native-video-media-probe.stdout.original.bin")
        );
        let native = members
            .iter()
            .find(|row| {
                row["basename"] == "native-input-probe-000000-media-probe.stdout.original.bin"
            })
            .unwrap();
        let host = members
            .iter()
            .find(|row| row["basename"] == "native-host-call-000001.stdout.original.bin")
            .unwrap();
        assert_eq!(
            native["maximum_bytes"],
            crate::runtime::FFV1_CUSTODY_STREAM_PREFIX as u64
        );
        assert_eq!(
            host["maximum_bytes"],
            crate::runtime::FFV1_CUSTODY_HOST_STREAM as u64
        );
        assert_eq!(members.len(), 2 + 22 + 7);
        for basename in [
            crate::runtime::NATIVE_DIAGNOSTIC_FILE,
            crate::runtime::VALIDATION_DIAGNOSTIC_FILE,
            crate::color::NATIVE_OBSERVATION_FILE,
        ] {
            assert!(members.iter().any(|row| row["basename"] == basename));
        }
        let metadata =
            2 * crate::xml::MAX_XML as u64 + crate::color::NATIVE_OBSERVATION_REQUEST_LIMIT as u64;
        assert_eq!(receipt["native_metadata_bound"], metadata);
        assert_eq!(
            plan.required_free_bytes() - before,
            metadata + receipt["workspace_receipt_maximum_bytes"].as_u64().unwrap()
        );
        assert_eq!(receipt["staged_source_aggregate_maximum_bytes"], 0);
    }
}

#[cfg(test)]
mod auxiliary_workspace_contract_tests {
    use super::*;

    fn output() -> (tempfile::TempDir, Root) {
        let directory = tempfile::tempdir().unwrap();
        let canonical = std::fs::canonicalize(directory.path()).unwrap();
        let root = Root::open(&canonical, true, true).unwrap();
        (directory, root)
    }

    #[test]
    fn import_declaration_requires_original_arguments_and_preserves_probe_growth_bound() {
        let (_directory, root) = output();
        let scope = format!("asset-import-probe-{}", "a".repeat(64));
        let (inputs, work) = root.create_original_workspaces(&scope).unwrap();
        let work_root = work.owned_root(true, true).unwrap();
        let mut plan = VideoOriginals::new_host_asset_probe(&root, &work_root, &scope).unwrap();
        // Omitting the original arguments cannot create a usable handoff.
        assert!(
            plan.declare_workspaces(
                &inputs,
                &work,
                &[("probe.bin".into(), crate::jobs::MAX_MEDIA_BYTES + 1)]
            )
            .is_err()
        );
        assert!(plan.workspace_receipt().is_none());
        assert!(
            !root
                .exists(&format!("{scope}.raw-original-workspaces.json"))
                .unwrap()
        );
        let receipt = plan
            .declare_workspaces(
                &inputs,
                &work,
                &[
                    ("probe.bin".into(), crate::jobs::MAX_MEDIA_BYTES + 1),
                    (
                        "import-request.original.json".into(),
                        crate::json::MAX_JSON as u64,
                    ),
                ],
            )
            .unwrap();
        let document: serde_json::Value = serde_json::from_slice(
            &root
                .read(&receipt.path, crate::color::REPORT_LIMIT)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(document["original_kind"], "input-probe");
        assert_eq!(
            document["staged_source_aggregate_maximum_bytes"],
            crate::jobs::MAX_MEDIA_BYTES + 1
        );
        assert_eq!(
            document["auxiliary_input_metadata_bound"],
            crate::json::MAX_JSON as u64
        );
        assert_eq!(document["existing_staged_source_peak_allowance"], 0);
        assert_eq!(plan.members.len(), 9);
        assert!(
            plan.members.iter().any(|member| member.basename
                == "native-input-probe-000000-media-probe.stdout.original.bin")
        );
        assert!(
            plan.members
                .iter()
                .any(|member| member.basename == "native-host-call-000000.stdout.original.bin")
        );
        assert!(
            !plan
                .members
                .iter()
                .any(|member| member.basename == "project.mlt" || member.basename == "partial.mkv")
        );
    }

    #[test]
    fn av_auxiliary_admission_charges_both_original_copies_and_the_normal_wav() {
        let (_directory, root) = output();
        let (inputs, work) = root
            .create_original_workspaces("master.mp4.av-aux")
            .unwrap();
        let work_root = work.owned_root(true, true).unwrap();
        assert!(VideoOriginals::new_av_auxiliary(&root, &work_root, "master.mp4", 0).is_err());
        let maximum = 32 * 1024 * 1024u64;
        let mut plan =
            VideoOriginals::new_av_auxiliary(&root, &work_root, "master.mp4", maximum).unwrap();
        let raw = plan
            .members
            .iter()
            .map(|member| member.maximum)
            .sum::<u64>();
        assert_eq!(
            plan.required_free_bytes(),
            2 * raw + plan.manifest_limit as u64 + plan.failure_receipt_limit as u64 + maximum
        );
        let mut roster = vec![
            ("video-probe.bin".into(), crate::jobs::MAX_MEDIA_BYTES + 1),
            ("audio-probe.bin".into(), crate::jobs::MAX_MEDIA_BYTES + 1),
            ("master.mp4".into(), maximum),
        ];
        // A smaller declared probe bound cannot discard the old writer's
        // MAX+1 physical sentinel or silently produce a misleading manifest.
        roster[0].1 -= 1;
        assert!(plan.declare_workspaces(&inputs, &work, &roster).is_err());
        roster[0].1 += 1;
        let before = plan.required_free_bytes();
        let receipt = plan.declare_workspaces(&inputs, &work, &roster).unwrap();
        let document: serde_json::Value = serde_json::from_slice(
            &root
                .read(&receipt.path, crate::color::REPORT_LIMIT)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            plan.required_free_bytes() - before,
            document["workspace_receipt_maximum_bytes"]
                .as_u64()
                .unwrap()
        );
        assert_eq!(
            document["staged_source_aggregate_maximum_bytes"],
            2 * (crate::jobs::MAX_MEDIA_BYTES + 1) + maximum
        );
        assert_eq!(document["work_members"].as_array().unwrap().len(), 17);
        assert_eq!(document["original_kind"], "av-audio-decode");
        assert_eq!(document["existing_staged_source_peak_allowance"], 0);
        assert_eq!(document["process_tree_quiescence_observed"], false);
    }

    #[test]
    fn av_decode_publication_error_keeps_original_inputs_partial_wav_and_primary_unknown() {
        let (_directory, root) = output();
        let scope = "master.mp4.av-aux";
        let (mut inputs, mut work) = root.create_original_workspaces(scope).unwrap();
        let input_root = inputs.owned_root(true, true).unwrap();
        let work_root = work.owned_root(true, true).unwrap();
        let maximum = 32 * 1024 * 1024u64;
        let mut plan =
            VideoOriginals::new_av_auxiliary(&root, &work_root, "master.mp4", maximum).unwrap();
        plan.declare_workspaces(
            &inputs,
            &work,
            &[
                ("video-probe.bin".into(), crate::jobs::MAX_MEDIA_BYTES + 1),
                ("audio-probe.bin".into(), crate::jobs::MAX_MEDIA_BYTES + 1),
                ("master.mp4".into(), maximum),
            ],
        )
        .unwrap();
        input_root
            .write_new("owned-inputs", "video-probe.bin", b"original-video-probe")
            .unwrap();
        input_root
            .write_new("owned-inputs", "audio-probe.bin", b"original-audio-probe")
            .unwrap();
        input_root
            .write_new(
                "owned-inputs",
                "master.mp4",
                b"original-master-before-decode",
            )
            .unwrap();
        work_root
            .write_new("owned-work", "decoded-audio.wav", b"partial-wav\0\xff")
            .unwrap();
        root.write_new(
            "output",
            &format!("{scope}.raw-originals.json"),
            b"prior-manifest-no-replace",
        )
        .unwrap();
        let mut primary = Error::new("BackendFailed", "original-audio-publication-unknown");
        primary.outcome_known = false;
        let error = plan
            .finish::<()>(
                Err(primary),
                Some(&"b".repeat(64)),
                false,
                &[],
                &mut inputs,
                &mut work,
                &input_root,
                &work_root,
            )
            .unwrap_err();
        assert_eq!(error.message, "original-audio-publication-unknown");
        assert!(!error.outcome_known);
        drop(inputs);
        drop(work);
        assert_eq!(
            root.read(&format!("{scope}.raw-original-work/decoded-audio.wav"), 64)
                .unwrap(),
            b"partial-wav\0\xff"
        );
        assert_eq!(
            root.read(&format!("{scope}.raw-original-inputs/audio-probe.bin"), 64)
                .unwrap(),
            b"original-audio-probe"
        );
        assert_eq!(
            root.read(&format!("{scope}.raw-original.decoded-audio.wav"), 64)
                .unwrap(),
            b"partial-wav\0\xff"
        );
        assert_eq!(
            root.read(&format!("{scope}.raw-originals.json"), 64)
                .unwrap(),
            b"prior-manifest-no-replace"
        );
        let receipt: serde_json::Value = serde_json::from_slice(
            &root
                .read(
                    &format!(
                        "{scope}.raw-original-work/av-audio-raw-originals-retention.original.json"
                    ),
                    plan.failure_receipt_limit,
                )
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            receipt["raw_manifest"]["original_error"]["outcome_known"],
            false
        );
        assert_eq!(receipt["secondary_error"]["code"], "Conflict");
        assert_eq!(receipt["output_publication_retried"], false);
    }
}

#[cfg(test)]
mod zscale_help_reservation_tests {
    use super::*;

    fn output() -> (tempfile::TempDir, Root) {
        let directory = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(directory.path()).unwrap();
        let root = Root::open(&path, true, true).unwrap();
        (directory, root)
    }

    #[test]
    fn help_originals_charge_both_streams_and_both_copies_before_effects() {
        let (_directory, root) = output();
        let (_work_directory, work) = output();
        let plan = VideoOriginals::new(
            &root,
            &work,
            "case.mkv",
            (32, 32),
            2,
            "mezzanine.mkv",
            VideoOriginalKind::MotionFrames,
            0,
        )
        .unwrap();
        let help = plan
            .members
            .iter()
            .filter(|member| {
                member.basename.starts_with("ffv1-zscale-help.")
                    && member.basename.ends_with(".bin")
            })
            .collect::<Vec<_>>();
        assert_eq!(help.len(), 2);
        for member in &help {
            assert_eq!(member.maximum, 262_144);
            assert_eq!(
                member.maximum,
                crate::runtime::ffv1_stream_original_limit("zscale-help") as u64
            );
        }
        let old_process_bound = (5 * 2 * crate::runtime::FFV1_CUSTODY_STREAM_PREFIX
            + 2 * crate::runtime::FFV1_CUSTODY_PROBE_STREAM
            + 7 * crate::runtime::FFV1_CUSTODY_METADATA) as u64;
        let one_copy_growth = 2 * (262_144 - 16_384);
        assert_eq!(
            plan.process_backup_bound - old_process_bound,
            one_copy_growth
        );
        let old_raw_bound = plan.manifest_limit as u64
            + plan
                .members
                .iter()
                .map(|member| {
                    if member.basename.starts_with("ffv1-zscale-help.")
                        && member.basename.ends_with(".bin")
                    {
                        16_384
                    } else {
                        member.maximum
                    }
                })
                .sum::<u64>();
        let old_peak = crate::color::observation_budget(32, 32, 2)
            .unwrap()
            .peak_free_bytes
            + old_raw_bound
            + old_process_bound
            + plan.failure_receipt_limit as u64;
        assert_eq!(plan.required_free_bytes - old_peak, 983_040);
        assert!(
            !root
                .exists("case.mkv.raw-original-workspaces.json")
                .unwrap()
        );
        assert!(!work.exists("mezzanine.mkv").unwrap());
    }

    #[test]
    fn declared_help_roster_charges_its_exact_extra_serialized_bytes() {
        let (_directory, root) = output();
        let (inputs, work) = root.create_original_workspaces("case.mkv").unwrap();
        let work_root = work.owned_root(true, true).unwrap();
        let mut plan = VideoOriginals::new(
            &root,
            &work_root,
            "case.mkv",
            (32, 32),
            2,
            "mezzanine.mkv",
            VideoOriginalKind::MotionFrames,
            0,
        )
        .unwrap();
        let before = plan.required_free_bytes;
        let artifact = plan
            .declare_workspaces(
                &inputs,
                &work,
                &[
                    ("000000.png".into(), 32 * 1024 * 1024),
                    ("000001.png".into(), 32 * 1024 * 1024),
                ],
            )
            .unwrap();
        let mut document: serde_json::Value = serde_json::from_slice(
            &root
                .read(&artifact.path, crate::color::REPORT_LIMIT)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(document["native_effect_started"], false);
        let receipt_limit = document["workspace_receipt_maximum_bytes"]
            .as_u64()
            .unwrap();
        let metadata = document["native_metadata_bound"].as_u64().unwrap();
        assert_eq!(plan.required_free_bytes - before, receipt_limit + metadata);
        for role in ["output_root", "inputs", "work"] {
            document[role]["device"] = serde_json::json!(u64::MAX);
            document[role]["inode"] = serde_json::json!(u64::MAX);
        }
        document["workspace_receipt_maximum_bytes"] = serde_json::json!(u64::MAX);
        document["required_free_bytes"] = serde_json::json!(u64::MAX);
        assert_eq!(
            serde_json::to_vec(&document).unwrap().len() as u64,
            receipt_limit
        );
        let mut changed = 0;
        for row in document["work_members"].as_array_mut().unwrap() {
            let name = row["basename"].as_str().unwrap();
            if name.starts_with("ffv1-zscale-help.") && name.ends_with(".bin") {
                assert_eq!(row["maximum_bytes"], 262_144);
                row["maximum_bytes"] = serde_json::json!(16_384);
                changed += 1;
            }
        }
        assert_eq!(changed, 2);
        assert_eq!(
            receipt_limit - serde_json::to_vec(&document).unwrap().len() as u64,
            2
        );
        assert!(!work_root.exists("mezzanine.mkv").unwrap());
    }

    #[test]
    fn unavailable_custody_space_denies_the_workspace_receipt_before_writing() {
        let (_directory, root) = output();
        let (inputs, work) = root.create_original_workspaces("case.mkv").unwrap();
        let work_root = work.owned_root(true, true).unwrap();
        let mut plan = VideoOriginals::new(
            &root,
            &work_root,
            "case.mkv",
            (32, 32),
            2,
            "mezzanine.mkv",
            VideoOriginalKind::MotionFrames,
            0,
        )
        .unwrap();
        // Exercise the existing pre-write space check without filling a disk,
        // changing quotas or adding a production space-check override.
        assert!(root.available_bytes_for("case.mkv").unwrap() < u64::MAX / 2);
        plan.required_free_bytes = u64::MAX / 2;
        let error = plan
            .declare_workspaces(
                &inputs,
                &work,
                &[
                    ("000000.png".into(), 32 * 1024 * 1024),
                    ("000001.png".into(), 32 * 1024 * 1024),
                ],
            )
            .unwrap_err();
        assert_eq!(error.code, "ResourceExhausted");
        assert_eq!(
            error.message,
            "Output lacks the original declared workspace and custody peak"
        );
        assert!(plan.workspace_receipt().is_none());
        assert!(
            !root
                .exists("case.mkv.raw-original-workspaces.json")
                .unwrap()
        );
        assert!(!work_root.exists("mezzanine.mkv").unwrap());
        assert!(
            !work_root
                .exists("ffv1-zscale-help.stdout.original.bin")
                .unwrap()
        );
        assert!(
            !inputs
                .owned_root(true, false)
                .unwrap()
                .exists("000000.png")
                .unwrap()
        );
    }
}
