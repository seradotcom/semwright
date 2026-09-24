const SEMWRIGHT_MOTION_PROPERTY_FIELDS = new Set<KeyframePropertyFieldName>([
  "CORNER_RADIUS","STROKE_WEIGHT","STACK_SPACING","STACK_PADDING_LEFT","STACK_PADDING_TOP",
  "STACK_PADDING_RIGHT","STACK_PADDING_BOTTOM","WIDTH","HEIGHT",
  "RECTANGLE_TOP_LEFT_CORNER_RADIUS","RECTANGLE_TOP_RIGHT_CORNER_RADIUS",
  "RECTANGLE_BOTTOM_LEFT_CORNER_RADIUS","RECTANGLE_BOTTOM_RIGHT_CORNER_RADIUS",
  "BORDER_TOP_WEIGHT","BORDER_BOTTOM_WEIGHT","BORDER_LEFT_WEIGHT","BORDER_RIGHT_WEIGHT",
  "STACK_COUNTER_SPACING","OPACITY","GRID_ROW_GAP","GRID_COLUMN_GAP",
  "TRANSLATION_X","TRANSLATION_Y","TRANSLATION_XY","ROTATION","SCALE_X","SCALE_Y",
  "SCALE_XY","PATH_TRIM_START","PATH_TRIM_END"
]);
const SEMWRIGHT_MOTION_EFFECT_FIELDS = new Set<EffectKeyframeFieldName>([
  "OFFSET_X","OFFSET_Y","RADIUS","SPREAD","COLOR","REFRACTION_RADIUS","SPECULAR_ANGLE",
  "SPECULAR_INTENSITY","CHROMATIC_ABERRATION","SPLAY","REFRACTION_INTENSITY",
  "START_RADIUS","NOISE_SIZE_X","NOISE_SIZE_Y","DENSITY","EFFECT_OPACITY","SECONDARY_COLOR"
]);
const SEMWRIGHT_MOTION_ALIASES: Record<string,{name:KeyframePropertyFieldName;kind:"FLOAT"|"VECTOR"}> = {
  x:{name:"TRANSLATION_X",kind:"FLOAT"}, translatex:{name:"TRANSLATION_X",kind:"FLOAT"},
  y:{name:"TRANSLATION_Y",kind:"FLOAT"}, translatey:{name:"TRANSLATION_Y",kind:"FLOAT"},
  translate:{name:"TRANSLATION_XY",kind:"VECTOR"}, move:{name:"TRANSLATION_XY",kind:"VECTOR"},
  opacity:{name:"OPACITY",kind:"FLOAT"}, fade:{name:"OPACITY",kind:"FLOAT"},
  rotate:{name:"ROTATION",kind:"FLOAT"}, rotation:{name:"ROTATION",kind:"FLOAT"},
  scale:{name:"SCALE_XY",kind:"VECTOR"}, scalex:{name:"SCALE_X",kind:"FLOAT"},
  scaley:{name:"SCALE_Y",kind:"FLOAT"}, radius:{name:"CORNER_RADIUS",kind:"FLOAT"},
  cornerradius:{name:"CORNER_RADIUS",kind:"FLOAT"}, width:{name:"WIDTH",kind:"FLOAT"},
  w:{name:"WIDTH",kind:"FLOAT"}, height:{name:"HEIGHT",kind:"FLOAT"}, h:{name:"HEIGHT",kind:"FLOAT"},
  strokeweight:{name:"STROKE_WEIGHT",kind:"FLOAT"}, gap:{name:"STACK_SPACING",kind:"FLOAT"},
  spacing:{name:"STACK_SPACING",kind:"FLOAT"}, trimstart:{name:"PATH_TRIM_START",kind:"FLOAT"},
  trimend:{name:"PATH_TRIM_END",kind:"FLOAT"}
};
const SEMWRIGHT_MOTION_EASINGS: Record<string,MotionEasing["type"]> = {
  linear:"LINEAR","ease-in":"EASE_IN","ease-out":"EASE_OUT","ease-in-out":"EASE_IN_AND_OUT",
  "ease-in-back":"EASE_IN_BACK","ease-out-back":"EASE_OUT_BACK",
  "ease-in-out-back":"EASE_IN_AND_OUT_BACK",gentle:"GENTLE",quick:"QUICK",
  bouncy:"BOUNCY",slow:"SLOW",hold:"HOLD"
};
function semanticMotionFinite(value: unknown, label: string): number {
  const n=Number(value); if(!Number.isFinite(n)) throw new Error("invalid_"+label); return n;
}
function semanticMotionEasing(raw: unknown): MotionEasing | VariableAlias | undefined {
  if(raw===undefined||raw===null||raw==="") return undefined;
  if(typeof raw==="string"){
    const key=raw.toLowerCase();
    const mapped=SEMWRIGHT_MOTION_EASINGS[key] ?? (/^[A-Z][A-Z0-9_]*$/.test(raw)?raw as MotionEasing["type"]:undefined);
    if(!mapped) throw new Error("invalid_motion_easing");
    return {type:mapped} as MotionEasing;
  }
  if(typeof raw!=="object"||Array.isArray(raw)) throw new Error("invalid_motion_easing");
  const value=raw as any;
  if(value.type==="VARIABLE_ALIAS"&&typeof value.id==="string"&&value.id.length<=256)return {type:"VARIABLE_ALIAS",id:value.id} as VariableAlias;
  const allowed=new Set(Object.values(SEMWRIGHT_MOTION_EASINGS).concat(["CUSTOM_CUBIC_BEZIER","CUSTOM_SPRING"] as any));
  if(!allowed.has(value.type)) throw new Error("invalid_motion_easing");
  if(value.type==="CUSTOM_SPRING"){
    const bounce=semanticMotionFinite(value.easingFunctionSpring?.bounce,"spring_bounce");
    if(bounce<0||bounce>1)throw new Error("invalid_spring_bounce");
    return {type:"CUSTOM_SPRING",easingFunctionSpring:{bounce}};
  }
  if(value.type==="CUSTOM_CUBIC_BEZIER"){
    const c=value.easingFunctionCubicBezier;
    if(!c)throw new Error("missing_cubic_bezier");
    return {type:"CUSTOM_CUBIC_BEZIER",easingFunctionCubicBezier:{
      x1:semanticMotionFinite(c.x1,"bezier"),y1:semanticMotionFinite(c.y1,"bezier"),
      x2:semanticMotionFinite(c.x2,"bezier"),y2:semanticMotionFinite(c.y2,"bezier")
    }};
  }
  return {type:value.type} as MotionEasing;
}
function semanticMotionField(raw: unknown): KeyframeField {
  if(!raw||typeof raw!=="object"||Array.isArray(raw))throw new Error("invalid_motion_field");
  const f=raw as any;
  if(f.type==="PROPERTY"&&SEMWRIGHT_MOTION_PROPERTY_FIELDS.has(f.name))return {type:"PROPERTY",name:f.name};
  if(f.type!=="INDEXED_ITEM"||!Number.isInteger(f.index)||f.index<0||f.index>1024)throw new Error("invalid_motion_field");
  if((f.collection==="fills"||f.collection==="strokes")){
    if(f.propertyId!==undefined){
      if(typeof f.propertyId!=="string"||f.propertyId.length===0||f.propertyId.length>256)throw new Error("invalid_motion_property_id");
      return {type:"INDEXED_ITEM",collection:f.collection,index:f.index,propertyId:f.propertyId};
    }
    return {type:"INDEXED_ITEM",collection:f.collection,index:f.index};
  }
  if(f.collection==="effects"){
    if(f.propertyId!==undefined){
      if(typeof f.propertyId!=="string"||f.propertyId.length===0||f.propertyId.length>256)throw new Error("invalid_motion_property_id");
      return {type:"INDEXED_ITEM",collection:"effects",index:f.index,propertyId:f.propertyId};
    }
    if(!SEMWRIGHT_MOTION_EFFECT_FIELDS.has(f.field))throw new Error("invalid_motion_effect_field");
    return {type:"INDEXED_ITEM",collection:"effects",index:f.index,field:f.field};
  }
  throw new Error("invalid_motion_field");
}
function semanticMotionValue(raw: unknown): KeyframeValue {
  if(!raw||typeof raw!=="object"||Array.isArray(raw))throw new Error("invalid_motion_value");
  const v=raw as any;
  switch(v.type){
    case "FLOAT": return {type:"FLOAT",value:semanticMotionFinite(v.value,"motion_value")};
    case "BOOL": if(typeof v.value!=="boolean")throw new Error("invalid_motion_value");return {type:"BOOL",value:v.value};
    case "TEXT_DATA": if(typeof v.value!=="string"||v.value.length>65536)throw new Error("invalid_motion_value");return {type:"TEXT_DATA",value:v.value};
    case "VECTOR": return {type:"VECTOR",value:{x:semanticMotionFinite(v.value?.x,"motion_vector"),y:semanticMotionFinite(v.value?.y,"motion_vector")}};
    case "COLOR": {
      const c=v.value; const out={r:semanticMotionFinite(c?.r,"motion_color"),g:semanticMotionFinite(c?.g,"motion_color"),b:semanticMotionFinite(c?.b,"motion_color"),a:semanticMotionFinite(c?.a,"motion_color")};
      if(Object.values(out).some(n=>n<0||n>1))throw new Error("invalid_motion_color");return {type:"COLOR",value:out};
    }
    case "CIRCLE": return {type:"CIRCLE",value:{x:semanticMotionFinite(v.value?.x,"motion_circle"),y:semanticMotionFinite(v.value?.y,"motion_circle"),radius:semanticMotionFinite(v.value?.radius,"motion_circle")}};
    case "LINE": return {type:"LINE",value:{x:semanticMotionFinite(v.value?.x,"motion_line"),y:semanticMotionFinite(v.value?.y,"motion_line"),x2:semanticMotionFinite(v.value?.x2,"motion_line"),y2:semanticMotionFinite(v.value?.y2,"motion_line")}};
    case "CIRCLE_POINT": return {type:"CIRCLE_POINT",value:{x:semanticMotionFinite(v.value?.x,"motion_circle_point"),y:semanticMotionFinite(v.value?.y,"motion_circle_point"),radius:semanticMotionFinite(v.value?.radius,"motion_circle_point"),angle:semanticMotionFinite(v.value?.angle,"motion_circle_point")}};
    case "COLOR_POINT": {
      const c=semanticMotionValue({type:"COLOR",value:v.value?.color}) as Extract<KeyframeValue,{type:"COLOR"}>;
      return {type:"COLOR_POINT",value:{x:semanticMotionFinite(v.value?.x,"motion_color_point"),y:semanticMotionFinite(v.value?.y,"motion_color_point"),color:c.value}};
    }
    default: throw new Error("invalid_motion_value");
  }
}
function semanticMotionTrack(raw: unknown): ManualKeyframeTrackInput {
  if(!raw||typeof raw!=="object"||Array.isArray(raw))throw new Error("invalid_motion_track");
  const t=raw as any;
  if(!Array.isArray(t.keyframes)||t.keyframes.length===0||t.keyframes.length>MAX_KEYFRAMES)throw new Error("keyframe_limit");
  let last=-1;
  const keyframes=t.keyframes.map((k:any)=>{
    if(!k||typeof k!=="object"||Array.isArray(k))throw new Error("invalid_keyframe");
    const timelinePosition=semanticMotionFinite(k.timelinePosition,"keyframe_time");
    if(timelinePosition<0||timelinePosition>3600||timelinePosition<last)throw new Error("invalid_keyframe_time");
    last=timelinePosition;
    const out:any={timelinePosition,value:semanticMotionValue(k.value)};
    if(k.id!==undefined){if(typeof k.id!=="string"||k.id.length>256)throw new Error("invalid_keyframe_id");out.id=k.id}
    const easing=semanticMotionEasing(k.easing);if(easing)out.easing=easing;
    return out as ManualKeyframeInput;
  });
  const track:any={keyframes};
  if(t.id!==undefined){if(typeof t.id!=="string"||t.id.length>256)throw new Error("invalid_track_id");track.id=t.id}
  if(t.baseValue!==undefined)track.baseValue=semanticMotionValue(t.baseValue);
  return track as ManualKeyframeTrackInput;
}
function semanticMotionAlias(raw: unknown): {field:KeyframeField;kind:"FLOAT"|"VECTOR"} {
  const key=String(raw??"").trim(); if(!key)throw new Error("missing_motion_field");
  const alias=SEMWRIGHT_MOTION_ALIASES[key.toLowerCase()] ?? (SEMWRIGHT_MOTION_PROPERTY_FIELDS.has(key as KeyframePropertyFieldName)?{name:key as KeyframePropertyFieldName,kind:"FLOAT" as const}:undefined);
  if(!alias)throw new Error("unknown_motion_field");
  return {field:{type:"PROPERTY",name:alias.name},kind:alias.kind};
}
function semanticMotionMacroValue(kind:"FLOAT"|"VECTOR",raw:unknown):KeyframeValue{
  if(kind==="FLOAT")return {type:"FLOAT",value:semanticMotionFinite(raw,"motion_value")};
  if(typeof raw==="number")return {type:"VECTOR",value:{x:raw,y:raw}};
  const v=raw as any;return {type:"VECTOR",value:{x:semanticMotionFinite(v?.x,"motion_vector"),y:semanticMotionFinite(v?.y,"motion_vector")}};
}
function semanticMotionPair(field:string,from:unknown,to:unknown,at:number,duration:number,easing:unknown){
  const def=semanticMotionAlias(field);
  return {field:def.field,track:{keyframes:[
    {timelinePosition:at,value:semanticMotionMacroValue(def.kind,from)},
    {timelinePosition:at+duration,value:semanticMotionMacroValue(def.kind,to),easing:semanticMotionEasing(easing)}
  ]} as ManualKeyframeTrackInput};
}
function semanticMotionPreset(name:string,at:number,duration:number,easing:unknown,distance:number,scaleFrom:number){
  switch(name){
    case "fade-in": return [semanticMotionPair("opacity",0,1,at,duration,easing)];
    case "fade-out": return [semanticMotionPair("opacity",1,0,at,duration,easing)];
    case "fade-up": return [semanticMotionPair("opacity",0,1,at,duration,easing),semanticMotionPair("translateY",distance,0,at,duration,easing)];
    case "fade-down": return [semanticMotionPair("opacity",0,1,at,duration,easing),semanticMotionPair("translateY",-distance,0,at,duration,easing)];
    case "slide-left": return [semanticMotionPair("opacity",0,1,at,duration,easing),semanticMotionPair("translateX",distance,0,at,duration,easing)];
    case "slide-right": return [semanticMotionPair("opacity",0,1,at,duration,easing),semanticMotionPair("translateX",-distance,0,at,duration,easing)];
    case "pop": return [semanticMotionPair("opacity",0,1,at,duration,easing),semanticMotionPair("scale",scaleFrom,1,at,duration,easing)];
    case "spin": return [semanticMotionPair("rotate",-360,0,at,duration,easing)];
    default: throw new Error("unknown_motion_preset");
  }
}
function semanticMotionEnsureTimeline(node:any,end:number){
  const timeline=(node.timelines??[])[0];
  if(timeline&&typeof node.setTimelineDuration==="function"&&timeline.duration<end)node.setTimelineDuration(timeline.id,end);
}
function semanticMotionApply(node:any,fieldRaw:unknown,trackRaw:unknown){
  const field=semanticMotionField(fieldRaw);const track=semanticMotionTrack(trackRaw);
  node.applyManualKeyframeTrack(field,track);
  const end=Math.max(...track.keyframes.map(k=>k.timelinePosition));
  semanticMotionEnsureTimeline(node,end);
  return {field,end};
}
function semanticSlotSettings(raw:unknown):SlotSettings|undefined{
  if(raw===undefined)return undefined;
  if(!raw||typeof raw!=="object"||Array.isArray(raw))throw new Error("invalid_slot_settings");
  const value=raw as any; const out:SlotSettings={};
  for(const key of Object.keys(value))if(!["stretchChildOnInsert","displayEmptyByDefault","minChildren","maxChildren","allowPreferredValuesOnly"].includes(key))throw new Error("unknown_slot_setting");
  if(value.stretchChildOnInsert!==undefined){if(typeof value.stretchChildOnInsert!=="boolean")throw new Error("invalid_slot_setting");out.stretchChildOnInsert=value.stretchChildOnInsert}
  if(value.displayEmptyByDefault!==undefined){if(typeof value.displayEmptyByDefault!=="boolean")throw new Error("invalid_slot_setting");out.displayEmptyByDefault=value.displayEmptyByDefault}
  for(const key of ["minChildren","maxChildren"] as const){
    if(value[key]!==undefined){
      if(value[key]!==null&&(!Number.isInteger(value[key])||value[key]<0||value[key]>1024))throw new Error("invalid_slot_limit");
      out[key]=value[key];
    }
  }
  if(out.minChildren!==undefined&&out.minChildren!==null&&out.maxChildren!==undefined&&out.maxChildren!==null&&out.minChildren>out.maxChildren)throw new Error("invalid_slot_limits");
  if(value.allowPreferredValuesOnly!==undefined){if(typeof value.allowPreferredValuesOnly!=="boolean")throw new Error("invalid_slot_setting");out.allowPreferredValuesOnly=value.allowPreferredValuesOnly}
  return out;
}
function semanticSlotParent(slot:BaseNode):{component:ComponentNode;propertyName:string}{
  let parent:BaseNode|null=slot.parent;
  while(parent&&parent.type!=="COMPONENT")parent=parent.parent;
  if(!parent||parent.type!=="COMPONENT")throw new Error("slot_component_not_found");
  const propertyName=(slot as any).componentPropertyReferences?.slotContentId;
  if(typeof propertyName!=="string"||!propertyName)throw new Error("slot_property_not_found");
  return {component:parent,propertyName};
}
async function handleSemanticProduct(request:BridgeRequest,a:any):Promise<BridgeResponse|null>{
  switch(request.operation){
    case "motion.apply":{
      const specs=extraBoundedArray(a.tracks,64,"motion_track_limit"); const results:any[]=[];let maxEnd=0;
      for(const spec of specs){
        const node=asScene(await nodeById(String(spec.nodeId))) as any;
        const applied=semanticMotionApply(node,spec.field,spec.track);maxEnd=Math.max(maxEnd,applied.end);
        results.push({nodeId:node.id,field:applied.field,end:applied.end});
      }
      if(a.duration!==undefined){
        const duration=semanticMotionFinite(a.duration,"motion_duration");if(duration<=0||duration>3600||duration<maxEnd)throw new Error("invalid_motion_duration");
        for(const id of new Set(results.map(r=>r.nodeId))){const node=asScene(await nodeById(id)) as any;semanticMotionEnsureTimeline(node,duration)}
        maxEnd=duration;
      }
      return ok(request.id,{duration:maxEnd,results},true);
    }
    case "motion.preset.apply":{
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      const at=semanticMotionFinite(a.at??0,"motion_offset"),duration=semanticMotionFinite(a.duration??0.5,"motion_duration");
      if(at<0||duration<=0||at+duration>3600)throw new Error("invalid_motion_duration");
      const tracks=semanticMotionPreset(String(a.preset),at,duration,a.easing??"ease-out",semanticMotionFinite(a.distance??24,"motion_distance"),semanticMotionFinite(a.scaleFrom??0.8,"motion_scale"));
      const fields=[] as KeyframeField[];for(const track of tracks){node.applyManualKeyframeTrack(track.field,track.track);fields.push(track.field)}
      semanticMotionEnsureTimeline(node,at+duration);
      return ok(request.id,{nodeId:node.id,preset:String(a.preset),duration:at+duration,fields},true);
    }
    case "motion.stagger":{
      const ids=extraBoundedArray(a.nodeIds,128,"motion_node_limit").map(String);if(!ids.length)throw new Error("empty_motion_nodes");
      const at=semanticMotionFinite(a.at??0,"motion_offset"),duration=semanticMotionFinite(a.duration??0.5,"motion_duration"),step=semanticMotionFinite(a.step??0.1,"motion_step");
      if(at<0||duration<=0||step<0||at+duration+step*Math.max(0,ids.length-1)>3600)throw new Error("invalid_motion_duration");
      const results:any[]=[];
      for(let i=0;i<ids.length;i++){
        const node=asScene(await nodeById(ids[i])) as any;const offset=at+i*step;
        const tracks=a.preset!==undefined
          ? semanticMotionPreset(String(a.preset),offset,duration,a.easing??"ease-out",semanticMotionFinite(a.distance??24,"motion_distance"),semanticMotionFinite(a.scaleFrom??0.8,"motion_scale"))
          : [semanticMotionPair(String(a.field),a.from,a.to,offset,duration,a.easing??"ease-out")];
        for(const track of tracks)node.applyManualKeyframeTrack(track.field,track.track);
        semanticMotionEnsureTimeline(node,offset+duration);results.push({nodeId:node.id,offset,fields:tracks.map(t=>t.field)});
      }
      return ok(request.id,{duration:at+duration+step*Math.max(0,ids.length-1),results},true);
    }
    case "slot.convert":{
      const node=await nodeById(String(a.nodeId));if(node.type!=="FRAME")throw new Error("slot_convert_requires_frame");
      if((node as any).componentPropertyReferences?.slotContentId)throw new Error("already_slot_content");
      let parent:BaseNode|null=node.parent;while(parent&&parent.type!=="COMPONENT")parent=parent.parent;
      if(!parent||parent.type!=="COMPONENT")throw new Error("slot_component_not_found");
      const component=parent as ComponentNode;
      const preferredValues=a.componentKeys===undefined?undefined:extraBoundedArray(a.componentKeys,64).map((key:any)=>({type:"COMPONENT" as const,key:String(key)}));
      const options:ComponentPropertyOptions={};
      if(preferredValues)options.preferredValues=preferredValues;
      if(a.description!==undefined)options.description=String(a.description).slice(0,4096);
      const slotSettings=semanticSlotSettings(a.slotSettings);if(slotSettings)options.slotSettings=slotSettings;
      const propertyName=component.addComponentProperty(String(a.name??node.name??"Slot").slice(0,256),"SLOT","",options);
      (node as any).componentPropertyReferences={...((node as any).componentPropertyReferences??{}),slotContentId:propertyName};
      return ok(request.id,{propertyName,node:extraNodeJson(node),slotSettings:component.componentPropertyDefinitions[propertyName]?.slotSettings??null},true);
    }
    case "slot.settings.patch":{
      const node=await nodeById(String(a.nodeId));if(node.type!=="SLOT"&&node.type!=="FRAME")throw new Error("not_slot_content");
      const {component,propertyName}=semanticSlotParent(node);
      const current=component.componentPropertyDefinitions[propertyName];if(!current||current.type!=="SLOT")throw new Error("slot_property_not_found");
      const patch=semanticSlotSettings(a.slotSettings);if(!patch)throw new Error("slot_settings_required");
      const merged={...(current.slotSettings??{}),...patch};
      component.editComponentProperty(propertyName,{slotSettings:merged});
      return ok(request.id,{propertyName,slotSettings:component.componentPropertyDefinitions[propertyName]?.slotSettings??merged},true);
    }
    default:return null;
  }
}
