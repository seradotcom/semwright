function moreDecodeBase64(value: unknown): Uint8Array {
  const text = String(value ?? "");
  if (text.length > 300_000) throw new Error("artifact_chunk_too_large");
  const binary = atob(text);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i);
  return bytes;
}
function moreArtifact(token: unknown): Uint8Array {
  const bytes = extraArtifacts.get(String(token));
  if (!bytes) throw new Error("artifact_not_found");
  return bytes;
}
async function moreTable(value: unknown): Promise<TableNode> {
  const node = await nodeById(String(value));
  if (node.type !== "TABLE") throw new Error("not_table");
  return node;
}
function moreTableSummary(node: TableNode) {
  return {...extraNodeJson(node), numRows: node.numRows, numColumns: node.numColumns};
}
function morePermutationAxes(axes: unknown): Array<Record<string,string>> {
  if (!axes || typeof axes !== "object" || Array.isArray(axes)) throw new Error("invalid_variant_axes");
  let rows: Array<Record<string,string>> = [{}];
  for (const [name, raw] of Object.entries(axes as Record<string,unknown>)) {
    const values = extraBoundedArray(raw, 16, "variant_axis_limit").map(String);
    const next: Array<Record<string,string>> = [];
    for (const row of rows) for (const value of values) next.push({...row, [name]: value});
    rows = next;
    if (rows.length > 64) throw new Error("variant_matrix_limit");
  }
  return rows;
}
async function handleSemanticMore(request: BridgeRequest, a: any): Promise<BridgeResponse | null> {
  switch (request.operation) {
    case "group.create": {
      const ids=extraBoundedArray(a.nodeIds,128); if(!ids.length) throw new Error("empty_group");
      const nodes:BaseNode[]=[]; for(const id of ids) nodes.push(await nodeById(String(id)));
      const parent=a.parentId?await nodeById(String(a.parentId)):figma.currentPage;
      if(!("appendChild" in parent)) throw new Error("parent_cannot_have_children");
      return ok(request.id,extraNodeJson(figma.group(nodes,parent as BaseNode&ChildrenMixin,a.index===undefined?undefined:Number(a.index))),true);
    }
    case "group.ungroup": {
      const node=await nodeById(String(a.nodeId)); if(!("children" in node)||!("visible" in node)) throw new Error("not_groupable");
      return ok(request.id,figma.ungroup(node as SceneNode&ChildrenMixin).slice(0,MAX_RESULTS).map(extraNodeJson),true);
    }
    case "slice.create": {
      const node=figma.createSlice(); applyBasicSceneArgs(node,a); return ok(request.id,extraNodeJson(node),true);
    }
    case "page.divider.create": {
      const name=a.name===undefined?undefined:String(a.name);
      return ok(request.id,summarize(figma.createPageDivider(name)),true);
    }
    case "file.thumbnail.get": {
      const node=await figma.getFileThumbnailNodeAsync(); return ok(request.id,node?extraNodeJson(node):null);
    }
    case "file.thumbnail.set": {
      const node=a.nodeId===null||a.nodeId===undefined?null:await nodeById(String(a.nodeId));
      if(node && !["FRAME","COMPONENT","COMPONENT_SET","SECTION"].includes(node.type)) throw new Error("invalid_thumbnail_node");
      await figma.setFileThumbnailNodeAsync(node as FrameNode|ComponentNode|ComponentSetNode|SectionNode|null);
      return ok(request.id,{set:true,nodeId:node?.id??null},true);
    }
    case "motion.playhead.get":
      return ok(request.id,{position:figma.motion.playheadPosition??null});
    case "history.commit":
      figma.commitUndo(); return ok(request.id,{committed:true},true);
    case "history.undo":
      figma.triggerUndo(); return ok(request.id,{triggered:true},true);
    case "brush.load": {
      const brushType=String(a.brushType) as "STRETCH"|"SCATTER";
      await figma.loadBrushesAsync(brushType); return ok(request.id,{loaded:true,brushType});
    }
    case "artifact.upload.begin": {
      while(extraArtifacts.size>=EXTRA_MAX_ARTIFACTS){const first=extraArtifacts.keys().next().value as string|undefined;if(!first)break;extraArtifacts.delete(first);}
      const token=crypto.randomUUID(); extraArtifacts.set(token,new Uint8Array());
      return ok(request.id,{token,bytes:0,mediaType:String(a.mediaType??"application/octet-stream"),name:String(a.name??"upload.bin").slice(0,256)});
    }
    case "artifact.upload.append": {
      const token=String(a.token), current=moreArtifact(token), offset=Number(a.offset);
      if(!Number.isInteger(offset)||offset!==current.length) throw new Error("artifact_offset_conflict");
      const chunk=moreDecodeBase64(a.base64); if(chunk.length>EXTRA_CHUNK_BYTES||current.length+chunk.length>EXTRA_MAX_ARTIFACT_BYTES) throw new Error("artifact_limit");
      const next=new Uint8Array(current.length+chunk.length); next.set(current); next.set(chunk,current.length); extraArtifacts.set(token,next);
      return ok(request.id,{token,bytes:next.length,nextOffset:next.length});
    }
    case "artifact.status": {
      const bytes=moreArtifact(a.token); return ok(request.id,{token:String(a.token),bytes:bytes.length});
    }
    case "image.create": {
      const image=figma.createImage(moreArtifact(a.token)); const size=await image.getSizeAsync();
      return ok(request.id,{hash:image.hash,width:size.width,height:size.height},true);
    }
    case "image.inspect": {
      const image=figma.getImageByHash(String(a.hash)); if(!image) throw new Error("image_not_found");
      const size=await image.getSizeAsync(); return ok(request.id,{hash:image.hash,width:size.width,height:size.height});
    }
    case "image.export": {
      const image=figma.getImageByHash(String(a.hash)); if(!image) throw new Error("image_not_found");
      const bytes=await image.getBytesAsync(); return ok(request.id,extraStoreArtifact(bytes,String(a.mediaType??"application/octet-stream"),String(a.name??"figma-image.bin")));
    }
    case "video.create": {
      const video=await figma.createVideoAsync(moreArtifact(a.token)); return ok(request.id,{hash:video.hash},true);
    }
    case "media.fill.apply": {
      const node=asScene(await nodeById(String(a.nodeId))) as any; if(!("fills" in node)) throw new Error("fills_unavailable");
      const scaleMode=String(a.scaleMode??"FILL") as "FILL"|"FIT"|"CROP"|"TILE";
      const paint:Paint=String(a.mediaType)==="VIDEO"
        ? {type:"VIDEO",videoHash:String(a.hash),scaleMode} as VideoPaint
        : {type:"IMAGE",imageHash:String(a.hash),scaleMode} as ImagePaint;
      node.fills=[paint]; return ok(request.id,{applied:true,nodeId:node.id,hash:String(a.hash)},true);
    }
    case "buzz.media_content.set": {
      extraRequireEditor("buzz"); const node=asScene(await nodeById(String(a.nodeId))); const fields=figma.buzz.getMediaContent(node);
      const index=Number(a.index); if(!Number.isInteger(index)||index<0||index>=fields.length) throw new Error("buzz_field_index");
      const scaleMode=String(a.scaleMode??"FILL") as "FILL"|"FIT"|"CROP"|"TILE";
      const paint=String(a.mediaType)==="VIDEO"?{type:"VIDEO",videoHash:String(a.hash),scaleMode} as VideoPaint:{type:"IMAGE",imageHash:String(a.hash),scaleMode} as ImagePaint;
      await fields[index].setMediaAsync(paint); return ok(request.id,{updated:true,index},true);
    }
    case "component.variant_matrix.create": {
      const base=await nodeById(String(a.componentId)); if(base.type!=="COMPONENT") throw new Error("not_component");
      const permutations=morePermutationAxes(a.axes); if(!permutations.length) throw new Error("empty_variant_matrix");
      const clones:ComponentNode[]=[]; for(const props of permutations){const c=base.clone();c.name=Object.entries(props).map(([k,v])=>`${k}=${v}`).join(", ");clones.push(c);}
      const set=figma.combineAsVariants(clones,figma.currentPage); if(a.name!==undefined)set.name=String(a.name).slice(0,256);
      return ok(request.id,{...extraNodeJson(set),variants:set.children.slice(0,64).map(extraNodeJson)},true);
    }
    case "component.size_variants.create": {
      const base=await nodeById(String(a.componentId)); if(base.type!=="COMPONENT") throw new Error("not_component");
      const sizes=extraBoundedArray(a.sizes,32,"size_variant_limit"); const clones:ComponentNode[]=[];
      for(const spec of sizes){const c=base.clone();const w=extraFinite(spec.width),h=extraFinite(spec.height);if(w<=0||h<=0)throw new Error("invalid_size");c.resize(w,h);c.name=`Size=${String(spec.name).slice(0,128)}`;clones.push(c);}
      const set=figma.combineAsVariants(clones,figma.currentPage);if(a.name!==undefined)set.name=String(a.name).slice(0,256);
      return ok(request.id,{...extraNodeJson(set),variants:set.children.slice(0,32).map(extraNodeJson)},true);
    }
    case "node.plugin_data.get": {
      const node=await nodeById(String(a.nodeId)); const key=`semwright:${String(a.key).slice(0,128)}`; return ok(request.id,{key:String(a.key),value:node.getPluginData(key)});
    }
    case "node.plugin_data.set": {
      const node=await nodeById(String(a.nodeId));const key=`semwright:${String(a.key).slice(0,128)}`;const value=String(a.value??"");
      if(value.length>32_768)throw new Error("plugin_data_limit");node.setPluginData(key,value);return ok(request.id,{key:String(a.key),bytes:value.length},true);
    }
    case "node.relaunch_data.get": {
      const node=await nodeById(String(a.nodeId));return ok(request.id,node.getRelaunchData());
    }
    case "node.relaunch_data.set": {
      const node=await nodeById(String(a.nodeId));const data=a.data as Record<string,string>;if(!data||typeof data!=="object"||Array.isArray(data)||Object.keys(data).length>32)throw new Error("relaunch_data_limit");
      const clean:Record<string,string>={};for(const [k,v] of Object.entries(data)){if(k.length>128||String(v).length>256)throw new Error("relaunch_data_limit");clean[k]=String(v)}node.setRelaunchData(clean);
      return ok(request.id,{count:Object.keys(clean).length},true);
    }
    case "figjam.table.inspect": {
      extraRequireEditor("figjam");return ok(request.id,moreTableSummary(await moreTable(a.nodeId)));
    }
    case "figjam.table.cell.inspect": {
      extraRequireEditor("figjam");const table=await moreTable(a.nodeId);const cell=table.cellAt(Number(a.row),Number(a.column));
      return ok(request.id,{rowIndex:cell.rowIndex,columnIndex:cell.columnIndex,width:cell.width,height:cell.height,characters:(cell.text as any).characters??""});
    }
    case "figjam.table.cell.text.set": {
      extraRequireEditor("figjam");const table=await moreTable(a.nodeId);const cell=table.cellAt(Number(a.row),Number(a.column));const text=cell.text as any;
      if(text.fontName&&text.fontName!==figma.mixed)await figma.loadFontAsync(text.fontName as FontName);text.characters=String(a.characters).slice(0,65_536);
      return ok(request.id,{rowIndex:cell.rowIndex,columnIndex:cell.columnIndex,characters:text.characters},true);
    }
    case "figjam.table.row.insert": {extraRequireEditor("figjam");const t=await moreTable(a.nodeId);t.insertRow(Number(a.index));return ok(request.id,moreTableSummary(t),true);}
    case "figjam.table.row.remove": {extraRequireEditor("figjam");const t=await moreTable(a.nodeId);t.removeRow(Number(a.index));return ok(request.id,moreTableSummary(t),true);}
    case "figjam.table.column.insert": {extraRequireEditor("figjam");const t=await moreTable(a.nodeId);t.insertColumn(Number(a.index));return ok(request.id,moreTableSummary(t),true);}
    case "figjam.table.column.remove": {extraRequireEditor("figjam");const t=await moreTable(a.nodeId);t.removeColumn(Number(a.index));return ok(request.id,moreTableSummary(t),true);}
    case "figjam.table.row.move": {extraRequireEditor("figjam");const t=await moreTable(a.nodeId);t.moveRow(Number(a.from),Number(a.to));return ok(request.id,moreTableSummary(t),true);}
    case "figjam.table.column.move": {extraRequireEditor("figjam");const t=await moreTable(a.nodeId);t.moveColumn(Number(a.from),Number(a.to));return ok(request.id,moreTableSummary(t),true);}
    case "figjam.table.row.resize": {extraRequireEditor("figjam");const t=await moreTable(a.nodeId);t.resizeRow(Number(a.index),extraFinite(a.size));return ok(request.id,moreTableSummary(t),true);}
    case "figjam.table.column.resize": {extraRequireEditor("figjam");const t=await moreTable(a.nodeId);t.resizeColumn(Number(a.index),extraFinite(a.size));return ok(request.id,moreTableSummary(t),true);}
    default:
      return null;
  }
}
