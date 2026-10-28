"""Physical color-enum regression. Run with Blender; no renderer is invoked."""
import argparse,hashlib,importlib.util,json,sys
from pathlib import Path
from types import SimpleNamespace
import bpy

args=argparse.ArgumentParser()
args.add_argument("--mode",choices=["probe","create","reopen-edit","verify-edited"],required=True)
args.add_argument("--artifact-root",required=True)
args.add_argument("--semantic",default=str(Path(__file__).resolve().parents[1]/"src"/"semantic.py"))
cfg=args.parse_args(sys.argv[sys.argv.index("--")+1:])
root=Path(cfg.artifact_root).resolve(strict=True)
module_path=Path(cfg.semantic).resolve(strict=True)
spec=importlib.util.spec_from_file_location("a4_native_color_semantic",module_path)
semantic=importlib.util.module_from_spec(spec)
spec.loader.exec_module(semantic)
checks=[]
def check(name,value):
 if not value:raise AssertionError(name)
 checks.append(name)
def ref(store,scene,relation):
 return store._ref("scenes",scene.name,[["p",relation]])
def ids(descriptor):
 return [entry["id"] for entry in descriptor.get("enum",[])]
def check_descriptors(store,scene):
 for relation,prop,want in [
  ("view_settings","view_transform",["AgX","Standard"]),
  ("view_settings","look",["None"]),
  ("display_settings","display_device",["sRGB"])]:
  got=store.property_get(ref(store,scene,relation),prop)
  check("instance descriptor "+prop,ids(got["descriptor"])==want)
  described=store.object_describe(ref(store,scene,relation))
  selected=[p for p in described["properties"] if p["id"]==prop]
  check("object descriptor parity "+prop,len(selected)==1 and ids(selected[0])==want)
  type_description=store.rna_describe(type(getattr(scene,relation)).__name__)
  selected=[p for p in type_description["properties"] if p["id"]==prop]
  check("type descriptor does not invent instance "+prop,len(selected)==1 and not selected[0].get("enum"))
def reject(store,scene,relation,prop,value,code="InvalidArgument",reference=None):
 generation=store.generation
 old=getattr(getattr(scene,relation),prop)
 try:store.property_set(reference or ref(store,scene,relation),prop,value)
 except semantic.SemanticError as exc:check("rejection code "+repr(value),exc.code==code)
 else:raise AssertionError("invalid enum accepted: "+repr(value))
 check("rejection preserves generation "+repr(value),store.generation==generation)
 check("rejection preserves value "+repr(value),getattr(getattr(scene,relation),prop)==old)

if cfg.mode in {"reopen-edit","verify-edited"}:
 source=root/("created.blend" if cfg.mode=="reopen-edit" else "edited.blend")
 check("input native project exists",source.is_file() and source.stat().st_size>0)
 opened=bpy.ops.wm.open_mainfile(filepath=str(source),load_ui=False,use_scripts=False)
 check("independent native open finished","FINISHED" in opened)
store=semantic.SemanticStore(bpy,root)
if cfg.mode=="probe":
 check_descriptors(store,bpy.context.scene)
