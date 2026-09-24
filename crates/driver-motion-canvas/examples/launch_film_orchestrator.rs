#[cfg(target_os = "linux")]
mod linux {
    use semwright_backend_api::{Context, ProvidedCapability, Provider};
    use semwright_driver_host::DriverProvider;
    use semwright_driver_sdk::{
        ApplicationMatch, DriverInterfaces, DriverMount, DriverResources, Manifest,
        SystemConfigMount, Transport,
    };
    use semwright_policy::FilesystemGrant;
    use serde_json::{Value, json};
    use sha2::{Digest, Sha256};
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
        process::Command,
        time::{Duration, Instant},
    };
    use tokio_util::sync::CancellationToken;

    type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

    struct Args {
        motion_driver: PathBuf,
        mlt_driver: PathBuf,
        sandbox_helper: PathBuf,
        motion_runtime: PathBuf,
        mlt_runtime: PathBuf,
        semantic: PathBuf,
        sound: PathBuf,
        work: PathBuf,
        output: PathBuf,
    }

    fn args() -> AnyResult<Args> {
        let values = std::env::args_os()
            .skip(1)
            .map(PathBuf::from)
            .collect::<Vec<_>>();
        if values.len() != 9 {
            return Err("usage: launch_film_orchestrator MOTION_DRIVER MLT_DRIVER SANDBOX_HELPER MOTION_RUNTIME MLT_RUNTIME SEMANTIC SOUND WORK OUTPUT".into());
        }
        Ok(Args {
            motion_driver: values[0].clone(),
            mlt_driver: values[1].clone(),
            sandbox_helper: values[2].clone(),
            motion_runtime: values[3].clone(),
            mlt_runtime: values[4].clone(),
            semantic: values[5].clone(),
            sound: values[6].clone(),
            work: values[7].clone(),
            output: values[8].clone(),
        })
    }

    fn digest(path: &Path) -> AnyResult<String> {
        let mut f = fs::File::open(path)?;
        let mut h = Sha256::new();
        std::io::copy(&mut f, &mut DigestWriter(&mut h))?;
        Ok(format!("{:x}", h.finalize()))
    }
    struct DigestWriter<'a>(&'a mut Sha256);
    impl std::io::Write for DigestWriter<'_> {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.update(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    fn canonical(path: &Path) -> AnyResult<PathBuf> {
        Ok(fs::canonicalize(path)?)
    }
    fn private_dir(path: &Path) -> AnyResult<PathBuf> {
        fs::create_dir_all(path)?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
        canonical(path)
    }
    fn grant(name: &str, path: &Path, write: bool) -> AnyResult<FilesystemGrant> {
        Ok(FilesystemGrant {
            name: name.into(),
            path: canonical(path)?,
            read: true,
            write,
        })
    }
    fn find<'a>(caps: &'a [ProvidedCapability], name: &str) -> AnyResult<&'a ProvidedCapability> {
        caps.iter()
            .find(|c| c.descriptor.name == name)
            .ok_or_else(|| format!("missing capability {name}").into())
    }
    async fn call(
        provider: &DriverProvider,
        caps: &[ProvidedCapability],
        provider_id: &str,
        name: &str,
        args: Value,
        trace: &mut Vec<Value>,
    ) -> AnyResult<Value> {
        let started = Instant::now();
        let result = Provider::execute(
            provider,
            &Context {
                session: "launch-film".into(),
                request_id: semwright_types::unique_id(),
                cancellation: CancellationToken::new(),
            },
            &find(caps, name)?.descriptor,
            &args,
        )
        .await;
        let duration_ms = started.elapsed().as_millis() as u64;
        match &result {
            Ok(_) => trace.push(json!({"provider":provider_id,"capability":name,"duration_ms":duration_ms,"status":"ok"})),
            Err(error) => trace.push(json!({"provider":provider_id,"capability":name,"duration_ms":duration_ms,"status":"error","error_code":format!("{:?}",error.code)})),
        }
        Ok(result?)
    }
    fn created_ref(value: &Value, kind: &str) -> AnyResult<String> {
        value["created_refs"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["kind"] == kind))
            .and_then(|row| row["reference"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| format!("missing created {kind} ref").into())
    }
    fn named_ref(value: &Value, name: &str) -> AnyResult<String> {
        value["items"]
            .as_array()
            .and_then(|rows| rows.iter().find(|row| row["name"] == name))
            .and_then(|row| row["reference"].as_str())
            .map(str::to_owned)
            .ok_or_else(|| format!("missing ref for {name}").into())
    }
    async fn sequence_ref(
        provider: &DriverProvider,
        caps: &[ProvidedCapability],
        project: &str,
        trace: &mut Vec<Value>,
    ) -> AnyResult<String> {
        let rows = call(
            provider,
            caps,
            "driver:mlt-video",
            "driver.mlt-video.sequence.list",
            json!({"project":project,"limit":100}),
            trace,
        )
        .await?;
        named_ref(&rows, "Main")
    }
    fn manifest(
        id: &str,
        executable: &Path,
        process: &str,
        mounts: Vec<DriverMount>,
    ) -> AnyResult<Manifest> {
        Ok(Manifest {
            manifest_version: 1,
            protocol: 1,
            id: id.into(),
            version: env!("CARGO_PKG_VERSION").into(),
            publisher: "semwright-launch-film".into(),
            executable: canonical(executable)?,
            sha256: digest(executable)?,
            application: ApplicationMatch {
                desktop_id: None,
                process_names: vec![process.into()],
                supported_versions: vec![],
            },
            transport: Transport::StdioV1,
            mounts,
            system_config: vec![],
            network: false,
            resources: DriverResources {
                open_files: 512,
                processes: 128,
                cpu_seconds: 300,
                address_space_bytes: 4_294_967_296,
                file_size_bytes: 1_073_741_824,
            },
            request_timeout_ms: 300_000,
            interfaces: DriverInterfaces::default(),
        })
    }
    fn command_ok(mut command: Command, label: &str) -> AnyResult<()> {
        let status = command.status()?;
        if !status.success() {
            return Err(format!("{label} failed with {status}").into());
        }
        Ok(())
    }

    pub async fn run() -> AnyResult<()> {
        let a = args()?;
        for p in [
            &a.motion_driver,
            &a.mlt_driver,
            &a.sandbox_helper,
            &a.motion_runtime,
            &a.mlt_runtime,
            &a.semantic,
            &a.sound,
        ] {
            canonical(p)?;
        }
        let work = private_dir(&a.work)?;
        let output = private_dir(&a.output)?;
        let motion_project = private_dir(&work.join("motion-project"))?;
        let motion_output = private_dir(&work.join("motion-output"))?;
        let motion_state = private_dir(&work.join("motion-state"))?;
        fs::copy(&a.semantic, motion_project.join("semwright-motion.json"))?;

        let mut operations = vec![];
        let mut motion_manifest = manifest(
            "motion-canvas",
            &a.motion_driver,
            "node",
            vec![
                DriverMount {
                    root: "project".into(),
                    read_only: false,
                },
                DriverMount {
                    root: "output".into(),
                    read_only: false,
                },
                DriverMount {
                    root: "runtime".into(),
                    read_only: true,
                },
            ],
        )?;
        motion_manifest.system_config.push(SystemConfigMount {
            root: "fontconfig".into(),
            destination: "/etc/fonts".into(),
        });
        let motion_grants = vec![
            grant("project", &motion_project, true)?,
            grant("output", &motion_output, true)?,
            grant("runtime", &a.motion_runtime, false)?,
            grant("fontconfig", Path::new("/etc/fonts"), false)?,
        ];
        let motion = DriverProvider::connect(
            motion_manifest,
            &motion_state,
            &canonical(&a.sandbox_helper)?,
            &motion_grants,
            false,
        )
        .await?;
        let motion_caps = Provider::capabilities(motion.as_ref()).await?;
        let doctor = call(
            motion.as_ref(),
            &motion_caps,
            "driver:motion-canvas",
            "driver.motion-canvas.doctor",
            json!({}),
            &mut operations,
        )
        .await?;
        if doctor["render_available"] != true {
            return Err(format!("Motion Canvas runtime unavailable: {doctor}").into());
        }
        let inspected = call(
            motion.as_ref(),
            &motion_caps,
            "driver:motion-canvas",
            "driver.motion-canvas.project.inspect",
            json!({}),
            &mut operations,
        )
        .await?;
        let fingerprint = inspected["fingerprint"]
            .as_str()
            .ok_or("missing Motion Canvas fingerprint")?
            .to_owned();
        let started = call(motion.as_ref(),&motion_caps,"driver:motion-canvas","driver.motion-canvas.render.start",json!({
            "expected_fingerprint":fingerprint,
            "profile":{"first_frame":0,"end_frame_exclusive":1560,"scale":"full","transparent":false,"timeout_ms":300000}
        }),&mut operations).await?;
        let motion_job = started["job_ref"]
            .as_str()
            .ok_or("missing motion job ref")?
            .to_owned();
        let motion_terminal = loop {
            let status = call(
                motion.as_ref(),
                &motion_caps,
                "driver:motion-canvas",
                "driver.motion-canvas.render.status",
                json!({"job_ref":motion_job}),
                &mut operations,
            )
            .await?;
            match status["state"].as_str() {
                Some("succeeded" | "failed" | "cancelled") => break status,
                _ => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        };
        if motion_terminal["state"] != "succeeded" {
            return Err(format!("Motion Canvas full render failed: {motion_terminal}").into());
        }
        let motion_result = call(
            motion.as_ref(),
            &motion_caps,
            "driver:motion-canvas",
            "driver.motion-canvas.render.result",
            json!({"job_ref":motion_job}),
            &mut operations,
        )
        .await?;
        Provider::shutdown(motion.as_ref()).await?;
        let directory = motion_result["artifact"]["directory"]
            .as_str()
            .ok_or("missing motion artifact directory")?;
        let frames = motion_output.join(directory).join("frames");
        let poster_source = frames.join("001500.png");
        fs::copy(&poster_source, output.join("poster.png"))?;
        let review_dir = private_dir(&output.join("review"))?;
        let mut review_artifacts = vec![];
        for frame in [90u64, 285, 525, 795, 1035, 1260, 1500] {
            let source = frames.join(format!("{frame:06}.png"));
            let name = format!("frame-{frame:06}.png");
            let destination = review_dir.join(&name);
            fs::copy(&source, &destination)?;
            review_artifacts.push(json!({
                "path": format!("review/{name}"),
                "sha256": digest(&destination)?,
                "bytes": fs::metadata(&destination)?.len()
            }));
        }

        let media = private_dir(&work.join("mlt-media"))?;
        let intermediate = media.join("motion.mp4");
        let ffmpeg = Path::new("/usr/bin/ffmpeg");
        let mut encode = Command::new(ffmpeg);
        encode
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-framerate",
                "30",
                "-start_number",
                "0",
                "-i",
            ])
            .arg(frames.join("%06d.png"))
            .args([
                "-frames:v",
                "1560",
                "-c:v",
                "libx264",
                "-preset",
                "veryfast",
                "-crf",
                "12",
                "-pix_fmt",
                "yuv420p",
                "-an",
            ])
            .arg(&intermediate);
        let t = Instant::now();
        command_ok(encode, "controlled frame encode")?;
        operations.push(json!({"provider":"build-tool:ffmpeg","capability":"frames.to_intermediate_video","duration_ms":t.elapsed().as_millis() as u64,"status":"ok"}));
        fs::copy(&a.sound, media.join("launch-sound.wav"))?;

        let mlt_project = private_dir(&work.join("mlt-project"))?;
        let mlt_state = private_dir(&work.join("mlt-state"))?;
        let mlt_manifest = manifest(
            "mlt-video",
            &a.mlt_driver,
            "melt",
            vec![
                DriverMount {
                    root: "project".into(),
                    read_only: true,
                },
                DriverMount {
                    root: "media".into(),
                    read_only: true,
                },
                DriverMount {
                    root: "output".into(),
                    read_only: false,
                },
                DriverMount {
                    root: "runtime".into(),
                    read_only: true,
                },
            ],
        )?;
        let mlt_grants = vec![
            grant("project", &mlt_project, false)?,
            grant("media", &media, false)?,
            grant("output", &output, true)?,
            grant("runtime", &a.mlt_runtime, false)?,
        ];
        let mlt = DriverProvider::connect(
            mlt_manifest,
            &mlt_state,
            &canonical(&a.sandbox_helper)?,
            &mlt_grants,
            false,
        )
        .await?;
        let mlt_caps = Provider::capabilities(mlt.as_ref()).await?;
        let mlt_doctor = call(
            mlt.as_ref(),
            &mlt_caps,
            "driver:mlt-video",
            "driver.mlt-video.doctor",
            json!({}),
            &mut operations,
        )
        .await?;
        if mlt_doctor["render_available"] != true {
            return Err(format!("MLT runtime unavailable: {mlt_doctor}").into());
        }
        let created = call(mlt.as_ref(),&mlt_caps,"driver:mlt-video","driver.mlt-video.project.create",json!({"profile":{
            "width":1920,"height":1080,"fps_num":30,"fps_den":1,"progressive":true,"sample_aspect_num":1,"sample_aspect_den":1,"display_aspect_num":16,"display_aspect_den":9,"colorspace":709,"audio_channels":2
        }}),&mut operations).await?;
        let mut project_ref = created["project"]
            .as_str()
            .ok_or("missing MLT project ref")?
            .to_owned();
        let mut revision = created["revision"]
            .as_str()
            .ok_or("missing MLT revision")?
            .to_owned();
        let seq = call(
            mlt.as_ref(),
            &mlt_caps,
            "driver:mlt-video",
            "driver.mlt-video.sequence.create",
            json!({"project":project_ref,"expected_revision":revision,"name":"Main"}),
            &mut operations,
        )
        .await?;
        let _ = created_ref(&seq, "sequence")?;
        project_ref = seq["project"].as_str().unwrap().to_owned();
        revision = seq["resulting_revision"].as_str().unwrap().to_owned();
        for (name, path) in [("Motion", "motion.mp4"), ("Sound", "launch-sound.wav")] {
            let v=call(mlt.as_ref(),&mlt_caps,"driver:mlt-video","driver.mlt-video.asset.import",json!({"project":project_ref,"expected_revision":revision,"kind":"file","name":name,"root":"media","path":path}),&mut operations).await?;
            project_ref = v["project"].as_str().unwrap().to_owned();
            revision = v["resulting_revision"].as_str().unwrap().to_owned();
        }
        for (name, kind) in [("Video", "video"), ("Audio", "audio")] {
            let sr = sequence_ref(mlt.as_ref(), &mlt_caps, &project_ref, &mut operations).await?;
            let v=call(mlt.as_ref(),&mlt_caps,"driver:mlt-video","driver.mlt-video.track.create",json!({"project":project_ref,"expected_revision":revision,"sequence":sr,"name":name,"kind":kind}),&mut operations).await?;
            project_ref = v["project"].as_str().unwrap().to_owned();
            revision = v["resulting_revision"].as_str().unwrap().to_owned();
        }
        let assets = call(
            mlt.as_ref(),
            &mlt_caps,
            "driver:mlt-video",
            "driver.mlt-video.asset.list",
            json!({"project":project_ref,"limit":100}),
            &mut operations,
        )
        .await?;
        let sr = sequence_ref(mlt.as_ref(), &mlt_caps, &project_ref, &mut operations).await?;
        let tracks = call(
            mlt.as_ref(),
            &mlt_caps,
            "driver:mlt-video",
            "driver.mlt-video.track.list",
            json!({"project":project_ref,"sequence":sr,"limit":100}),
            &mut operations,
        )
        .await?;
        let video_asset = named_ref(&assets, "Motion")?;
        let audio_asset = named_ref(&assets, "Sound")?;
        let video_track = named_ref(&tracks, "Video")?;
        let audio_track = named_ref(&tracks, "Audio")?;
        for (track, asset, name) in [
            (video_track, video_asset, "Motion"),
            (audio_track, audio_asset, "Sound"),
        ] {
            let sr = sequence_ref(mlt.as_ref(), &mlt_caps, &project_ref, &mut operations).await?;
            let v=call(mlt.as_ref(),&mlt_caps,"driver:mlt-video","driver.mlt-video.clip.insert",json!({
                "project":project_ref,"expected_revision":revision,"sequence":sr,"track":track,"asset":asset,"start":0,"source_in":0,"source_out":1560,"name":name
            }),&mut operations).await?;
            project_ref = v["project"].as_str().unwrap().to_owned();
            revision = v["resulting_revision"].as_str().unwrap().to_owned();
        }
        let sr = sequence_ref(mlt.as_ref(), &mlt_caps, &project_ref, &mut operations).await?;
        let plan=call(mlt.as_ref(),&mlt_caps,"driver:mlt-video","driver.mlt-video.render.plan",json!({"project":project_ref,"sequence":sr,"profile":"h264-1080p","output":"launch-film-1080p.mp4"}),&mut operations).await?;
        if plan["runnable"] != true || plan["frames"] != 1560 {
            return Err(format!("Unexpected MLT render plan: {plan}").into());
        }
        let started=call(mlt.as_ref(),&mlt_caps,"driver:mlt-video","driver.mlt-video.render.start",json!({"project":project_ref,"expected_revision":revision,"sequence":sr,"profile":"h264-1080p","output":"launch-film-1080p.mp4"}),&mut operations).await?;
        let job = started["job"].as_str().ok_or("missing MLT job")?.to_owned();
        let terminal = loop {
            let status = call(
                mlt.as_ref(),
                &mlt_caps,
                "driver:mlt-video",
                "driver.mlt-video.render.status",
                json!({"job":job}),
                &mut operations,
            )
            .await?;
            match status["state"].as_str() {
                Some("succeeded" | "failed" | "cancelled" | "unknown") => break status,
                _ => tokio::time::sleep(Duration::from_millis(500)).await,
            }
        };
        if terminal["state"] != "succeeded" {
            return Err(format!("MLT launch-film render failed: {terminal}").into());
        }
        let final_result = call(
            mlt.as_ref(),
            &mlt_caps,
            "driver:mlt-video",
            "driver.mlt-video.render.result",
            json!({"job":job}),
            &mut operations,
        )
        .await?;
        Provider::shutdown(mlt.as_ref()).await?;

        let final_video = output.join("launch-film-1080p.mp4");
        let poster = output.join("poster.png");
        let ffprobe_output = Command::new("/usr/bin/ffprobe")
            .args([
                "-v",
                "error",
                "-show_streams",
                "-show_format",
                "-of",
                "json",
            ])
            .arg(&final_video)
            .output()?;
        if !ffprobe_output.status.success() {
            return Err("final ffprobe failed".into());
        }
        let ffprobe: Value = serde_json::from_slice(&ffprobe_output.stdout)?;
        let trace = json!({
            "schema":1,
            "source_commit":std::env::var("GITHUB_SHA").ok(),
            "semantic_source":"demos/launch-film/semwright-motion.json",
            "semantic_source_sha256":digest(&a.semantic)?,
            "providers":[
                {"id":"driver:motion-canvas","capability_count":motion_caps.len(),"network":false},
                {"id":"driver:mlt-video","capability_count":mlt_caps.len(),"network":false}
            ],
            "build_tools":[{"id":"ffmpeg","path":"/usr/bin/ffmpeg","sha256":digest(ffmpeg)?}],
            "operations":operations,
            "errors_retries":[],
            "motion_canvas_artifact":motion_result["artifact"].clone(),
            "mlt_result":final_result,
            "artifacts":[
                {"path":"launch-film-1080p.mp4","sha256":digest(&final_video)?,"bytes":fs::metadata(&final_video)?.len()},
                {"path":"poster.png","sha256":digest(&poster)?,"bytes":fs::metadata(&poster)?.len()}
            ],
            "review_frames":review_artifacts,
            "ffprobe":ffprobe
        });
        fs::write(
            output.join("DEMO_TRACE.json"),
            serde_json::to_vec_pretty(&trace)?,
        )?;
        println!("{}", serde_json::to_string_pretty(&trace)?);
        Ok(())
    }
}

#[cfg(target_os = "linux")]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    if let Err(error) = linux::run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("launch-film orchestration is currently certified on Linux CI only");
    std::process::exit(5);
}
