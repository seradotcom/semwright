function saRequireEditor(...types:string[]):void{
  if(!types.includes(figma.editorType))throw new Error("unsupported_editor");
}
function saStyleType(style:BaseStyle):"PAINT"|"TEXT"|"EFFECT"|"GRID"{
  if(!["PAINT","TEXT","EFFECT","GRID"].includes(style.type))throw new Error("unsupported_style_type");
  return style.type as "PAINT"|"TEXT"|"EFFECT"|"GRID";
}
async function saLocalStyle(id:string):Promise<BaseStyle>{
  const style=await figma.getStyleByIdAsync(id);
  if(!style)throw new Error("style_not_found");
  if(style.remote)throw new Error("remote_style_read_only");
  return style;
}
function saCategory(category:AnnotationCategory|null){
  return category?{id:category.id,label:category.label,color:category.color,isPreset:category.isPreset}:null;
}
function saUser(user:User|null){
  return user?{id:user.id,name:user.name,photoUrl:user.photoUrl,color:user.color,sessionId:user.sessionId}:null;
}
function saActiveUser(user:ActiveUser){
  return {...saUser(user),position:user.position,viewport:user.viewport,selection:user.selection.slice(0,512)};
}
function saTextReview():TextReviewAPI{
  if(!figma.textreview)throw new Error("textreview_manifest_required");
  return figma.textreview;
}
function saPayments():PaymentsAPI{
  if(!figma.payments)throw new Error("payments_unavailable");
  return figma.payments;
}
function saMeasurement(m:Measurement){
  return {id:m.id,start:{node:summarize(m.start.node),side:m.start.side},end:{node:summarize(m.end.node),side:m.end.side},offset:m.offset,freeText:m.freeText};
}
function saMeasurementAxis(side:string):"H"|"V"{
  if(side==="LEFT"||side==="RIGHT")return "H";
  if(side==="TOP"||side==="BOTTOM")return "V";
  throw new Error("invalid_measurement_side");
}
async function saDataTarget(kind:string,id:string):Promise<any>{
  if(kind==="NODE"){const v=await figma.getNodeByIdAsync(id);if(v)return v;}
  else if(kind==="STYLE"){const v=await figma.getStyleByIdAsync(id);if(v)return v;}
  else if(kind==="VARIABLE"){const v=await figma.variables.getVariableByIdAsync(id);if(v)return v;}
  else if(kind==="COLLECTION"){const v=await figma.variables.getVariableCollectionByIdAsync(id);if(v)return v;}
  throw new Error("semantic_target_not_found");
}
async function handleSemanticAdmin(request:BridgeRequest,a:any):Promise<BridgeResponse|null>{
  switch(request.operation){
    case "style.order.after": {
      saRequireEditor("figma");
      const target=await saLocalStyle(String(a.styleId));
      const reference=a.referenceStyleId==null?null:await saLocalStyle(String(a.referenceStyleId));
      const type=saStyleType(target);
      if(reference&&saStyleType(reference)!==type)throw new Error("style_type_mismatch");
      if(type==="PAINT")figma.moveLocalPaintStyleAfter(target as PaintStyle,reference as PaintStyle|null);
      else if(type==="TEXT")figma.moveLocalTextStyleAfter(target as TextStyle,reference as TextStyle|null);
      else if(type==="EFFECT")figma.moveLocalEffectStyleAfter(target as EffectStyle,reference as EffectStyle|null);
      else figma.moveLocalGridStyleAfter(target as GridStyle,reference as GridStyle|null);
      return ok(request.id,{moved:true,targetId:target.id,referenceId:reference?.id??null},true);
    }
    case "style.folder.order.after": {
      saRequireEditor("figma");
      const type=String(a.styleType) as "PAINT"|"TEXT"|"EFFECT"|"GRID";
      const target=String(a.targetFolder),reference=a.referenceFolder==null?null:String(a.referenceFolder);
      if(type==="PAINT")figma.moveLocalPaintFolderAfter(target,reference);
      else if(type==="TEXT")figma.moveLocalTextFolderAfter(target,reference);
      else if(type==="EFFECT")figma.moveLocalEffectFolderAfter(target,reference);
      else figma.moveLocalGridFolderAfter(target,reference);
      return ok(request.id,{moved:true,styleType:type,targetFolder:target,referenceFolder:reference},true);
    }
    case "slides.grid.inspect": {
      saRequireEditor("slides");
      return ok(request.id,figma.getSlideGrid().slice(0,100).map(row=>row.slice(0,100).map(summarize)));
    }
    case "slides.grid.set": {
      saRequireEditor("slides");
      const rows=extraBoundedArray(a.rows,100,"slide_grid_row_limit");
      const grid:SlideNode[][]=[];
      let slides=0;
      for(const rawRow of rows){
        const ids=extraBoundedArray(rawRow,100,"slide_grid_column_limit");
        const row:SlideNode[]=[];
        for(const id of ids){
          const node=await nodeById(String(id));
          if(node.type!=="SLIDE")throw new Error("slide_grid_requires_slides");
          row.push(node);slides++;
        }
        grid.push(row);
      }
      figma.setSlideGrid(grid);
      return ok(request.id,{rows:grid.length,slides},true);
    }
    case "annotation.category.inspect":
      return ok(request.id,saCategory(await figma.annotations.getAnnotationCategoryByIdAsync(String(a.id))));
    case "font.load": {
      const font=spFontNameInput(a);
      await figma.loadFontAsync(font);
      return ok(request.id,{
        loaded:true,
        family:font.family,
        style:font.style??null,
        variationSettings:font.variationSettings??null,
      });
    }
    case "dev.focused_node": {
      saRequireEditor("dev","slides","buzz");
      const focused=(figma.currentPage as any).focusedNode as SceneNode|null|undefined;
      return ok(request.id,focused?summarize(focused):null);
    }
    case "codegen.status": {
      saRequireEditor("dev");
      const p=figma.codegen.preferences;
      return ok(request.id,{
        editorType:"dev",mode:figma.mode,
        preferences:{unit:p.unit,scaleFactor:p.scaleFactor??null,customSettings:{...p.customSettings}}
      });
    }
    case "codegen.refresh":
      saRequireEditor("dev");figma.codegen.refresh();return ok(request.id,{refreshed:true},true);
    case "payments.status": {
      const payments=saPayments();
      return ok(request.id,{type:payments.status.type});
    }
    case "payments.first_run_age": {
      const payments=saPayments();
      return ok(request.id,{seconds:payments.getUserFirstRanSecondsAgo()});
    }
    case "payments.checkout": {
      const payments=saPayments();
      const interstitial=a.interstitial===undefined
        ? undefined
        : String(a.interstitial) as "PAID_FEATURE"|"TRIAL_ENDED"|"SKIP";
      await payments.initiateCheckoutAsync(interstitial===undefined?undefined:{interstitial});
      return ok(request.id,{type:payments.status.type},true);
    }
    case "payments.checkout.request": {
      const payments=saPayments();
      payments.requestCheckout();
      return ok(request.id,{requested:true},true);
    }
    case "payments.dev.status.set": {
      const payments=saPayments();
      const type=String(a.status) as PaymentStatus["type"];
      payments.setPaymentStatusInDevelopment({type});
      return ok(request.id,{type:payments.status.type},true);
    }
    case "user.current": {
      let user:User|null;
      try{user=figma.currentUser;}catch{throw new Error("permission_currentuser_required");}
      return ok(request.id,saUser(user));
    }
    case "figjam.active_users": {
      saRequireEditor("figjam");
      let users:readonly ActiveUser[];
      try{users=figma.activeUsers;}catch{throw new Error("permission_activeusers_required");}
      return ok(request.id,users.slice(0,128).map(saActiveUser));
    }
    case "textreview.status": {
      const api=saTextReview();
      return ok(request.id,{enabled:api.isEnabled});
    }
    case "textreview.enable": {
      const api=saTextReview();
      if(!api.isEnabled)await api.requestToBeEnabledAsync();
      return ok(request.id,{enabled:api.isEnabled},true);
    }
    case "textreview.disable": {
      const api=saTextReview();
      if(api.isEnabled)await api.requestToBeDisabledAsync();
      return ok(request.id,{enabled:api.isEnabled},true);
    }
    case "node.top_level_frame": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      if(typeof node.getTopLevelFrame!=="function")throw new Error("top_level_frame_unavailable");
      const frame=node.getTopLevelFrame() as FrameNode|undefined;
      return ok(request.id,frame?summarize(frame):null);
    }
    case "node.plugin_data.keys": {
      const node=await nodeById(String(a.nodeId)) as BaseNode&PluginDataMixin;
      return ok(request.id,node.getPluginDataKeys().slice(0,1024));
    }
    case "node.shared_plugin_data.get": {
      const node=await nodeById(String(a.nodeId)) as BaseNode&PluginDataMixin;
      const namespace=String(a.namespace),key=String(a.key);
      if(!/^[A-Za-z0-9]{3,128}$/.test(namespace))throw new Error("invalid_shared_namespace");
      return ok(request.id,{namespace,key,value:node.getSharedPluginData(namespace,key)});
    }
    case "node.shared_plugin_data.keys": {
      const node=await nodeById(String(a.nodeId)) as BaseNode&PluginDataMixin;
      const namespace=String(a.namespace);
      if(!/^[A-Za-z0-9]{3,128}$/.test(namespace))throw new Error("invalid_shared_namespace");
      return ok(request.id,node.getSharedPluginDataKeys(namespace).slice(0,1024));
    }
    case "node.shared_plugin_data.set": {
      const node=await nodeById(String(a.nodeId)) as BaseNode&PluginDataMixin;
      const namespace=String(a.namespace),key=String(a.key),value=String(a.value);
      if(!/^[A-Za-z0-9]{3,128}$/.test(namespace))throw new Error("invalid_shared_namespace");
      const bytes=new TextEncoder().encode(namespace+key+value).byteLength;
      if(bytes>100000)throw new Error("shared_plugin_data_limit");
      node.setSharedPluginData(namespace,key,value);
      return ok(request.id,{stored:value.length>0,removed:value.length===0,bytes},true);
    }
    case "library.publish_status.inspect": {
      const id=String(a.targetId),kind=String(a.targetKind??"AUTO");
      let target:any=null,targetKind:"NODE"|"STYLE"|"VARIABLE"|"COLLECTION"="NODE";
      if(kind==="AUTO"||kind==="NODE"){target=await figma.getNodeByIdAsync(id);targetKind="NODE";if(target&&typeof target.getPublishStatusAsync!=="function")target=null;}
      if(!target&&(kind==="AUTO"||kind==="STYLE")){target=await figma.getStyleByIdAsync(id);targetKind="STYLE";}
      if(!target&&(kind==="AUTO"||kind==="VARIABLE")){target=await figma.variables.getVariableByIdAsync(id);targetKind="VARIABLE";}
      if(!target&&(kind==="AUTO"||kind==="COLLECTION")){target=await figma.variables.getVariableCollectionByIdAsync(id);targetKind="COLLECTION";}
      if(!target||typeof target.getPublishStatusAsync!=="function")throw new Error("publishable_not_found");
      return ok(request.id,{targetKind,targetId:id,status:await target.getPublishStatusAsync()});
    }
    case "figjam.stamp.author.inspect": {
      saRequireEditor("figjam");
      const node=await nodeById(String(a.nodeId));if(node.type!=="STAMP")throw new Error("not_stamp");
      const user=await node.getAuthorAsync();
      return ok(request.id,user?{id:user.id,name:user.name,photoUrl:user.photoUrl}:null);
    }
    case "node.resize_unconstrained": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      if(typeof node.resizeWithoutConstraints!=="function")throw new Error("resize_unavailable");
      const width=Number(a.width),height=Number(a.height);
      if(!Number.isFinite(width)||!Number.isFinite(height)||width<0.01||height<0.01)throw new Error("invalid_size");
      node.resizeWithoutConstraints(width,height);
      return ok(request.id,summarize(node),true);
    }
    case "node.aspect_ratio.lock":
    case "node.aspect_ratio.unlock": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      const lock=request.operation.endsWith(".lock");
      const fn=lock?node.lockAspectRatio:node.unlockAspectRatio;
      if(typeof fn!=="function")throw new Error("aspect_ratio_unavailable");
      fn.call(node);
      return ok(request.id,{nodeId:node.id,targetAspectRatio:node.targetAspectRatio??null},true);
    }
    case "instance.overrides.remove_all": {
      const node=await nodeById(String(a.nodeId));if(node.type!=="INSTANCE")throw new Error("not_instance");
      const removed=node.overrides.length;
      node.removeOverrides();
      return ok(request.id,{nodeId:node.id,removed},true);
    }
    case "layout.grid.rows.reorder":
    case "layout.grid.columns.reorder": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      if(node.layoutMode!=="GRID")throw new Error("not_grid_layout");
      const fromIndices=extraBoundedArray(a.fromIndices,1000,"grid_reorder_limit").map(Number);
      if(fromIndices.some((v:number)=>!Number.isInteger(v)||v<0))throw new Error("invalid_grid_index");
      const insertionIndex=Number(a.insertionIndex);
      if(!Number.isInteger(insertionIndex)||insertionIndex<0)throw new Error("invalid_grid_index");
      const result=request.operation.endsWith("rows.reorder")
        ? node.reorderRows({fromIndices,insertionIndex})
        : node.reorderColumns({fromIndices,insertionIndex});
      return ok(request.id,result,true);
    }
    case "layout.grid.child.position": {
      const node=asScene(await nodeById(String(a.nodeId))) as any;
      if(typeof node.setGridChildPosition!=="function")throw new Error("grid_child_position_unavailable");
      const rowIndex=Number(a.rowIndex),columnIndex=Number(a.columnIndex);
      if(!Number.isInteger(rowIndex)||!Number.isInteger(columnIndex)||rowIndex<0||columnIndex<0)throw new Error("invalid_grid_index");
      node.setGridChildPosition(rowIndex,columnIndex);
      return ok(request.id,{nodeId:node.id,rowIndex,columnIndex},true);
    }
    case "slides.transition.inspect": {
      saRequireEditor("slides");
      const node=await nodeById(String(a.nodeId));if(node.type!=="SLIDE")throw new Error("not_slide");
      return ok(request.id,node.getSlideTransition());
    }
    case "slides.transition.set": {
      saRequireEditor("slides");
      const node=await nodeById(String(a.nodeId));if(node.type!=="SLIDE")throw new Error("not_slide");
      node.setSlideTransition(a.transition as SlideTransition);
      return ok(request.id,{nodeId:node.id,transition:node.getSlideTransition()},true);
    }
    case "widget.find_by_widget_id": {
      const root=a.rootNodeId?await nodeById(String(a.rootNodeId)):figma.root;
      if(!a.rootNodeId)await figma.loadAllPagesAsync();
      if(root.type==="PAGE"&&"loadAsync" in root)await root.loadAsync();
      if(!("findWidgetNodesByWidgetId" in root)||typeof (root as any).findWidgetNodesByWidgetId!=="function")throw new Error("widget_search_unavailable");
      const limit=Math.min(500,Math.max(0,Number(a.limit??100)));
      const nodes=(root as any).findWidgetNodesByWidgetId(String(a.widgetId)) as WidgetNode[];
      return ok(request.id,nodes.slice(0,limit).map(summarize));
    }
    case "dev.measurement.list": {
      saRequireEditor("dev");
      return ok(request.id,figma.currentPage.getMeasurements().slice(0,1000).map(saMeasurement));
    }
    case "dev.measurement.for_node": {
      saRequireEditor("dev");
      const node=asScene(await nodeById(String(a.nodeId)));
      return ok(request.id,figma.currentPage.getMeasurementsForNode(node).slice(0,1000).map(saMeasurement));
    }
    case "dev.measurement.add": {
      saRequireEditor("dev");
      const startNode=asScene(await nodeById(String(a.startNodeId))),endNode=asScene(await nodeById(String(a.endNodeId)));
      const startSide=String(a.startSide) as MeasurementSide,endSide=String(a.endSide) as MeasurementSide;
      if(saMeasurementAxis(startSide)!==saMeasurementAxis(endSide))throw new Error("measurement_axis_mismatch");
      const options:{offset?:MeasurementOffset;freeText?:string}={};
      if(a.offset!==undefined)options.offset=a.offset as MeasurementOffset;
      if(a.freeText!==undefined)options.freeText=String(a.freeText).slice(0,4096);
      return ok(request.id,saMeasurement(figma.currentPage.addMeasurement({node:startNode,side:startSide},{node:endNode,side:endSide},options)),true);
    }
    case "dev.measurement.edit": {
      saRequireEditor("dev");
      const value:{offset?:MeasurementOffset;freeText?:string}={};
      if(a.offset!==undefined)value.offset=a.offset as MeasurementOffset;
      if(a.freeText!==undefined)value.freeText=String(a.freeText).slice(0,4096);
      return ok(request.id,saMeasurement(figma.currentPage.editMeasurement(String(a.measurementId),value)),true);
    }
    case "dev.measurement.remove":
      saRequireEditor("dev");figma.currentPage.deleteMeasurement(String(a.measurementId));
      return ok(request.id,{removed:true,measurementId:String(a.measurementId)},true);
    case "variable.collection.extend": {
      const c=await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId));
      if(!c||c.remote)throw new Error("local_collection_not_found");
      if(typeof (c as any).extend!=="function")throw new Error("extended_collections_unavailable");
      const extended=(c as any).extend(String(a.name).slice(0,256)) as ExtendedVariableCollection;
      return ok(request.id,{id:extended.id,key:extended.key,name:extended.name,isExtension:extended.isExtension,modeCount:extended.modes.length,variableCount:extended.variableIds.length},true);
    }
    case "variable.values_for_collection": {
      const v=await figma.variables.getVariableByIdAsync(String(a.variableId));
      const c=await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId));
      if(!v||!c)throw new Error("variable_or_collection_not_found");
      if(typeof (v as any).valuesByModeForCollectionAsync!=="function")throw new Error("extended_collections_unavailable");
      const values=await (v as any).valuesByModeForCollectionAsync(c);
      return ok(request.id,{variableId:v.id,collectionId:c.id,values:spSerializable(values)});
    }
    case "variable.override.remove_mode": {
      const v=await figma.variables.getVariableByIdAsync(String(a.variableId));
      if(!v||v.remote)throw new Error("local_variable_not_found");
      if(typeof (v as any).removeOverrideForMode!=="function")throw new Error("extended_collections_unavailable");
      (v as any).removeOverrideForMode(String(a.modeId));
      return ok(request.id,{removed:true},true);
    }
    case "variable.collection.overrides.remove_variable": {
      const c=await figma.variables.getVariableCollectionByIdAsync(String(a.collectionId));
      const v=await figma.variables.getVariableByIdAsync(String(a.variableId));
      if(!c||!v||c.remote)throw new Error("variable_or_collection_not_found");
      if(typeof (c as any).removeOverridesForVariable!=="function")throw new Error("extended_collection_required");
      (c as any).removeOverridesForVariable(v);
      return ok(request.id,{removed:true},true);
    }
    case "variable.resolve_for_consumer": {
      const v=await figma.variables.getVariableByIdAsync(String(a.variableId));
      if(!v)throw new Error("variable_not_found");
      const node=asScene(await nodeById(String(a.nodeId)));
      const resolved=v.resolveForConsumer(node);
      return ok(request.id,{value:spSerializable(resolved.value),resolvedType:resolved.resolvedType});
    }
    case "style.consumers.list": {
      const style=await figma.getStyleByIdAsync(String(a.styleId));
      if(!style)throw new Error("style_not_found");
      const limit=Math.min(1000,Math.max(0,Number(a.limit??100)));
      const consumers=await style.getStyleConsumersAsync();
      return ok(request.id,consumers.slice(0,limit).map(c=>({node:summarize(c.node),fields:[...c.fields]})));
    }
    case "object.plugin_data.get": {
      const kind=String(a.targetKind),id=String(a.targetId),target=await saDataTarget(kind,id),key=String(a.key);
      return ok(request.id,{targetKind:kind,targetId:id,key,value:target.getPluginData(key)});
    }
    case "object.plugin_data.keys": {
      const target=await saDataTarget(String(a.targetKind),String(a.targetId));
      return ok(request.id,target.getPluginDataKeys().slice(0,1024));
    }
    case "object.plugin_data.set": {
      const target=await saDataTarget(String(a.targetKind),String(a.targetId)),key=String(a.key),value=String(a.value);
      const bytes=new TextEncoder().encode(key+value).byteLength;if(bytes>100000)throw new Error("plugin_data_limit");
      target.setPluginData(key,value);return ok(request.id,{stored:value.length>0,removed:value.length===0,bytes},true);
    }
    case "object.shared_plugin_data.get": {
      const kind=String(a.targetKind),id=String(a.targetId),target=await saDataTarget(kind,id),namespace=String(a.namespace),key=String(a.key);
      if(!/^[A-Za-z0-9]{3,128}$/.test(namespace))throw new Error("invalid_shared_namespace");
      return ok(request.id,{targetKind:kind,targetId:id,namespace,key,value:target.getSharedPluginData(namespace,key)});
    }
    case "object.shared_plugin_data.keys": {
      const target=await saDataTarget(String(a.targetKind),String(a.targetId)),namespace=String(a.namespace);
      if(!/^[A-Za-z0-9]{3,128}$/.test(namespace))throw new Error("invalid_shared_namespace");
      return ok(request.id,target.getSharedPluginDataKeys(namespace).slice(0,1024));
    }
    case "object.shared_plugin_data.set": {
      const target=await saDataTarget(String(a.targetKind),String(a.targetId)),namespace=String(a.namespace),key=String(a.key),value=String(a.value);
      if(!/^[A-Za-z0-9]{3,128}$/.test(namespace))throw new Error("invalid_shared_namespace");
      const bytes=new TextEncoder().encode(namespace+key+value).byteLength;if(bytes>100000)throw new Error("shared_plugin_data_limit");
      target.setSharedPluginData(namespace,key,value);return ok(request.id,{stored:value.length>0,removed:value.length===0,bytes},true);
    }
    case "style.variable.bind": {
      const style=await figma.getStyleByIdAsync(String(a.styleId));if(!style||style.type!=="TEXT"||style.remote)throw new Error("local_text_style_not_found");
      const variable=a.variableId==null?null:await figma.variables.getVariableByIdAsync(String(a.variableId));
      if(a.variableId!=null&&!variable)throw new Error("variable_not_found");
      const field=String(a.field) as VariableBindableTextField;style.setBoundVariable(field,variable);
      return ok(request.id,{styleId:style.id,field,bound:variable!==null,variableId:variable?.id??null},true);
    }
    case "object.property.get": {
      const kind=String(a.targetKind),id=String(a.targetId),property=String(a.property);
      const allowed=SEMWRIGHT_FIGMA_AUX_READ_PROPERTIES[kind];
      if(!allowed?.has(property))throw new Error("aux_property_not_readable");
      const target=await saDataTarget(kind,id);
      return ok(request.id,{targetKind:kind,targetId:id,property,value:spSerializable((target as any)[property])});
    }
    case "object.property.set": {
      const kind=String(a.targetKind),id=String(a.targetId),property=String(a.property);
      const allowed=SEMWRIGHT_FIGMA_AUX_WRITE_PROPERTIES[kind];
      if(!allowed?.has(property))throw new Error("aux_property_not_writable");
      const target=await saDataTarget(kind,id);
      if((target as any).remote===true)throw new Error("remote_target_read_only");
      (target as any)[property]=a.value;
      return ok(request.id,{targetKind:kind,targetId:id,property,value:spSerializable((target as any)[property]),mutated:true},true);
    }
    default:return null;
  }
}
