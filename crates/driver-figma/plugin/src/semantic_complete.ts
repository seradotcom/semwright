const EXTRA_MAX_BATCH = 128;
const EXTRA_MAX_ARTIFACTS = 8;
const EXTRA_MAX_ARTIFACT_BYTES = 16 * 1024 * 1024;
const EXTRA_CHUNK_BYTES = 192 * 1024;
const extraArtifacts = new Map<string, Uint8Array>();

function extraRequireEditor(...types: string[]) {
  if (!types.includes(figma.editorType)) throw new Error("unsupported_editor");
}
function extraBoundedArray(value: unknown, max: number, code = "array_limit"): any[] {
  if (!Array.isArray(value) || value.length > max) throw new Error(code);
  return value;
}
function extraFinite(value: unknown, code = "invalid_number"): number {
  const n = Number(value);
  if (!Number.isFinite(n)) throw new Error(code);
  return n;
}
function extraNodeJson(node: BaseNode): Record<string, unknown> {
  const n = node as any;
  const out: Record<string, unknown> = summarize(node);
  for (const field of [
    "rotation","opacity","blendMode","layoutMode","layoutSizingHorizontal","layoutSizingVertical",
    "layoutGrow","layoutAlign","layoutPositioning","clipsContent","cornerRadius","strokeWeight",
    "strokeAlign","strokeCap","strokeJoin","dashPattern","constraints","minWidth","maxWidth",
    "minHeight","maxHeight","componentPropertyReferences","boundVariables","annotations",
    "explicitVariableModes","resolvedVariableModes","overflowDirection"
  ]) {
    if (field in n && n[field] !== figma.mixed) out[field] = n[field];
  }
  if ("fills" in n && n.fills !== figma.mixed) out.fills = n.fills;
  if ("strokes" in n && n.strokes !== figma.mixed) out.strokes = n.strokes;
  if ("effects" in n) out.effects = n.effects;
  if ("reactions" in n) out.reactions = n.reactions;
  return out;
}
function extraStyleSummary(style: BaseStyle): Record<string, unknown> {
  const s = style as any;
  const out: Record<string, unknown> = {
    id: style.id, key: style.key, name: style.name, type: style.type,
    remote: style.remote, description: style.description, descriptionMarkdown: style.descriptionMarkdown,
  };
  if (style.type === "PAINT") out.paints = s.paints;
  if (style.type === "TEXT") {
    for (const f of ["fontSize","fontName","letterSpacing","lineHeight","leadingTrim","paragraphIndent",
      "paragraphSpacing","textWrapStyle","listSpacing","hangingPunctuation","hangingList","textCase","textDecoration"]) {
      out[f] = s[f];
    }
  }
  if (style.type === "EFFECT") out.effects = s.effects;
  if (style.type === "GRID") out.layoutGrids = s.layoutGrids;
  return out;
}
async function extraAllStyles(): Promise<BaseStyle[]> {
  const groups = await Promise.all([
    figma.getLocalPaintStylesAsync(), figma.getLocalTextStylesAsync(),
    figma.getLocalEffectStylesAsync(), figma.getLocalGridStylesAsync(),
  ]);
  return groups.flat();
}
function extraBase64(bytes: Uint8Array): string {
  let text = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    text += String.fromCharCode(...bytes.subarray(i, Math.min(i + 0x8000, bytes.length)));
  }
  return btoa(text);
}
function extraStoreArtifact(bytes: Uint8Array, mediaType: string, name: string) {
  if (bytes.byteLength > EXTRA_MAX_ARTIFACT_BYTES) throw new Error("artifact_too_large");
  while (extraArtifacts.size >= EXTRA_MAX_ARTIFACTS) {
    const first = extraArtifacts.keys().next().value as string | undefined;
    if (!first) break;
    extraArtifacts.delete(first);
  }
  const token = crypto.randomUUID();
  extraArtifacts.set(token, bytes);
  return {token, bytes: bytes.byteLength, mediaType, name};
}
function extraMediaType(format: string): string {
  return ({PNG:"image/png",JPG:"image/jpeg",SVG:"image/svg+xml",PDF:"application/pdf",
    MP4:"video/mp4",GIF:"image/gif",WEBM:"video/webm"} as Record<string,string>)[format] ?? "application/octet-stream";
}
function extraWalk(root: BaseNode, limit = MAX_TREE): BaseNode[] {
  const out: BaseNode[] = [], stack: BaseNode[] = [root];
  while (stack.length && out.length < limit) {
    const node = stack.pop()!;
    out.push(node);
    if ("children" in node) for (let i=node.children.length-1;i>=0;i--) stack.push(node.children[i]);
  }
  return out;
}
function extraDiff(a: any, b: any, limit = 500): any[] {
  const out: any[] = [];
  function walk(path: string, x: any, y: any) {
    if (out.length >= limit || JSON.stringify(x) === JSON.stringify(y)) return;
    if (x && y && typeof x === "object" && typeof y === "object" && !Array.isArray(x) && !Array.isArray(y)) {
      const keys = Array.from(new Set([...Object.keys(x), ...Object.keys(y)])).sort();
      for (const key of keys) walk(path + "/" + key, x[key], y[key]);
      return;
    }
    if (Array.isArray(x) && Array.isArray(y)) {
      for (let i=0;i<Math.max(x.length,y.length);i++) walk(path + "/" + i, x[i], y[i]);
      return;
    }
    out.push({path, kind: x === undefined ? "added" : y === undefined ? "removed" : "changed", before:x, after:y});
  }
  walk("", a, b);
  return out;
}
function extraContrastChannel(v:number){return v<=0.04045?v/12.92:Math.pow((v+0.055)/1.055,2.4)}
function extraContrast(a:{r:number,g:number,b:number},b:{r:number,g:number,b:number}){
  const lum=(c:{r:number,g:number,b:number})=>0.2126*extraContrastChannel(c.r)+0.7152*extraContrastChannel(c.g)+0.0722*extraContrastChannel(c.b);
  const x=lum(a),y=lum(b); return (Math.max(x,y)+0.05)/(Math.min(x,y)+0.05);
}
function extraSolidColor(value:any): {r:number,g:number,b:number}|null {
  if (!Array.isArray(value) || value.length !== 1 || value[0]?.type !== "SOLID") return null;
  return value[0].color ?? null;
}
async function extraCreateComposeNode(spec:any): Promise<SceneNode> {
  if (!spec || typeof spec !== "object") throw new Error("invalid_compose_node");
  const kind=String(spec.type??"").toLowerCase();
  let node: SceneNode;
  if(kind==="frame") node=figma.createFrame();
  else if(kind==="section") node=figma.createSection();
  else if(kind==="rect"||kind==="rectangle") node=figma.createRectangle();
  else if(kind==="ellipse") node=figma.createEllipse();
  else if(kind==="line") node=figma.createLine();
  else if(kind==="polygon") node=figma.createPolygon();
  else if(kind==="star") node=figma.createStar();
  else if(kind==="text"){
    const t=figma.createText(); await figma.loadFontAsync(t.fontName as FontName); t.characters=String(spec.text??"").slice(0,65536); node=t;
  } else if(kind==="instance"){
    const c=await nodeById(String(spec.componentId)); if(c.type!=="COMPONENT") throw new Error("not_component"); node=c.createInstance();
  } else throw new Error("unsupported_compose_type");
  applyBasicSceneArgs(node,spec);
  if(spec.opacity!==undefined && "opacity" in node) (node as any).opacity=Number(spec.opacity);
  if(spec.fill && "fills" in node) (node as any).fills=[solidPaint(spec.fill)];
  if(spec.layout && "layoutMode" in node) patchLayout(node,{...spec.layout,nodeId:node.id});
  const children=spec.children===undefined?[]:extraBoundedArray(spec.children,64,"compose_children_limit");
  if(children.length){
    if(!("appendChild" in node)) throw new Error("compose_parent_cannot_have_children");
    for(const childSpec of children) (node as BaseNode & ChildrenMixin).appendChild(await extraCreateComposeNode(childSpec));
  }
  return node;
}


