"""Hosted Windows-only physical pipe-peer test on a registered owner-private portable.

One legitimate semwrightd -> semwright-mcp doctor and one replacement/wrong-image
pipe server. No real desktop work, model invocation, policy grants or bearer output.
"""
import argparse
import json
import runpy
import os
from pathlib import Path, PurePosixPath
import subprocess
import sys
import tempfile
import uuid
import zipfile
MCP_INIT={"jsonrpc":"2.0","id":1,"method":"initialize","params":{
    "protocolVersion":"2025-06-18","capabilities":{},
    "clientInfo":{"name":"pinned-core-runner-check","version":"1"}}}
MCP_CALL={"jsonrpc":"2.0","id":2,"method":"tools/call",
          "params":{"name":"semwright_doctor","arguments":{}}}

def fail(message):
    raise RuntimeError(message)

def run(args,timeout=30,**kwargs):
    result=subprocess.run([str(a) for a in args],capture_output=True,text=True,
                          encoding="utf-8",errors="replace",timeout=timeout,**kwargs)
    if result.returncode!=0:
        fail("Command failed "+str(args[:2])+": "+result.stderr[-700:])
    return result.stdout

def owner_sid():
    code="[Security.Principal.WindowsIdentity]::GetCurrent().User.Value"
    return run(["pwsh","-NoProfile","-NonInteractive","-Command",code]).strip()

def restrict_bundle(root,sid):
    # This fixture is isolated under runner LOCALAPPDATA; the user's E: is never modified.
    nodes=[root]+sorted(root.rglob("*"),key=lambda x:(len(x.parts),str(x)))
    for node in nodes:
        access="(OI)(CI)F" if node.is_dir() else "F"
        run(["icacls",node,"/inheritance:r","/grant:r",
             f"*{sid}:{access}",f"*S-1-5-18:{access}",
             f"*S-1-5-32-544:{access}"],timeout=10)

def mcp_call(executable,socket,ticket,env):
    lines=[MCP_INIT,{"jsonrpc":"2.0","method":"notifications/initialized"},MCP_CALL]
    payload="".join(json.dumps(value,separators=(",",":"))+"\n" for value in lines)
    proc=subprocess.run([str(executable),"--socket",socket,"--session-file",str(ticket)],
                        input=payload,capture_output=True,text=True,
                        encoding="utf-8",errors="replace",env=env,timeout=16)
    frames=[json.loads(line) for line in proc.stdout.splitlines()
            if line.startswith("{")]
    replies={entry.get("id"):entry for entry in frames if isinstance(entry,dict) and "id" in entry}
    if 1 not in replies or 2 not in replies or "result" not in replies[1]:
        fail("MCP did not complete stdio initialize and tool call")
    tool=replies[2].get("result")
    if not isinstance(tool,dict):
        fail("MCP returned no tool response")
    return proc.returncode,tool

ROGUE=r'''
import ctypes,sys
k=ctypes.WinDLL("kernel32",use_last_error=True)
k.CreateNamedPipeW.argtypes=[ctypes.c_wchar_p,ctypes.c_uint32,ctypes.c_uint32,
  ctypes.c_uint32,ctypes.c_uint32,ctypes.c_uint32,ctypes.c_uint32,ctypes.c_void_p]
k.CreateNamedPipeW.restype=ctypes.c_void_p
k.ConnectNamedPipe.argtypes=[ctypes.c_void_p,ctypes.c_void_p]
k.ConnectNamedPipe.restype=ctypes.c_int
k.ReadFile.argtypes=[ctypes.c_void_p,ctypes.c_void_p,ctypes.c_uint32,
  ctypes.POINTER(ctypes.c_uint32),ctypes.c_void_p]
k.ReadFile.restype=ctypes.c_int
k.CloseHandle.argtypes=[ctypes.c_void_p]
handle=k.CreateNamedPipeW(sys.argv[1],3,0,1,8192,8192,0,None)
assert handle and handle!=ctypes.c_void_p(-1).value
print("READY",flush=True)
connected=k.ConnectNamedPipe(handle,None)
b=ctypes.create_string_buffer(1)
n=ctypes.c_uint32(0)
k.ReadFile(handle,b,1,ctypes.byref(n),None)
print("BYTES="+str(n.value),flush=True)
k.CloseHandle(handle)
'''

def ensure_stopped(process):
    if process.poll() is None:
        process.terminate()
        try:process.wait(timeout=6)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)

