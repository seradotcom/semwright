from pathlib import Path
import subprocess,os,json,datetime,time,hashlib,shutil
R=Path(r"E:\Projects\semwright-blender52")
env=os.environ.copy()
env.update({"PATH":r"C:\msys64\mingw64\bin;C:\Users\sergi\.cargo\bin;"+env["PATH"],
"CARGO_TARGET_DIR":str(R/".local/target"),"CARGO_BUILD_JOBS":"2",
"CARGO_PROFILE_DEV_DEBUG":"0","CARGO_PROFILE_TEST_DEBUG":"0","CARGO_INCREMENTAL":"0",
"TMP":str(R/".local/tmp"),"TEMP":str(R/".local/tmp"),
"SEMWRIGHT_TEST_BLENDER_RUNTIME":r"E:\Projects\worldwright-archipelago\tools\blender-5.2.1-runtime"})
assert shutil.disk_usage(R).free>15*1024**3
base=R/".local/logs/windows-fsctl-causal-control-20261028-attempt01"
log=base.with_suffix(".log");receipt_path=base.with_suffix(".json")
assert not log.exists() and not receipt_path.exists(),"Existing run: inspect, never replay"
files=["crates/driver-blender/tests/windows_fsctl_control.rs",
"crates/driver-blender/tests/windows_native_launch.rs",
"crates/platform-windows-sys/src/launch.rs","crates/platform-windows-sys/src/job.rs",
"crates/platform-windows-sys/src/launch/initial_attestation.rs",
"crates/driver-blender/src/bin/session_runner/windows_cpu_images.rs","Cargo.lock"]
snapshot=R/".local/resume-driver-first-before-20261028"
snapshot.mkdir(exist_ok=False)
before={}
for name in files:
    data=(R/name).read_bytes();before[name]=hashlib.sha256(data).hexdigest()
    dest=snapshot/name;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes(data)
diff=subprocess.run(["git","diff","--binary"],cwd=R,capture_output=True,check=True).stdout
(snapshot/"tracked-wip.diff").write_bytes(diff)
(snapshot/"manifest.json").write_text(json.dumps(before,indent=2)+"\n",encoding="utf-8")
cmd=[r"C:\Users\sergi\.cargo\bin\cargo.exe","+1.98.1-x86_64-pc-windows-gnu","test",
"-p","semwright-driver-blender","--features","windows-launch-test-fixture",
"--test","windows_fsctl_control","--target","x86_64-pc-windows-gnu","--locked","--offline",
"owner_fsctl_restriction_control","--","--ignored","--exact","--nocapture","--test-threads=1"]
start=time.monotonic();utc=datetime.datetime.now(datetime.timezone.utc).isoformat()
timed_out=False;cleanup=None
with log.open("xb") as f:
    p=subprocess.Popen(cmd,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT)
    print(json.dumps({"phase":"started","pid":p.pid,"log":str(log)}),flush=True)
    try:code=p.wait(timeout=180)
    except subprocess.TimeoutExpired:
        timed_out=True
        k=subprocess.run(["taskkill","/PID",str(p.pid),"/T","/F"],capture_output=True,text=True,timeout=15)
        cleanup={"returncode":k.returncode,"stdout":k.stdout,"stderr":k.stderr};code=p.wait(timeout=10)
text=log.read_text(encoding="utf-8",errors="replace");marker="FSCTL_RESTRICTION_CONTROL "
rows=[line.split(marker,1)[1] for line in text.splitlines() if marker in line]
observed=json.loads(rows[0]) if len(rows)==1 else None
receipt={"utc_start":utc,"seconds":time.monotonic()-start,"argv":cmd,"pid":p.pid,
"exit_code":code,"timed_out":timed_out,"cleanup":cleanup,
"log_sha256":hashlib.sha256(log.read_bytes()).hexdigest(),"source_before":before,
"source_after":{n:hashlib.sha256((R/n).read_bytes()).hexdigest() for n in files},
"observation":observed,"observation_count":len(rows),"A05":"BLOCKED"}
if observed:receipt["harness_sha256"]=hashlib.sha256(Path(observed["harness_executable"]).read_bytes()).hexdigest()
with receipt_path.open("x",encoding="utf-8") as f:f.write(json.dumps(receipt,indent=2)+"\n")
print(json.dumps({"exit_code":code,"seconds":receipt["seconds"],"log":str(log),
"receipt":str(receipt_path),"observation":observed},indent=2),flush=True)
raise SystemExit(124 if timed_out else code)
