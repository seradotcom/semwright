from pathlib import Path
import json,hashlib,shutil
R=Path(r"E:\Projects\semwright-blender52")
script=R/".local/run_initial_attestation.py"
backup=R/".local/run_initial_attestation_before_parser_fix.py"
assert not backup.exists()
shutil.copyfile(script,backup)
source=script.read_text(encoding="utf-8")
old="""raw=line[line.find('{"component":"platform-windows"'):]"""
assert source.count(old)==1
script.write_text(source.replace(old,'raw=line[line.find("{"):]',1),encoding="utf-8")
log=R/".local/logs/windows-initial-attestation-stdio-20261028.log"
rows=[]
for line in log.read_text(encoding="utf-8").splitlines():
    if '"phase":"initial_process_attest' in line:
        rows.append(json.loads(line[line.find("{"):]))
assert len(rows)==1 and rows[0]["token"]["capability_count"]==0
out=R/".local/logs/windows-initial-attestation-stdio-20261028-extracted.json"
with out.open("x",encoding="utf-8") as f:
    json.dump({"source_log_sha256":hashlib.sha256(log.read_bytes()).hexdigest(),
        "parser_fix":"JSON object starts at first brace; key order is not assumed",
        "attestations":rows,"new_execution":False},f,indent=2)
print(json.dumps({"extracted":len(rows),"pid":rows[0]["pid"],"new_execution":False}))

