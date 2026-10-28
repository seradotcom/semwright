from pathlib import Path
import ctypes,hashlib,json
R=Path(r'E:\Projects\semwright-blender52');B=Path(r'E:\Projects\worldwright-archipelago\tools\blender-5.2.1-runtime')
payload=(R/'crates/driver-blender/tests/fixtures/windows_cpu_probe_payload.json').read_bytes()
assert hashlib.sha256(payload).hexdigest()=='43709145f359cb0ff4f1c42062dfa27f9b532e8f1f8f358417ad17cbc38b6cb7'
expected={e['relative']:e for e in json.loads(payload)}
fixed=['blender.crt/vcruntime140.dll','blender.crt/vcruntime140_1.dll','blender.crt/msvcp140.dll','blender_cpu_check.dll']
O=R/'.local/logs/cpu-image-readonly';O.mkdir(exist_ok=False)
k=ctypes.WinDLL('kernel32',use_last_error=True)
k.LoadLibraryExW.argtypes=[ctypes.c_wchar_p,ctypes.c_void_p,ctypes.c_uint32];k.LoadLibraryExW.restype=ctypes.c_void_p
k.FindResourceW.argtypes=[ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p];k.FindResourceW.restype=ctypes.c_void_p
k.SizeofResource.argtypes=[ctypes.c_void_p,ctypes.c_void_p];k.SizeofResource.restype=ctypes.c_uint32
k.LoadResource.argtypes=[ctypes.c_void_p,ctypes.c_void_p];k.LoadResource.restype=ctypes.c_void_p
k.LockResource.argtypes=[ctypes.c_void_p];k.LockResource.restype=ctypes.c_void_p
k.FreeLibrary.argtypes=[ctypes.c_void_p];k.FreeLibrary.restype=ctypes.c_int
CB=ctypes.WINFUNCTYPE(ctypes.c_int,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_ssize_t)
k.EnumResourceNamesW.argtypes=[ctypes.c_void_p,ctypes.c_void_p,CB,ctypes.c_ssize_t];k.EnumResourceNamesW.restype=ctypes.c_int
prior=json.loads((R/'.local/logs/blender-loader-readonly/receipt.json').read_text(encoding='utf-8'))
imports={r['file'].replace('\\','/').lower():r for r in prior['pe_imports']};rows=[]
for relative in fixed:
    p=B/relative;raw=p.read_bytes();e=expected[relative]
    assert len(raw)==e['bytes'] and hashlib.sha256(raw).hexdigest()==e['sha256']
    old=imports[relative.lower()];assert old['sha256']==e['sha256']
    module=k.LoadLibraryExW(str(p),None,0x2|0x20);assert module,ctypes.get_last_error()
    ids=[];unsupported=[]
    @CB
    def callback(_module,_type,name,_param):
        value=int(name or 0)
        if not 0<value<=65535 or len(ids)>=16:
            unsupported.append('named_or_excess_manifest_resources');return 0
        ids.append(value);return 1
    resources=[]
    try:
        ctypes.set_last_error(0)
        ok=k.EnumResourceNamesW(module,24,callback,0);error=ctypes.get_last_error()
        assert not unsupported,unsupported
        assert ok or error in (1812,1813),(relative,error)
        for resource_id in ids:
            res=k.FindResourceW(module,resource_id,24);assert res,ctypes.get_last_error()
            n=k.SizeofResource(module,res);assert 0<n<=65536
            data=k.LoadResource(module,res);assert data
            ptr=k.LockResource(data);assert ptr
            manifest=ctypes.string_at(ptr,n);name=relative.replace('/','_')+f'.manifest-{resource_id}.xml'
            (O/name).write_bytes(manifest)
            resources.append({'resource_id':resource_id,'bytes':n,'sha256':hashlib.sha256(manifest).hexdigest(),'file':name,'text':manifest.decode('utf-8-sig',errors='replace')})
    finally:assert k.FreeLibrary(module)
    rows.append({'relative':relative,'image_sha256':e['sha256'],'resources':resources,'imports':old['imports'],'delay_imports':old['delay_imports'],'dll_code_executed':False,'resource_mapping_flags':34})
receipt={'read_only_inputs':True,'blender_component_code_executed':False,'files':rows,'resource_language_scope':'FindResourceW default language for every enumerated integer RT_MANIFEST ID'}
(O/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n',encoding='utf-8')
print(json.dumps([{'relative':r['relative'],'manifest_ids':[x['resource_id'] for x in r['resources']],'imports':r['imports']} for r in rows]),flush=True)
