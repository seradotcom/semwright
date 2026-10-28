#![cfg(all(target_os="windows",feature="windows-launch-test-fixture"))]
use semwright_platform_api::launch::{
    encode_host_tool_arg_ref,HostToolArgRef,Mount,MountClass,ResourceLimits,SandboxKind,SandboxSpec,
    SealedToolMount,SealedToolSource,SANDBOX_HOST_TOOL_CHILD_ENV,SANDBOX_HOST_TOOL_CWD_ENV,
    SANDBOX_HOST_TOOL_TYPED_ARGS_ENV,
};
use sha2::{Digest,Sha256};
use std::{path::Path,time::Duration};
use tokio::io::{AsyncReadExt,AsyncWriteExt};
fn harden(path:&Path){
    let principal=format!("{}\\{}",std::env::var("USERDOMAIN").unwrap(),std::env::var("USERNAME").unwrap());
    let status=std::process::Command::new("icacls").arg(path).arg("/inheritance:r")
        .arg("/grant:r").arg(format!("{principal}:(F)")).status().unwrap();
    assert!(status.success());
}
#[test]
fn launch_fixture_refuses_an_owner_token(){
    let output=std::process::Command::new(env!("CARGO_BIN_EXE_semwright-blender-launch-fixture"))
        .arg("--child-probe").output().unwrap();
    assert!(!output.status.success());assert!(output.stdout.is_empty());
}
#[tokio::test]
async fn exact_native_launcher_creates_only_a_same_lpac_child(){run_case("--launch-probe").await;}
#[tokio::test]
async fn no_stdio_diagnostic_preserves_the_same_lpac_identity(){run_case("--launch-no-stdio-probe").await;}
async fn run_case(mode:&str){
    let binaries=tempfile::tempdir().unwrap();let workspace=tempfile::tempdir().unwrap();
    let parent=binaries.path().join("parent.exe");let dependency=binaries.path().join("child.exe");
    for target in [&parent,&dependency]{
        std::fs::copy(env!("CARGO_BIN_EXE_semwright-blender-launch-fixture"),target).unwrap();
        harden(target);
    }
    let digest=hex::encode(Sha256::digest(std::fs::read(&dependency).unwrap()));
    let spec=SandboxSpec{
        kind:SandboxKind::Driver,staged_executable:parent,helper:std::env::current_exe().unwrap(),
        mounts:vec![Mount{source:workspace.path().to_path_buf(),class:MountClass::Workspace,
            logical_name:"work".into(),read_only:false,execute:false}],
        args:vec![mode.into(),
            encode_host_tool_arg_ref(&HostToolArgRef::ToolPath{tool:"fixture-child".into()}).unwrap(),
            encode_host_tool_arg_ref(&HostToolArgRef::MountPath{mount:"work".into(),relative:String::new()}).unwrap()],
        environment:vec![(SANDBOX_HOST_TOOL_CHILD_ENV.into(),"1".into()),
            (SANDBOX_HOST_TOOL_TYPED_ARGS_ENV.into(),"1".into()),
            (SANDBOX_HOST_TOOL_CWD_ENV.into(),workspace.path().to_string_lossy().into_owned())],
        sealed_tools:vec![SealedToolMount{name:"fixture-child".into(),
            source:SealedToolSource::VerifiedFile{path:dependency,sha256:digest}}],
        network:false,nvidia_gpu:false,
        limits:Some(ResourceLimits{open_files:32,processes:8,cpu_seconds:5,
            address_space_bytes:256*1024*1024,file_size_bytes:1024*1024}),
    };
    let mut process=semwright_platform_services::sandbox_spawn(&spec).expect("isolated fixture spawn");
    let mut input=process.take_stdin().unwrap();input.shutdown().await.unwrap();drop(input);
    let mut output=process.take_stdout().unwrap().take(32769);let mut bytes=Vec::new();
    let read=tokio::time::timeout(Duration::from_secs(10),output.read_to_end(&mut bytes)).await;
    if !matches!(read,Ok(Ok(_))){let _=process.kill().await;let _=process.wait().await;}
    assert!(matches!(read,Ok(Ok(_))),"bounded fixture response");
    assert!(bytes.len()<=32768);
    let waited=tokio::time::timeout(Duration::from_secs(5),process.wait_exit_code()).await;
    if waited.is_err(){let _=process.kill().await;let _=process.wait().await;}
    let exit=waited.expect("fixture bounded exit").expect("fixture exit query");
    let text=String::from_utf8(bytes).unwrap();eprintln!("isolated fixture result: {text}");
    let row:serde_json::Value=serde_json::from_str(text.trim()).expect("structured fixture result");
    assert_eq!(row["native_blender"],false);
    assert_eq!(exit,Some(0),"fixture must execute under the same confinement");
    assert_eq!(row["ok"],true);
}

#[tokio::test]
#[ignore="Requires the explicitly pinned private Blender 5.2.1 runtime on the Windows test host"]
async fn direct_host_isolated_blender_version_probe(){
    let runtime=std::path::PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_BLENDER_RUNTIME").expect("explicit runtime"));
    let executable=runtime.join("blender.exe");
    let expected="284f4041f98e113f3dc10654a7193ffaaa9bfdfec8b87fa116620a48b5f6d4cb";
    let bytes=semwright_platform_services::verify_sealed_tool_executable(&executable,expected)
        .expect("owner pinned application image");
    assert_eq!(hex::encode(Sha256::digest(&bytes)),expected);drop(bytes);
    let spec=SandboxSpec{
        kind:SandboxKind::Driver,staged_executable:executable,helper:std::env::current_exe().unwrap(),
        mounts:vec![Mount{source:runtime.clone(),class:MountClass::Workspace,
            logical_name:"runtime".into(),read_only:true,execute:true}],
        args:vec!["--version".into()],
        environment:vec![(SANDBOX_HOST_TOOL_CHILD_ENV.into(),"1".into()),
            (SANDBOX_HOST_TOOL_TYPED_ARGS_ENV.into(),"1".into()),
            (SANDBOX_HOST_TOOL_CWD_ENV.into(),runtime.to_string_lossy().into_owned())],
        sealed_tools:vec![],network:false,nvidia_gpu:false,
        limits:Some(ResourceLimits{open_files:256,processes:8,cpu_seconds:30,
            address_space_bytes:4*1024*1024*1024,file_size_bytes:1024*1024}),
    };
    let mut process=semwright_platform_services::sandbox_spawn(&spec).expect("direct contained Blender");
    let mut input=process.take_stdin().unwrap();input.shutdown().await.unwrap();drop(input);
    let mut output=process.take_stdout().unwrap().take(16385);let mut bytes=Vec::new();
    let read=tokio::time::timeout(Duration::from_secs(30),output.read_to_end(&mut bytes)).await;
    if !matches!(read,Ok(Ok(_))){let _=process.kill().await;let _=process.wait().await;}
    assert!(matches!(read,Ok(Ok(_))),"bounded native version response");
    assert!(bytes.len()<=16384);
    let exit=process.wait_exit_code().await.expect("native exit");
    let text=String::from_utf8(bytes).unwrap();
    eprintln!("direct Host Blender version exit={exit:?}, broker_used=false, scene_operation=false:\n{text}");
    assert_eq!(exit,Some(0));assert!(text.starts_with("Blender 5.2.1"));
}
