"""Read-only SxS DACL/capability-name evidence. No values, tokens or ACLs changed."""
import ctypes as c, ctypes.wintypes as w, hashlib, json, pathlib, winreg
from datetime import datetime, timezone
ROOT=pathlib.Path(r"E:\Projects\semwright-blender52")
OUT=ROOT/".local/logs/sxs-registry-acl-20261028.json"
kb=c.WinDLL("kernelbase",use_last_error=True)
adv=c.WinDLL("advapi32",use_last_error=True)
kernel=c.WinDLL("kernel32",use_last_error=True)
P=c.c_void_p
kb.DeriveCapabilitySidsFromName.argtypes=[w.LPCWSTR,c.POINTER(P),c.POINTER(w.DWORD),c.POINTER(P),c.POINTER(w.DWORD)]
kb.DeriveCapabilitySidsFromName.restype=w.BOOL
adv.ConvertSidToStringSidW.argtypes=[P,c.POINTER(P)]
adv.ConvertSidToStringSidW.restype=w.BOOL
kernel.LocalFree.argtypes=[P]
kernel.LocalFree.restype=P
groups=P(); caps=P(); ng=w.DWORD(); nc=w.DWORD()
if not kb.DeriveCapabilitySidsFromName("registryRead",c.byref(groups),c.byref(ng),c.byref(caps),c.byref(nc)):
    raise c.WinError(c.get_last_error())
def sid_text(sid):
    text=P()
    if not adv.ConvertSidToStringSidW(sid,c.byref(text)): raise c.WinError(c.get_last_error())
    try:return c.wstring_at(text)
    finally:kernel.LocalFree(text)
derived={}
try:
    for name,ptr,count in [("groups",groups,ng.value),("capabilities",caps,nc.value)]:
        if count>16: raise RuntimeError("unexpected capability count")
        array=c.cast(ptr,c.POINTER(P))
        derived[name]=[sid_text(array[i]) for i in range(count)]
finally:
    for ptr,count in [(groups,ng.value),(caps,nc.value)]:
        if ptr:
            array=c.cast(ptr,c.POINTER(P))
            for i in range(count): kernel.LocalFree(array[i])
            kernel.LocalFree(ptr)
adv.RegGetKeySecurity.argtypes=[P,w.DWORD,P,c.POINTER(w.DWORD)]
adv.RegGetKeySecurity.restype=w.LONG
adv.ConvertSecurityDescriptorToStringSecurityDescriptorW.argtypes=[P,w.DWORD,w.DWORD,c.POINTER(P),c.POINTER(w.DWORD)]
adv.ConvertSecurityDescriptorToStringSecurityDescriptorW.restype=w.BOOL
rows=[]
for name,view in [("Registry64",winreg.KEY_WOW64_64KEY),("Registry32",winreg.KEY_WOW64_32KEY)]:
    with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,r"Software\Microsoft\Windows\CurrentVersion\SideBySide",0,winreg.KEY_READ|view) as key:
        size=w.DWORD()
        rc=adv.RegGetKeySecurity(int(key),7,None,c.byref(size))
        if rc!=122 or not 0<size.value<=65536:raise RuntimeError((rc,size.value))
        data=c.create_string_buffer(size.value)
        rc=adv.RegGetKeySecurity(int(key),7,data,c.byref(size))
        if rc:raise OSError(rc,"RegGetKeySecurity")
        text=P()
        if not adv.ConvertSecurityDescriptorToStringSecurityDescriptorW(data,1,7,c.byref(text),None):raise c.WinError(c.get_last_error())
        try:sddl=c.wstring_at(text)
        finally:kernel.LocalFree(text)
        rows.append({"view":name,"sddl":sddl,"descriptor_sha256":hashlib.sha256(data.raw[:size.value]).hexdigest(),
            "derived_registryRead_sid_in_dacl":all(sid in sddl for sid in derived["capabilities"])})
receipt={"utc":datetime.now(timezone.utc).isoformat(),"key":r"HKLM\Software\Microsoft\Windows\CurrentVersion\SideBySide",
 "derived_name":"registryRead","derived":derived,"views":rows,"registry_values_read":False,
 "registry_written":False,"token_changed":False,"capability_added":False,"acl_changed":False,"A05":"BLOCKED"}
with OUT.open("x",encoding="utf-8",newline="\n") as f:json.dump(receipt,f,indent=2);f.write("\n")
print(json.dumps(receipt,indent=2))
