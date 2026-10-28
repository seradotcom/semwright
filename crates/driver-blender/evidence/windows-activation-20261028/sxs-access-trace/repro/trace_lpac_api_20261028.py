# Run only inside the installed GDB against the just-created first-party LPAC fixture.
import gdb,json,os,time,struct
from pathlib import Path
D=Path(os.environ["SEMWRIGHT_GDB_READY_DIR"])
ready=json.loads((D/"ready.json").read_text())
inferior=gdb.selected_inferior()
assert inferior.pid==ready["pid"],"Refuse any other process"
apis={int(addr,16):name for name,addr in ready["apis"].items()}
for command in ["set pagination off","set confirm off","set print thread-events off","set auto-load off"]:
    gdb.execute(command,to_string=True)
def reg(name): return int(gdb.parse_and_eval("$"+name)) & ((1<<64)-1)
def mem(addr,n):
    if not addr or n<0 or n>1024: return b""
    return bytes(inferior.read_memory(addr,n))
def u64(addr): return struct.unpack("<Q",mem(addr,8))[0]
def unicode_string(addr):
    if not addr:return None
    b=mem(addr,16); length=struct.unpack_from("<H",b)[0]; ptr=struct.unpack_from("<Q",b,8)[0]
    return mem(ptr,min(length,1024)).decode("utf-16-le",errors="replace")
def object_name(addr):
    if not addr:return None
    b=mem(addr,48)
    return {"root_handle":hex(struct.unpack_from("<Q",b,8)[0]),
            "name":unicode_string(struct.unpack_from("<Q",b,16)[0])}
def emit(row):
    print("LPAC_API_TRACE "+json.dumps(row,ensure_ascii=True),flush=True)
entries={addr:gdb.Breakpoint("*0x%x"%addr,internal=True) for addr in apis}
returns={};active=0;sequence=0;observations=[];start=time.monotonic()
with (D/"debugger-ready.json").open("x",encoding="utf-8") as f:
    json.dump({"pid":ready["pid"],"nonce":ready["nonce"],"breakpoints_ready":True},f)
emit({"phase":"attached","pid":inferior.pid,"api_count":len(apis),"host_policy_changed":False})
completed=False
try:
    for stop in range(1024):
        if time.monotonic()-start>90:raise RuntimeError("Bounded debugger time exceeded")
        gdb.execute("continue",to_string=True)
        if not inferior.is_valid() or inferior.pid==0:break
        pc=reg("rip");thread=gdb.selected_thread().global_num
        key=(pc,thread)
        if key in returns:
            call=returns[key].pop()
            if not returns[key]:del returns[key]
            call.pop("_breakpoint").delete()
            retval=reg("rax"); name=call["api"]
            call["return_u64"]=hex(retval);call["ntstatus"]=format(retval&0xffffffff,"08x")
            if name in ["NtAlpcSendWaitReceivePort"] and call.get("receive_message"):
                try:call["receive_header_hex"]=mem(call["receive_message"],128).hex()
                except Exception as e:call["receive_read_error"]=str(e)
            if name=="CreateActCtxW":
                active-=1;completed=True
            if (retval&0xffffffff)==0xc0000022 or name=="CreateActCtxW":
                call["backtrace"]=gdb.execute("bt 14",to_string=True)[:7000]
            emit(call);observations.append(call)
            if completed:break
            continue
        if pc not in apis:
            emit({"phase":"unexpected_stop","pc":hex(pc),"thread":thread,
                  "backtrace":gdb.execute("bt 6",to_string=True)[:3000]})
            raise RuntimeError("Unexpected stop; no blind resume")
        name=apis[pc]
        if name=="CreateActCtxW":active+=1
        if not active:continue
        sequence+=1
        call={"phase":"api_return","sequence":sequence,"api":name,"thread":thread,
              "entry_pc":hex(pc)}
        rcx,rdx,r8,r9,rsp=(reg(x) for x in ["rcx","rdx","r8","r9","rsp"])
        try:
            if name in ["NtOpenKey","NtOpenKeyEx","NtCreateFile","NtOpenFile","NtOpenSection","NtCreateSection"]:
                call["access_mask"]=hex(rdx);call["object"]=object_name(r8)
            elif name=="NtQueryValueKey":
                call["key_handle"]=hex(rcx);call["value_name"]=unicode_string(rdx)
            elif name in ["NtAlpcConnectPort","NtConnectPort"]:
                call["port_name"]=unicode_string(rdx)
            elif name=="NtAlpcConnectPortEx":
                call["port_object"]=object_name(rdx)
            elif name=="NtAlpcSendWaitReceivePort":
                call["port_handle"]=hex(rcx);call["send_message"]=r8
                call["send_header_hex"]=mem(r8,128).hex()
                call["receive_message"]=u64(rsp+40)
            elif name=="NtFsControlFile":
                call["file_handle"]=hex(rcx);call["fs_control_code"]=hex(u64(rsp+48)&0xffffffff)
        except Exception as e:call["argument_read_error"]=str(e)
        ret=u64(rsp)
        breakpoint=gdb.Breakpoint("*0x%x"%ret,internal=True)
        breakpoint.thread=thread
        call["_breakpoint"]=breakpoint
        returns.setdefault((ret,thread),[]).append(call)
    else:raise RuntimeError("Bounded debugger stop count exceeded")
finally:
    for bp in list(entries.values())+[c["_breakpoint"] for rows in returns.values() for c in rows]:
        try:
            if bp.is_valid():bp.delete()
        except Exception:pass
    if inferior.is_valid() and inferior.pid:
        gdb.execute("detach",to_string=True)
    outcome={"pid":ready["pid"],"completed_createactctx":completed,
        "seconds":time.monotonic()-start,"calls":len(observations),
        "access_denied":[r for r in observations if r.get("ntstatus")=="c0000022"],
        "production_acceptance":False,"A05":"BLOCKED"}
    with (D/"trace-summary.json").open("x",encoding="utf-8") as f:json.dump(outcome,f,indent=2)
    emit({"phase":"summary",**outcome})
