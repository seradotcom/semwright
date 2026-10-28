from pathlib import Path
import subprocess,os,json,datetime,time,hashlib
R=Path(r'E:\Projects\semwright-blender52');env=os.environ.copy()
env.update({'PATH':r'C:\msys64\mingw64\bin;C:\Users\sergi\.cargo\bin;'+env['PATH'],'CARGO_TARGET_DIR':str(R/'.local/target'),'CARGO_BUILD_JOBS':'2','CARGO_PROFILE_DEV_DEBUG':'0','CARGO_PROFILE_TEST_DEBUG':'0','CARGO_INCREMENTAL':'0','TMP':str(R/'.local/tmp'),'TEMP':str(R/'.local/tmp'),'SEMWRIGHT_TEST_BLENDER_RUNTIME':r'E:\Projects\worldwright-archipelago\tools\blender-5.2.1-runtime'})
toolchain='1.98.1-x86_64-pc-windows-gnu'
files=['crates/driver-blender/src/bin/session_runner/windows_fixture.rs','crates/driver-blender/src/bin/session_runner/windows_activation_readback.rs','crates/driver-blender/tests/windows_native_launch.rs','crates/driver-blender/src/bin/session_runner/windows_cpu_payload.rs','crates/driver-blender/src/bin/session_runner/windows_cpu_fixture.rs','crates/driver-blender/src/bin/session_runner/windows_cpu_images.rs','crates/driver-blender/src/bin/session_runner/windows_cpu_resource_activation.rs']
subprocess.run([r'C:\Users\sergi\.cargo\bin\rustfmt.exe','+'+toolchain,'--edition','2024','--config','skip_children=true']+files,cwd=R,env=env,check=True,timeout=30)
cmd=[r'C:\Users\sergi\.cargo\bin\cargo.exe','+'+toolchain,'test','-p','semwright-driver-blender','--features','windows-launch-test-fixture','--test','windows_native_launch','--target','x86_64-pc-windows-gnu','--locked','--offline','initial_host_cpu_resource_activation_probe','--','--ignored','--nocapture','--test-threads=1']
base=R/'.local/logs/windows-initial-cpu-resource-activation-resume'
log=base.with_suffix('.log');receipt_path=base.with_suffix('.json')
assert not log.exists() and not receipt_path.exists(),'Existing run: inspect, never replay'
start=time.monotonic();utc=datetime.datetime.now(datetime.timezone.utc).isoformat();timed_out=False;cleanup=None
with log.open('xb') as f:
    p=subprocess.Popen(cmd,cwd=R,env=env,stdout=f,stderr=subprocess.STDOUT)
    print(json.dumps({'phase':'started','pid':p.pid,'log':str(log)}),flush=True)
    try:code=p.wait(timeout=180)
    except subprocess.TimeoutExpired:
        timed_out=True
        k=subprocess.run(['taskkill','/PID',str(p.pid),'/T','/F'],capture_output=True,text=True,timeout=15)
        cleanup={'returncode':k.returncode,'stdout':k.stdout,'stderr':k.stderr}
        code=p.wait(timeout=10)
receipt={'utc_start':utc,'seconds':time.monotonic()-start,'argv':cmd,'pid':p.pid,'exit_code':code,'timed_out':timed_out,'cleanup':cleanup,'source_sha256':{name:hashlib.sha256((R/name).read_bytes()).hexdigest() for name in files+["crates/driver-blender/tests/fixtures/windows_cpu_probe_payload.json"]}}
with receipt_path.open('x',encoding='utf-8') as f:f.write(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt),flush=True)
raise SystemExit(124 if timed_out else code)
