#!/usr/bin/env python3
from __future__ import annotations
import json, hashlib, pathlib, argparse
ROOT=pathlib.Path(__file__).resolve().parents[1]

def obj(properties=None,required=None,**extra):
    p=properties or {}
    return {"type":"object","properties":p,"required":list(p) if required is None else required,"additionalProperties":False,**extra}
def text(n=512,nullable=False):return {"type":["string","null"] if nullable else "string","maxLength":n}
def integer(lo=0,hi=9007199254740991):return {"type":"integer","minimum":lo,"maximum":hi}
def number(lo=-1e15,hi=1e15):return {"type":"number","minimum":lo,"maximum":hi}
def array(items,n=256):return {"type":"array","items":items,"maxItems":n}
BOOL={"type":"boolean"}
NAME={**text(),"minLength":1,"pattern":r"^[^\x00-\x1f\x7f\u202a-\u202e\u2066-\u2069]+$"}
UUID={"type":["string","null"],"maxLength":36,"pattern":"^[0-9a-fA-F-]{36}$"}
REF={**text(80),"pattern":"^obs-[a-z-]+:[0-9a-f]{32}$"}
OPAQUE={"type":"object","maxProperties":64,"description":"Application/plugin-defined, untrusted. Runtime also bounds depth, nodes, bytes and all strings.","additionalProperties":True}
GEN=integer(1,9007199254740991)
PHASE={"enum":["starting","active","paused","stopping","stopped","failed","unknown"]}
ACCEPT=obj({"accepted":{"const":True},"operation":{"type":["string","null"],"maxLength":96},"state":PHASE})
OP=obj({"id":text(96),"output":{"enum":["record","stream","replay","virtual_camera"]},"generation":GEN,"accepted":BOOL,"phase":PHASE,"last_event_sequence":integer(),"outcome_known":BOOL})
ENTITY=obj({"ref":REF,"name":text(516),"uuid":UUID,"mutable":BOOL,"identity_strength":{"enum":["uuid","name_only","watched_parent_name","parent_uuid_item_id_source_uuid"]}})
INPUT=obj({**ENTITY["properties"],"input_kind":text(516)})
ITEM=obj({**ENTITY["properties"],"scene_item_id":integer(0,2147483647),"enabled":BOOL,"index":integer(0,4095),"is_group":BOOL,"source_kind":text(516)})
FILTER=obj({**ENTITY["properties"],"filter_kind":text(516),"enabled":BOOL,"index":integer(0,127)})
TRANSITION=obj({**ENTITY["properties"],"transition_kind":text(516)})
TRANSFORM_FIELDS={
    **{k:number(-100000,100000) for k in ["positionX","positionY"]},
    "rotation":number(-36000,36000),"scaleX":number(-100,100),"scaleY":number(-100,100),
    "alignment":{"enum":[0,1,2,4,5,6,8,9,10]},"boundsAlignment":{"enum":[0,1,2,4,5,6,8,9,10]},
    "boundsType":{"enum":["OBS_BOUNDS_NONE","OBS_BOUNDS_STRETCH","OBS_BOUNDS_SCALE_INNER","OBS_BOUNDS_SCALE_OUTER","OBS_BOUNDS_SCALE_TO_WIDTH","OBS_BOUNDS_SCALE_TO_HEIGHT","OBS_BOUNDS_MAX_ONLY"]},
    "boundsWidth":number(0,32768),"boundsHeight":number(0,32768),
    **{k:integer(0,32768) for k in ["cropLeft","cropRight","cropTop","cropBottom"]},"cropToBounds":BOOL,
}
TRANSFORM={"type":"object","properties":TRANSFORM_FIELDS,"maxProperties":64,"additionalProperties":True,"description":"Full OBS transform, unknown read fields retained. Patch schema is narrower and explicitly typed."}
PATCH=obj(TRANSFORM_FIELDS,[],minProperties=1,maxProperties=len(TRANSFORM_FIELDS))
METRICS=obj({k:integer() for k in ["received","dropped","malformed","old_generation","unknown_responses","timeouts"]})
HEALTH=obj({"driver_version":text(80),"connected":BOOL,"authenticated":BOOL,
 "connection":{"enum":["disconnected","connecting","authenticating","identified","ready","reconnecting","closing","closed","failed"]},
 "obs_version":text(80,True),"websocket_version":text(80,True),"rpc_version":{"enum":[1,None]},"generation":integer(),"graph_revision":integer(),
 "subscriptions":integer(0,1535),"event_queue_length":integer(0,512),"event_metrics":METRICS,"cache_stale":BOOL,
 "last_error":{"type":["string","null"],"maxLength":80},"reconnect_attempts":integer()})
