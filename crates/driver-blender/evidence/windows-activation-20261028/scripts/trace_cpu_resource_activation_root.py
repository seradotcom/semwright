"""Bounded observation of the existing CPU activation probe; no elevation."""
from pathlib import Path
import ctypes as C,datetime as D,hashlib,json,os,subprocess,time,uuid
R=Path(r'E:\Projects\semwright-blender52');SYS=Path(os.environ['SystemRoot'])/'System32'
PREVIOUS=R/'.local/logs/windows-initial-cpu-resource-activation-resume.json'
START=time.monotonic();WORK_END=START+90.;END=START+120.
O=R/'.local/logs'/('cpu-resource-sxs-'+D.datetime.now(D.timezone.utc).strftime('%Y%m%dT%H%M%SZ-')+uuid.uuid4().hex[:8])
O.mkdir(exist_ok=False);ETL=O/'cpu-resource-activation.etl';PARSED=O/'fixture-cpu-sxs.txt'
SXS=str(SYS/'sxstrace.exe');PS=str(SYS/'WindowsPowerShell/v1.0/powershell.exe')
FILES=['crates/driver-blender/src/bin/session_runner/'+name for name in [
 'windows_fixture.rs','windows_activation_readback.rs','windows_cpu_payload.rs',
 'windows_cpu_fixture.rs','windows_cpu_images.rs','windows_cpu_resource_activation.rs']]
FILES+=['crates/driver-blender/tests/windows_native_launch.rs','crates/driver-blender/tests/fixtures/windows_cpu_probe_payload.json']
CMD=[r'C:\Users\owner\.cargo\bin\cargo.exe','+1.98.1-x86_64-pc-windows-gnu',
 'test','-p','semwright-driver-blender','--features','windows-launch-test-fixture',
 '--test','windows_native_launch','--target','x86_64-pc-windows-gnu','--locked','--offline',
 'initial_host_cpu_resource_activation_probe','--','--ignored','--nocapture','--test-threads=1']
env=os.environ.copy()
env.update({'PATH':r'C:\msys64\mingw64\bin;C:\Users\owner\.cargo\bin;'+env['PATH'],
 'CARGO_TARGET_DIR':str(R/'.local/target'),'CARGO_BUILD_JOBS':'2',
 'CARGO_PROFILE_DEV_DEBUG':'0','CARGO_PROFILE_TEST_DEBUG':'0','CARGO_INCREMENTAL':'0',
 'TMP':str(R/'.local/tmp'),'TEMP':str(R/'.local/tmp'),
 'SEMWRIGHT_TEST_BLENDER_RUNTIME':r'E:\Projects\worldwright-archipelago\tools\blender-5.2.1-runtime'})
receipt={'utc_start':D.datetime.now(D.timezone.utc).isoformat(),'status':'PREPARING',
 'overall_budget_seconds':120,'work_budget_seconds':90,'elevation_requested':False,
 'permission_changes':False,'policy_changes':False,'cold_cache_claim':False,'broker_used':False,
 'A05':'BLOCKED','cargo_repetitions':0,'commands':[],'api_calls':[],'output_directory':str(O)}
def sha(path):
    h=hashlib.sha256()
    with Path(path).open('rb') as f:
        for block in iter(lambda:f.read(1024*1024),b''):h.update(block)
    return h.hexdigest()
def sources():return {name:sha(R/name) for name in FILES}
def save():
    p=O/'receipt.pending';p.write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
    os.replace(p,O/'receipt.json')
class StopCapture(Exception):pass
def reap_owned(p,tag):
    row={'root_pid':p.pid,'already_exited':p.poll() is not None}
    try:
        if p.poll() is None:
            args=[str(SYS/'taskkill.exe'),'/PID',str(p.pid),'/T','/F'];row['taskkill_argv']=args
            with (O/(tag+'.taskkill.stdout.bin')).open('xb') as out,(O/(tag+'.taskkill.stderr.bin')).open('xb') as err:
                killer=subprocess.Popen(args,stdout=out,stderr=err,cwd=R,env=env)
                try:row['taskkill_code']=killer.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    killer.kill();row['taskkill_code']=killer.wait(timeout=2);row['taskkill_timed_out']=True
    except Exception as error:row['tree_cleanup_error']=str(error)
    finally:
        if p.poll() is None:
            try:p.wait(timeout=2)
            except subprocess.TimeoutExpired:
                p.kill();p.wait(timeout=2);row['root_kill_fallback']=True
    row['reaped_exit_code']=p.returncode;return row
