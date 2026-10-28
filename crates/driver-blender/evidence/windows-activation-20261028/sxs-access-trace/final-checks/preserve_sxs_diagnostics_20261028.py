"""Preserve diagnostics before source cleanup; never overwrites raw evidence."""
from pathlib import Path
import hashlib,json,re,shutil,subprocess,datetime
R=Path(r"E:\Projects\semwright-blender52")
D=R/"crates/driver-blender/evidence/windows-activation-20261028/sxs-access-trace"
assert not D.exists(),"Existing evidence: reconcile before retry"
D.mkdir()
sha=lambda b:hashlib.sha256(b).hexdigest()
files=[]
sources=[
"crates/driver-blender/tests/windows_fsctl_control.rs",
"crates/driver-blender/tests/windows_native_launch.rs",
"crates/driver-blender/src/bin/session_runner/windows_fixture.rs",
"crates/driver-blender/src/bin/session_runner/windows_debug_rendezvous.rs",
"crates/driver-blender/src/bin/session_runner/windows_cpu_images.rs",
"crates/platform-windows-sys/src/launch.rs",
"crates/platform-windows-sys/src/job.rs",
"crates/platform-windows-sys/src/launch/initial_attestation.rs",
"crates/platform-windows-sys/src/launch/initial_lpac.rs"]
def save(source,dest,sanitize=False):
    p=R/source; raw=p.read_bytes()
    value=raw
    sid_hashes=[]
    if sanitize:
        text=raw.decode("utf-8")
        def replacement(m):
            h=sha(m.group().encode("utf-8"));sid_hashes.append(h)
            return "OWNER_SID_SHA256_"+h
        text=re.sub(r"S-1-5-21-(?:\d+-){3}\d+",replacement,text)
        value=text.encode("utf-8")
    target=D/dest
    target.parent.mkdir(parents=True,exist_ok=True)
    with target.open("xb") as f:f.write(value)
    files.append({"path":dest,"raw_source":source,"raw_sha256":sha(raw),
        "bytes":len(value),"sha256":sha(value),"owner_sid_redacted":bool(sid_hashes),
        "redacted_sid_sha256":sorted(set(sid_hashes))})
for p in sources:save(p,"source-at-execution/"+p)
for name in ["run_fsctl_causal_control_20261028.py","trace_lpac_api_20261028.py",
             "run_cpu_debug_20261028.py","inspect_sxs_acl_20261028.py",
             "add_debug_rendezvous_20261028.py"]:
    save(".local/"+name,"repro/"+name)
for name in ["windows-fsctl-causal-control-20261028-attempt01.log",
             "windows-fsctl-causal-control-20261028-attempt01.json",
             "windows-fsctl-control-1793224067469-38956.json",
             "sxs-registry-acl-20261028.json"]:
    save(".local/logs/"+name,"logs/"+name,True)
for p in (R/".local/logs/cpu-debug-20261028-attempt01").iterdir():
    if p.is_file():save(p.relative_to(R).as_posix(),"cpu-debug/"+p.name,True)
diff=subprocess.run(["git","diff","--binary"],cwd=R,check=True,capture_output=True).stdout
with (D/"source-at-execution/tracked-wip.patch").open("xb") as f:f.write(diff)
files.append({"path":"source-at-execution/tracked-wip.patch","bytes":len(diff),"sha256":sha(diff)})
index={"schema_version":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),
 "baseline_head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=R,text=True).strip(),
 "scope":"query-only CPU resource activation under attested LPAC; not Blender startup",
 "results":{"owner_flags0":"CREATE_QUERY_RELEASE_PASS","owner_flags4":"CREATE_QUERY_RELEASE_PASS",
 "lpac_cpu_resource":"DENIED_5_C0000022","debugger":"TRACE_COMPLETED_DETACHED",
 "direct_blender_new_run":"NOT_RUN","Linux_4_5_14":"NOT_RUN","P0":"BLOCKED","A05":"BLOCKED"},
 "limitations":["No cold-cache proof","NtOpenKey SideBySide denial observed, causal intervention not authorized or run",
 "No claim that the complete Blender startup fails only for this cause",
 "GDB system symbol names are export labels; application frame unwind is incomplete"],
 "mutations":{"global_policy":False,"host_policy":False,"runtime_bytes":False,
 "registry_or_acl":False,"additional_lpac_capability":False},
 "source_snapshots":"Exact sources used before formatting/defensive guard follow-up",
 "raw_evidence_preserved":True,"files":files}
with (D/"INDEX.json").open("x",encoding="utf-8",newline="\n") as f:json.dump(index,f,indent=2);f.write("\n")
for item in files:assert sha((D/item["path"]).read_bytes())==item["sha256"]
print(json.dumps({"path":str(D),"files":len(files),"index_sha256":sha((D/"INDEX.json").read_bytes())}))