elif cfg.mode=="create":
 ambient=bpy.context.scene
 ambient_before=(ambient.view_settings.view_transform,ambient.view_settings.look,ambient.display_settings.display_device)
 target=bpy.data.scenes.new("A4_COLOR_ENUM_DIAGNOSTIC_20261027")
 check("fixture target distinct",target!=ambient)
 check_descriptors(store,target)
 for bad in [None,False,0,[],{},"","NONE","__A4_INVALID_ENUM__","x"*257,"AgX\x00"]:
  reject(store,target,"view_settings","view_transform",bad)
 old_ref=ref(store,target,"view_settings")
 result=store.property_set(old_ref,"view_transform","Standard")
 check("real native edit Standard",target.view_settings.view_transform=="Standard" and result["value"]=="Standard")
 reject(store,target,"view_settings","view_transform","AgX",code="StaleReference",reference=old_ref)
 for relation,prop,value in [("view_settings","look","None"),("display_settings","display_device","sRGB")]:
  result=store.property_set(ref(store,target,relation),prop,value)
  check("real native typed setter "+prop,getattr(getattr(target,relation),prop)==value and result["value"]==value)
 image_settings=target.render.image_settings
 prop=image_settings.bl_rna.properties["file_format"]
 check("unregistered static enum preserved",semantic._coerce("PNG",prop,image_settings,bpy)=="PNG")
 try:semantic._coerce("__A4_INVALID_ENUM__",prop,image_settings,bpy)
 except ValueError:check("unregistered static enum still rejects invalid",True)
 else:raise AssertionError("static enum guard weakened")
 # Controlled lookup negatives exercise the same setter with a real Scene pointer.
 calls=[]
 def unavailable(owner,property_id,candidate):
  calls.append((owner.as_pointer(),property_id,candidate))
  raise RuntimeError("controlled unavailable lookup")
 proxy=SimpleNamespace(data=bpy.data,context=bpy.context,types=SimpleNamespace(Scene=bpy.types.Scene,UILayout=SimpleNamespace(enum_item_name=unavailable)))
 store.bpy=proxy
 try:reject(store,target,"view_settings","view_transform","AgX")
 finally:store.bpy=bpy
 check("lookup failure did not use static fallback",len(calls)==1)
 def only_standard(owner,property_id,candidate):
  calls.append((owner.as_pointer(),property_id,candidate))
  return "Standard" if candidate=="Standard" else ""
 proxy.types.UILayout.enum_item_name=only_standard
 store.bpy=proxy
 try:
  observed=store.property_get(ref(store,target,"view_settings"),"view_transform")
  check("descriptor reflects current lookup subset",ids(observed["descriptor"])==["Standard"])
  reject(store,target,"view_settings","view_transform","AgX")
 finally:store.bpy=bpy
 check("target instance passed to all controlled lookups",all(p==target.view_settings.as_pointer() for p,_,_ in calls))
 check("ambient scene identity unchanged",bpy.context.scene==ambient)
 check("ambient scene values unchanged",(ambient.view_settings.view_transform,ambient.view_settings.look,ambient.display_settings.display_device)==ambient_before)
 check("target Standard retained before save",target.view_settings.view_transform=="Standard")
 output=root/"created.blend"
 check("original native output absent before save",not output.exists())
 saved=bpy.ops.wm.save_as_mainfile(filepath=str(output),check_existing=False)
 check("native save finished","FINISHED" in saved and output.is_file())
 (root/"created-reference.json").write_text(json.dumps({"old_ref":ref(store,target,"view_settings"),"generation":store.generation})+"\n",encoding="utf-8")
elif cfg.mode=="reopen-edit":
 target=bpy.data.scenes["A4_COLOR_ENUM_DIAGNOSTIC_20261027"]
 check("reopened Standard persisted",target.view_settings.view_transform=="Standard")
 check("reopened look persisted",target.view_settings.look=="None")
 check("reopened display persisted",target.display_settings.display_device=="sRGB")
 check_descriptors(store,target)
 old=json.loads((root/"created-reference.json").read_text())["old_ref"]
 reject(store,target,"view_settings","view_transform","AgX",code="StaleReference",reference=old)
 result=store.property_set(ref(store,target,"view_settings"),"view_transform","AgX")
 check("independent session edit AgX",target.view_settings.view_transform=="AgX" and result["value"]=="AgX")
 output=root/"edited.blend"
 check("edited output is distinct and new",not output.exists() and (root/"created.blend").is_file())
 saved=bpy.ops.wm.save_as_mainfile(filepath=str(output),check_existing=False)
 check("edited native save finished","FINISHED" in saved and output.is_file())
elif cfg.mode=="verify-edited":
 target=bpy.data.scenes["A4_COLOR_ENUM_DIAGNOSTIC_20261027"]
 check("third process verifies edited AgX",target.view_settings.view_transform=="AgX")
 check("third process verifies look",target.view_settings.look=="None")
 check("third process verifies display",target.display_settings.display_device=="sRGB")
 check_descriptors(store,target)
result={"schema":"motionwright.a4.blender-color-component-physical/1","mode":cfg.mode,"runtime":bpy.app.version_string,"module_sha256":hashlib.sha256(module_path.read_bytes()).hexdigest(),"checks_passed":checks,"check_count":len(checks),"native_renders":0,"current_motionwright_product_g2":False,"original_unknown_replayed":False,"status":"PASS_COMPONENT_SCOPE"}
print("A4_COLOR_COMPONENT_JSON="+json.dumps(result,sort_keys=True))