def command(args,tag,cap=10.,cleanup=False,command_env=None):
    deadline=END-10. if cleanup else WORK_END
    budget=min(cap,deadline-time.monotonic())
    if budget<=0:raise StopCapture('BUDGET_EXHAUSTED_BEFORE_'+tag)
    op,ep=O/(tag+'.stdout.bin'),O/(tag+'.stderr.bin')
    row={'tag':tag,'argv':args,'utc_start':D.datetime.now(D.timezone.utc).isoformat(),
      'timeout_seconds':budget,'timed_out':False,'stdout':str(op),'stderr':str(ep)}
    receipt['commands'].append(row)
    with op.open('xb') as out,ep.open('xb') as err:
        p=subprocess.Popen(args,cwd=R,env=command_env or env,stdout=out,stderr=err)
        row['pid']=p.pid
        try:
            save()
            print(json.dumps({'phase':tag,'pid':p.pid,'receipt':str(O/'receipt.json')}),flush=True)
            row['exit_code']=p.wait(timeout=budget)
        except subprocess.TimeoutExpired:
            row['timed_out']=True;row['cleanup']=reap_owned(p,tag);row['exit_code']=p.returncode
        except BaseException:
            row['cleanup']=reap_owned(p,tag);row['exit_code']=p.returncode;raise
    save();return row
U32,U64=C.c_uint32,C.c_uint64
class GUID(C.Structure):
    _fields_=[('Data1',U32),('Data2',C.c_uint16),('Data3',C.c_uint16),('Data4',C.c_ubyte*8)]
class WNODE(C.Structure):
    _fields_=[('BufferSize',U32),('ProviderId',U32),('HistoricalContext',U64),
      ('TimeStamp',U64),('Guid',GUID),('ClientContext',U32),('Flags',U32)]
class ETP(C.Structure):
    _fields_=[('Wnode',WNODE),('BufferSize',U32),('MinimumBuffers',U32),
      ('MaximumBuffers',U32),('MaximumFileSize',U32),('LogFileMode',U32),
      ('FlushTimer',U32),('EnableFlags',U32),('AgeLimit',C.c_int32),
      ('NumberOfBuffers',U32),('FreeBuffers',U32),('EventsLost',U32),
      ('BuffersWritten',U32),('LogBuffersLost',U32),('RealTimeBuffersLost',U32),
      ('LoggerThreadId',C.c_void_p),('LogFileNameOffset',U32),('LoggerNameOffset',U32)]
ADV=C.WinDLL('advapi32',use_last_error=True)
ADV.QueryAllTracesW.argtypes=[C.POINTER(C.POINTER(ETP)),U32,C.POINTER(U32)]
ADV.QueryAllTracesW.restype=U32
ADV.ControlTraceW.argtypes=[U64,C.c_wchar_p,C.POINTER(ETP),U32]
ADV.ControlTraceW.restype=U32
BASE,CHARS=C.sizeof(ETP),1024;SIZE=BASE+4*CHARS
def etw_buffer(guid=None):
    if C.sizeof(C.c_void_p)!=8 or C.sizeof(C.c_wchar)!=2 or C.sizeof(WNODE)!=48 or BASE!=120:
        raise StopCapture('UNEXPECTED_ETW_ABI_NO_TRACE')
    b=C.create_string_buffer(SIZE);p=ETP.from_buffer(b)
    p.Wnode.BufferSize=SIZE;p.LoggerNameOffset=BASE;p.LogFileNameOffset=BASE+2*CHARS
    if guid:p.Wnode.Guid=GUID.from_buffer_copy(bytes.fromhex(guid))
    return b
def etw_string(b,offset):
    if offset==0:return ''
    if offset<BASE or offset>=SIZE or offset%2:raise StopCapture('INVALID_ETW_STRING_BOUND')
    data=bytes(b[offset:min(offset+2*CHARS,SIZE)])
    for i in range(0,len(data)-1,2):
        if data[i:i+2]==b'\0\0':return data[:i].decode('utf-16-le',errors='strict')
    raise StopCapture('UNTERMINATED_ETW_STRING')
