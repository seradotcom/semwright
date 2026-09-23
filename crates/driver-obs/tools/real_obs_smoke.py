#!/usr/bin/env python3
"""Optional read-only real OBS smoke. Fails closed without isolation prerequisites.
No external stream, recording, source capture, existing profile or user session is used.
This is NOT a Semwright Driver Host conformance run.
"""
from __future__ import annotations
import argparse,json,os,pathlib,selectors,shutil,signal,socket,subprocess,sys,tempfile,time
ROOT=pathlib.Path(__file__).resolve().parents[1]
TEST_PASSWORD='semwright-fixture-only-password'

class Missing(RuntimeError):pass

def report(status,**fields):print(json.dumps({'status':status,'scope':'real OBS transport read-only smoke, NOT Semwright host conformance',**fields}),flush=True)

SAFE_RUNTIME_DETAILS=frozenset({
    'child output exceeded evidence budget','real hardware unexpectedly visible','non-loopback interface visible',
    'virtual display timeout','virtual display exited','invalid virtual display','OBS exited before readiness','invalid probe evidence',
    'unexpected scene graph; refuse existing or contaminated profile','OBS WebSocket did not become ready',
    'network namespace isolation failed',
})
def safe_runtime_detail(error):
    message=str(error)
    return message if message in SAFE_RUNTIME_DETAILS else 'runtime_failure'
def stderr_category(data:bytes):
    text=data.decode('utf-8','replace').lower()
    if 'unshare' in text and 'operation not permitted' in text:return 'user_namespace_denied'
    if 'bwrap' in text:return 'bubblewrap_launch_failed'
    if 'permission denied' in text:return 'permission_denied'
    if 'no such file or directory' in text:return 'child_path_missing'
    return 'child_stderr_present' if data else 'no_child_stderr'

def safe_log_tail(path:pathlib.Path,limit:int=4096):
    try:data=path.read_bytes()[-limit:]
    except OSError:return ''
    text=data.decode('utf-8','replace').replace(TEST_PASSWORD,'<redacted-fixture-password>')
    return ''.join(ch if ch in (chr(10),chr(13),chr(9)) or ord(ch)>=32 else '?' for ch in text)

def loopback_port_open(port:int):
    try:
        with socket.create_connection(('127.0.0.1',port),timeout=.25):
            return True
    except OSError:
        return False

def safe_log_signals(path:pathlib.Path,limit:int=12288):
    try:text=path.read_bytes().decode('utf-8','replace')
    except OSError:return ''
    text=text.replace(TEST_PASSWORD,'<redacted-fixture-password>')
    needles=('error:', 'warning:', 'Startup complete', 'Loaded scenes', 'Switched to scene',
             'Failed to', 'obs_module_', 'Config::Load', 'FrontendFinishedLoading',
             'encoder', 'service')
    lines=[line for line in text.splitlines() if any(n.lower() in line.lower() for n in needles)]
    data='\n'.join(lines[-120:])
    data=''.join(ch if ch in (chr(10),chr(13),chr(9)) or ord(ch)>=32 else '?' for ch in data)
    return data[-limit:]

def config_tree(root:pathlib.Path,port:int):
    # Keep HOME and XDG config roots unified. OBS core follows XDG_CONFIG_HOME,
    # while some bundled plugins resolve their config through the home-derived
    # default. A single private tree avoids split-brain fixture state.
    config=root/'home'/'.config'/'obs-studio'
    profile=config/'basic/profiles/SemwrightFixture';profile.mkdir(parents=True)
    # Let OBS create its own scene collection using the current on-disk schema
    # instead of hand-authoring an internal scene JSON that can drift by version.
    (config/'basic/scenes').mkdir(parents=True)
    plugin=config/'plugin_config/obs-websocket';plugin.mkdir(parents=True)
    (root/'runtime').mkdir(mode=0o700)
    # OBS 30's bundled obs-websocket starts disabled unless ServerEnabled is
    # loaded. Seed its officially supported legacy migration section too;
    # the plugin consumes/removes these keys and persists the modern config.
    (config/'global.ini').write_text(
        '[General]\nFirstRun=false\n\n'
        '[Basic]\nProfile=SemwrightFixture\nProfileDir=SemwrightFixture\n'
        'SceneCollection=SemwrightFixture\nSceneCollectionFile=SemwrightFixture\n\n'
        '[OBSWebSocket]\nFirstLoad=false\nServerEnabled=true\n'
        'AlertsEnabled=false\nAuthRequired=true\n'
    )
    (profile/'basic.ini').write_text('[General]\nName=SemwrightFixture\n[Video]\nBaseCX=320\nBaseCY=180\nOutputCX=320\nOutputCY=180\nFPSType=0\nFPSCommon=10\n[Audio]\nSampleRate=48000\nChannelSetup=Stereo\n[Output]\nMode=Simple\n')
    # No scene file is supplied. The disposable HOME/XDG tree guarantees there
    # is no user content; OBS initializes a valid empty/default collection itself.
    # The password is a fixed non-secret test fixture. The endpoint still exists only
    # inside an isolated network namespace and no host interface is reachable.
    (plugin/'config.json').write_text(json.dumps({'first_load':False,'server_enabled':True,'server_port':port,'alerts_enabled':False,'auth_required':True,'server_password':TEST_PASSWORD}))
    return config

