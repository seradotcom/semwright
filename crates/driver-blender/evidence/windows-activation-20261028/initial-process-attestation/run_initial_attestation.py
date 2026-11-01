from pathlib import Path
import subprocess,os,json,datetime,time,hashlib,shutil,sys,re
R=Path(r"E:\Projects\semwright-blender52")
cases={
    "unit":["--lib","initial_attestation"],
    "stdio":["--test","secure_spawn","appcontainer_spawn_roundtrips_only_allowlisted_stdio"],
    "capabilities":["--test","secure_spawn","network_authority_materializes_only_internet_client_capability"]}
case=sys.argv[1];assert case in cases
kind,selection,filter_name=(cases[case][0],None,cases[case][-1])
env=os.environ.copy()
env.update({
    "PATH":r"C:\msys64\mingw64\bin;C:\Users\owner\.cargo\bin;"+env["PATH"],
    "CARGO_TARGET_DIR":str(R/".local/target"),"CARGO_BUILD_JOBS":"2",
    "CARGO_PROFILE_DEV_DEBUG":"0","CARGO_PROFILE_TEST_DEBUG":"0",
    "CARGO_INCREMENTAL":"0","TMP":str(R/".local/tmp"),"TEMP":str(R/".local/tmp")})
free=shutil.disk_usage(R).free
assert free>15*1024**3
target=["--lib"] if case=="unit" else ["--test","secure_spawn"]
cmd=[r"C:\Users\owner\.cargo\bin\cargo.exe","+1.98.1-x86_64-pc-windows-gnu",
    "test","-p","semwright-platform-windows-sys",*target,
    "--target","x86_64-pc-windows-gnu","--locked","--offline",filter_name,
    "--",*([] if case=="unit" else ["--exact"]),"--nocapture","--test-threads=1"]
base=R/(".local/logs/windows-initial-attestation-"+case+"-20261028")
log=base.with_suffix(".log");receipt_path=base.with_suffix(".json")
assert not log.exists() and not receipt_path.exists(),"Existing run: inspect, never replay"
snapshot=json.loads((R/".local/initial-attestation-before-20261028-attempt02/applied.json")
    .read_text(encoding="utf-8"))
paths=[*snapshot["sha256"],"Cargo.lock"]
if case!="unit":
    paths+=["crates/platform-windows-sys/tests/secure_spawn.rs",
        "crates/platform-windows-sys/src/bin/windows_sandbox_fixture.rs"]
digest=lambda p:hashlib.sha256((R/p).read_bytes()).hexdigest()
before={p:digest(p) for p in paths}
assert all(before[p]==sha for p,sha in snapshot["sha256"].items())
start=time.monotonic();utc=datetime.datetime.now(datetime.timezone.utc).isoformat()
timed_out=False;cleanup=None
with log.open("xb") as f:
    p=subprocess.Popen(cmd,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT)
    print(json.dumps({"phase":"started","case":case,"pid":p.pid,"log":str(log)}),flush=True)
    try:code=p.wait(timeout=300)
    except subprocess.TimeoutExpired:
        timed_out=True
        k=subprocess.run(["taskkill","/PID",str(p.pid),"/T","/F"],
            capture_output=True,text=True,timeout=15)
        cleanup={"exit_code":k.returncode,"stdout":k.stdout,"stderr":k.stderr}
        code=p.wait(timeout=10)
text=log.read_text(encoding="utf-8",errors="replace")
after={p:digest(p) for p in paths}
attestations=[]
for line in text.splitlines():
    if '"phase":"initial_process_attest' in line:
        raw=line[line.find("{"):]
        try:attestations.append(json.loads(raw))
        except json.JSONDecodeError:pass
harnesses=[]
for name in re.findall(r"Running .*?\(([^()\r\n]+\.exe)\)",text):
    path=Path(name)
    if not path.is_absolute():path=R/path
    if path.is_file():
        harnesses.append({"path":str(path),"sha256":hashlib.sha256(path.read_bytes()).hexdigest()})
receipt={"utc_start":utc,"seconds":time.monotonic()-start,"argv":cmd,
    "pid":p.pid,"exit_code":code,"timed_out":timed_out,"cleanup":cleanup,
    "free_bytes_at_start":free,"source_sha256_before":before,"source_sha256_after":after,
    "sources_unchanged_during_run":before==after,"log_sha256":hashlib.sha256(log.read_bytes()).hexdigest(),
    "harnesses":harnesses,"attestations":attestations,"P0":"BLOCKED","A05":"BLOCKED",
    "Linux_4_5_14":"NOT_RUN","ARM_emulated_network":"NOT_RUN",
    "acceptance_scope":"selected native platform checks; not Blender startup"}
with receipt_path.open("x",encoding="utf-8") as f:
    f.write(json.dumps(receipt,indent=2)+"\n")
print(json.dumps({k:v for k,v in receipt.items() if not k.startswith("source_")},indent=2))
print(text[-18000:],flush=True)
assert before==after,"Sources changed during test"
raise SystemExit(124 if timed_out else code)