def etw_row(b):
    p=ETP.from_buffer(b)
    return {'handle':int(p.Wnode.HistoricalContext),'guid':C.string_at(C.byref(p.Wnode.Guid),16).hex(),
      'name':etw_string(b,int(p.LoggerNameOffset)),'logfile':etw_string(b,int(p.LogFileNameOffset)),
      'events_lost':int(p.EventsLost),'log_buffers_lost':int(p.LogBuffersLost)}
def norm_path(value):
    value=str(value).replace('/','\\')
    for prefix in ('\\\\?\\','\\??\\'):
        if value.startswith(prefix):value=value[len(prefix):]
    return os.path.normpath(value).casefold()
def session_key(row):return row['handle'],row['guid'],row['name'].casefold()
def same_session(a,b):return session_key(a)==session_key(b) and norm_path(a['logfile'])==norm_path(b['logfile'])
def sessions(tag):
    for capacity in (64,256):
        buffers=[etw_buffer() for _ in range(capacity)]
        pointers=(C.POINTER(ETP)*capacity)(*[C.cast(b,C.POINTER(ETP)) for b in buffers]);count=U32()
        code=int(ADV.QueryAllTracesW(pointers,capacity,C.byref(count)))
        receipt['api_calls'].append({'api':'QueryAllTracesW','tag':tag,'code':code,'capacity':capacity,'reported_count':count.value})
        if code==234 and capacity==64:continue
        if code!=0 or count.value>capacity:raise StopCapture('ETW_QUERY_UNAVAILABLE_'+str(code))
        rows=[etw_row(buffers[i]) for i in range(count.value)]
        keep=[r for r in rows if 'sxs' in r['name'].casefold() or norm_path(r['logfile'])==norm_path(ETL)]
        (O/(tag+'.json')).write_text(json.dumps(keep,indent=2)+'\n',encoding='utf-8')
        return rows
    raise StopCapture('ETW_SESSION_COUNT_EXCEEDED_BOUND')
def discover_owned(before,after):
    prior={session_key(row) for row in before}
    matches=[r for r in after if r['handle'] and norm_path(r['logfile'])==norm_path(ETL) and session_key(r) not in prior]
    return matches[0] if len(matches)==1 else None
def query_owned(owner,tag):
    b=etw_buffer(owner['guid']);code=int(ADV.ControlTraceW(owner['handle'],None,C.cast(b,C.POINTER(ETP)),0))
    row=etw_row(b) if code==0 else None
    observed=row if row is not None and same_session(owner,row) else {'owner_identity_matches':False}
    receipt['api_calls'].append({'api':'ControlTraceW','control':'QUERY','tag':tag,'handle':owner['handle'],'code':code,'observed':observed})
    return code,row
def stop_owned(owner):
    code,current=query_owned(owner,'before-owned-stop')
    if code==4201:return {'called':False,'status':'ALREADY_GONE','owned_remaining':False}
    if code!=0 or not same_session(owner,current):
        return {'called':False,'status':'IDENTITY_UNCONFIRMED_NO_STOP','query_code':code,'owned_remaining':None}
    b=etw_buffer(owner['guid']);stopped=int(ADV.ControlTraceW(owner['handle'],None,C.cast(b,C.POINTER(ETP)),1))
    receipt['api_calls'].append({'api':'ControlTraceW','control':'STOP','handle':owner['handle'],'guid':owner['guid'],'logfile':owner['logfile'],'code':stopped})
    check,remaining=query_owned(owner,'after-owned-stop')
    gone=check==4201 or (check==0 and not same_session(owner,remaining))
    return {'called':True,'status':'STOP_RETURNED','code':stopped,'verification_code':check,'owned_remaining':False if gone else (True if check==0 else None)}