async function handleSemanticComplete(request: BridgeRequest, a: any): Promise<BridgeResponse | null> {
  switch (request.operation) {
    case "document.snapshot": {
      await figma.loadAllPagesAsync();
      return ok(request.id, await tree(figma.root));
    }
    case "document.diff":
      return ok(request.id, {changes: extraDiff(a.before, a.after, Math.min(Number(a.limit ?? 500), 1000))});
    case "node.inspect.full":
      return ok(request.id, extraNodeJson(await nodeById(String(a.nodeId))));
    case "compose.apply": {
      const node = await extraCreateComposeNode(a.root);
      return ok(request.id, await tree(node), true);
    }
    case "compose.batch": {
      const specs = extraBoundedArray(a.roots, EXTRA_MAX_BATCH, "compose_batch_limit");
      const created: unknown[] = [];
      for (const spec of specs) created.push(summarize(await extraCreateComposeNode(spec)));
      return ok(request.id, {created}, true);
    }
    case "vector.create": {
      const node = figma.createVector();
      applyBasicSceneArgs(node,a);
      if(a.vectorNetwork) node.vectorNetwork=a.vectorNetwork as VectorNetwork;
      return ok(request.id, extraNodeJson(node), true);
    }
    case "vector.inspect": {
      const node=await nodeById(String(a.nodeId));
      if(node.type!=="VECTOR") throw new Error("not_vector");
      return ok(request.id,{...extraNodeJson(node),vectorNetwork:node.vectorNetwork,vectorPaths:node.vectorPaths});
    }
    case "vector.network.set": {
      const node=await nodeById(String(a.nodeId));
      if(node.type!=="VECTOR") throw new Error("not_vector");
      const network=a.vectorNetwork as VectorNetwork;
      if(!network || !Array.isArray(network.vertices) || network.vertices.length>10000 || !Array.isArray(network.segments) || network.segments.length>20000) throw new Error("vector_network_limit");
      node.vectorNetwork=network;
      return ok(request.id,extraNodeJson(node),true);
    }
    case "node.flatten": {
      const ids=extraBoundedArray(a.nodeIds,128);
      const nodes:BaseNode[]=[]; for(const id of ids) nodes.push(await nodeById(String(id)));
      const parent=a.parentId?await nodeById(String(a.parentId)):undefined;
      if(parent && !("appendChild" in parent)) throw new Error("parent_cannot_have_children");
      return ok(request.id,extraNodeJson(figma.flatten(nodes,parent as (BaseNode&ChildrenMixin)|undefined,a.index===undefined?undefined:Number(a.index))),true);
    }
    case "node.outline_stroke": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      if(typeof node.outlineStroke!=="function") throw new Error("outline_unavailable");
      const result=node.outlineStroke();
      return ok(request.id,result?extraNodeJson(result):null,true);
    }
    case "boolean.union":
    case "boolean.subtract":
    case "boolean.intersect":
    case "boolean.exclude": {
      const ids=extraBoundedArray(a.nodeIds,128); if(ids.length<2) throw new Error("boolean_requires_two_nodes");
      const nodes:BaseNode[]=[]; for(const id of ids) nodes.push(await nodeById(String(id)));
      const parent=a.parentId?await nodeById(String(a.parentId)):figma.currentPage;
      if(!("appendChild" in parent)) throw new Error("parent_cannot_have_children");
      const method=request.operation.split(".")[1] as "union"|"subtract"|"intersect"|"exclude";
      const result=figma[method](nodes,parent as BaseNode&ChildrenMixin,a.index===undefined?undefined:Number(a.index));
      return ok(request.id,extraNodeJson(result),true);
    }
    case "transform_group.create": {
      const ids=extraBoundedArray(a.nodeIds,128); if(ids.length===0) throw new Error("empty_transform_group");
      const nodes:SceneNode[]=[]; for(const id of ids) nodes.push(asScene(await nodeById(String(id))));
      const parent=a.parentId?await nodeById(String(a.parentId)):figma.currentPage;
      if(!("appendChild" in parent)) throw new Error("parent_cannot_have_children");
      const modifiers=extraBoundedArray(a.modifiers,128,"modifier_limit") as TransformModifier[];
      const result=figma.transformGroup(nodes,parent as BaseNode&ChildrenMixin,Number(a.index??("children" in parent?parent.children.length:0)),modifiers);
      return ok(request.id,extraNodeJson(result),true);
    }
    case "transform_group.inspect": {
      const node=await nodeById(String(a.nodeId)); if(node.type!=="TRANSFORM_GROUP") throw new Error("not_transform_group");
      return ok(request.id,{...extraNodeJson(node),transformModifiers:node.transformModifiers});
    }
    case "text.runs.inspect": {
      const node=await nodeById(String(a.nodeId)) as any;
      if(node.type!=="TEXT"&&node.type!=="TEXT_PATH") throw new Error("not_text");
      const fields=extraBoundedArray(a.fields??["fontName","fontSize","fills","textDecoration","textCase","hyperlink"],32);
      return ok(request.id,node.getStyledTextSegments(fields as any).slice(0,MAX_RESULTS));
    }
    case "text.range.patch": {
      const node=await nodeById(String(a.nodeId)) as any;
      if(node.type!=="TEXT"&&node.type!=="TEXT_PATH") throw new Error("not_text");
      const start=Number(a.start),end=Number(a.end); if(!Number.isInteger(start)||!Number.isInteger(end)||start<0||end<start||end>node.characters.length) throw new Error("invalid_text_range");
      await ensureFonts(node as TextNode);
      if(a.fontSize!==undefined) node.setRangeFontSize(start,end,Number(a.fontSize));
      if(a.fontName){await figma.loadFontAsync(a.fontName as FontName);node.setRangeFontName(start,end,a.fontName as FontName);}
      if(a.fills) node.setRangeFills(start,end,a.fills as Paint[]);
      if(a.letterSpacing) node.setRangeLetterSpacing(start,end,a.letterSpacing);
      if(a.textDecoration) node.setRangeTextDecoration(start,end,a.textDecoration);
      if(a.textCase) node.setRangeTextCase(start,end,a.textCase);
      return ok(request.id,{start,end},true);
    }
    case "text.hyperlink.set": {
      const node=await nodeById(String(a.nodeId)) as any; if(node.type!=="TEXT"&&node.type!=="TEXT_PATH") throw new Error("not_text");
      const start=Number(a.start),end=Number(a.end); node.setRangeHyperlink(start,end,a.hyperlink??null);
      return ok(request.id,{start,end,hyperlink:a.hyperlink??null},true);
    }
    case "text.variable.bind_range": {
      const node=await nodeById(String(a.nodeId)) as any; if(node.type!=="TEXT"&&node.type!=="TEXT_PATH") throw new Error("not_text");
      const variable=a.variableId?await figma.variables.getVariableByIdAsync(String(a.variableId)):null;
      if(a.variableId&&!variable) throw new Error("variable_not_found");
      node.setRangeBoundVariable(Number(a.start),Number(a.end),String(a.field) as VariableBindableTextField,variable);
      return ok(request.id,{bound:Boolean(variable)},true);
    }
    case "text.path.create": {
      const base=await nodeById(String(a.nodeId)); if(!["VECTOR","RECTANGLE","ELLIPSE","POLYGON","STAR","LINE"].includes(base.type)) throw new Error("invalid_text_path_base");
      const node=figma.createTextPath(base as VectorNode,Number(a.startSegment??0),Number(a.startPosition??0));
      await figma.loadFontAsync(node.fontName as FontName);
      if(a.characters!==undefined) node.characters=String(a.characters).slice(0,65536);
      return ok(request.id,extraNodeJson(node),true);
    }
    case "text.path.inspect": {
      const node=await nodeById(String(a.nodeId)); if(node.type!=="TEXT_PATH") throw new Error("not_text_path");
      return ok(request.id,{...extraNodeJson(node),characters:node.characters,textPathStartData:node.textPathStartData,fontName:node.fontName===figma.mixed?"MIXED":node.fontName,fontSize:node.fontSize===figma.mixed?"MIXED":node.fontSize});
    }
    case "font.list":
      return ok(request.id,(await figma.listAvailableFontsAsync()).slice(0,1000));
    case "font.variation_axes":
      return ok(request.id,{family:String(a.family),axes:figma.getFontFamilyVariationAxes(String(a.family))});


    case "style.list":
      return ok(request.id,(await extraAllStyles()).slice(0,MAX_RESULTS).map(extraStyleSummary));
    case "style.inspect": {
      const style=await figma.getStyleByIdAsync(String(a.styleId)); if(!style) throw new Error("style_not_found");
      return ok(request.id,extraStyleSummary(style));
    }
    case "style.create": {
      extraRequireEditor("figma");
      const type=String(a.styleType).toUpperCase();
      let style:BaseStyle;
      if(type==="PAINT") style=figma.createPaintStyle();
      else if(type==="TEXT") style=figma.createTextStyle();
      else if(type==="EFFECT") style=figma.createEffectStyle();
      else if(type==="GRID") style=figma.createGridStyle();
      else throw new Error("invalid_style_type");
      style.name=String(a.name).slice(0,256);
      if(a.description!==undefined) style.description=String(a.description).slice(0,4096);
      return ok(request.id,extraStyleSummary(style),true);
    }
    case "style.patch": {
      const style=await figma.getStyleByIdAsync(String(a.styleId)); if(!style||style.remote) throw new Error("local_style_not_found");
      const s=style as any;
      if(a.name!==undefined) s.name=String(a.name).slice(0,256);
      if(a.description!==undefined) s.description=String(a.description).slice(0,4096);
      if(style.type==="PAINT"&&a.paints) s.paints=extraBoundedArray(a.paints,64) as Paint[];
      if(style.type==="EFFECT"&&a.effects) s.effects=extraBoundedArray(a.effects,32) as Effect[];
      if(style.type==="GRID"&&a.layoutGrids) s.layoutGrids=extraBoundedArray(a.layoutGrids,32) as LayoutGrid[];
      if(style.type==="TEXT"){
        if(a.fontName){await figma.loadFontAsync(a.fontName as FontName);s.fontName=a.fontName}
        for(const field of ["fontSize","letterSpacing","lineHeight","leadingTrim","paragraphIndent","paragraphSpacing","textWrapStyle","listSpacing","hangingPunctuation","hangingList","textCase","textDecoration"]) if(a[field]!==undefined)s[field]=a[field];
      }
      return ok(request.id,extraStyleSummary(style),true);
    }
    case "style.apply": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      const style=await figma.getStyleByIdAsync(String(a.styleId)); if(!style) throw new Error("style_not_found");
      const field=String(a.field);
      const method=field==="fill"?"setFillStyleIdAsync":field==="stroke"?"setStrokeStyleIdAsync":field==="text"?"setTextStyleIdAsync":field==="effect"?"setEffectStyleIdAsync":field==="grid"?"setGridStyleIdAsync":null;
      if(!method||typeof node[method]!=="function") throw new Error("style_field_unavailable");
      await node[method](style.id);
      return ok(request.id,{applied:true,nodeId:node.id,styleId:style.id,field},true);
    }
    case "style.remove": {
      const style=await figma.getStyleByIdAsync(String(a.styleId)); if(!style||style.remote) throw new Error("local_style_not_found");
      style.remove(); return ok(request.id,{removed:true},true);
    }
    case "component.property.add": {
      const node=await nodeById(String(a.nodeId)); if(node.type!=="COMPONENT"&&node.type!=="COMPONENT_SET") throw new Error("not_component");
      const id=(node as any).addComponentProperty(String(a.name).slice(0,256),String(a.propertyType),a.defaultValue,a.options??{});
      return ok(request.id,{propertyName:id},true);
    }
    case "component.property.edit": {
      const node=await nodeById(String(a.nodeId)); if(node.type!=="COMPONENT"&&node.type!=="COMPONENT_SET") throw new Error("not_component");
      (node as any).editComponentProperty(String(a.propertyName),{...a.patch});
      return ok(request.id,{updated:true},true);
    }
    case "component.property.delete": {
      const node=await nodeById(String(a.nodeId)); if(node.type!=="COMPONENT"&&node.type!=="COMPONENT_SET") throw new Error("not_component");
      (node as any).deleteComponentProperty(String(a.propertyName));
      return ok(request.id,{removed:true},true);
    }
    case "component.instances.list": {
      const node=await nodeById(String(a.nodeId)); if(node.type!=="COMPONENT") throw new Error("not_component");
      return ok(request.id,(await node.getInstancesAsync()).slice(0,MAX_RESULTS).map(extraNodeJson));
    }
    case "component.description.patch": {
      const node=await nodeById(String(a.nodeId)); if(node.type!=="COMPONENT"&&node.type!=="COMPONENT_SET") throw new Error("not_component");
      node.description=String(a.description??"").slice(0,4096);
      if(a.descriptionMarkdown!==undefined) node.descriptionMarkdown=String(a.descriptionMarkdown).slice(0,16384);
      return ok(request.id,extraNodeJson(node),true);
    }
    case "instance.properties.patch": {
      const node=await nodeById(String(a.nodeId)); if(node.type!=="INSTANCE") throw new Error("not_instance");
      const props=a.properties; if(!props||typeof props!=="object"||Array.isArray(props)||Object.keys(props).length>64) throw new Error("properties_limit");
      node.setProperties(props as Record<string,string|boolean|VariableAlias>);
      return ok(request.id,await instanceSummary(node),true);
    }
    case "slot.create": {
      const component=await nodeById(String(a.componentId)); if(component.type!=="COMPONENT") throw new Error("not_component");
      const slot=component.createSlot();
      if(a.name!==undefined) slot.name=String(a.name).slice(0,256);
      return ok(request.id,extraNodeJson(slot),true);
    }
    case "slot.inspect": {
      const node=await nodeById(String(a.nodeId)); if(node.type!=="SLOT") throw new Error("not_slot");
      return ok(request.id,{...extraNodeJson(node),limitViolations:node.limitViolations,preferredValues:(node as any).preferredValues??[]});
    }
    case "slot.reset": {
      const node=await nodeById(String(a.nodeId)); if(node.type!=="SLOT") throw new Error("not_slot");
      node.resetSlot(); return ok(request.id,extraNodeJson(node),true);
    }
    case "library.variable_collections.list":
      return ok(request.id,(await figma.teamLibrary.getAvailableLibraryVariableCollectionsAsync()).slice(0,MAX_RESULTS));
    case "library.variables.list":
      return ok(request.id,(await figma.teamLibrary.getVariablesInLibraryCollectionAsync(String(a.collectionKey))).slice(0,MAX_RESULTS));
    case "library.component.import":
      return ok(request.id,await componentSummary(await figma.importComponentByKeyAsync(String(a.key))),true);
    case "library.component_set.import": {
      const node=await figma.importComponentSetByKeyAsync(String(a.key));
      return ok(request.id,{...extraNodeJson(node),variants:node.children.slice(0,MAX_RESULTS).map(summarize)},true);
    }
    case "library.style.import":
      return ok(request.id,extraStyleSummary(await figma.importStyleByKeyAsync(String(a.key))),true);
    case "library.variable.import": {
      const v=await figma.variables.importVariableByKeyAsync(String(a.key));
      return ok(request.id,{id:v.id,key:v.key,name:v.name,resolvedType:v.resolvedType,valuesByMode:v.valuesByMode},true);
    }
    case "variable.inspect": {
      const v=await figma.variables.getVariableByIdAsync(String(a.variableId)); if(!v) throw new Error("variable_not_found");
      return ok(request.id,{id:v.id,key:v.key,name:v.name,description:v.description,resolvedType:v.resolvedType,valuesByMode:v.valuesByMode,scopes:v.scopes,codeSyntax:v.codeSyntax,remote:v.remote,collectionId:v.variableCollectionId,hiddenFromPublishing:v.hiddenFromPublishing,publishStatus:await v.getPublishStatusAsync()});
    }
    case "variable.rename": {
      const v=await figma.variables.getVariableByIdAsync(String(a.variableId)); if(!v||v.remote) throw new Error("local_variable_not_found");
      v.name=String(a.name).slice(0,256); return ok(request.id,{id:v.id,name:v.name},true);
    }
    case "variable.remove": {
      const v=await figma.variables.getVariableByIdAsync(String(a.variableId)); if(!v||v.remote) throw new Error("local_variable_not_found");
      v.remove(); return ok(request.id,{removed:true},true);
    }
    case "variable.scopes.set": {
      const v=await figma.variables.getVariableByIdAsync(String(a.variableId)); if(!v||v.remote) throw new Error("local_variable_not_found");
      v.scopes=extraBoundedArray(a.scopes,32) as VariableScope[];
      return ok(request.id,{id:v.id,scopes:v.scopes},true);
    }
    case "variable.code_syntax.set": {
      const v=await figma.variables.getVariableByIdAsync(String(a.variableId)); if(!v||v.remote) throw new Error("local_variable_not_found");
      v.setVariableCodeSyntax(String(a.platform) as CodeSyntaxPlatform,String(a.value).slice(0,1024));
      return ok(request.id,{id:v.id,codeSyntax:v.codeSyntax},true);
    }
    case "variable.code_syntax.remove": {
      const v=await figma.variables.getVariableByIdAsync(String(a.variableId)); if(!v||v.remote) throw new Error("local_variable_not_found");
      v.removeVariableCodeSyntax(String(a.platform) as CodeSyntaxPlatform);
      return ok(request.id,{id:v.id,codeSyntax:v.codeSyntax},true);
    }
    case "variable.collection.inspect": {
      const c=await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId)); if(!c) throw new Error("collection_not_found");
      return ok(request.id,{id:c.id,key:c.key,name:c.name,modes:c.modes,defaultModeId:c.defaultModeId,variableIds:c.variableIds,remote:c.remote,isExtension:c.isExtension,hiddenFromPublishing:c.hiddenFromPublishing,publishStatus:await c.getPublishStatusAsync()});
    }
    case "variable.collection.rename": {
      const c=await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId)); if(!c||c.remote) throw new Error("local_collection_not_found");
      c.name=String(a.name).slice(0,256); return ok(request.id,{id:c.id,name:c.name},true);
    }
    case "variable.collection.remove": {
      const c=await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId)); if(!c||c.remote) throw new Error("local_collection_not_found");
      c.remove(); return ok(request.id,{removed:true},true);
    }
    case "mode.rename": {
      const c=await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId)); if(!c||c.remote) throw new Error("local_collection_not_found");
      c.renameMode(String(a.modeId),String(a.name).slice(0,256)); return ok(request.id,{modes:c.modes},true);
    }
    case "mode.remove": {
      const c=await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId)); if(!c||c.remote) throw new Error("local_collection_not_found");
      c.removeMode(String(a.modeId)); return ok(request.id,{modes:c.modes},true);
    }
    case "variable.mode.set_explicit": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      const c=await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId)); if(!c) throw new Error("collection_not_found");
      node.setExplicitVariableModeForCollection(c,String(a.modeId)); return ok(request.id,{set:true},true);
    }
    case "variable.mode.clear_explicit": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      const c=await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId)); if(!c) throw new Error("collection_not_found");
      node.clearExplicitVariableModeForCollection(c); return ok(request.id,{cleared:true},true);
    }


    case "design_system.import": {
      const spec=a.designSystem??a;
      const collections=extraBoundedArray(spec.collections??[],64,"collection_limit");
      const collectionByName=new Map<string,VariableCollection>();
      let createdCollections=0,createdVariables=0,createdStyles=0;
      for(const cSpec of collections){
        const c=figma.variables.createVariableCollection(String(cSpec.name).slice(0,256)); createdCollections++;
        collectionByName.set(String(cSpec.name),c);
        const modes=extraBoundedArray(cSpec.modes??[],16,"mode_limit");
        for(let i=0;i<modes.length;i++){
          if(i===0)c.renameMode(c.defaultModeId,String(modes[i].name??"Mode 1").slice(0,256));
          else c.addMode(String(modes[i].name).slice(0,256));
        }
        const vars=extraBoundedArray(cSpec.variables??[],256,"variable_limit");
        for(const vSpec of vars){
          const v=figma.variables.createVariable(String(vSpec.name).slice(0,256),c,String(vSpec.resolvedType) as VariableResolvedDataType); createdVariables++;
          const values=vSpec.valuesByMode??{};
          for(const [mode,value] of Object.entries(values)){
            const modeId=c.modes.find(m=>m.modeId===mode||m.name===mode)?.modeId;
            if(modeId)v.setValueForMode(modeId,value as VariableValue);
          }
          if(Array.isArray(vSpec.scopes))v.scopes=vSpec.scopes.slice(0,32) as VariableScope[];
        }
      }
      const styles=extraBoundedArray(spec.styles??[],128,"style_limit");
      for(const sSpec of styles){
        const type=String(sSpec.type).toUpperCase(); let s:BaseStyle;
        if(type==="PAINT"){s=figma.createPaintStyle();(s as PaintStyle).paints=extraBoundedArray(sSpec.paints??[],64) as Paint[]}
        else if(type==="EFFECT"){s=figma.createEffectStyle();(s as EffectStyle).effects=extraBoundedArray(sSpec.effects??[],32) as Effect[]}
        else if(type==="GRID"){s=figma.createGridStyle();(s as GridStyle).layoutGrids=extraBoundedArray(sSpec.layoutGrids??[],32) as LayoutGrid[]}
        else if(type==="TEXT"){const t=figma.createTextStyle();if(sSpec.fontName){await figma.loadFontAsync(sSpec.fontName as FontName);t.fontName=sSpec.fontName as FontName}if(sSpec.fontSize!==undefined)t.fontSize=Number(sSpec.fontSize);s=t}
        else throw new Error("invalid_style_type");
        s.name=String(sSpec.name).slice(0,256); createdStyles++;
      }
      return ok(request.id,{createdCollections,createdVariables,createdStyles},true);
    }
    case "prototype.validate":
      return ok(request.id,await extraValidatePrototype());
    case "validate.a11y":
      return ok(request.id,await extraValidateA11y(a));
    case "validate.layout":
      return ok(request.id,await extraValidateLayout());
    case "validate.variables":
      return ok(request.id,await extraValidateVariables());
    case "validate.components":
      return ok(request.id,await extraValidateComponents());
    case "validate.design_system":
      return ok(request.id,{a11y:await extraValidateA11y(a),layout:await extraValidateLayout(),variables:await extraValidateVariables(),components:await extraValidateComponents()});
    case "shader.list":
      return ok(request.id,(await figma.listAvailableShaders()).slice(0,MAX_RESULTS));
    case "shader.import":
      return ok(request.id,await figma.importShaderById(String(a.shaderId)),true);
    case "shader.apply_fill":
    case "shader.apply_stroke":
    case "shader.apply_effect": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      const shader=await figma.importShaderById(String(a.shaderId));
      const properties=a.properties&&typeof a.properties==="object"?a.properties:undefined;
      if(request.operation==="shader.apply_effect"){
        if(shader.type!=="effect"||!("effects" in node))throw new Error("shader_type_mismatch");
        node.effects=[...(node.effects??[]),{type:"SHADER",id:shader.id,properties} as any];
      }else{
        if(shader.type!=="fill")throw new Error("shader_type_mismatch");
        const field=request.operation.endsWith("stroke")?"strokes":"fills"; if(!(field in node))throw new Error("paint_unavailable");
        node[field]=[...(node[field]===figma.mixed?[]:node[field]),{type:"SHADER",id:shader.id,properties} as any];
      }
      return ok(request.id,{applied:true,shaderId:shader.id,nodeId:node.id},true);
    }
    case "viewport.inspect":
      return ok(request.id,{center:figma.viewport.center,zoom:figma.viewport.zoom,bounds:figma.viewport.bounds,slidesView:figma.editorType==="slides"?figma.viewport.slidesView:undefined});
    case "viewport.center":
      figma.viewport.center={x:extraFinite(a.x),y:extraFinite(a.y)}; return ok(request.id,{center:figma.viewport.center},true);
    case "viewport.zoom":
      figma.viewport.zoom=Math.max(0.01,Math.min(256,extraFinite(a.zoom))); return ok(request.id,{zoom:figma.viewport.zoom},true);
    case "viewport.fit": {
      const ids=extraBoundedArray(a.nodeIds,128);const nodes:BaseNode[]=[];for(const id of ids)nodes.push(await nodeById(String(id)));
      figma.viewport.scrollAndZoomIntoView(nodes); return ok(request.id,{count:nodes.length},true);
    }
    case "annotation.categories.list":
      return ok(request.id,(await figma.annotations.getAnnotationCategoriesAsync()).slice(0,MAX_RESULTS));
    case "annotation.category.create":
      return ok(request.id,await figma.annotations.addAnnotationCategoryAsync({label:String(a.label).slice(0,256),color:String(a.color) as AnnotationCategoryColor}),true);
    case "annotation.category.patch": {
      const c=await figma.annotations.getAnnotationCategoryByIdAsync(String(a.categoryId));if(!c)throw new Error("annotation_category_not_found");
      if(a.label!==undefined)c.setLabel(String(a.label).slice(0,256));if(a.color!==undefined)c.setColor(String(a.color) as AnnotationCategoryColor);
      return ok(request.id,{id:c.id,label:c.label,color:c.color,isPreset:c.isPreset},true);
    }
    case "annotation.category.remove": {
      const c=await figma.annotations.getAnnotationCategoryByIdAsync(String(a.categoryId));if(!c||c.isPreset)throw new Error("annotation_category_not_removable");
      c.remove(); return ok(request.id,{removed:true},true);
    }
    case "annotation.node.inspect": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;if(!("annotations" in node))throw new Error("annotations_unavailable");
      return ok(request.id,node.annotations);
    }
    case "annotation.node.set": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;if(!("annotations" in node))throw new Error("annotations_unavailable");
      node.annotations=extraBoundedArray(a.annotations,64) as Annotation[];return ok(request.id,{count:node.annotations.length},true);
    }
    case "dev.resources.list": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;if(typeof node.getDevResourcesAsync!=="function")throw new Error("dev_resources_unavailable");
      return ok(request.id,(await node.getDevResourcesAsync({includeChildren:Boolean(a.includeChildren)})).slice(0,MAX_RESULTS));
    }
    case "dev.resources.add": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;if(typeof node.addDevResourceAsync!=="function")throw new Error("dev_resources_unavailable");
      await node.addDevResourceAsync(String(a.url),a.name===undefined?undefined:String(a.name).slice(0,256));return ok(request.id,{added:true},true);
    }
    case "dev.resources.edit": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;if(typeof node.editDevResourceAsync!=="function")throw new Error("dev_resources_unavailable");
      await node.editDevResourceAsync(String(a.url),{url:a.newUrl===undefined?undefined:String(a.newUrl),name:a.name===undefined?undefined:String(a.name).slice(0,256)});return ok(request.id,{updated:true},true);
    }
    case "dev.resources.remove": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;if(typeof node.deleteDevResourceAsync!=="function")throw new Error("dev_resources_unavailable");
      await node.deleteDevResourceAsync(String(a.url));return ok(request.id,{removed:true},true);
    }
    case "selection.colors":
      return ok(request.id,figma.getSelectionColors());
    case "file.version.save":
      return ok(request.id,await figma.saveVersionHistoryAsync(String(a.title).slice(0,256),a.description===undefined?undefined:String(a.description).slice(0,4096)),true);


    case "figjam.table.create": {
      extraRequireEditor("figjam");
      const rows=Math.max(1,Math.min(50,Number(a.rows??2))),cols=Math.max(1,Math.min(50,Number(a.columns??2)));
      const node=figma.createTable(rows,cols); if(a.name!==undefined)node.name=String(a.name).slice(0,256);
      return ok(request.id,extraNodeJson(node),true);
    }
    case "figjam.link_preview.create": {
      extraRequireEditor("figjam");
      const node=await figma.createLinkPreviewAsync(String(a.url));
      return ok(request.id,extraNodeJson(node),true);
    }
    case "figjam.gif.create": {
      extraRequireEditor("figjam");
      return ok(request.id,extraNodeJson(figma.createGif(String(a.imageHash))),true);
    }
    case "figjam.timer.status":
      extraRequireEditor("figjam"); return ok(request.id,{state:figma.timer?.state,remaining:figma.timer?.remaining,total:figma.timer?.total});
    case "figjam.timer.start":
      extraRequireEditor("figjam"); figma.timer?.start(Math.max(1,Math.min(86400,Number(a.seconds)))); return ok(request.id,{state:figma.timer?.state,remaining:figma.timer?.remaining,total:figma.timer?.total},true);
    case "figjam.timer.pause":
      extraRequireEditor("figjam"); figma.timer?.pause(); return ok(request.id,{state:figma.timer?.state,remaining:figma.timer?.remaining,total:figma.timer?.total},true);
    case "figjam.timer.resume":
      extraRequireEditor("figjam"); figma.timer?.resume(); return ok(request.id,{state:figma.timer?.state,remaining:figma.timer?.remaining,total:figma.timer?.total},true);
    case "figjam.timer.stop":
      extraRequireEditor("figjam"); figma.timer?.stop(); return ok(request.id,{state:figma.timer?.state,remaining:figma.timer?.remaining,total:figma.timer?.total},true);
    case "figjam.diagram.create": {
      extraRequireEditor("figjam");
      const nodes=extraBoundedArray(a.nodes,128,"diagram_node_limit"),edges=extraBoundedArray(a.edges??[],256,"diagram_edge_limit");
      const created=new Map<string,SceneNode>();const outputNodes:unknown[]=[];
      for(const spec of nodes){
        const node=spec.kind==="sticky"?figma.createSticky():figma.createShapeWithText();
        node.name=String(spec.name??spec.id??"Node").slice(0,256);
        if("text" in node && spec.text!==undefined)(node as any).text.characters=String(spec.text).slice(0,4096);
        if(spec.x!==undefined)(node as any).x=Number(spec.x);if(spec.y!==undefined)(node as any).y=Number(spec.y);
        created.set(String(spec.id),node);outputNodes.push(summarize(node));
      }
      const outputEdges:unknown[]=[];
      for(const e of edges){
        const from=created.get(String(e.from)),to=created.get(String(e.to));if(!from||!to)throw new Error("diagram_edge_target_missing");
        const c=figma.createConnector();c.connectorStart={endpointNodeId:from.id,magnet:"AUTO"};c.connectorEnd={endpointNodeId:to.id,magnet:"AUTO"};
        outputEdges.push(summarize(c));
      }
      return ok(request.id,{nodes:outputNodes,edges:outputEdges},true);
    }
    case "canvas.grid.inspect":
      extraRequireEditor("slides","buzz"); return ok(request.id,figma.getCanvasGrid().map(row=>row.slice(0,MAX_RESULTS).map(summarize)).slice(0,MAX_RESULTS));
    case "canvas.grid.set": {
      extraRequireEditor("slides","buzz");
      const rows=extraBoundedArray(a.rows,100,"grid_row_limit");const grid:SceneNode[][]=[];
      for(const row of rows){const ids=extraBoundedArray(row,100,"grid_column_limit");const nodes:SceneNode[]=[];for(const id of ids)nodes.push(asScene(await nodeById(String(id))));grid.push(nodes)}
      figma.setCanvasGrid(grid);return ok(request.id,{rows:grid.length},true);
    }
    case "canvas.row.create":
      extraRequireEditor("slides","buzz"); return ok(request.id,extraNodeJson(figma.createCanvasRow(a.rowIndex===undefined?undefined:Number(a.rowIndex))),true);
    case "canvas.nodes.move": {
      extraRequireEditor("slides","buzz");const ids=extraBoundedArray(a.nodeIds,128).map(String);
      figma.moveNodesToCoord(ids,a.rowIndex===undefined?undefined:Number(a.rowIndex),a.columnIndex===undefined?undefined:Number(a.columnIndex));
      return ok(request.id,{moved:ids.length},true);
    }
    case "slides.slide.create": {
      extraRequireEditor("slides");const node=figma.createSlide(a.row===undefined?undefined:Number(a.row),a.column===undefined?undefined:Number(a.column));
      if(a.name!==undefined)node.name=String(a.name).slice(0,256);return ok(request.id,extraNodeJson(node),true);
    }
    case "slides.row.create":
      extraRequireEditor("slides"); return ok(request.id,extraNodeJson(figma.createSlideRow(a.row===undefined?undefined:Number(a.row))),true);
    case "slides.view.get":
      extraRequireEditor("slides"); return ok(request.id,{view:figma.viewport.slidesView});
    case "slides.view.set":
      extraRequireEditor("slides"); figma.viewport.slidesView=String(a.view) as "grid"|"single-slide";return ok(request.id,{view:figma.viewport.slidesView},true);
    case "buzz.frame.create": {
      extraRequireEditor("buzz");const node=figma.buzz.createFrame(a.row===undefined?undefined:Number(a.row),a.column===undefined?undefined:Number(a.column));
      if(a.name!==undefined)node.name=String(a.name).slice(0,256);return ok(request.id,extraNodeJson(node),true);
    }
    case "buzz.instance.create": {
      extraRequireEditor("buzz");const component=await nodeById(String(a.componentId));if(component.type!=="COMPONENT")throw new Error("not_component");
      const node=figma.buzz.createInstance(component,Number(a.row),a.column===undefined?undefined:Number(a.column));return ok(request.id,await instanceSummary(node),true);
    }
    case "buzz.asset_type.get": {
      extraRequireEditor("buzz");const node=asScene(await nodeById(String(a.nodeId)));return ok(request.id,{assetType:figma.buzz.getBuzzAssetTypeForNode(node)});
    }
    case "buzz.asset_type.set": {
      extraRequireEditor("buzz");const node=asScene(await nodeById(String(a.nodeId)));figma.buzz.setBuzzAssetTypeForNode(node,String(a.assetType) as BuzzAssetType);return ok(request.id,{assetType:figma.buzz.getBuzzAssetTypeForNode(node)},true);
    }
    case "buzz.text_content.inspect": {
      extraRequireEditor("buzz");const node=asScene(await nodeById(String(a.nodeId)));return ok(request.id,figma.buzz.getTextContent(node).slice(0,MAX_RESULTS).map((f,i)=>({index:i,value:f.value,nodeId:f.node?.id??null})));
    }
    case "buzz.text_content.set": {
      extraRequireEditor("buzz");const node=asScene(await nodeById(String(a.nodeId)));const fields=figma.buzz.getTextContent(node);const index=Number(a.index);if(!Number.isInteger(index)||index<0||index>=fields.length)throw new Error("buzz_field_index");
      await fields[index].setValueAsync(String(a.value).slice(0,65536));return ok(request.id,{index,value:fields[index].value},true);
    }
    case "buzz.media_content.inspect": {
      extraRequireEditor("buzz");const node=asScene(await nodeById(String(a.nodeId)));return ok(request.id,figma.buzz.getMediaContent(node).slice(0,MAX_RESULTS).map((f:any,i)=>({index:i,type:f.type??null,nodeId:f.node?.id??null,value:f.value??null})));
    }
    case "buzz.smart_resize": {
      extraRequireEditor("buzz");const node=asScene(await nodeById(String(a.nodeId)));figma.buzz.smartResize(node,Number(a.width),Number(a.height));return ok(request.id,extraNodeJson(node),true);
    }
    case "group.create": {
      const ids=extraBoundedArray(a.nodeIds,128);const nodes:BaseNode[]=[];for(const id of ids)nodes.push(await nodeById(String(id)));
      const parent=a.parentId?await nodeById(String(a.parentId)):figma.currentPage;
      if(!("appendChild" in parent))throw new Error("parent_cannot_have_children");
      const group=figma.group(nodes,parent as BaseNode&ChildrenMixin,a.index===undefined?undefined:Number(a.index));
      if(a.name!==undefined)group.name=String(a.name).slice(0,256);
      return ok(request.id,extraNodeJson(group),true);
    }
    case "group.ungroup": {
      const node=await nodeById(String(a.nodeId));if(node.type!=="GROUP")throw new Error("not_group");
      return ok(request.id,{children:figma.ungroup(node).slice(0,MAX_RESULTS).map(extraNodeJson)},true);
    }
    case "slice.create": {
      const node=figma.createSlice();applyBasicSceneArgs(node,a);return ok(request.id,extraNodeJson(node),true);
    }
    case "page.divider.create": {
      const page=figma.createPageDivider(a.name===undefined?undefined:String(a.name).slice(0,256));
      return ok(request.id,summarize(page),true);
    }
    case "node.bindings.inspect": {
      const node=await nodeById(String(a.nodeId)) as any;
      return ok(request.id,{boundVariables:node.boundVariables??{},componentPropertyReferences:node.componentPropertyReferences??null,explicitVariableModes:node.explicitVariableModes??{}});
    }
    case "node.properties.patch": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      const props=a.properties;if(!props||typeof props!=="object"||Array.isArray(props)||Object.keys(props).length>32)throw new Error("property_limit");
      const allowed=new Set(["name","visible","locked","opacity","x","y","rotation","clipsContent","cornerRadius","strokeWeight","strokeAlign","strokeCap","strokeJoin","dashPattern","constraints","minWidth","maxWidth","minHeight","maxHeight","layoutGrow","layoutAlign","layoutPositioning"]);
      for(const [key,value] of Object.entries(props)){
        if(!allowed.has(key)||!(key in node))throw new Error("unsupported_property");
        node[key]=value;
      }
      return ok(request.id,extraNodeJson(node),true);
    }
    case "canvas.info":
      return ok(request.id,{page:summarize(figma.currentPage),children:figma.currentPage.children.slice(0,MAX_RESULTS).map(summarize),selection:figma.currentPage.selection.slice(0,MAX_RESULTS).map(summarize)});
    case "canvas.next_position":
      return ok(request.id,extraNextCanvasPosition(Math.max(0,Number(a.gap??80))));
    case "canvas.arrange": {
      const ids=a.nodeIds?extraBoundedArray(a.nodeIds,256).map(String):figma.currentPage.selection.map(n=>n.id);
      const nodes:SceneNode[]=[];for(const id of ids)nodes.push(asScene(await nodeById(id)));
      const columns=Math.max(1,Math.min(64,Number(a.columns??Math.ceil(Math.sqrt(nodes.length||1)))));
      const gap=Math.max(0,Math.min(10000,Number(a.gap??80)));
      let x=Number(a.x??0),y=Number(a.y??0),rowH=0;
      for(let i=0;i<nodes.length;i++){
        const n=nodes[i] as any;if(i>0&&i%columns===0){x=Number(a.x??0);y+=rowH+gap;rowH=0}
        if("x" in n)n.x=x;if("y" in n)n.y=y;x+=Number(n.width??100)+gap;rowH=Math.max(rowH,Number(n.height??100));
      }
      return ok(request.id,{arranged:nodes.length,columns,gap},true);
    }
    case "slot.list": {
      const root=await nodeById(String(a.nodeId));if(!["COMPONENT","COMPONENT_SET","INSTANCE"].includes(root.type))throw new Error("slot_parent_invalid");
      const slots=extraWalk(root).filter(n=>n.type==="SLOT").slice(0,MAX_RESULTS) as SlotNode[];
      return ok(request.id,slots.map(slot=>({...extraNodeJson(slot),componentPropertyReferences:slot.componentPropertyReferences,limitViolations:slot.limitViolations,children:slot.children.slice(0,MAX_RESULTS).map(summarize)})));
    }
    case "slot.preferred.set": {
      const slot=await nodeById(String(a.nodeId));if(slot.type!=="SLOT")throw new Error("not_slot");
      let parent:BaseNode|null=slot.parent;while(parent&&parent.type!=="COMPONENT")parent=parent.parent;
      if(!parent||parent.type!=="COMPONENT")throw new Error("slot_component_not_found");
      const propertyName=(slot.componentPropertyReferences as any)?.slotContentId;if(!propertyName)throw new Error("slot_property_not_found");
      const keys=extraBoundedArray(a.componentKeys,64).map(String);
      parent.editComponentProperty(propertyName,{preferredValues:keys.map(key=>({type:"COMPONENT",key}))});
      return ok(request.id,{propertyName,preferredValues:keys},true);
    }
    case "slot.content.add": {
      const slot=await nodeById(String(a.nodeId));if(slot.type!=="SLOT")throw new Error("not_slot");
      let child:SceneNode;
      if(a.sourceNodeId){const source=asScene(await nodeById(String(a.sourceNodeId))) as any;child=asScene(source.clone() as BaseNode);}
      else if(a.componentId){const c=await nodeById(String(a.componentId));if(c.type!=="COMPONENT")throw new Error("not_component");child=c.createInstance();}
      else if(a.text!==undefined){const t=figma.createText();await figma.loadFontAsync(t.fontName as FontName);t.characters=String(a.text).slice(0,65536);child=t;}
      else {child=figma.createFrame();}
      slot.appendChild(child);return ok(request.id,extraNodeJson(child),true);
    }
    case "export.node":
    case "motion.export": {
      const node=asScene(await nodeById(String(a.nodeId))) as ExportMixin;
      const format=String(a.format??(request.operation==="motion.export"?"MP4":"PNG")).toUpperCase();
      const settings:any={format};
      if(a.fps!==undefined)settings.fps=Number(a.fps);
      if(a.quality!==undefined)settings.quality=String(a.quality);
      if(a.loopCount!==undefined)settings.loopCount=Number(a.loopCount);
      if(a.constraint)settings.constraint=a.constraint;
      const bytes=await (node as any).exportAsync(settings);
      const artifact=extraStoreArtifact(bytes,extraMediaType(format),String(a.name??("figma-export."+format.toLowerCase())));
      return ok(request.id,artifact);
    }
    case "artifact.read": {
      const bytes=extraArtifacts.get(String(a.token));if(!bytes)throw new Error("artifact_not_found");
      const offset=Math.max(0,Number(a.offset??0));const length=Math.min(EXTRA_CHUNK_BYTES,Math.max(1,Number(a.length??EXTRA_CHUNK_BYTES)));
      const chunk=bytes.subarray(offset,Math.min(offset+length,bytes.length));
      return ok(request.id,{token:String(a.token),offset,nextOffset:offset+chunk.length,totalBytes:bytes.length,eof:offset+chunk.length>=bytes.length,base64:extraBase64(chunk)});
    }
    case "artifact.release":
      return ok(request.id,{released:extraArtifacts.delete(String(a.token))},true);
    default:
      return null;
  }
}


