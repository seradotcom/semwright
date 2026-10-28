from pathlib import Path
import hashlib,json
R=Path(r"E:\Projects\semwright-blender52")
p=R/"crates/driver-blender/tests/windows_native_launch.rs"
s=p.read_text(encoding="utf-8")
snapshot=R/".local/debug-rendezvous-before-20261028"
snapshot.mkdir(exist_ok=False)
(snapshot/p.name).write_bytes(p.read_bytes())
def change(a,b,n=1):
    global s
    assert s.count(a)==n,(a[:100],s.count(a),n)
    s=s.replace(a,b)
for args in ["false, false, false, false, false","true, false, false, false, false","true, true, false, false, false","true, true, true, false, false","true, true, true, true, false","true, true, true, false, true"]:
    change("run_initial_manifest_case("+args+").await;","run_initial_manifest_case("+args+", false).await;")
change('#[test]\n#[ignore = "Owner-only policy/resource comparison; not isolated Blender acceptance"]',
'''#[tokio::test]
#[ignore = "Explicit own-fixture debugger rendezvous; preserves LPAC/Job/zero capabilities"]
async fn initial_host_cpu_resource_debug_probe() {
    run_initial_manifest_case(true, true, true, false, true, true).await;
}
#[test]
#[ignore = "Owner-only policy/resource comparison; not isolated Blender acceptance"]''')
change("    cpu_actctx: bool,\n) {\n    use std::os::windows::ffi::OsStrExt;",
"""    cpu_actctx: bool,
    debug_rendezvous: bool,
) {
    use std::os::windows::ffi::OsStrExt;
    use tokio::io::AsyncBufReadExt;
    assert!(!debug_rendezvous || (cpu_actctx && !cpu_component));""")
change('            if cpu_actctx {\n                "--embedded-cpu-actctx-probe".into()',
'''            if debug_rendezvous {
                "--embedded-cpu-actctx-debug-probe".into()
            } else if cpu_actctx {
                "--embedded-cpu-actctx-probe".into()''')
old="""    let mut input = process.take_stdin().unwrap();
    input.shutdown().await.unwrap();
    drop(input);
    let mut output = process.take_stdout().unwrap().take(8193);
    let mut bytes = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(10), output.read_to_end(&mut bytes)).await;
    if !matches!(read, Ok(Ok(_))) {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    assert!(matches!(read, Ok(Ok(_))), "bounded initial fixture");"""
new="""    let mut input = process.take_stdin().unwrap();
    let mut output = tokio::io::BufReader::new(process.take_stdout().unwrap().take(16385));
    if debug_rendezvous {
        let gate = async {
            let mut line = String::new();
            output.read_line(&mut line).await?;
            if line.len() > 4096 {
                return Err(std::io::Error::other("Debugger readiness exceeds bound"));
            }
            let payload = line.strip_prefix("LPAC_CPU_DEBUG_READY ")
                .ok_or_else(|| std::io::Error::other("Missing fixed readiness marker"))?;
            let ready: serde_json::Value = serde_json::from_str(payload.trim())?;
            if ready["nonce"] != nonce || ready["executable"] != spec.staged_executable.to_string_lossy().as_ref() {
                return Err(std::io::Error::other("Debugger fixture identity mismatch"));
            }
            let directory = std::path::PathBuf::from(std::env::var_os("SEMWRIGHT_TEST_DEBUG_DIR")
                .ok_or_else(|| std::io::Error::other("Explicit private debugger directory required"))?);
            let allowed_parent = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../.local/logs").canonicalize()?;
            let directory = directory.canonicalize()?;
            if directory.parent() != Some(allowed_parent.as_path()) {
                return Err(std::io::Error::other("Debugger output must be a direct private log child"));
            }
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true)
                .open(directory.join("ready.json"))?;
            std::io::Write::write_all(&mut file, ready.to_string().as_bytes())?;
            file.sync_all()?;
            eprintln!("LPAC_DEBUG_RENDEZVOUS {}", directory.display());
            let ack_path = directory.join("debugger-ready.json");
            while !ack_path.exists() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let ack: serde_json::Value = serde_json::from_slice(&std::fs::read(ack_path)?)?;
            if ack["pid"] != ready["pid"] || ack["nonce"] != ready["nonce"] || ack["breakpoints_ready"] != true {
                return Err(std::io::Error::other("Debugger acknowledgment mismatch"));
            }
            input.write_all(b"G").await?;
            Ok::<(), std::io::Error>(())
        };
        let gated = tokio::time::timeout(Duration::from_secs(120), gate).await;
        if !matches!(gated, Ok(Ok(()))) {
            let _ = process.kill().await;
            let _ = process.wait().await;
            panic!("Owned debugger rendezvous failed: {gated:?}");
        }
    }
    input.shutdown().await.unwrap();
    drop(input);
    let mut bytes = Vec::new();
    let read = tokio::time::timeout(Duration::from_secs(if debug_rendezvous {120} else {10}),
        output.read_to_end(&mut bytes)).await;
    if !matches!(read, Ok(Ok(_))) {
        let _ = process.kill().await;
        let _ = process.wait().await;
    }
    assert!(matches!(read, Ok(Ok(_))), "bounded initial fixture");"""
change(old,new)
p.write_text(s,encoding="utf-8",newline="\n")
(snapshot/"patch_receipt.json").write_text(json.dumps({"path":str(p),
"before":hashlib.sha256((snapshot/p.name).read_bytes()).hexdigest(),
"after":hashlib.sha256(p.read_bytes()).hexdigest(),"production_host_policy_changed":False},indent=2)+"\n",encoding="utf-8")
print("Test-only bounded rendezvous applied; production Host unchanged")
