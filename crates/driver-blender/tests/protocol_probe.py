import argparse
import hashlib
import json
import os
import select
import struct
import subprocess
import tempfile
import time
from pathlib import Path


def write_frame(stream, value):
    body=json.dumps(value,separators=(",",":")).encode()
    if not 0 < len(body) <= 1_048_576:
        raise RuntimeError(f"request frame out of bounds: {len(body)}")
    stream.write(struct.pack(">I",len(body))+body)
    stream.flush()


def read_exact(stream,size,timeout=20.0):
    out=bytearray()
    deadline=time.monotonic()+timeout
    fd=stream.fileno()
    while len(out)<size:
        remaining=deadline-time.monotonic()
        if remaining<=0:
            raise TimeoutError("protocol read timed out")
        ready,_,_=select.select([fd],[],[],remaining)
        if not ready:
            raise TimeoutError("protocol read timed out")
        chunk=os.read(fd,size-len(out))
        if not chunk:
            raise EOFError("protocol stream closed")
        out.extend(chunk)
    return bytes(out)


def read_frame(stream,timeout=20.0):
    size=struct.unpack(">I",read_exact(stream,4,timeout))[0]
    if not 0 < size <= 1_048_576:
        raise RuntimeError(f"response frame out of bounds: {size}")
    body=read_exact(stream,size,timeout)
    return size,json.loads(body)


def descriptor_digest(descriptor):
    body=json.dumps(descriptor,separators=(",",":"),ensure_ascii=False).encode()
    return hashlib.sha256(body).hexdigest()


def execute(proc, catalog_by_name, command, args, request_id):
    capability=catalog_by_name[command]
    request={
        "type":"execute",
        "id":request_id,
        "command":command,
        "descriptor_sha256":descriptor_digest(capability["descriptor"]),
        "args":args,
    }
    write_frame(proc.stdin,request)
    size,response=read_frame(proc.stdout,30.0)
    print("execute",command,size,response.get("type"),response.get("error",{}).get("code",""),flush=True)
    if response.get("type")!="result":
        raise RuntimeError(f"{command} failed: {response}")
    return response["value"]


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("--driver",required=True)
    parser.add_argument("--version",required=True)
    parser.add_argument("--blender-root",required=True)
    args=parser.parse_args()
    driver=str(Path(args.driver).resolve())
    blender_root=Path(args.blender_root).resolve(strict=True)
    blender=blender_root/"blender"
    if not blender.is_file():
        raise RuntimeError(f"Blender executable missing: {blender}")
    for relative in ("lib","4.5/scripts","4.5/extensions","4.5/datafiles","4.5/python"):
        if not (blender_root/relative).is_dir():
            raise RuntimeError(f"Blender runtime missing: {relative}")
    with tempfile.TemporaryDirectory(prefix="semwright-blender-probe-") as workspace:
        command=[
            "/usr/bin/bwrap","--die-with-parent","--new-session","--unshare-all","--clearenv",
            "--cap-drop","ALL","--proc","/proc","--dev","/dev","--perms","1777","--tmpfs","/dev/shm",
            "--tmpfs","/tmp","--dir","/home","--dir","/workspace","--dir","/plugin","--dir","/plugin/tools",
        ]
        for runtime in ["/usr","/lib","/lib64"]:
            if Path(runtime).exists():
                command += ["--ro-bind",runtime,runtime]
        command += ["--dir","/etc"]
        if Path("/etc/ld.so.cache").exists():
            command += ["--ro-bind","/etc/ld.so.cache","/etc/ld.so.cache"]
        command += [
            "--ro-bind","/etc/fonts","/etc/fonts",
            "--ro-bind",str(blender_root),"/workspace/blender-runtime",
            "--ro-bind",str(blender),"/plugin/tools/blender",
            "--ro-bind",driver,"/plugin/bin",
            "--bind",workspace,"/workspace/workspace",
            "--setenv","HOME","/home",
            "--setenv","PATH","/usr/bin:/bin",
            "--setenv","LANG","C.UTF-8",
            "--setenv","XDG_CACHE_HOME","/tmp/cache",
            "--setenv","XDG_CONFIG_HOME","/tmp/config",
            "--setenv","XDG_DATA_HOME","/tmp/data",
            "--chdir","/tmp","--","/plugin/bin",
        ]
        proc=subprocess.Popen(command,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
        try:
            hello={
                "type":"hello",
                "protocol":1,
                "provider":{
                    "id":"driver:blender",
                    "kind":"driver",
                    "version":args.version,
                    "namespace":"driver.blender.",
                    "application":"org.blender.Blender",
                    "origin":"driver-manifest:semwright-tests",
                },
                "executable_sha256":"a"*64,
            }
            write_frame(proc.stdin,hello)
            size,ready=read_frame(proc.stdout,30.0)
            print("hello",size,ready.get("type"),flush=True)
            if ready.get("type")!="ready":
                raise RuntimeError(f"unexpected hello response: {ready}")
            write_frame(proc.stdin,{"type":"capabilities","id":"probe-capabilities"})
            size,catalog=read_frame(proc.stdout,30.0)
            print("capabilities",size,catalog.get("type"),len(catalog.get("capabilities",[])),flush=True)
            if catalog.get("type")!="capabilities":
                raise RuntimeError(f"unexpected capabilities response: {catalog}")
            capabilities=catalog.get("capabilities",[])
            if len(capabilities) < 80:
                raise RuntimeError(f"unexpected Blender catalog size: {len(capabilities)}")
            if not all(item["descriptor"]["name"].startswith("driver.blender.") for item in capabilities):
                raise RuntimeError("Blender catalog escaped its driver namespace")

            write_frame(proc.stdin,{"type":"health","id":"probe-health"})
            size,health=read_frame(proc.stdout,30.0)
            print("health",size,health.get("type"),flush=True)
            if health.get("type")!="healthy":
                raise RuntimeError(f"unexpected health response: {health}")

            write_frame(proc.stdin,{"type":"shutdown","id":"probe-shutdown"})
            size,shutdown=read_frame(proc.stdout,10.0)
            print("shutdown",size,shutdown.get("type"),flush=True)
            if shutdown.get("type")!="shutdown":
                raise RuntimeError(f"unexpected shutdown response: {shutdown}")
            rc=proc.wait(timeout=10)
            if rc!=0:
                raise RuntimeError(f"driver exited with {rc}")
        except Exception:
            try:
                proc.terminate()
                proc.wait(timeout=3)
            except Exception:
                proc.kill()
            stderr=proc.stderr.read().decode("utf-8","replace")
            print("--- driver stderr ---",flush=True)
            print(stderr[-16000:],flush=True)
            raise


if __name__=="__main__":
    main()