async function extraValidatePrototype() {
  const findings:any[]=[];const nodes=extraWalk(figma.currentPage);
  const ids=new Set(nodes.map(n=>n.id));
  for(const node of nodes){
    if(!("reactions" in node))continue;
    const reactions=(node as any).reactions as any[];
    for(let i=0;i<reactions.length;i++){
      const text=JSON.stringify(reactions[i]);
      for(const match of text.matchAll(/"destinationId":"([^"]+)"/g)){
        if(!ids.has(match[1])) findings.push({rule:"prototype.dangling_destination",severity:"error",nodeId:node.id,reactionIndex:i,destinationId:match[1]});
      }
    }
  }
  return {findings:findings.slice(0,500),truncated:findings.length>500,flowStartingPoints:figma.currentPage.flowStartingPoints.slice(0,MAX_RESULTS)};
}
async function extraValidateA11y(args:any) {
  const findings:any[]=[];const minTarget=Math.max(1,Number(args.minTouchTarget??44));
  for(const node of extraWalk(figma.currentPage)){
    const n=node as any;
    if("width" in n&&"height" in n&&n.visible!==false&&(n.width<minTarget||n.height<minTarget)&&["FRAME","COMPONENT","INSTANCE","RECTANGLE","ELLIPSE"].includes(node.type)){
      findings.push({rule:"a11y.touch_target",severity:"warning",nodeId:node.id,width:n.width,height:n.height,min:minTarget});
    }
    if(node.type==="TEXT"){
      const t=node as TextNode;
      const size=t.fontSize===figma.mixed?null:Number(t.fontSize);
      if(size!==null&&size<12)findings.push({rule:"a11y.text_size",severity:"warning",nodeId:node.id,fontSize:size});
      const fg=extraSolidColor(t.fills);
      const parent=t.parent as any;const bg=parent&&"fills" in parent?extraSolidColor(parent.fills):null;
      if(fg&&bg){
        const ratio=extraContrast(fg,bg);
        const threshold=size!==null&&size>=24?3:4.5;
        if(ratio<threshold)findings.push({rule:"a11y.contrast",severity:"warning",nodeId:node.id,ratio,threshold});
      }
    }
    if(findings.length>=500)break;
  }
  return {findings,truncated:findings.length>=500};
}
async function extraValidateLayout() {
  const findings:any[]=[];
  for(const node of extraWalk(figma.currentPage)){
    const n=node as any;
    if(["FRAME","COMPONENT","COMPONENT_SET","INSTANCE"].includes(node.type)&&"children" in node&&node.children.length>1&&"layoutMode" in n&&n.layoutMode==="NONE"){
      findings.push({rule:"layout.manual_cluster",severity:"info",nodeId:node.id,children:node.children.length});
    }
    if("layoutMode" in n&&n.layoutMode!=="NONE"&&typeof n.itemSpacing==="number"&&!Number.isFinite(n.itemSpacing)){
      findings.push({rule:"layout.invalid_spacing",severity:"error",nodeId:node.id});
    }
    if(findings.length>=500)break;
  }
  return {findings,truncated:findings.length>=500};
}
async function extraValidateVariables() {
  const findings:any[]=[];const vars=await figma.variables.getLocalVariablesAsync();const collections=await figma.variables.getLocalVariableCollectionsAsync();
  const ids=new Set(vars.map(v=>v.id));
  for(const c of collections){
    for(const id of c.variableIds)if(!ids.has(id))findings.push({rule:"variables.missing_member",severity:"error",collectionId:c.id,variableId:id});
  }
  for(const v of vars){
    for(const [mode,value] of Object.entries(v.valuesByMode)){
      if(value&&typeof value==="object"&&"type" in value&&(value as any).type==="VARIABLE_ALIAS"&&!ids.has((value as any).id)){
        findings.push({rule:"variables.dangling_alias",severity:"error",variableId:v.id,modeId:mode,targetId:(value as any).id});
      }
    }
  }
  return {findings:findings.slice(0,500),truncated:findings.length>500};
}
async function extraValidateComponents() {
  const findings:any[]=[];const nodes=extraWalk(figma.currentPage);const names=new Map<string,string>();
  for(const node of nodes){
    if(node.type==="COMPONENT"){
      const key=node.name.toLowerCase();if(names.has(key))findings.push({rule:"components.duplicate_name",severity:"warning",nodeId:node.id,otherNodeId:names.get(key),name:node.name});else names.set(key,node.id);
    }
    if(node.type==="INSTANCE"&&(await node.getMainComponentAsync())===null)findings.push({rule:"components.detached_instance",severity:"warning",nodeId:node.id});
    if(node.type==="COMPONENT_SET"){
      const variants=node.children.map(c=>c.name);const unique=new Set(variants);if(unique.size!==variants.length)findings.push({rule:"components.duplicate_variant",severity:"warning",nodeId:node.id});
    }
    if(findings.length>=500)break;
  }
  return {findings,truncated:findings.length>=500};
}