def bounded_process(command,timeout,env=None):
    proc=subprocess.Popen(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True,env=env)
    end=time.monotonic()+timeout
    selector=selectors.DefaultSelector()
    output=[bytearray(),bytearray()]
    selector.register(proc.stdout,selectors.EVENT_READ,0)
    selector.register(proc.stderr,selectors.EVENT_READ,1)
    try:
        while selector.get_map():
            remaining=end-time.monotonic()
            if remaining<=0:raise subprocess.TimeoutExpired(command,timeout)
            for key,_ in selector.select(min(.1,remaining)):
                chunk=os.read(key.fileobj.fileno(),65536)
                if not chunk:selector.unregister(key.fileobj);continue
                if len(output[key.data])+len(chunk)>1048576:raise RuntimeError('child output exceeded evidence budget')
                output[key.data].extend(chunk)
        code=proc.wait(timeout=max(.001,end-time.monotonic()))
        return code,bytes(output[0]),bytes(output[1])
    except BaseException:
        try:os.killpg(proc.pid,signal.SIGTERM)
        except ProcessLookupError:pass
        try:proc.wait(timeout=3)
        except subprocess.TimeoutExpired:
            try:os.killpg(proc.pid,signal.SIGKILL)
            except ProcessLookupError:pass
            proc.wait()
        raise
    finally:
        selector.close()
        proc.stdout.close();proc.stderr.close()

def sandbox(probe:pathlib.Path):
    # bwrap created /dev from scratch and hid host home, /run, X11 and device nodes.
    if pathlib.Path('/dev/video0').exists() or pathlib.Path('/dev/snd').exists():raise RuntimeError('real hardware unexpectedly visible')
    if any(name!='lo' for _,name in socket.if_nameindex()):raise RuntimeError('non-loopback interface visible')
    with tempfile.TemporaryDirectory(prefix='obs-fixture-') as directory:
        root=pathlib.Path(directory);home=root/'home';home.mkdir()
        with socket.socket() as s:s.bind(('127.0.0.1',0));port=s.getsockname()[1]
        config_tree(root,port)
        env={'PATH':'/usr/bin:/bin','HOME':str(home),'XDG_CONFIG_HOME':str(home/'.config'),'XDG_DATA_HOME':str(root/'data'),
             'XDG_CACHE_HOME':str(root/'cache'),'XDG_RUNTIME_DIR':str(root/'runtime'),'QT_QPA_PLATFORM':'xcb','LIBGL_ALWAYS_SOFTWARE':'1',
             'PULSE_SERVER':'unix:/nonexistent','PIPEWIRE_REMOTE':'nonexistent','DBUS_SESSION_BUS_ADDRESS':'unix:path=/nonexistent','LANG':'C.UTF-8'}
        children=[]
        try:
            with (root/'xvfb.log').open('wb') as xlog,(root/'obs.log').open('wb') as olog:
                # /tmp is a private tmpfs inside bubblewrap, so a fixed display cannot
                # collide with the host or another test.  Creating the socket directory
                # explicitly avoids Xvfb startup differences across distro images.
                x11_dir=pathlib.Path('/tmp/.X11-unix');x11_dir.mkdir(mode=0o1777,exist_ok=True);os.chmod(x11_dir,0o1777)
                display=':99';x11_socket=x11_dir/'X99'
                xvfb=subprocess.Popen(['/usr/bin/Xvfb',display,'-screen','0','640x360x24','-nolisten','tcp'],stdout=xlog,stderr=xlog,env=env)
                children.append(xvfb)
                display_deadline=time.monotonic()+5
                while time.monotonic()<display_deadline:
                    if xvfb.poll() is not None:raise RuntimeError('virtual display exited')
                    if x11_socket.exists():break
                    time.sleep(.05)
                else:raise RuntimeError('virtual display timeout')
                env['DISPLAY']=display
                obs=subprocess.Popen([
                    '/usr/bin/obs','--multi','--only-bundled-plugins','--disable-missing-files-check',
                    '--profile','SemwrightFixture','--collection','SemwrightFixture',
                    f'--websocket_port={port}',f'--websocket_password={TEST_PASSWORD}','--websocket_ipv4_only','--websocket_debug'
                ],stdout=olog,stderr=olog,env=env)
                children.append(obs)
                deadline=time.monotonic()+35
                while time.monotonic()<deadline:
                    if obs.poll() is not None:raise RuntimeError('OBS exited before readiness')
                    code,out,_=bounded_process([str(probe),str(port),TEST_PASSWORD],8,env)
                    if code==0:
                        data=json.loads(out)
                        if not isinstance(data,dict) or 'version' not in data or 'scenes' not in data:raise RuntimeError('invalid probe evidence')
                        names=[s.get('sceneName') for s in data['scenes'].get('scenes',[])]
                        if not names or len(names)>8:raise RuntimeError('unexpected scene graph; refuse existing or contaminated profile')
                        report('PASS_READ_ONLY',obs_version=data['version'].get('obsVersion'),websocket_version=data['version'].get('obsWebSocketVersion'),scene_names=names,recording_started=False,streaming_started=False)
                        return
                    time.sleep(.2)
                olog.flush();xlog.flush()
                report('OBS_DIAGNOSTIC',
                       websocket_port_open=loopback_port_open(port),
                       obs_log_signals=safe_log_signals(root/'obs.log'),
                       obs_log_tail=safe_log_tail(root/'obs.log'),
                       xvfb_log_tail=safe_log_tail(root/'xvfb.log'))
                raise RuntimeError('OBS WebSocket did not become ready')
        finally:
            for proc in reversed(children):
                if proc.poll() is None:proc.terminate()
                try:proc.wait(timeout=3)
                except subprocess.TimeoutExpired:proc.kill();proc.wait()