EVENT_PS=r'''
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [Console]::OutputEncoding
$start = [DateTimeOffset]::Parse($env:SEMWRIGHT_DIAG_START_UTC)
$end = [DateTimeOffset]::Parse($env:SEMWRIGHT_DIAG_END_UTC)
$result = [ordered]@{ok=$false;status='NOT_READ';start_utc=$start.ToString('o');end_utc=$end.ToString('o');events=@();truncated=$false}
try {
    $all = @(Get-WinEvent -FilterHashtable @{LogName='Application';ProviderName='SideBySide';StartTime=$start.UtcDateTime;EndTime=$end.UtcDateTime} -MaxEvents 129 -ErrorAction Stop)
    $rows = @()
    foreach ($e in ($all | Select-Object -First 128)) {
        $message = ''
        try {$message = [string]$e.Message} catch {$message = '[Message formatting unavailable]'}
        if ($message.Length -gt 4096) {$message = $message.Substring(0,4096)}
        $xml = $e.ToXml();$truncated = $xml.Length -gt 8192
        if ($truncated) {$xml = $xml.Substring(0,8192)}
        $rows += [ordered]@{utc=$e.TimeCreated.ToUniversalTime().ToString('o');id=$e.Id;record_id=$e.RecordId;provider=$e.ProviderName;pid=$e.ProcessId;tid=$e.ThreadId;message=$message;xml=$xml;xml_truncated=$truncated}
    }
    $result.ok=$true;$result.status='READ';$result.events=$rows;$result.truncated=($all.Count -gt 128)
} catch {
    if ($_.FullyQualifiedErrorId -like 'NoMatchingEventsFound*') {$result.ok=$true;$result.status='NO_MATCHING_EVENTS'}
    else {$result.status='READ_ERROR';$result.error_id=$_.FullyQualifiedErrorId;$result.error_message=$_.Exception.Message;$result.hresult=$_.Exception.HResult}
}
$result | ConvertTo-Json -Depth 8 -Compress
'''
before=owner=parse_result=cargo_result=None;trace_attempted=False
try:
    previous=json.loads(PREVIOUS.read_text(encoding='utf-8'))
    receipt['previous_receipt']=str(PREVIOUS);receipt['previous_receipt_sha256']=sha(PREVIOUS)
    if previous['utc_start']!='2026-10-28T18:22:18.485171+00:00':raise StopCapture('PREVIOUS_INTERVAL_CHANGED_NO_REPLAY')
    begin=D.datetime.fromisoformat(previous['utc_start']);finish=begin+D.timedelta(seconds=24)
    event_env=env.copy();event_env.update({'SEMWRIGHT_DIAG_START_UTC':begin.isoformat(),'SEMWRIGHT_DIAG_END_UTC':finish.isoformat()})
    receipt['event_window']={'start_utc':begin.isoformat(),'end_utc':finish.isoformat()}
    receipt['event_query_source_sha256']=hashlib.sha256(EVENT_PS.encode()).hexdigest()
    result=command([PS,'-NoLogo','-NoProfile','-NonInteractive','-Command',EVENT_PS],'events-sidebyside',cap=10,command_env=event_env)
    if result['exit_code']==0 and not result['timed_out']:
        try:receipt['event_query']=json.loads(Path(result['stdout']).read_text(encoding='utf-8-sig'))
        except Exception as error:receipt['event_query']={'ok':False,'status':'UNREADABLE_JSON','error':str(error)}
    else:receipt['event_query']={'ok':False,'status':'COMMAND_FAILED_OR_TIMED_OUT'}
    receipt['source_sha256_before']=sources();receipt['runner_sha256']=sha(Path(__file__));receipt['cargo_argv']=CMD
    receipt['source_matches_previous']=all(receipt['source_sha256_before'][n]==previous['source_sha256'].get(n) for n in FILES)
    if previous['argv']!=CMD or not receipt['source_matches_previous']:raise StopCapture('SOURCE_OR_ARGV_CHANGED_NO_TEST_REPLAY')
    receipt['build_inputs_sha256']={n:sha(R/n) for n in ('Cargo.toml','Cargo.lock')}
    before=sessions('sessions-before')
    if any('sxs' in r['name'].casefold() for r in before):raise StopCapture('PREEXISTING_SXS_SESSION_NO_TRACE')
    trace_attempted=True
    started=command([SXS,'Trace','-logfile:'+str(ETL),'-nostop'],'trace-start',cap=5)
    owner=discover_owned(before,sessions('sessions-after-start'));receipt['owned_trace']=owner
    if started['exit_code']!=0 or started['timed_out']:raise StopCapture('TRACE_START_UNAVAILABLE_WITH_CURRENT_RIGHTS_OR_TIMED_OUT')
    if owner is None:raise StopCapture('TRACE_OWNERSHIP_UNPROVEN_NO_TEST')
    cargo_result=command(CMD,'cargo-test',cap=120);receipt['test_exit_code']=cargo_result['exit_code']
    receipt['status']='TEST_OBSERVED_AWAITING_TRACE_CLEANUP'
