"""Probe invalid pairing without reading credentials into evidence or sending an operation."""
from pathlib import Path
import hashlib,json,os,re,subprocess,time,datetime
A=Path(r"E:\Projects\worldwright-archipelago");S=Path(r"E:\Projects\semwright-blender52")
D=A/"evidence/integration/20261029-godot-broker/godot-bridge01"
r=json.loads((D/"prepared.json").read_text())
out=D/"pairing-attempt01";assert not out.exists()
for p in [D/"pairing-attempt01.log",D/"pairing-attempt01.runner.json"]:assert not p.exists()
H=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
build=S/".local/logs/native-profile-20261028-godot-pairing-proof-build-attempt01"
report=json.loads((build/"run.json").read_text());assert report["exit_code"]==0 and report["sources_unchanged"]
source="crates/driver-godot/tests/windows_pairing.rs";assert H(S/source)==report["source_after"][source]
paths=re.findall(r"Executable.*?\(([^\r\n]+windows_pairing-[a-f0-9]+\.exe)\)",(build/"cargo.log").read_text())
assert len(paths)==1
binary=Path(paths[0])
if not binary.is_absolute():binary=S/binary
# Verify the negative key is distinct; never log the private value.
assert Path(r["secret_file"]).read_text().strip()!="a7"*32
cli=Path(r["paths"]["bin"])/"semwright.exe"
def sessions(label):
    p=subprocess.run([str(cli),"--socket",r["socket"],"--session-file",str(Path(r["paths"]["config"])/"client-session.json"),
                      "--json","execute","driver.godot.session.list","--args-json","{}"],capture_output=True,timeout=20)
    with (D/(label+".json")).open("xb") as f:f.write(p.stdout)
    assert p.returncode==0
    e=json.loads(p.stdout);assert e["ok"]
    return e["data"]["sessions"]
before=sessions("pairing-sessions-before")
env=os.environ.copy();env.update(SEMWRIGHT_TEST_GODOT_BRIDGE_PORT=str(r["port"]),SEMWRIGHT_TEST_GODOT_PROJECT_ID=r["project_id"],
SEMWRIGHT_TEST_GODOT_EVIDENCE_DIR=str(out))
start=time.monotonic()
with (D/"pairing-attempt01.log").open("xb") as log:
    p=subprocess.run([str(binary),"--ignored","--exact","wrong_key_pairing_closes_before_ready","--nocapture"],env=env,cwd=S,
                     stdout=log,stderr=subprocess.STDOUT,timeout=20)
after=sessions("pairing-sessions-after")
result={"schema":1,"utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"exit_code":p.returncode,
"seconds":time.monotonic()-start,"binary_sha256":H(binary),"source_sha256":H(S/source),"build_report_sha256":H(build/"run.json"),
"deliberate_key_distinct":True,"existing_sessions_unchanged":before==after,"valid_session_count":len(after),
"receipt_sha256":H(out/"receipt.json"),"secret_recorded":False,"operations_sent":0}
with (D/"pairing-attempt01.runner.json").open("x",encoding="utf-8") as f:json.dump(result,f,indent=2);f.write("\n")
print(json.dumps(result,indent=2))
raise SystemExit(p.returncode or (0 if before==after and len(after)==1 else 3))
