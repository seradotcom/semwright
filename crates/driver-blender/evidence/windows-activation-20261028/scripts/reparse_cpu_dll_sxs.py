from pathlib import Path
import datetime,hashlib,json,subprocess,time
R=Path(r'E:\Projects\semwright-blender52')
O=R/'.local/logs/cpu-resource-sxs-20261028T185809Z-0c04a049'
src=O/'cpu-resource-activation.etl';out=O/'cpu-dll-resource-sxs.txt'
receipt=O/'cpu-dll-reparse-receipt.json'
assert not out.exists() and not receipt.exists()
sha=lambda p:hashlib.sha256(p.read_bytes()).hexdigest()
before=sha(src);assert before=='2db670e8a1ba8449bc7bf43841a76de7c7615edcfb08674b3214f542623ff6ba'
args=[r'C:\WINDOWS\System32\sxstrace.exe','Parse','-logfile:'+str(src),'-outfile:'+str(out),'-filter:blender_cpu_check.dll']
row={'utc_start':datetime.datetime.now(datetime.timezone.utc).isoformat(),'argv':args,'etl_sha256_before':before,'new_trace':False,'cargo_repetitions':0,'A05':'BLOCKED'}
t=time.monotonic()
try:
    r=subprocess.run(args,cwd=R,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=15)
    row.update(exit_code=r.returncode,stdout=r.stdout.decode('utf-8',errors='replace'),stderr=r.stderr.decode('utf-8',errors='replace'))
except subprocess.TimeoutExpired as e:row.update(timed_out=True,error=str(e))
row.update(elapsed_seconds=time.monotonic()-t,etl_sha256_after=sha(src))
if out.exists():row.update(output_path=str(out),output_bytes=out.stat().st_size,output_sha256=sha(out))
receipt.write_text(json.dumps(row,indent=2),encoding='utf-8')
print(json.dumps(row,indent=2))