const extraUploads = new Map<string,{bytes:Uint8Array;written:number;mediaType:string}>();
function extraDecodeBase64(value:string):Uint8Array{
  const raw=atob(value);const out=new Uint8Array(raw.length);
  for(let i=0;i<raw.length;i++)out[i]=raw.charCodeAt(i);return out;
}
function extraSpacingValues(root:BaseNode){
  const values:number[]=[];
  for(const node of extraWalk(root)){
    const n=node as any;
    for(const f of ["itemSpacing","paddingTop","paddingRight","paddingBottom","paddingLeft","cornerRadius"]){
      if(typeof n[f]==="number"&&Number.isFinite(n[f]))values.push(n[f]);
    }
  }
  const counts=new Map<number,number>();for(const v of values)counts.set(v,(counts.get(v)??0)+1);
  return [...counts].sort((a,b)=>b[1]-a[1]).slice(0,MAX_RESULTS).map(([value,count])=>({value,count}));
}
function extraColorUsage(root:BaseNode){
  const map=new Map<string,{color:any,count:number,nodeIds:string[]}>();
  for(const node of extraWalk(root)){
    const n=node as any;
    for(const field of ["fills","strokes"]){
      if(!Array.isArray(n[field]))continue;
      for(const paint of n[field]){
        if(paint?.type!=="SOLID"||!paint.color)continue;
        const c=paint.color;const key=[c.r,c.g,c.b,paint.opacity??1].map((x:number)=>Number(x).toFixed(4)).join(",");
        const e=map.get(key)??{color:{...c,opacity:paint.opacity??1},count:0,nodeIds:[]};e.count++;if(e.nodeIds.length<32)e.nodeIds.push(node.id);map.set(key,e);
      }
    }
  }
  return [...map.values()].sort((a,b)=>b.count-a.count).slice(0,MAX_RESULTS);
}
function extraTypographyUsage(root:BaseNode){
  const map=new Map<string,{fontName:any,fontSize:any,count:number}>();
  for(const node of extraWalk(root)){
    if(node.type!=="TEXT")continue;const n=node as TextNode;
    const font=n.fontName===figma.mixed?"MIXED":n.fontName;const size=n.fontSize===figma.mixed?"MIXED":n.fontSize;
    const key=JSON.stringify([font,size]);const e=map.get(key)??{fontName:font,fontSize:size,count:0};e.count++;map.set(key,e);
  }
  return [...map.values()].sort((a,b)=>b.count-a.count).slice(0,MAX_RESULTS);
}
function extraNextCanvasPosition(gap=80){
  const frames=figma.currentPage.children.filter(n=>"x" in n&&"width" in n) as any[];
  if(!frames.length)return{x:0,y:0};
  const maxX=Math.max(...frames.map(n=>Number(n.x)+Number(n.width)));
  const minY=Math.min(...frames.map(n=>Number(n.y)));
  return{x:maxX+gap,y:minY};
}


