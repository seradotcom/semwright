from pathlib import Path
import datetime,hashlib,json,subprocess,time
R=Path(r'E:\Projects\semwright-blender52');O=R/'.local/logs/cpu-resource-sxs-20261028T185809Z-0c04a049'
src=R/'.local/export_cpu_sxs_metadata.ps1';out=O/'etl-metadata.json';err=O/'etl-metadata.stderr.bin';receipt=O/'etl-metadata-reader-receipt.json'
assert not out.exists() and not receipt.exists()
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
argv=[r'C:\WINDOWS\System32\WindowsPowerShell\v1.0\powershell.exe','-NoLogo','-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',str(src)]
row={'argv':argv,'source_sha256':sha(src),'utc_start':datetime.datetime.now(datetime.timezone.utc).isoformat(),'new_trace':False,'cargo_repetitions':0,'A05':'BLOCKED'}
t=time.monotonic()
try:
    with out.open('xb') as o,err.open('xb') as e:p=subprocess.run(argv,cwd=R,stdout=o,stderr=e,timeout=15)
    row['exit_code']=p.returncode
    try:
        data=json.loads(out.read_text(encoding='utf-8-sig'));row['summary']={k:v for k,v in data.items() if k!='events'}
        row['emitter_pids']=sorted(set(x['pid'] for x in data.get('events',[])))
    except Exception as e:row['json_read_error']=str(e)
except subprocess.TimeoutExpired as e:row.update(timed_out=True,error=str(e))
row.update(elapsed_seconds=time.monotonic()-t,output_sha256=sha(out),stderr_sha256=sha(err))
receipt.write_text(json.dumps(row,indent=2)+'\n',encoding='utf-8')
print(json.dumps(row,indent=2))