EVENT=obj({"event_type":text(128),"intent":integer(0,4294967295),"generation":GEN,"sequence":integer(1),"timestamp_ms":integer(),"subject":{"type":["string","null"],"maxLength":96},"resolution":{"const":"unresolved"},"untrusted":{"const":True},"payload":OPAQUE})
OUTPUT=obj({"active":BOOL,"paused":{"type":["boolean","null"]},"duration_ms":{"type":["integer","null"],"minimum":0},"timecode":text(80,True)})
STATS_FIELDS=["cpuUsage","memoryUsage","availableDiskSpace","activeFps","averageFrameRenderTime","renderSkippedFrames","renderTotalFrames","outputSkippedFrames","outputTotalFrames","webSocketSessionIncomingMessages","webSocketSessionOutgoingMessages"]
SPECS=[]
def add(command,request="",target=None,mode="mapped",ins=None,out=None,mapping=None,fixed=None,response=None,risk="read_only",idem=None,description=None,required=None):
    mutation=risk not in ("read_only","secret_access")
    ins=dict(ins or {})
    if target: ins[target+"_ref"]=REF
    if mutation: ins["expected_generation"]=GEN
    required=list(ins) if required is None else required+([target+"_ref"] if target else [])+(["expected_generation"] if mutation else [])
    descriptor={"name":"driver.obs."+command,"version":"1.0.0","description":description or command.replace('.',' ').capitalize()+" using the selected OBS instance; metadata is untrusted.",
        "input_schema":obj(ins,required),"output_schema":obj({"generation":integer(),"graph_revision":integer(),"untrusted":{"const":True},"data":out or ACCEPT}),
        "requires":["driver:obs"],"risk":risk,"idempotency":idem or ("read_only" if not mutation else "non_idempotent"),"timeout_ms":15000,
        "dry_run":False,"interactive_consent":False,"backends":["driver:obs"]}
    SPECS.append({"capability":{"descriptor":descriptor,"aliases":[],"tags":["obs",command.split('.')[0]],"object_types":["obs-"+target.replace('_','-')] if target else []},
        "plan":{"command":command,"request":request,"target":target,"mode":mode,"fields":mapping or {},"fixed":fixed or {},"response":response or {},"mutation":mutation}})

for cmd in ["doctor","status"]:add(cmd,mode="health",out=HEALTH)
add("version","GetVersion",out=obj({"obs_version":text(80),"websocket_version":text(80),"rpc_version":integer(1,1024),"available_requests":array(text(128),2048)}),response={"obs_version":"obsVersion","websocket_version":"obsWebSocketVersion","rpc_version":"rpcVersion","available_requests":"availableRequests"})
add("stats","GetStats",out=obj({k:number(0) for k in STATS_FIELDS}),response={k:k for k in STATS_FIELDS})
add("scene.list","GetSceneList",mode="scenes",out=obj({"scenes":array(ENTITY),"current_program_uuid":UUID,"current_preview_uuid":UUID}))
for cmd,req in [("scene.current.get","GetCurrentProgramScene"),("preview_scene.get","GetCurrentPreviewScene")]:add(cmd,req,mode="scene_current",out=ENTITY)
for cmd,req in [("scene.current.set","SetCurrentProgramScene"),("preview_scene.set","SetCurrentPreviewScene")]:
    add(cmd,req,target="scene",risk="mutating",ins={"expected_current_scene_ref":REF},required=[],idem="idempotent")