function extraLintDocument() {
  const findings:any[]=[];
  function add(rule:string,severity:string,node:BaseNode,message:string){if(findings.length<1000)findings.push({rule,severity,nodeId:node.id,message})}
  function walk(node:BaseNode,depth:number){
    const n=node as any;
    if(depth>12)add("no-deeply-nested","warning",node,`node depth ${depth} exceeds 12`);
    if(/^(Frame|Rectangle|Ellipse|Group|Component|Instance)( \d+)?$/.test(node.name))add("no-default-names","info",node,"default-generated layer name");
    if(["FRAME","COMPONENT","SECTION"].includes(node.type)&&"children" in node&&node.children.length===0)add("no-empty-frames","info",node,"container has no children");
    if(["FRAME","COMPONENT","INSTANCE"].includes(node.type)&&"children" in node&&node.children.length>2&&n.layoutMode==="NONE")add("prefer-auto-layout","info",node,"multi-child container uses manual layout");
    if(Array.isArray(n.fills)&&n.fills.some((p:any)=>p?.type==="SOLID")&&!n.boundVariables?.fills)add("no-hardcoded-colors","info",node,"solid fill is not variable-bound");
    if(["FRAME","COMPONENT","INSTANCE"].includes(node.type)&&n.visible!==false&&typeof n.width==="number"&&typeof n.height==="number"&&(n.width<44||n.height<44)){
      add("touch-target-size","warning",node,`interactive-sized node is ${n.width}×${n.height}; recommended minimum is 44×44`);
    }
    if(node.type==="TEXT"){
      const text=node as TextNode;
      const size=text.fontSize===figma.mixed?null:Number(text.fontSize);
      if(size!==null&&size<12)add("min-text-size","warning",node,`font size ${size} is below 12`);
      const fg=extraSolidColor(text.fills);
      const parent=text.parent as any;
      const bg=parent&&"fills" in parent?extraSolidColor(parent.fills):null;
      if(fg&&bg){
        const ratio=extraContrast(fg,bg);
        const threshold=size!==null&&size>=24?3:4.5;
        if(ratio<threshold)add("color-contrast","warning",node,`contrast ${ratio.toFixed(2)} is below ${threshold}`);
      }
    }
    if("children" in node)for(const child of node.children)walk(child,depth+1);
  }
  walk(figma.currentPage,0);
  return findings;
}
function extraClusterAnalysis() {
  const groups=new Map<string,{signature:string,count:number,nodeIds:string[]}>();
  for(const node of extraWalk(figma.currentPage)){
    const n=node as any;if(!("width" in n)||!("height" in n))continue;
    const sig=[node.type,Math.round(Number(n.width)/8)*8,Math.round(Number(n.height)/8)*8,n.layoutMode??"NONE",("children" in node?node.children.length:0)].join("|");
    const e=groups.get(sig)??{signature:sig,count:0,nodeIds:[]};e.count++;if(e.nodeIds.length<64)e.nodeIds.push(node.id);groups.set(sig,e);
  }
  return [...groups.values()].filter(x=>x.count>1).sort((a,b)=>b.count-a.count).slice(0,MAX_RESULTS);
}
function extraScaleNode(node:SceneNode,ratio:number,depth=0){
  if(depth>12)return;const n=node as any;
  if("resize" in n&&Number.isFinite(n.width)&&Number.isFinite(n.height))n.resize(Math.max(1,n.width*ratio),Math.max(1,n.height*ratio));
  if("children" in node){
    for(const child of node.children) extraScaleNode(child as SceneNode,ratio,depth+1);
  }
}