except StopCapture as error:receipt['status']=str(error)
except BaseException as error:
    receipt['status']='RUNNER_ERROR';receipt['runner_error']=type(error).__name__+': '+str(error)
finally:
    receipt['cargo_repetitions']=sum(r['tag']=='cargo-test' and 'pid' in r for r in receipt['commands'])
    if trace_attempted and before is not None and owner is None:
        try:owner=discover_owned(before,sessions('sessions-owned-recovery'));receipt['owned_trace']=owner
        except Exception as error:receipt['ownership_recovery_error']=str(error)
    if owner is not None:
        try:receipt['trace_cleanup']=stop_owned(owner)
        except Exception as error:receipt['trace_cleanup']={'called':False,'status':'CLEANUP_ERROR_NO_GLOBAL_STOP','error':str(error)}
    elif trace_attempted:receipt['trace_cleanup']={'called':False,'status':'NO_PROVEN_OWNED_SESSION_NO_STOP'}
    cleanup=receipt.get('trace_cleanup',{})
    if owner is not None and cleanup.get('owned_remaining') is False and ETL.is_file():
        try:
            parse_result=command([SXS,'Parse','-logfile:'+str(ETL),'-outfile:'+str(PARSED),'-filter:fixture-embedded-manifest.exe'],'parse-fixture-cpu',cap=10,cleanup=True)
            receipt['parse_exit_code']=parse_result['exit_code']
        except Exception as error:receipt['parse_error']=str(error)
    if cargo_result is not None:
        complete=not cargo_result['timed_out'] and cleanup.get('owned_remaining') is False and parse_result is not None and parse_result['exit_code']==0 and not parse_result['timed_out']
        receipt['status']='TRACE_CAPTURED_DIAGNOSTIC_ONLY' if complete else 'TRACE_CAPTURE_INCOMPLETE'
        receipt['test_exit_code']=cargo_result['exit_code']
    try:
        receipt['source_sha256_after']=sources()
        receipt['source_unchanged_during_capture']=receipt['source_sha256_after']==receipt.get('source_sha256_before')
        if receipt.get('source_sha256_before') and not receipt['source_unchanged_during_capture']:receipt['status']='SOURCE_CHANGED_DURING_CAPTURE_NOT_COMPARABLE'
    except Exception as error:receipt['source_after_error']=str(error)
    artifacts={}
    for path in O.iterdir():
        if not path.is_file() or path.name in ('receipt.json','receipt.pending'):continue
        info={'bytes':path.stat().st_size};stable=path!=ETL or cleanup.get('owned_remaining') is False
        if stable and info['bytes']<=64*1024*1024 and time.monotonic()<END-1:info['sha256']=sha(path)
        else:info.update(sha256=None,hash_not_run_reason='ACTIVE_OR_UNCONFIRMED_TRACE' if not stable else 'SIZE_OR_TIME_BOUND')
        artifacts[path.name]=info
    receipt['artifacts']=artifacts;receipt['elapsed_seconds']=time.monotonic()-START
    receipt['overall_budget_exceeded']=receipt['elapsed_seconds']>120;save()
    print(json.dumps({'status':receipt['status'],'test_exit_code':receipt.get('test_exit_code'),
      'cargo_repetitions':receipt['cargo_repetitions'],'cleanup':receipt.get('trace_cleanup'),
      'receipt':str(O/'receipt.json'),'parsed':str(PARSED)}),flush=True)
raise SystemExit(0 if receipt['status']=='TRACE_CAPTURED_DIAGNOSTIC_ONLY' else 2)