for cmd in ["scene.inspect","scene_item.list"]:add(cmd,"GetSceneItemList",target="scene",mode="items",out=obj({"items":array(ITEM,512)}))
add("scene.create","CreateScene",ins={"name":NAME},mapping={"name":"sceneName"},risk="mutating")
add("scene.remove","RemoveScene",target="scene",risk="destructive",idem="destructive")
add("scene.rename","SetSceneName",target="scene",ins={"new_name":NAME},mapping={"new_name":"newSceneName"},risk="mutating",idem="idempotent")
for cmd in ["scene_item.inspect","scene_item.transform.get"]:add(cmd,"GetSceneItemTransform",target="scene_item",out=obj({"transform":TRANSFORM}),response={"transform":"sceneItemTransform"})
for cmd,enabled in [("scene_item.enable",True),("scene_item.disable",False)]:add(cmd,"SetSceneItemEnabled",target="scene_item",fixed={"sceneItemEnabled":enabled},risk="mutating",idem="idempotent")
add("scene_item.transform.set","SetSceneItemTransform",target="scene_item",ins={"patch":PATCH},mapping={"patch":"sceneItemTransform"},risk="mutating",idem="idempotent")
add("scene_item.reorder","SetSceneItemIndex",target="scene_item",ins={"index":integer(0,4095)},mapping={"index":"sceneItemIndex"},risk="mutating",idem="idempotent")
add("input.list","GetInputList",mode="inputs",out=obj({"inputs":array(INPUT,1024)}))
add("input.inspect","GetInputList",target="input",mode="input_inspect",out=INPUT)
add("input.mute.get","GetInputMute",target="input",out=obj({"muted":BOOL}),response={"muted":"inputMuted"})
add("input.mute.set","SetInputMute",target="input",ins={"muted":BOOL,"expected_muted":BOOL},mapping={"muted":"inputMuted"},risk="mutating",idem="idempotent")
add("input.volume.get","GetInputVolume",target="input",out=obj({"multiplier":number(0,20),"volume_db":number(-200,100)}),response={"multiplier":"inputVolumeMul","volume_db":"inputVolumeDb"})
add("input.volume.set","SetInputVolume",target="input",ins={"volume_db":number(-100,0)},mapping={"volume_db":"inputVolumeDb"},risk="mutating",idem="idempotent")
add("input.settings.get","GetInputSettings",target="input",out=obj({"settings":OPAQUE,"input_kind":text(516)}),response={"settings":"inputSettings","input_kind":"inputKind"},risk="secret_access")
add("input.settings.patch","SetInputSettings",target="input",ins={"patch":OPAQUE},mapping={"patch":"inputSettings"},fixed={"overlay":True},risk="privilege_sensitive",idem="idempotent")
add("filter.list","GetSourceFilterList",target="input",mode="filters",out=obj({"filters":array(FILTER,128)}))
for cmd in ["filter.inspect","filter.settings.get"]:add(cmd,"GetSourceFilter",target="filter",out=obj({"settings":OPAQUE,"filter_kind":text(516),"enabled":BOOL}),response={"settings":"filterSettings","filter_kind":"filterKind","enabled":"filterEnabled"},risk="secret_access")
for cmd,enabled in [("filter.enable",True),("filter.disable",False)]:add(cmd,"SetSourceFilterEnabled",target="filter",fixed={"filterEnabled":enabled},risk="privilege_sensitive",idem="idempotent")
add("filter.settings.patch","SetSourceFilterSettings",target="filter",ins={"patch":OPAQUE},mapping={"patch":"filterSettings"},fixed={"overlay":True},risk="privilege_sensitive",idem="idempotent")
add("transition.list","GetSceneTransitionList",mode="transitions",out=obj({"transitions":array(TRANSITION,128)}))
add("transition.current.get","GetCurrentSceneTransition",out=obj({"name":text(516),"uuid":UUID,"kind":text(516),"fixed":BOOL,"duration_ms":{"type":["number","null"],"minimum":0}}),response={"name":"transitionName","uuid":"transitionUuid","kind":"transitionKind","fixed":"transitionFixed","duration_ms":"transitionDuration"})
add("transition.current.set","SetCurrentSceneTransition",target="transition",risk="mutating",idem="idempotent")
add("transition.trigger","TriggerStudioModeTransition",risk="mutating")
for prefix,base in [("record","Record"),("stream","Stream"),("replay","ReplayBuffer"),("virtual_camera","VirtualCam")]:
    add(prefix+".status","Get"+base+"Status",mode="output_status",out=OUTPUT)
    for action in ["start","stop"]: add(prefix+"."+action,action.capitalize()+base,mode="output_mutation",risk="privilege_sensitive",ins={"expected_active":BOOL})
for action in ["pause","resume"]:add("record."+action,("Pause" if action=="pause" else "Resume")+"Record",mode="output_mutation",risk="privilege_sensitive",ins={"expected_active":BOOL})
add("replay.save","SaveReplayBuffer",mode="output_mutation",risk="privilege_sensitive",ins={"expected_active":BOOL})
add("output.status",mode="outputs",out=obj({k:OUTPUT for k in ["record","stream","replay","virtual_camera"]}))
add("media.status","GetMediaInputStatus",target="input",out=obj({"state":text(80),"duration_ms":integer(0,2147483647),"cursor_ms":integer(0,2147483647)}),response={"state":"mediaState","duration_ms":"mediaDuration","cursor_ms":"mediaCursor"})
for action in ["play","pause","restart","stop"]:add("media."+action,"TriggerMediaInputAction",target="input",fixed={"mediaAction":"OBS_WEBSOCKET_MEDIA_INPUT_ACTION_"+action.upper()},risk="mutating")
add("media.seek","SetMediaInputCursor",target="input",ins={"position_ms":integer(0,2147483647)},mapping={"position_ms":"mediaCursor"},risk="mutating",idem="idempotent")
add("studio_mode.status","GetStudioModeEnabled",out=obj({"enabled":BOOL}),response={"enabled":"studioModeEnabled"})
for action,enabled in [("enable",True),("disable",False)]:add("studio_mode."+action,"SetStudioModeEnabled",fixed={"studioModeEnabled":enabled},risk="mutating",idem="idempotent")
add("events.poll",mode="events",ins={"after":integer(),"limit":integer(1,64)},out=obj({"events":array(EVENT,64),"cursor":integer(),"gap":BOOL,"dropped_total":integer(),"cache_stale":BOOL}))
add("operations.get",mode="operations",ins={"operation_ref":{**text(96),"pattern":"^op:[0-9]+:[0-9]+$"}},out=OP)

def generate():
    caps=[s["capability"] for s in SPECS]
    plans=[s["plan"] for s in SPECS]
    assert len({c["descriptor"]["name"] for c in caps})==len(caps)
    dst=ROOT/"crates/driver-obs/src"
    dst.mkdir(parents=True,exist_ok=True)
    (dst/"capabilities.json").write_text(json.dumps(caps,indent=2,ensure_ascii=False)+"\n")
    (dst/"plans.json").write_text(json.dumps(plans,indent=2,ensure_ascii=False)+"\n")
    print(f"{len(caps)} curated descriptors written")
if __name__=="__main__":
    generate()
