"""Read pinned PE metadata and private assembly manifests; does not execute application code."""
from pathlib import Path
import ctypes,hashlib,json,struct
R=Path(r"E:\Projects\semwright-blender52")
B=Path(r"E:\Projects\worldwright-archipelago\tools\blender-5.2.1-runtime")
O=R/".local/logs/blender-loader-readonly";O.mkdir(exist_ok=False)
exe=B/"blender.exe";data=exe.read_bytes()
assert hashlib.sha256(data).hexdigest()=="284f4041f98e113f3dc10654a7193ffaaa9bfdfec8b87fa116620a48b5f6d4cb"
def imports(p):
    raw=p.read_bytes();pe=struct.unpack_from("<I",raw,0x3c)[0];assert raw[pe:pe+4]==b"PE\0\0"
    n=struct.unpack_from("<H",raw,pe+6)[0];size=struct.unpack_from("<H",raw,pe+20)[0];op=pe+24
    magic=struct.unpack_from("<H",raw,op)[0];directory=op+(112 if magic==0x20b else 96)
    sections=[]
    for i in range(n):
        off=op+size+i*40;vsize,va,rsize,rptr=struct.unpack_from("<IIII",raw,off+8);sections.append((va,max(vsize,rsize),rptr,rsize))
    def offset(rva):
        for va,span,ptr,sz in sections:
            if va<=rva<va+span:
                at=ptr+rva-va;assert ptr<=at<ptr+sz and at<len(raw);return at
        raise ValueError(("RVA missing",rva))
    def name(rva):
        at=offset(rva);end=raw.find(b"\0",at,at+512);assert end>=at;return raw[at:end].decode("ascii")
    result={}
    for kind,index,width,name_index in(("imports",1,20,3),("delay_imports",13,32,1)):
        rva,length=struct.unpack_from("<II",raw,directory+index*8);names=[]
        if rva:
            at=offset(rva)
            for i in range(min(length//width,1024)):
                values=struct.unpack_from("<"+"I"*(width//4),raw,at+i*width)
                if not any(values):break
                if kind=="delay_imports":assert values[0]&1,"VA delay descriptor unsupported"
                names.append(name(values[name_index]))
        result[kind]=names
    return {"file":str(p.relative_to(B)),"sha256":hashlib.sha256(raw).hexdigest(),**result}
k=ctypes.WinDLL("kernel32",use_last_error=True)
k.LoadLibraryExW.argtypes=[ctypes.c_wchar_p,ctypes.c_void_p,ctypes.c_uint32];k.LoadLibraryExW.restype=ctypes.c_void_p
k.FindResourceW.argtypes=[ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p];k.FindResourceW.restype=ctypes.c_void_p
k.SizeofResource.argtypes=[ctypes.c_void_p,ctypes.c_void_p];k.SizeofResource.restype=ctypes.c_uint32
k.LoadResource.argtypes=[ctypes.c_void_p,ctypes.c_void_p];k.LoadResource.restype=ctypes.c_void_p
k.LockResource.argtypes=[ctypes.c_void_p];k.LockResource.restype=ctypes.c_void_p
k.FreeLibrary.argtypes=[ctypes.c_void_p];k.FreeLibrary.restype=ctypes.c_int
module=k.LoadLibraryExW(str(exe),None,0x2|0x20);assert module,ctypes.get_last_error()
try:
    res=k.FindResourceW(module,1,24);assert res,ctypes.get_last_error()
    size=k.SizeofResource(module,res);assert 0<size<=65536
    handle=k.LoadResource(module,res);assert handle
    address=k.LockResource(handle);assert address
    manifest=ctypes.string_at(address,size)
finally:assert k.FreeLibrary(module)
(O/"blender.embedded.manifest").write_bytes(manifest)
rows=[imports(exe)];private=[]
for rel in ["blender.shared/blender.shared.manifest","blender.crt/blender.crt.manifest"]:
    p=B/rel;raw=p.read_bytes();private.append({"path":rel,"sha256":hashlib.sha256(raw).hexdigest(),"text":raw.decode("utf-8-sig")})
for name in rows[0]["imports"]:
    found=[p for p in (B/name,B/"blender.shared"/name,B/"blender.crt"/name) if p.is_file()]
    rows += [imports(p) for p in found]
receipt={"read_only":True,"application_executed":False,"embedded_manifest":manifest.decode("utf-8-sig"),"embedded_sha256":hashlib.sha256(manifest).hexdigest(),"private_manifests":private,"pe_imports":rows}
(O/"receipt.json").write_text(json.dumps(receipt,indent=2)+"\n",encoding="utf-8")
print(json.dumps(receipt,indent=2),flush=True)
