from pathlib import Path
import subprocess,os,json,datetime,time,hashlib,shutil,re
R=Path(r"E:\Projects\semwright-blender52")
D=R/".local/logs/driver-first-final-checks-20261028"
assert not D.exists(),"Existing run: inspect, never replay"
D.mkdir()
env=os.environ.copy()
env.update({"PATH":r"C:\msys64\mingw64\bin;C:\Users\sergi\.cargo\bin;"+env["PATH"],
 "CARGO_TARGET_DIR":str(R/".local/target"),"CARGO_BUILD_JOBS":"2",
 "CARGO_PROFILE_DEV_DEBUG":"0","CARGO_PROFILE_TEST_DEBUG":"0","CARGO_INCREMENTAL":"0",
 "TMP":str(R/".local/tmp"),"TEMP":str(R/".local/tmp"),
 "SEMWRIGHT_TEST_BLENDER_RUNTIME":r"E:\Projects\worldwright-archipelago\tools\blender-5.2.1-runtime"})
assert shutil.disk_usage(R).free>15*1024**3
sources=[
"crates/driver-blender/tests/windows_fsctl_control.rs",
"crates/driver-blender/tests/windows_native_launch.rs",
"crates/driver-blender/src/bin/session_runner/windows_fixture.rs",
"crates/driver-blender/src/bin/session_runner/windows_debug_rendezvous.rs",
"crates/driver-blender/src/bin/session_runner/windows_cpu_images.rs",
"crates/platform-windows-sys/src/launch.rs",
"crates/platform-windows-sys/src/job.rs",
"crates/platform-windows-sys/src/launch/initial_attestation.rs",
"crates/platform-windows-sys/src/launch/initial_lpac.rs",
"crates/platform-windows-sys/Cargo.toml","crates/driver-blender/Cargo.toml","Cargo.lock"]
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
hashes=lambda:{p:sha(R/p) for p in sources}
before_fmt=hashes()
fmt=[r"C:\Users\sergi\.cargo\bin\rustfmt.exe","+1.98.1-x86_64-pc-windows-gnu",
 "--edition","2024","--config","skip_children=true",*[p for p in sources if p.endswith(".rs")]]
fmt_result=subprocess.run(fmt,cwd=R,env=env,capture_output=True,timeout=60)
(D/"rustfmt.log").write_bytes(fmt_result.stdout+fmt_result.stderr)
fmt_receipt={"argv":fmt,"exit_code":fmt_result.returncode,"before":before_fmt,"after":hashes()}
(D/"rustfmt.json").write_text(json.dumps(fmt_receipt,indent=2)+"\n",encoding="utf-8")
assert fmt_result.returncode==0
subprocess.run(["git","diff","--check"],cwd=R,check=True,timeout=20)
cargo=[r"C:\Users\sergi\.cargo\bin\cargo.exe","+1.98.1-x86_64-pc-windows-gnu"]
common=["--target","x86_64-pc-windows-gnu","--locked","--offline"]
cases=[
("attestation-unit",["test","-p","semwright-platform-windows-sys","--lib",*common,
 "initial_attestation","--","--nocapture","--test-threads=1"]),
("host-stdio",["test","-p","semwright-platform-windows-sys","--test","secure_spawn",*common,
 "appcontainer_spawn_roundtrips_only_allowlisted_stdio","--","--exact","--nocapture","--test-threads=1"]),
("exact-manifest",["test","-p","semwright-driver-blender","--features","windows-launch-test-fixture",
 "--test","windows_native_launch",*common,"initial_host_exact_blender_manifest_probe",
 "--","--ignored","--exact","--nocapture","--test-threads=1"]),
("fsctl-control",["test","-p","semwright-driver-blender","--features","windows-launch-test-fixture",
 "--test","windows_fsctl_control",*common,"owner_fsctl_restriction_control",
 "--","--ignored","--exact","--nocapture","--test-threads=1"]),
("production-runner-check",["check","-p","semwright-driver-blender","--bin",
 "semwright-blender-session-runner",*common])]
runtime=Path(env["SEMWRIGHT_TEST_BLENDER_RUNTIME"])
runtime_paths=[runtime/"blender.exe",runtime/"blender_cpu_check.dll"]
runtime_before={p.name:sha(p) for p in runtime_paths}
rows=[]
for name,args in cases:
    command=cargo+args;start=time.monotonic();before=hashes()
    log=D/(name+".log");cleanup=None;timeout=False
    with log.open("xb") as f:
        p=subprocess.Popen(command,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT)
        print(json.dumps({"phase":"started","case":name,"pid":p.pid,"log":str(log)}),flush=True)
        try:code=p.wait(timeout=300)
        except subprocess.TimeoutExpired:
            timeout=True
            k=subprocess.run(["taskkill","/PID",str(p.pid),"/T","/F"],capture_output=True,text=True,timeout=15)
            cleanup={"code":k.returncode,"stdout":k.stdout,"stderr":k.stderr}
            code=p.wait(timeout=10)
    text=log.read_text(encoding="utf-8",errors="replace")
    after=hashes()
    attestations=[]
    for line in text.splitlines():
        if '"phase":"initial_process_attest' in line:
            try:attestations.append(json.loads(line[line.find("{"):]))
            except json.JSONDecodeError:pass
    row={"case":name,"argv":command,"pid":p.pid,"exit_code":code,"seconds":time.monotonic()-start,
     "timed_out":timeout,"cleanup":cleanup,"source_before":before,"source_after":after,
     "sources_unchanged":before==after,"log_sha256":sha(log),
     "test_results":re.findall(r"test result:[^\r\n]+",text),"attestations":attestations}
    (D/(name+".json")).write_text(json.dumps(row,indent=2)+"\n",encoding="utf-8")
    rows.append(row)
    print(json.dumps({"phase":"finished","case":name,"exit_code":code,"seconds":row["seconds"],
        "test_results":row["test_results"],"attestations":len(attestations)}),flush=True)
    if code!=0 or timeout or before!=after:break
runtime_after={p.name:sha(p) for p in runtime_paths}
summary={"utc_end":datetime.datetime.now(datetime.timezone.utc).isoformat(),"cases":rows,
 "runtime_before":runtime_before,"runtime_after":runtime_after,"runtime_unchanged":runtime_before==runtime_after,
 "all_selected_pass":len(rows)==len(cases) and all(x["exit_code"]==0 and x["sources_unchanged"] for x in rows),
 "P0":"BLOCKED","A05":"BLOCKED","Linux_4_5_14":"NOT_RUN",
 "scope":"selected Windows hardening and fixture checks; no broker Blender startup acceptance"}
(D/"SUMMARY.json").write_text(json.dumps(summary,indent=2)+"\n",encoding="utf-8")
print(json.dumps({"summary":str(D/"SUMMARY.json"),"all_selected_pass":summary["all_selected_pass"],
 "runtime_unchanged":summary["runtime_unchanged"]}),flush=True)
raise SystemExit(0 if summary["all_selected_pass"] and summary["runtime_unchanged"] else 1)
