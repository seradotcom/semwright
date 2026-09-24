function spSerializable(value:any,depth=0,seen=new Set<any>()):any{
  if(value===null||value===undefined)return value??null;
  if(typeof value==="string"||typeof value==="boolean")return value;
  if(typeof value==="number")return Number.isFinite(value)?value:null;
  if(typeof value==="bigint")return value.toString();
  if(typeof value==="symbol")return value===figma.mixed?"MIXED":String(value);
  if(typeof value==="function")return undefined;
  if(depth>=6)return "[MAX_DEPTH]";
  if(value instanceof Uint8Array)return {kind:"bytes",length:value.byteLength};
  if(typeof value==="object"){
    if(seen.has(value))return "[CIRCULAR]";
    seen.add(value);
    if(typeof value.id==="string"&&typeof value.type==="string"&&typeof value.name==="string"){
      return summarize(value as BaseNode);
    }
    if(Array.isArray(value)){
      const out=value.slice(0,128).map(v=>spSerializable(v,depth+1,seen));
      seen.delete(value);return out;
    }
    const out:Record<string,unknown>={};
    for(const key of Object.keys(value).sort().slice(0,64)){
      const item=spSerializable(value[key],depth+1,seen);
      if(item!==undefined)out[key]=item;
    }
    seen.delete(value);return out;
  }
  return String(value);
}
function spValidateJson(value:any,depth=0,state={items:0}):void{
  if(++state.items>4096||depth>8)throw new Error("property_value_limit");
  if(value===null||typeof value==="string"||typeof value==="boolean")return;
  if(typeof value==="number"){if(!Number.isFinite(value))throw new Error("invalid_number");return;}
  if(Array.isArray(value)){if(value.length>256)throw new Error("property_array_limit");for(const v of value)spValidateJson(v,depth+1,state);return;}
  if(typeof value!=="object")throw new Error("invalid_property_value");
  const keys=Object.keys(value);if(keys.length>128)throw new Error("property_object_limit");
  for(const key of keys){if(["__proto__","prototype","constructor"].includes(key))throw new Error("unsafe_property_key");spValidateJson(value[key],depth+1,state);}
}
async function spInspectProperties(nodeId:string,args:any){
  const node=await nodeById(nodeId) as any;
  let requested:string[];
  if(Array.isArray(args.properties)){
    if(args.properties.length>128)throw new Error("property_list_limit");
    requested=args.properties.map(String);
  }else{
    const all=[...SEMWRIGHT_FIGMA_NODE_READ_PROPERTIES].filter(p=>p in node).sort();
    const offset=Math.max(0,Number(args.offset??0));
    const limit=Math.max(1,Math.min(128,Number(args.limit??128)));
    requested=all.slice(offset,offset+limit);
  }
  const values:Record<string,unknown>={},unavailable:string[]=[];
  for(const property of requested){
    if(!SEMWRIGHT_FIGMA_NODE_READ_PROPERTIES.has(property))throw new Error("property_not_public");
    if(!(property in node)){unavailable.push(property);continue;}
    try{values[property]=spSerializable(node[property]);}
    catch{unavailable.push(property);}
  }
  const allPresent=[...SEMWRIGHT_FIGMA_NODE_READ_PROPERTIES].filter(p=>p in node).sort();
  const offset=Array.isArray(args.properties)?0:Math.max(0,Number(args.offset??0));
  const next=Array.isArray(args.properties)?null:(offset+requested.length<allPresent.length?offset+requested.length:null);
  return {nodeId:node.id,nodeType:node.type,values,unavailable,offset,nextOffset:next,totalProperties:allPresent.length};
}
function spFontNameInput(value:any):FontNameInput{
  if(!value||typeof value!=="object"||Array.isArray(value)||typeof value.family!=="string"||value.family.length===0||value.family.length>256)throw new Error("invalid_font_name");
  if(value.style!==undefined&&(typeof value.style!=="string"||value.style.length===0||value.style.length>256))throw new Error("invalid_font_style");
  const font:any={family:value.family};
  if(value.style!==undefined)font.style=value.style;
  if(value.variationSettings!==undefined){
    if(!value.variationSettings||typeof value.variationSettings!=="object"||Array.isArray(value.variationSettings))throw new Error("invalid_font_variation_settings");
    const entries=Object.entries(value.variationSettings);
    if(entries.length>32)throw new Error("font_variation_axis_limit");
    const settings:Record<string,number>={};
    for(const [axis,raw] of entries){
      if(!/^[A-Za-z0-9]{1,32}$/.test(axis)||typeof raw!=="number"||!Number.isFinite(raw)||raw < -100000||raw > 100000)throw new Error("invalid_font_variation_axis");
      settings[axis]=raw;
    }
    font.variationSettings=settings;
  }
  return font as FontNameInput;
}
function spValidateReviewedPropertyType(property:string,value:any):void{
  const review=SEMWRIGHT_FIGMA_NODE_WRITE_TYPES[property];
  if(!review)throw new Error("property_type_not_reviewed");
  if(value===null){
    if(!review.nullable)throw new Error("property_null_not_allowed");
    return;
  }
  if(review.kind==="number"&&(typeof value!=="number"||!Number.isFinite(value)))throw new Error("property_type_number");
  if(review.kind==="boolean"&&typeof value!=="boolean")throw new Error("property_type_boolean");
  if(review.kind==="string"&&typeof value!=="string")throw new Error("property_type_string");
  if(review.kind==="array"&&!Array.isArray(value))throw new Error("property_type_array");
  if(review.kind==="object"&&(!value||typeof value!=="object"||Array.isArray(value)))throw new Error("property_type_object");
}
async function spSetProperty(node:any,property:string,value:any){
  if(!SEMWRIGHT_FIGMA_NODE_WRITE_PROPERTIES.has(property))throw new Error("property_not_writable");
  if(!(property in node))throw new Error("property_unavailable_on_node");
  if(value==="MIXED")throw new Error("mixed_value_read_only");
  spValidateReviewedPropertyType(property,value);
  spValidateJson(value);
  let normalized=value;
  if(property==="fontName"){
    const font=spFontNameInput(value);
    await figma.loadFontAsync(font);
    normalized=font;
  }
  if(property==="characters"&&node.type==="TEXT")await ensureFonts(node as TextNode);
  if(["fills","strokes"].includes(property)&&Array.isArray(value)&&value.length>64)throw new Error("paint_limit");
  if(property==="effects"&&Array.isArray(value)&&value.length>32)throw new Error("effect_limit");
  if(property==="layoutGrids"&&Array.isArray(value)&&value.length>32)throw new Error("grid_limit");
  if(property==="vectorNetwork"){
    if((value?.vertices?.length??0)>4096||(value?.segments?.length??0)>8192||(value?.regions?.length??0)>1024)throw new Error("vector_network_limit");
  }
  node[property]=normalized;
}
async function spPatchProperties(nodeId:string,properties:any){
  if(!properties||typeof properties!=="object"||Array.isArray(properties))throw new Error("invalid_properties");
  const entries=Object.entries(properties);
  if(entries.length>32)throw new Error("property_limit");
  const node=asScene(await nodeById(nodeId)) as any;
  for(const [property,value] of entries)await spSetProperty(node,property,value);
  const values:Record<string,unknown>={};
  for(const [property] of entries)values[property]=spSerializable(node[property]);
  return {nodeId:node.id,nodeType:node.type,changed:entries.map(([key])=>key),values};
}
async function handleSemanticProperties(request:BridgeRequest,a:any):Promise<BridgeResponse|null>{
  switch(request.operation){
    case "node.properties.inspect":
      return ok(request.id,await spInspectProperties(String(a.nodeId),a));
    default:return null;
  }
}