def namespace(probe:pathlib.Path,parent_netns:str):
    if os.readlink('/proc/self/ns/net')==parent_netns:raise RuntimeError('network namespace isolation failed')
    subprocess.run(['ip','link','set','lo','up'],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    # Only system runtime, pack source, and the explicitly supplied probe are visible.
    command=['bwrap','--unshare-all','--share-net','--die-with-parent','--new-session','--clearenv','--ro-bind','/usr','/usr']
    for path in ['/bin','/lib','/lib64']:
        if pathlib.Path(path).exists():command+=['--ro-bind',path,path]
    command+=['--proc','/proc','--dev','/dev','--tmpfs','/tmp','--dir','/run','--dir','/etc','--ro-bind',str(ROOT),'/pack','--ro-bind',str(probe),'/probe','--setenv','PATH','/usr/bin:/bin','--chdir','/tmp','/usr/bin/python3','/pack/tools/real_obs_smoke.py','--sandbox','/probe']
    code,out,err=bounded_process(command,55)
    if out:sys.stdout.buffer.write(out);sys.stdout.flush()
    if code!=0:
        report('FAILED_ISOLATED_LAUNCH',exit_code=code,diagnostic_category=stderr_category(err),diagnostic_bytes=len(err));raise SystemExit(code or 1)

def main():
    p=argparse.ArgumentParser();p.add_argument('--probe',type=pathlib.Path,default=ROOT/'driver/target/release/obs-probe');p.add_argument('--namespace',type=pathlib.Path);p.add_argument('--parent-netns');p.add_argument('--sandbox',type=pathlib.Path);a=p.parse_args()
    if a.sandbox:return sandbox(a.sandbox)
    if a.namespace:return namespace(a.namespace,a.parent_netns)
    missing=[x for x in ['obs','Xvfb','unshare','bwrap','ip'] if shutil.which(x) is None]
    probe=a.probe.resolve()
    if not probe.is_file() or not os.access(probe,os.X_OK):missing.append('compiled obs-probe')
    if missing:report('SKIPPED_PREREQUISITES',missing=missing);return 77
    parent=os.readlink('/proc/self/ns/net')
    command=['unshare','--user','--map-root-user','--net','--pid','--fork','--kill-child=KILL','--',sys.executable,str(pathlib.Path(__file__).resolve()),'--namespace',str(probe),'--parent-netns',parent]
    code,out,err=bounded_process(command,65)
    if out:sys.stdout.buffer.write(out);sys.stdout.flush()
    if code!=0:report('FAILED_ISOLATION_OR_SMOKE',exit_code=code,diagnostic_category=stderr_category(err),diagnostic_bytes=len(err))
    return code
if __name__=='__main__':
    try:raise SystemExit(main() or 0)
    except RuntimeError as error:
        report('FAIL',error_category='RuntimeError',detail=safe_runtime_detail(error));raise SystemExit(1)
    except (OSError,ValueError,subprocess.SubprocessError) as error:
        report('FAIL',error_category=type(error).__name__);raise SystemExit(1)
