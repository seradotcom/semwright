from pathlib import Path
import subprocess,json,hashlib,shutil,datetime
R=Path(r"E:\Projects\semwright-blender52")
git=r"C:\Program Files\Git\cmd\git.exe"
head=subprocess.check_output([git,"rev-parse","HEAD"],cwd=R,text=True).strip()
assert head=="8b6269223f2fb7e13dd1f8af3d41636a1531ffc7",head
B=R/".local/initial-attestation-before-20261028"
assert not B.exists(),"Existing snapshot: inspect before any repeat"
paths=["crates/platform-windows-sys/Cargo.toml",
    "crates/platform-windows-sys/src/job.rs",
    "crates/platform-windows-sys/src/launch.rs",
    "crates/driver-blender/src/bin/session_runner/windows_lpac.rs",
    ".local/initial_attestation.rs",".local/initial_attestation_positive_test.rs"]
before={p:hashlib.sha256((R/p).read_bytes()).hexdigest() for p in paths}
B.mkdir()
for p in paths:
    dst=B/p;dst.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(R/p,dst)
(B/"snapshot.json").write_text(json.dumps({
    "head":head,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "sha256":before},indent=2)+"\n",encoding="utf-8")
def replace_one(source,old,new):
    assert source.count(old)==1,old[:100]
    return source.replace(old,new,1)
cargo=(R/paths[0]).read_text(encoding="utf-8")
cargo=replace_one(cargo,
    'features = ["Win32_System_Console", "Win32_System_LibraryLoader"]',
    'features = ["Win32_System_Console", "Win32_System_LibraryLoader", "Win32_Security", "Win32_System_Threading", "Win32_System_JobObjects"]')
job=(R/paths[1]).read_text(encoding="utf-8")
job=replace_one(job,"    /// Must be called while a securely-created child is still suspended.",
r'''    /// Query membership in this exact Job; never infer it from any ambient Job.
    pub fn verify_member(&self, process: HANDLE) -> Result<()> {
        let mut member = 0i32;
        // SAFETY: both handles are live and the BOOL out-parameter is writable.
        let ok = unsafe {
            windows_sys::Win32::System::JobObjects::IsProcessInJob(
                process.0, self.0.0, &mut member)
        };
        if ok == 0 || member != 1 {
            return Err(Error::new(ErrorCode::SandboxDenied,
                "Initial process membership in the requested Job was not verified"));
        }
        Ok(())
    }

    /// Must be called while a securely-created child is still suspended.''')
job+=r'''

#[cfg(test)]
mod initial_attestation_tests {
    use super::*;
    #[test]
    fn initial_attestation_rejects_an_unrelated_job() {
        let job = ProcessJob::new(Some(1), None, None).unwrap();
        // SAFETY: current-process pseudohandle; this test only queries membership.
        let owner = unsafe { windows::Win32::System::Threading::GetCurrentProcess() };
        assert!(job.verify_member(owner).is_err());
    }
}
'''
launch=(R/paths[2]).read_text(encoding="utf-8")
launch='#[path = "launch/initial_attestation.rs"]\nmod initial_attestation;\n\n'+launch
launch=replace_one(launch,"        let capabilities = SECURITY_CAPABILITIES {",
r'''        // Snapshot the requested identities without modifying the capability request.
        let expected_user = current_user_sid_bytes()?;
        let expected_container = copy_sid_bytes(profile.sid)?;
        let expected_capabilities = capability_entries.iter()
            .map(|entry| Ok((copy_sid_bytes(entry.Sid)?, entry.Attributes)))
            .collect::<Result<Vec<_>>>()?;
        let capabilities = SECURITY_CAPABILITIES {''')
launch=replace_one(launch,
"        // SAFETY: containment is established; resuming now is the first point untrusted code runs.",
r'''        let attestation = job.verify_member(process.raw()).and_then(|()| {
            initial_attestation::verify(
                process.raw().0, &expected_user, &expected_container,
                &expected_capabilities,
            ).map_err(|error| Error::new(ErrorCode::SandboxDenied,
                format!("Initial process token attestation failed: {error}")))
        });
        let attestation = match attestation {
            Ok(value) => value,
            Err(error) => {
                let job_termination_ok = job.terminate(1).is_ok();
                // SAFETY: this is the still-suspended child owned by this invocation.
                let process_termination_ok = unsafe {
                    TerminateProcess(process.raw(), 1)
                }.is_ok();
                // SAFETY: wait is bounded and observes the same owned process handle.
                let exit_observed = unsafe {
                    WaitForSingleObject(process.raw(), 5_000)
                } == WAIT_OBJECT_0;
                eprintln!("{}", serde_json::json!({
                    "component": "platform-windows",
                    "phase": "initial_process_attestation_rejected",
                    "pid": process_info.dwProcessId,
                    "resumed": false,
                    "job_termination_ok": job_termination_ok,
                    "process_termination_ok": process_termination_ok,
                    "exit_observed": exit_observed,
                    "error": error.to_string(),
                }));
                if !exit_observed {
                    return Err(Error::new(ErrorCode::SandboxDenied,
                        "Rejected initial process cleanup could not be confirmed"));
                }
                return Err(error);
            }
        };
        eprintln!("{}", serde_json::json!({
            "component": "platform-windows",
            "phase": "initial_process_attested_before_resume",
            "pid": process_info.dwProcessId,
            "exact_job": true,
            "resumed": false,
            "application_startup_verified": false,
            "token": attestation,
        }));
        // SAFETY: containment is established; resuming now is the first point untrusted code runs.''')
helper=(R/".local/initial_attestation.rs").read_text(encoding="utf-8")
positive=(R/".local/initial_attestation_positive_test.rs").read_text(encoding="utf-8")
assert helper.endswith("}\n")
helper=helper[:-2]+positive+"}\n"
D=R/"crates/platform-windows-sys/src/launch"
assert not (D/"initial_attestation.rs").exists()
assert not (D/"initial_lpac.rs").exists()
D.mkdir(exist_ok=True)
outputs={paths[0]:cargo,paths[1]:job,paths[2]:launch,
    "crates/platform-windows-sys/src/launch/initial_attestation.rs":helper}
for p,content in outputs.items():
    (R/p).write_text(content,encoding="utf-8",newline="\n")
shutil.copyfile(R/paths[3],D/"initial_lpac.rs")
cmd=[r"C:\Users\owner\.cargo\bin\rustup.exe","run",
    "1.98.1-x86_64-pc-windows-gnu","rustfmt","--edition","2024",
    "--config","skip_children=true",str(D/"initial_attestation.rs")]
result=subprocess.run(cmd,cwd=R,capture_output=True,text=True)
(B/"rustfmt.json").write_text(json.dumps({"argv":cmd,"exit_code":result.returncode,
    "stdout":result.stdout,"stderr":result.stderr},indent=2)+"\n",encoding="utf-8")
assert result.returncode==0,result.stderr
assert hashlib.sha256((D/"initial_lpac.rs").read_bytes()).hexdigest()==before[paths[3]]
after={p:hashlib.sha256((R/p).read_bytes()).hexdigest() for p in
    [*outputs,"crates/platform-windows-sys/src/launch/initial_lpac.rs"]}
(B/"applied.json").write_text(json.dumps({
    "head":head,"sha256":after,"lpac_copy_matches_legacy":True,
    "capability_request_unchanged":True,"A05":"BLOCKED","tests":"NOT_RUN"
},indent=2)+"\n",encoding="utf-8")
print(json.dumps({"applied":after,"lpac_copy_matches_legacy":True,"tests":"NOT_RUN"},indent=2))
