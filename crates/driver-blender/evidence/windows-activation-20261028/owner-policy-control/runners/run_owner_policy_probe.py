from pathlib import Path
import subprocess,os,json,datetime,time,hashlib,shutil
R=Path(r"E:\Projects\semwright-blender52")
env=os.environ.copy()
env.update({
    "PATH":r"C:\msys64\mingw64\bin;C:\Users\owner\.cargo\bin;"+env["PATH"],
    "CARGO_TARGET_DIR":str(R/".local/target"),"CARGO_BUILD_JOBS":"2",
    "CARGO_PROFILE_DEV_DEBUG":"0","CARGO_PROFILE_TEST_DEBUG":"0",
    "CARGO_INCREMENTAL":"0","TMP":str(R/".local/tmp"),"TEMP":str(R/".local/tmp"),
    "SEMWRIGHT_TEST_BLENDER_RUNTIME":
        r"E:\Projects\worldwright-archipelago\tools\blender-5.2.1-runtime"})
assert shutil.disk_usage(R).free>15*1024**3
cmd=[r"C:\Users\owner\.cargo\bin\cargo.exe",
    "+1.98.1-x86_64-pc-windows-gnu","test","-p","semwright-driver-blender",
    "--features","windows-launch-test-fixture","--test","windows_native_launch",
    "--target","x86_64-pc-windows-gnu","--locked","--offline",
    "owner_cpu_resource_policy_probe","--","--ignored","--exact",
    "--nocapture","--test-threads=1"]
base=R/".local/logs/windows-owner-cpu-resource-policy-control-20261028"
log=base.with_suffix(".log");receipt_path=base.with_suffix(".json")
assert not log.exists() and not receipt_path.exists(),"Existing run: inspect, never replay"
start=time.monotonic();utc=datetime.datetime.now(datetime.timezone.utc).isoformat()
timed_out=False;cleanup=None
with log.open("xb") as f:
    p=subprocess.Popen(cmd,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT)
    print(json.dumps({"phase":"started","pid":p.pid,"log":str(log)}),flush=True)
    try:code=p.wait(timeout=180)
    except subprocess.TimeoutExpired:
        timed_out=True
        k=subprocess.run(["taskkill","/PID",str(p.pid),"/T","/F"],
            capture_output=True,text=True,timeout=15)
        cleanup={"returncode":k.returncode,"stdout":k.stdout,"stderr":k.stderr}
        code=p.wait(timeout=10)
text=log.read_text(encoding="utf-8",errors="replace")
marker="OWNER_CPU_RESOURCE_POLICY_RECEIPT "
rows=[line.split(marker,1)[1] for line in text.splitlines() if marker in line]
observed=json.loads(rows[0]) if len(rows)==1 else None
receipt={"utc_start":utc,"seconds":time.monotonic()-start,"argv":cmd,
    "pid":p.pid,"exit_code":code,"timed_out":timed_out,"cleanup":cleanup,
    "log_sha256":hashlib.sha256(log.read_bytes()).hexdigest(),
    "source_snapshot":str(R/".local/owner-policy-before-20261028/applied.json"),
    "owner_receipt":observed,"owner_receipt_count":len(rows),
    "P0_evaluated":False,"A05":"BLOCKED","isolated_blender_started":False}
if observed and observed["harness_executable"]:
    exe=Path(observed["harness_executable"])
    receipt["harness_sha256"]=hashlib.sha256(exe.read_bytes()).hexdigest()
with receipt_path.open("x",encoding="utf-8") as f:
    f.write(json.dumps(receipt,indent=2)+"\n")
summary={k:v for k,v in receipt.items() if k!="owner_receipt"}
if observed:
    summary.update({"diagnostic_ok":observed["diagnostic_ok"],
        "policies_before":observed["policies_before"],
        "policies_after":observed["policies_after"],
        "resource":observed["cpu_resource_activation"],
        "receipt_path":observed["receipt_path"]})
print(json.dumps(summary,indent=2),flush=True)
raise SystemExit(124 if timed_out else code)