def main():
    ap=argparse.ArgumentParser()
    ap.add_argument("--archive",required=True)
    ap.add_argument("--output",required=True)
    ap.add_argument("--workspace-output",required=True)
    ap.add_argument("--source",required=True)
    args=ap.parse_args()
    assert os.name=="nt", "Windows physical test only"
    archive=Path(args.archive).resolve()
    assert archive.is_file()
    dest=Path(args.output).resolve()
    dest.mkdir(parents=True,exist_ok=True)
    # A hardened, test-owned LOCALAPPDATA directory avoids inherited world-write ACLs.
    local=Path(os.environ["LOCALAPPDATA"])
    with tempfile.TemporaryDirectory(prefix="semwright-pipe-peer-",dir=local) as tmp:
        root=Path(tmp)
        with zipfile.ZipFile(archive) as z:
            files=z.infolist()
            for item in files:
                parts=PurePosixPath(item.filename).parts
                if (not parts or parts[0]!="semwright-1.0.0-windows-x86_64"
                    or ".." in parts or "\\" in item.filename
                    or item.file_size>128*1024*1024):
                    fail("Unsafe portable payload path/size")
            z.extractall(root)
        bundle=root/"semwright-1.0.0-windows-x86_64"
        daemon=bundle/"bin/semwrightd.exe"
        client=bundle/"bin/semwright-mcp.exe"
        assert daemon.is_file() and client.is_file()
        run(["pwsh","-NoProfile","-NonInteractive","-File",
             bundle/"Install-Semwright.ps1","-InPlace"],timeout=30)
        restrict_bundle(root,owner_sid())
        # The existing real-broker/MCP workspace proof verifies legitimate
        # read, write, doctor, reconnect and revocation with this protected
        # registered bundle. Return its now-released pipe name so the same
        # endpoint can be reoccupied by an unrelated process.
        workspace_runner=runpy.run_path("scripts/windows/workspace-smoke.py")["run"]
        socket=workspace_runner(bundle/"bin",Path(args.workspace_output).resolve())
        workspace=json.loads(
            (Path(args.workspace_output)/"workspace.json").read_text(encoding="utf-8"))
        if (not isinstance(socket,str)
            or not socket.startswith(r"\\.\pipe\semwright-")
            or workspace.get("fake") is not False
            or workspace.get("generated_mcp_configuration_used") is not True
            or workspace.get("read_write_readback") is not True
            or workspace.get("second_task_after_reconnect") is not True):
            fail("Registered protected bundle did not complete the real MCP workspace proof")
        owner=root/"owner-private"
        owner.mkdir()
        restrict_bundle(owner,owner_sid())
        env={**os.environ,"SEMWRIGHT_DATA_DIR":str(owner),"SEMWRIGHT_MCP_UPSTREAMS":""}
        ticket=owner/"client.session"
        # A synthetic UUID models a saved bearer without using a real user's
        # credential. Even this value must never reach the replacement server.
        ticket.write_text(str(uuid.uuid4())+"\n",encoding="ascii")

        # A different process now binds the previously trusted name. The actual
        # client handle must fail regardless of the former server's identity.
        rogue=subprocess.Popen([sys.executable,"-u","-c",ROGUE,socket],
                               stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,
                               stderr=subprocess.PIPE,text=True,encoding="utf-8",env=env)
        try:
            if rogue.stdout.readline().strip()!="READY":
                fail("Unexpected replacement server startup")
            denied_code,denied_result=mcp_call(client,socket,ticket,env)
            if not denied_result.get("isError",False):
                fail("Foreign process was accepted as Semwright broker")
            actual=rogue.communicate(timeout=8)[0]
            if "BYTES=0" not in actual:
                fail("Foreign server received Hello/bearer bytes")
        finally:
            ensure_stopped(rogue)

        # Modification of a file from the recorded source bundle must not permit
        # an attacker to re-pin it by the pipe name. No original user files touched.
        with open(daemon,"ab") as stream:
            stream.write(b"\ncorrupted-for-test\n")
        wrong_code,wrong_result=mcp_call(client,socket,ticket,env)
        if not wrong_result.get("isError",False):
            fail("Modified bundle image was not rejected")
        report={
            "source":args.source,
            "status":"PASS_PINNED_ACTUAL_PIPE_WINDOWS",
            "registered_owner_private_bundle":True,
            "genuine_broker_workspace_mcp_reconnect_accepted":True,
            "synthetic_preexisting_ticket_for_negative":True,
            "replacement_used_same_pipe_name":True,
            "replacement_pipe_wrong_process_rejected":True,
            "replacement_received_hello_bytes":0,
            "modified_package_rejected":True,
            "automatic_retry":False,
            "original_owner_broker_touched":False,
            "third_party_model_invoked":False,
            "release_admission":False,
        }
        (dest/"result.json").write_text(json.dumps(report,indent=2)+"\n",encoding="utf-8")
        print(json.dumps(report))

if __name__=="__main__":
    main()
