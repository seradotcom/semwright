from pathlib import Path
import subprocess,os,json,datetime,time,hashlib,shutil
R=Path(r"E:\Projects\semwright-blender52")
D=R/".local/logs/cpu-debug-20261028-attempt01"
D.mkdir(exist_ok=False)
env=os.environ.copy()
env.update({"PATH":r"C:\msys64\mingw64\bin;C:\Users\owner\.cargo\bin;"+env["PATH"],
"CARGO_TARGET_DIR":str(R/".local/target"),"CARGO_BUILD_JOBS":"2",
"CARGO_PROFILE_DEV_DEBUG":"0","CARGO_PROFILE_TEST_DEBUG":"0","CARGO_INCREMENTAL":"0",
"TMP":str(R/".local/tmp"),"TEMP":str(R/".local/tmp"),
"SEMWRIGHT_TEST_BLENDER_RUNTIME":r"E:\Projects\worldwright-archipelago\tools\blender-5.2.1-runtime",
"SEMWRIGHT_TEST_DEBUG_DIR":str(D),"SEMWRIGHT_GDB_READY_DIR":str(D),
"DEBUGINFOD_URLS":""})
assert shutil.disk_usage(R).free>15*1024**3
files=["crates/driver-blender/tests/windows_native_launch.rs",
"crates/driver-blender/src/bin/session_runner/windows_fixture.rs",
"crates/driver-blender/src/bin/session_runner/windows_debug_rendezvous.rs",
"crates/platform-windows-sys/src/launch.rs","crates/platform-windows-sys/src/job.rs",
"crates/platform-windows-sys/src/launch/initial_attestation.rs",
".local/trace_lpac_api_20261028.py"]
before={n:hashlib.sha256((R/n).read_bytes()).hexdigest() for n in files}
cmd=[r"C:\Users\owner\.cargo\bin\cargo.exe","+1.98.1-x86_64-pc-windows-gnu","test",
"-p","semwright-driver-blender","--features","windows-launch-test-fixture",
"--test","windows_native_launch","--target","x86_64-pc-windows-gnu","--locked","--offline",
"initial_host_cpu_resource_debug_probe","--","--ignored","--exact","--nocapture","--test-threads=1"]
start=time.monotonic();utc=datetime.datetime.now(datetime.timezone.utc).isoformat()
debugger=None;debug_code=None;cleanup=[];ready=None
with (D/"cargo.log").open("xb") as f:
    p=subprocess.Popen(cmd,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT)
    print(json.dumps({"phase":"test_started","pid":p.pid,"directory":str(D)}),flush=True)
    while p.poll() is None and time.monotonic()-start<180:
        if (D/"ready.json").exists():
            try:ready=json.loads((D/"ready.json").read_text())
            except json.JSONDecodeError:time.sleep(.1);continue
            break
        time.sleep(.2)
    if ready:
        assert ready["probe"]=="lpac_cpu_debug_ready" and ready["host_policy_changed"] is False
        with (D/"gdb.log").open("xb") as df:
            dbg_cmd=[r"C:\msys64\mingw64\bin\gdb.exe","--nx","--batch","--quiet",
                "-iex","set auto-load off","-iex","set debuginfod enabled off",
                "-p",str(ready["pid"]),"-ex","source "+str(R/".local/trace_lpac_api_20261028.py").replace("\\","/")]
            debugger=subprocess.Popen(dbg_cmd,cwd=R,env=env,stdout=df,stderr=subprocess.STDOUT)
            print(json.dumps({"phase":"debugger_started","gdb_pid":debugger.pid,"fixture_pid":ready["pid"]}),flush=True)
            try:debug_code=debugger.wait(timeout=110)
            except subprocess.TimeoutExpired:
                k=subprocess.run(["taskkill","/PID",str(debugger.pid),"/T","/F"],capture_output=True,text=True,timeout=15)
                cleanup.append({"owned":"debugger","pid":debugger.pid,"returncode":k.returncode})
                debug_code=debugger.wait(timeout=10)
    try:code=p.wait(timeout=150 if ready else 30)
    except subprocess.TimeoutExpired:
        k=subprocess.run(["taskkill","/PID",str(p.pid),"/T","/F"],capture_output=True,text=True,timeout=15)
        cleanup.append({"owned":"cargo","pid":p.pid,"returncode":k.returncode});code=p.wait(timeout=10)
receipt={"utc_start":utc,"seconds":time.monotonic()-start,"argv":cmd,"cargo_pid":p.pid,
"cargo_exit":code,"gdb_pid":debugger.pid if debugger else None,"gdb_exit":debug_code,
"fixture_ready":ready,"cleanup":cleanup,"source_before":before,
"source_after":{n:hashlib.sha256((R/n).read_bytes()).hexdigest() for n in files},
"artifacts":{f.name:{"bytes":f.stat().st_size,"sha256":hashlib.sha256(f.read_bytes()).hexdigest()}
for f in D.iterdir() if f.is_file()},"A05":"BLOCKED","debugger_not_production_acceptance":True}
with (D/"run.json").open("x",encoding="utf-8") as f:f.write(json.dumps(receipt,indent=2)+"\n")
print(json.dumps({"phase":"complete","cargo_exit":code,"gdb_exit":debug_code,
"seconds":receipt["seconds"],"directory":str(D),"cleanup":cleanup,
"summary":json.loads((D/"trace-summary.json").read_text()) if (D/"trace-summary.json").exists() else None},indent=2),flush=True)
raise SystemExit(code)
