from pathlib import Path
import subprocess,os,json,datetime,time,hashlib,shutil,re,sys
R=Path(r"E:\Projects\semwright-blender52")
case=sys.argv[1];attempt=sys.argv[2] if len(sys.argv)>2 else "01"
cases={
 "host-guards": ["test","-p","semwright-platform-windows-sys","--features","windows-sxs-test","--lib","registry_read_diagnostic"],
 "default-zero": ["test","-p","semwright-driver-blender","--features","windows-launch-test-fixture","--test","windows_native_launch","initial_host_exact_blender_manifest_probe"],
 "owner-rejection": ["test","-p","semwright-driver-blender","--features","windows-launch-test-fixture","--test","windows_native_launch","registry_fixture_refuses_an_owner_token"],
 "registry-aba-version": ["test","-p","semwright-driver-blender","--features","windows-launch-test-fixture","--test","windows_native_launch","registry_read_activation_then_blender_version"],
 "initial-bridge": ["test","-p","semwright-driver-blender","--features","windows-launch-test-fixture","--test","windows_native_launch","windows_initial_bridge::registry_read_initial_bridge_typed_edit_export"],
 "production-check": ["check","-p","semwright-driver-blender","--bin","semwright-blender-session-runner"],
}
assert case in cases
D=R/(".local/logs/registry-read-20261028-"+case+"-attempt"+attempt)
assert not D.exists(),"Existing execution: inspect before retry"
D.mkdir()
env=os.environ.copy()
env.update({"PATH":r"C:\msys64\mingw64\bin;C:\Users\owner\.cargo\bin;"+env["PATH"],
 "CARGO_TARGET_DIR":str(R/".local/target"),"CARGO_BUILD_JOBS":"2",
 "CARGO_PROFILE_DEV_DEBUG":"0","CARGO_PROFILE_TEST_DEBUG":"0","CARGO_INCREMENTAL":"0",
 "TMP":str(R/".local/tmp"),"TEMP":str(R/".local/tmp"),
 "SEMWRIGHT_TEST_BLENDER_RUNTIME":r"E:\Projects\worldwright-archipelago\tools\blender-5.2.1-runtime"})
env["SEMWRIGHT_TEST_RECEIPTS"]=str(D)
assert shutil.disk_usage(R).free>15*1024**3
paths=["crates/platform-windows-sys/src/launch.rs","crates/platform-windows-sys/src/lib.rs",
 "crates/platform-windows-sys/Cargo.toml","crates/platform-services/src/lib.rs",
 "crates/platform-services/Cargo.toml","crates/driver-blender/Cargo.toml",
 "crates/driver-blender/src/bin/session_runner/windows.rs",
 "crates/driver-blender/src/bin/session_runner/windows_fixture.rs",
 "crates/driver-blender/src/bin/session_runner/windows_registry_fixture.rs",
 "crates/driver-blender/tests/windows_native_launch.rs",
 "crates/driver-blender/tests/windows_initial_bridge/mod.rs",
 "crates/driver-blender/tests/windows_initial_bridge/bootstrap.py",
 "crates/driver-blender/src/bridge.py","crates/driver-blender/src/semantic.py",
 "crates/driver-blender/src/authoring_native.py",
 "adapters/blender/semwright_blender/commands.py","adapters/blender/semwright_blender/commands.json",
 "adapters/blender/semwright_blender/validation.py","adapters/blender/semwright_blender/export_scope.py",
 "Cargo.lock"]
digest=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
before={p:digest(R/p) for p in paths}
for rel in paths:
    target=D/"source"/rel;target.parent.mkdir(parents=True,exist_ok=True);shutil.copyfile(R/rel,target)
runtime=Path(env["SEMWRIGHT_TEST_BLENDER_RUNTIME"])
runtime_before={x:digest(runtime/x) for x in ["blender.exe","blender_cpu_check.dll"]}
cmd=[r"C:\Users\owner\.cargo\bin\cargo.exe","+1.98.1-x86_64-pc-windows-gnu",*cases[case],
 "--target","x86_64-pc-windows-gnu","--locked","--offline"]
if cases[case][0]=="test":
    cmd+=["--",*(["--ignored"] if case in ["default-zero","registry-aba-version","initial-bridge"] else []),
          *([] if case=="host-guards" else ["--exact"]),"--nocapture","--test-threads=1"]
utc=datetime.datetime.now(datetime.timezone.utc).isoformat();start=time.monotonic();cleanup=None;timed_out=False
log=D/"cargo.log"
with log.open("xb") as f:
    p=subprocess.Popen(cmd,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT)
    print(json.dumps({"phase":"started","case":case,"pid":p.pid,"directory":str(D)}),flush=True)
    try:code=p.wait(timeout=360)
    except subprocess.TimeoutExpired:
        timed_out=True
        k=subprocess.run(["taskkill","/PID",str(p.pid),"/T","/F"],capture_output=True,text=True,timeout=15)
        cleanup={"code":k.returncode,"stdout":k.stdout,"stderr":k.stderr}
        code=p.wait(timeout=10)
text=log.read_text(encoding="utf-8",errors="replace")
observations=[]
for line in text.splitlines():
    pos=line.find("{")
    if pos>=0:
        try:
            obj=json.loads(line[pos:])
            if isinstance(obj,dict):observations.append(obj)
        except json.JSONDecodeError:pass
after={p:digest(R/p) for p in paths}
runtime_after={x:digest(runtime/x) for x in runtime_before}
receipts=[x for x in observations if x.get("phase")=="initial_process_attested_before_resume"]
report={"utc_start":utc,"case":case,"argv":cmd,"pid":p.pid,"exit_code":code,
 "seconds":time.monotonic()-start,"timed_out":timed_out,"cleanup":cleanup,
 "source_before":before,"source_after":after,"sources_unchanged":before==after,
 "runtime_before":runtime_before,"runtime_after":runtime_after,"runtime_unchanged":runtime_before==runtime_after,
 "log_sha256":digest(log),"observations":observations,
 "test_results":re.findall(r"test result:[^\r\n]+",text),
 "authorization":"docs/blender/WINDOWS_SXS_AUTHORIZATION_20261028.md",
 "scope":"explicit bounded test-only registryRead; no production capability grant",
 "A05":"BLOCKED","Linux_4_5_14":"NOT_RUN"}
(D/"run.json").write_text(json.dumps(report,indent=2)+"\n",encoding="utf-8")
print(json.dumps({"phase":"finished","case":case,"exit_code":code,"seconds":report["seconds"],
 "test_results":report["test_results"],"sources_unchanged":report["sources_unchanged"],
 "runtime_unchanged":report["runtime_unchanged"],
 "attested_caps":[x["token"]["capability_count"] for x in receipts],
 "exact_jobs":(all(x["exact_job"] for x in receipts) if receipts else None),
 "observations":len(observations),"directory":str(D)},indent=2),flush=True)
if code!=0:print(text[-14000:],flush=True)
raise SystemExit(code if code else (0 if before==after and runtime_before==runtime_after else 2))
