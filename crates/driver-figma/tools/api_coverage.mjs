#!/usr/bin/env node
import fs from "node:fs";
import path from "node:path";
import {createRequire} from "node:module";
import {fileURLToPath} from "node:url";

const here=path.dirname(fileURLToPath(import.meta.url));
const root=path.resolve(here,"..");
const require=createRequire(import.meta.url);
const ts=require(path.join(root,"plugin/node_modules/typescript"));
const typingsPath=path.join(root,"plugin/node_modules/@figma/plugin-typings/plugin-api.d.ts");
const packagePath=path.join(root,"plugin/node_modules/@figma/plugin-typings/package.json");
const coveragePath=path.join(root,"docs/API_COVERAGE.json");
const generatedPath=path.join(root,"plugin/src/generated_api_surface.ts");
const sourceText=fs.readFileSync(typingsPath,"utf8");
const source=ts.createSourceFile(typingsPath,sourceText,ts.ScriptTarget.Latest,true);
const typingsVersion=JSON.parse(fs.readFileSync(packagePath,"utf8")).version;

const GLOBAL_INTERFACES=[
  "PluginAPI","VariablesAPI","TeamLibraryAPI","MotionAPI","AnnotationsAPI",
  "BuzzAPI","TimerAPI","ViewportAPI","CodegenAPI","TextReviewAPI",
  "DevResourcesAPI","VSCodeAPI","ParametersAPI","ClientStorageAPI",
  "UIAPI","UtilAPI","ConstantsAPI","PaymentsAPI",
];
const interfaces=new Map();
for(const stmt of source.statements){
  if(ts.isInterfaceDeclaration(stmt))interfaces.set(stmt.name.text,stmt);
}
function memberName(node){
  if(!node.name)return null;
  if(ts.isIdentifier(node.name)||ts.isStringLiteral(node.name)||ts.isNumericLiteral(node.name))return node.name.text;
  return node.name.getText(source);
}
function ownMembers(iface){
  const out=[];
  for(const member of iface.members){
    const name=memberName(member);if(!name)continue;
    if(ts.isPropertySignature(member)){
      const readonly=Boolean(member.modifiers?.some(m=>m.kind===ts.SyntaxKind.ReadonlyKeyword));
      out.push({name,kind:"property",readonly,optional:Boolean(member.questionToken),type:member.type?.getText(source)??"unknown"});
    }else if(ts.isMethodSignature(member)){
      out.push({name,kind:"method",readonly:true,optional:Boolean(member.questionToken),type:member.type?.getText(source)??"unknown"});
    }
  }
  return out;
}
function parents(iface){
  const names=[];
  for(const clause of iface.heritageClauses??[]){
    if(clause.token!==ts.SyntaxKind.ExtendsKeyword)continue;
    for(const t of clause.types)names.push(t.expression.getText(source));
  }
  return names;
}
function allMembers(name,seen=new Set()){
  if(seen.has(name))return [];
  seen.add(name);
  const iface=interfaces.get(name);if(!iface)return [];
  const byName=new Map();
  for(const parent of parents(iface)){
    for(const member of allMembers(parent,seen))byName.set(member.name,member);
  }
  for(const member of ownMembers(iface))byName.set(member.name,member);
  return [...byName.values()];
}
function findAlias(name){
  return source.statements.find(s=>ts.isTypeAliasDeclaration(s)&&s.name.text===name);
}
function typeNames(node){
  if(ts.isUnionTypeNode(node))return node.types.flatMap(typeNames);
  if(ts.isTypeReferenceNode(node))return [node.typeName.getText(source)];
  return [];
}
function nodeTypeLiteral(interfaceName){
  const iface=interfaces.get(interfaceName);if(!iface)return interfaceName.replace(/Node$/,"").toUpperCase();
  const typeMember=iface.members.find(m=>ts.isPropertySignature(m)&&memberName(m)==="type");
  const typeNode=typeMember?.type;
  if(typeNode&&ts.isLiteralTypeNode(typeNode)&&ts.isStringLiteral(typeNode.literal))return typeNode.literal.text;
  return interfaceName.replace(/Node$/,"").toUpperCase();
}
const sceneAlias=findAlias("SceneNode");
if(!sceneAlias)throw new Error("SceneNode alias missing from pinned typings");
const sceneInterfaces=typeNames(sceneAlias.type).filter(name=>interfaces.has(name));
const sceneHierarchyInterfaces=new Set();
function markSceneHierarchy(name){
  if(sceneHierarchyInterfaces.has(name))return;
  sceneHierarchyInterfaces.add(name);
  const iface=interfaces.get(name);if(!iface)return;
  for(const parent of parents(iface))markSceneHierarchy(parent);
}
for(const name of sceneInterfaces)markSceneHierarchy(name);
const sceneNodes={};
const readProperties=new Set(), writeProperties=new Set(), methodNames=new Set();
for(const interfaceName of sceneInterfaces){
  const nodeType=nodeTypeLiteral(interfaceName);
  const members={};
  for(const member of allMembers(interfaceName)){
    if(member.kind==="property"){
      readProperties.add(member.name);
      if(!member.readonly)writeProperties.add(member.name);
      members[member.name]={...member,status:member.readonly?"SUPPORTED_GENERIC_READ":"SUPPORTED_GENERIC_READ_WRITE"};
    }else{
      methodNames.add(member.name);
      members[member.name]={...member,status:"UNMAPPED_METHOD"};
    }
  }
  sceneNodes[nodeType]={interface:interfaceName,members};
}
const METHOD_MAP={
  remove:"node.remove", clone:"node.clone", resize:"node.resize", rescale:"node.resize",
  resizeWithoutConstraints:"node.resize_unconstrained", getTopLevelFrame:"node.top_level_frame",
  lockAspectRatio:"node.aspect_ratio.lock", unlockAspectRatio:"node.aspect_ratio.unlock",
  getPluginData:"node.plugin_data.get", setPluginData:"node.plugin_data.set",
  getPluginDataKeys:"node.plugin_data.keys", getSharedPluginData:"node.shared_plugin_data.get",
  getSharedPluginDataKeys:"node.shared_plugin_data.keys", setSharedPluginData:"node.shared_plugin_data.set",
  getRelaunchData:"node.relaunch_data.get", setRelaunchData:"node.relaunch_data.set",
  exportAsync:"export.node", getCSSAsync:"dev.css",
  getDevResourcesAsync:"dev.resources.list", addDevResourceAsync:"dev.resources.add",
  editDevResourceAsync:"dev.resources.edit", deleteDevResourceAsync:"dev.resources.remove",
  setBoundVariable:"variable.bind", setExplicitVariableModeForCollection:"variable.mode.set_explicit",
  clearExplicitVariableModeForCollection:"variable.mode.clear_explicit",
  getStyledTextSegments:"text.runs.inspect", setRangeHyperlink:"text.hyperlink.set",
  setRangeBoundVariable:"text.variable.bind_range", createInstance:"instance.create",
  getMainComponentAsync:"instance.inspect", swapComponent:"instance.swap",
  detachInstance:"instance.detach", removeOverrides:"instance.overrides.remove_all",
  resetOverrides:"instance.overrides.remove_all", addComponentProperty:"component.property.add",
  editComponentProperty:"component.property.edit", deleteComponentProperty:"component.property.delete",
  outlineStroke:"node.outline_stroke", setReactionsAsync:"prototype.reaction.set",
  applyAnimationStyle:"motion.style.apply", removeAnimationStyle:"motion.style.remove",
  applyManualKeyframeTrack:"motion.keyframe.apply", removeManualKeyframeTrack:"motion.keyframe.remove",
  setTimelineDuration:"motion.timeline.set_duration",
  appendChild:"node.reparent", appendChildAt:"node.reparent+node.reorder",
  insertChild:"node.reparent+node.reorder",
  cellAt:"figjam.table.cell.inspect", createSlot:"slot.create", resetSlot:"slot.reset",
  findAll:"node.search", findAllWithCriteria:"node.search", findChild:"node.search",
  findChildren:"node.children", findOne:"node.search",
  getInstancesAsync:"component.instances.list",
  insertRow:"figjam.table.row.insert", removeRow:"figjam.table.row.remove",
  moveRow:"figjam.table.row.move", resizeRow:"figjam.table.row.resize",
  insertColumn:"figjam.table.column.insert", removeColumn:"figjam.table.column.remove",
  moveColumn:"figjam.table.column.move", resizeColumn:"figjam.table.column.resize",
  reorderRows:"layout.grid.rows.reorder", reorderColumns:"layout.grid.columns.reorder",
  setGridChildPosition:"layout.grid.child.position",
  setEffectStyleIdAsync:"style.apply", setFillStyleIdAsync:"style.apply",
  setGridStyleIdAsync:"style.apply", setStrokeStyleIdAsync:"style.apply",
  setTextStyleIdAsync:"style.apply", getSlideTransition:"slides.transition.inspect",
  setSlideTransition:"slides.transition.set", getAuthorAsync:"figjam.stamp.author.inspect",
  getPublishStatusAsync:"library.publish_status.inspect",
  findWidgetNodesByWidgetId:"widget.find_by_widget_id", setFillsAsync:"node.properties.patch",
  setStrokesAsync:"node.properties.patch", setProperties:"instance.properties.patch",
  setVectorNetworkAsync:"vector.network.set",
  insertCharacters:"text.range.edit", deleteCharacters:"text.range.edit",
  getRangeAllFontNames:"text.range.inspect", getRangeBoundVariable:"text.range.inspect",
  getRangeFillStyleId:"text.range.inspect", getRangeFills:"text.range.inspect",
  getRangeFontName:"text.range.inspect", getRangeFontSize:"text.range.inspect",
  getRangeFontWeight:"text.range.inspect", getRangeHyperlink:"text.range.inspect",
  getRangeIndentation:"text.range.inspect", getRangeLetterSpacing:"text.range.inspect",
  getRangeLineHeight:"text.range.inspect", getRangeListOptions:"text.range.inspect",
  getRangeListSpacing:"text.range.inspect", getRangeOpenTypeFeatures:"text.range.inspect",
  getRangeParagraphIndent:"text.range.inspect", getRangeParagraphSpacing:"text.range.inspect",
  getRangeTextCase:"text.range.inspect", getRangeTextDecoration:"text.range.inspect",
  getRangeTextDecorationColor:"text.range.inspect", getRangeTextDecorationOffset:"text.range.inspect",
  getRangeTextDecorationSkipInk:"text.range.inspect", getRangeTextDecorationStyle:"text.range.inspect",
  getRangeTextDecorationThickness:"text.range.inspect", getRangeTextStyleId:"text.range.inspect",
  getRangeTextWrapStyle:"text.range.inspect",
  setRangeFillStyleId:"text.range.patch", setRangeFillStyleIdAsync:"text.range.patch",
  setRangeFills:"text.range.patch", setRangeFontName:"text.range.patch",
  setRangeFontSize:"text.range.patch", setRangeIndentation:"text.range.patch",
  setRangeLetterSpacing:"text.range.patch", setRangeLineHeight:"text.range.patch",
  setRangeListOptions:"text.range.patch", setRangeListSpacing:"text.range.patch",
  setRangeParagraphIndent:"text.range.patch", setRangeParagraphSpacing:"text.range.patch",
  setRangeTextCase:"text.range.patch", setRangeTextDecoration:"text.range.patch",
  setRangeTextDecorationColor:"text.range.patch", setRangeTextDecorationOffset:"text.range.patch",
  setRangeTextDecorationSkipInk:"text.range.patch", setRangeTextDecorationStyle:"text.range.patch",
  setRangeTextDecorationThickness:"text.range.patch", setRangeTextStyleId:"text.range.patch",
  setRangeTextStyleIdAsync:"text.range.patch", setRangeTextWrapStyle:"text.range.patch",
  getTopLevelFrame:"node.top_level_frame", getPluginDataKeys:"node.plugin_data.keys",
  getSharedPluginData:"node.shared_plugin_data.get", setSharedPluginData:"node.shared_plugin_data.set",
  getSharedPluginDataKeys:"node.shared_plugin_data.keys", getPublishStatusAsync:"library.publish_status.inspect",
  getAuthorAsync:"figjam.stamp.author.inspect", resizeWithoutConstraints:"node.resize_unconstrained",
  lockAspectRatio:"node.aspect_ratio.lock", unlockAspectRatio:"node.aspect_ratio.unlock",
  resetOverrides:"instance.overrides.remove_all", removeOverrides:"instance.overrides.remove_all",
  reorderRows:"layout.grid.rows.reorder", reorderColumns:"layout.grid.columns.reorder",
  setGridChildPosition:"layout.grid.child.position", getSlideTransition:"slides.transition.inspect",
  setSlideTransition:"slides.transition.set", findWidgetNodesByWidgetId:"widget.find_by_widget_id",
};
const METHOD_CLASSIFICATION={
  cloneWidget:"UPSTREAM_WIDGET_CONTEXT_RESTRICTED",
  setWidgetSyncedState:"UPSTREAM_WIDGET_CONTEXT_RESTRICTED",
  setDevResourcePreviewAsync:"UPSTREAM_PARTNER_RESTRICTED",
};
const rustCatalogSources=[
  "src/main.rs",
  "src/semantic_more_ops.rs",
  "src/semantic_admin_ops.rs",
].map(file=>fs.readFileSync(path.join(root,file),"utf8")).join("\n");
const advertisedCapabilities=new Set(
  [...rustCatalogSources.matchAll(/op\(\s*"([^"]+)"/g)].map(match=>match[1])
);
for(const [method,mapping] of Object.entries(METHOD_MAP)){
  for(const capability of mapping.split("+")){
    if(!advertisedCapabilities.has(capability)){
      throw new Error(`Scene method ${method} maps to missing capability ${capability}`);
    }
  }
}
for(const required of ["node.properties.inspect","node.properties.patch"]){
  if(!advertisedCapabilities.has(required))throw new Error(`Missing generic property capability ${required}`);
}
for(const node of Object.values(sceneNodes)){
  for(const member of Object.values(node.members)){
    if(member.kind!=="method")continue;
    if(METHOD_MAP[member.name]){
      member.status="SUPPORTED_METHOD";
      member.capability=METHOD_MAP[member.name];
    }else if(METHOD_CLASSIFICATION[member.name]){
      member.status=METHOD_CLASSIFICATION[member.name];
    }
  }
}
const AUX_CAPABILITY_BY_METHOD={
  addMeasurement:"dev.measurement.add",deleteMeasurement:"dev.measurement.remove",editMeasurement:"dev.measurement.edit",
  getMeasurements:"dev.measurement.list",getMeasurementsForNode:"dev.measurement.for_node",
  addMode:"mode.create",renameMode:"mode.rename",removeMode:"mode.remove",extend:"variable.collection.extend",
  valuesByModeForCollectionAsync:"variable.values_for_collection",removeOverrideForMode:"variable.override.remove_mode",
  removeOverridesForVariable:"variable.collection.overrides.remove_variable",resolveForConsumer:"variable.resolve_for_consumer",
  setValueForMode:"variable.set_value",setVariableCodeSyntax:"variable.code_syntax.set",removeVariableCodeSyntax:"variable.code_syntax.remove",
  getStyleConsumersAsync:"style.consumers.list",requestToBeEnabledAsync:"textreview.enable",requestToBeDisabledAsync:"textreview.disable",
  setColor:"annotation.category.patch",setLabel:"annotation.category.patch",setMediaAsync:"buzz.media_content.set",setValueAsync:"buzz.text_content.set",
};
const AUX_CAPABILITY_EXACT={
  "PluginDataMixin.getPluginData":"object.plugin_data.get","PluginDataMixin.setPluginData":"object.plugin_data.set",
  "PluginDataMixin.getPluginDataKeys":"object.plugin_data.keys","PluginDataMixin.getSharedPluginData":"object.shared_plugin_data.get",
  "PluginDataMixin.setSharedPluginData":"object.shared_plugin_data.set","PluginDataMixin.getSharedPluginDataKeys":"object.shared_plugin_data.keys",
  "PublishableMixin.getPublishStatusAsync":"library.publish_status.inspect","TextStyle.setBoundVariable":"style.variable.bind",
  "Variable.remove":"variable.remove","VariableCollection.remove":"variable.collection.remove","BaseStyleMixin.remove":"style.remove",
  "AnnotationCategory.remove":"annotation.category.remove","Image.getBytesAsync":"image.export","Image.getSizeAsync":"image.inspect",
};
const AUX_INTERFACE_CLASSIFICATION={
  UIAPI:"INTERNAL_PLUGIN_UI",ClientStorageAPI:"INTERNAL_PLUGIN_STATE",
  UtilAPI:"PURE_HELPER_INTERNAL",SuggestionResults:"EVENT_CALLBACK_HELPER",DropFile:"EVENT_PAYLOAD_HELPER",
  DevResourcesAPI:"EVENT_SOURCE_INTERNAL",ParametersAPI:"EVENT_SOURCE_INTERNAL",
};
const AUX_EXACT_CLASSIFICATION={
  "PageNode.loadAsync":"INTERNAL_DYNAMIC_PAGE_LIFECYCLE",
  "PageNode.on":"EVENT_SOURCE_INTERNAL",
  "PageNode.once":"EVENT_SOURCE_INTERNAL",
  "PageNode.off":"EVENT_SOURCE_INTERNAL",
};
const auxiliaryInterfaces={};let auxiliaryMethodEntries=0,unclassifiedAuxiliaryMethods=0;
for(const [interfaceName,iface] of interfaces){
  if(sceneHierarchyInterfaces.has(interfaceName)||GLOBAL_INTERFACES.includes(interfaceName))continue;
  const methods={};
  for(const member of ownMembers(iface)){
    if(member.kind!=="method")continue;
    auxiliaryMethodEntries++;
    const key=`${interfaceName}.${member.name}`;
    const capability=AUX_CAPABILITY_EXACT[key]??AUX_CAPABILITY_BY_METHOD[member.name]??METHOD_MAP[member.name];
    const status=capability?"SUPPORTED_METHOD":(AUX_EXACT_CLASSIFICATION[key]??AUX_INTERFACE_CLASSIFICATION[interfaceName]??"UNCLASSIFIED");
    if(status==="UNCLASSIFIED")unclassifiedAuxiliaryMethods++;
    methods[member.name]={status,...(capability?{capability}:{})};
  }
  if(Object.keys(methods).length)auxiliaryInterfaces[interfaceName]=methods;
}
const AUX_PROPERTY_TARGETS={
  STYLE:["PaintStyle","TextStyle","EffectStyle","GridStyle"],
  VARIABLE:["Variable"],
  COLLECTION:["VariableCollection","ExtendedVariableCollection"],
};
const AUX_WRITE_EXCLUDED_PROPERTIES=new Set(["type","boundVariables","consumers"]);
const auxPropertySurface={};
for(const [targetKind,names] of Object.entries(AUX_PROPERTY_TARGETS)){
  const readable=new Set(),writable=new Set();
  for(const name of names){
    for(const member of allMembers(name)){
      if(member.kind!=="property")continue;
      readable.add(member.name);
      if(!member.readonly&&!AUX_WRITE_EXCLUDED_PROPERTIES.has(member.name))writable.add(member.name);
    }
  }
  auxPropertySurface[targetKind]={readable:[...readable].sort(),writable:[...writable].sort()};
}
const previous=fs.existsSync(coveragePath)?JSON.parse(fs.readFileSync(coveragePath,"utf8")):{};
const GLOBAL_INTERFACE_DEFAULTS={
  TextReviewAPI:"SUPPORTED_SEPARATE_MANIFEST",
  CodegenAPI:"SUPPORTED_SEPARATE_MANIFEST",
  DevResourcesAPI:"SEPARATE_EDITOR_MODE",
  VSCodeAPI:"SEPARATE_EDITOR_MODE",
  ParametersAPI:"INVENTORIED_INTERNAL",
  ClientStorageAPI:"INTERNAL_PLUGIN_STATE",
  UIAPI:"INTERNAL_PLUGIN_UI",
  UtilAPI:"INVENTORIED_INTERNAL",
  ConstantsAPI:"INVENTORIED_INTERNAL",
};
const GLOBAL_CAPABILITY_MAP={
  PluginAPI:{
    getNodeByIdAsync:"node.get",getNodeById:"node.get",
    getStyleByIdAsync:"style.inspect",getStyleById:"style.inspect",
    setCurrentPageAsync:"page.current.set",
    createRectangle:"rect.create",createLine:"line.create",createEllipse:"ellipse.create",
    createPolygon:"polygon.create",createStar:"star.create",createVector:"vector.create",
    createText:"text.create",createFrame:"frame.create",createComponent:"component.create",
    createComponentFromNode:"component.from_node",createPage:"page.create",
    createPageDivider:"page.divider.create",createSlice:"slice.create",
    createSlide:"slides.slide.create",createSlideRow:"slides.row.create",
    createSticky:"figjam.sticky.create",createConnector:"figjam.connector.create",
    createShapeWithText:"figjam.shape.create",createCodeBlock:"figjam.code_block.create",
    createSection:"section.create",createTable:"figjam.table.create",
    createTextPath:"text.path.create",createNodeFromJSXAsync:"compose.apply",
    createBooleanOperation:"boolean.create",
    createPaintStyle:"style.create",createTextStyle:"style.create",
    createEffectStyle:"style.create",createGridStyle:"style.create",
    getLocalPaintStylesAsync:"style.list",getLocalPaintStyles:"style.list",
    getLocalTextStylesAsync:"style.list",getLocalTextStyles:"style.list",
    getLocalEffectStylesAsync:"style.list",getLocalEffectStyles:"style.list",
    getLocalGridStylesAsync:"style.list",getLocalGridStyles:"style.list",
    moveLocalPaintStyleAfter:"style.order.after",moveLocalTextStyleAfter:"style.order.after",
    moveLocalEffectStyleAfter:"style.order.after",moveLocalGridStyleAfter:"style.order.after",
    moveLocalPaintFolderAfter:"style.folder.order.after",moveLocalTextFolderAfter:"style.folder.order.after",
    moveLocalEffectFolderAfter:"style.folder.order.after",moveLocalGridFolderAfter:"style.folder.order.after",
    importComponentByKeyAsync:"library.component.import",
    importComponentSetByKeyAsync:"library.component_set.import",
    importStyleByKeyAsync:"library.style.import",
    listAvailableShaders:"shader.list",importShaderById:"shader.import",
    listAvailableFontsAsync:"font.list",loadFontAsync:"font.load",
    getFontFamilyVariationAxes:"font.variation_axes",createNodeFromSvg:"svg.import",
    createImage:"image.create",getImageByHash:"image.inspect",createVideoAsync:"video.create",
    createLinkPreviewAsync:"figjam.link_preview.create",createGif:"figjam.gif.create",
    combineAsVariants:"component_set.create",group:"group.create",
    transformGroup:"transform_group.create",flatten:"node.flatten",
    union:"boolean.union",subtract:"boolean.subtract",intersect:"boolean.intersect",
    exclude:"boolean.exclude",ungroup:"group.ungroup",
    getFileThumbnailNodeAsync:"file.thumbnail.get",getFileThumbnailNode:"file.thumbnail.get",
    setFileThumbnailNodeAsync:"file.thumbnail.set",getSlideGrid:"slides.grid.inspect",
    setSlideGrid:"slides.grid.set",getCanvasGrid:"canvas.grid.inspect",
    setCanvasGrid:"canvas.grid.set",createCanvasRow:"canvas.row.create",
    moveNodesToCoord:"canvas.nodes.move",loadBrushesAsync:"brush.load",
    currentUser:"user.current",activeUsers:"figjam.active_users",
    commitUndo:"history.commit",triggerUndo:"history.undo",
    saveVersionHistoryAsync:"file.version.save",getSelectionColors:"selection.colors",
  },
  VariablesAPI:{
    getVariableByIdAsync:"variable.inspect",getVariableById:"variable.inspect",
    getVariableCollectionByIdAsync:"variable.collection.inspect",getVariableCollectionById:"variable.collection.inspect",
    getLocalVariablesAsync:"variable.list",getLocalVariables:"variable.list",
    getLocalVariableCollectionsAsync:"variable.collection.list",getLocalVariableCollections:"variable.collection.list",
    createVariable:"variable.create",createVariableCollection:"variable.collection.create",
    extendLibraryCollectionByKeyAsync:"library.collection.extend",
    setBoundVariableForPaint:"variable.bind.paint",setBoundVariableForEffect:"variable.bind.effect",
    setBoundVariableForLayoutGrid:"variable.bind.layout_grid",importVariableByKeyAsync:"library.variable.import",
  },
  TeamLibraryAPI:{
    getAvailableLibraryVariableCollectionsAsync:"library.variable_collections.list",
    getVariablesInLibraryCollectionAsync:"library.variables.list",
  },
  MotionAPI:{playheadPosition:"motion.playhead.get",figmaAnimationStyles:"motion.styles.list",physicalSpringToNormalized:"motion.spring.normalize"},
  AnnotationsAPI:{
    getAnnotationCategoriesAsync:"annotation.categories.list",
    getAnnotationCategoryByIdAsync:"annotation.category.inspect",
    addAnnotationCategoryAsync:"annotation.category.create",
  },
  BuzzAPI:{
    createFrame:"buzz.frame.create",createInstance:"buzz.instance.create",
    getBuzzAssetTypeForNode:"buzz.asset_type.get",setBuzzAssetTypeForNode:"buzz.asset_type.set",
    getTextContent:"buzz.text_content.inspect",getMediaContent:"buzz.media_content.inspect",smartResize:"buzz.smart_resize",
  },
  TimerAPI:{remaining:"figjam.timer.status",total:"figjam.timer.status",state:"figjam.timer.status",pause:"figjam.timer.pause",resume:"figjam.timer.resume",start:"figjam.timer.start",stop:"figjam.timer.stop"},
  ViewportAPI:{center:"viewport.inspect+viewport.center",zoom:"viewport.inspect+viewport.zoom",bounds:"viewport.inspect",scrollAndZoomIntoView:"viewport.fit",slidesView:"slides.view.get+slides.view.set",canvasView:"viewport.canvas_view.get+viewport.canvas_view.set"},
  PaymentsAPI:{
    status:"payments.status",
    setPaymentStatusInDevelopment:"payments.dev.status.set",
    getUserFirstRanSecondsAgo:"payments.first_run_age",
    initiateCheckoutAsync:"payments.checkout",
    requestCheckout:"payments.checkout.request",
  },
};
const GLOBAL_EXACT_CLASSIFICATION={
  "PluginAPI.createImageAsync":"SEMANTICALLY_SUPERSEDED:image.create",
  "PluginAPI.loadAllPagesAsync":"INTERNAL_DYNAMIC_PAGE_LIFECYCLE",
  "VariablesAPI.createVariableAlias":"PURE_HELPER_INTERNAL_ALIAS_VALUE",
  "VariablesAPI.createVariableAliasByIdAsync":"PURE_HELPER_INTERNAL_ALIAS_VALUE",
  "PaymentsAPI.getPluginPaymentTokenAsync":"INTERNAL_SECRET_COMPOSITION",
};
for(const [iface,mappings] of Object.entries(GLOBAL_CAPABILITY_MAP)){
  for(const [member,mapping] of Object.entries(mappings)){
    for(const capability of mapping.split("+")){
      if(!advertisedCapabilities.has(capability)){
        throw new Error(`Global API ${iface}.${member} maps to missing capability ${capability}`);
      }
    }
  }
}
const GLOBAL_MEMBER_OVERRIDES={
  PluginAPI:{
    apiVersion:"RUNTIME_METADATA",command:"RUNTIME_INVOCATION_CONTEXT",editorType:"RUNTIME_EDITOR_CONTEXT",
    mode:"RUNTIME_EDITOR_CONTEXT",pluginId:"RUNTIME_PLUGIN_IDENTITY",widgetId:"RUNTIME_WIDGET_CONTEXT",
    fileKey:"RUNTIME_DOCUMENT_IDENTITY",skipInvisibleInstanceChildren:"RUNTIME_PERFORMANCE_TUNING",
    textreview:"SUPPORTED_SEPARATE_MANIFEST",codegen:"SUPPORTED_SEPARATE_MANIFEST",vscode:"SEPARATE_EDITOR_MODE",
    variables:"NAMESPACE_MAPPED",teamLibrary:"NAMESPACE_MAPPED",annotations:"NAMESPACE_MAPPED",
    buzz:"NAMESPACE_MAPPED",timer:"NAMESPACE_MAPPED",viewport:"NAMESPACE_MAPPED",motion:"BETA_NAMESPACE_MAPPED",
    devResources:"SEPARATE_EDITOR_MODE_NAMESPACE",
    root:"RUNTIME_DOCUMENT_CONTEXT",currentPage:"RUNTIME_DOCUMENT_CONTEXT",mixed:"RUNTIME_SENTINEL",
    on:"SUPPORTED_EVENT",once:"SUPPORTED_EVENT",off:"SUPPORTED_EVENT",
    openExternal:"SEMANTICALLY_DELEGATED:browser-provider",
    payments:"NAMESPACE_MAPPED",clientStorage:"INTERNAL_PLUGIN_STATE",
    parameters:"INTERNAL_PLUGIN_INVOCATION",showUI:"INTERNAL_PLUGIN_UI",
    ui:"INTERNAL_PLUGIN_UI",closePlugin:"INTERNAL_PLUGIN_LIFECYCLE",notify:"INTERNAL_PLUGIN_UI",
    util:"PURE_HELPER_INTERNAL",constants:"PURE_HELPER_INTERNAL",
    hasMissingFont:"CAPABILITY:font.status",base64Encode:"PURE_HELPER_INTERNAL",base64Decode:"PURE_HELPER_INTERNAL",
  },
};
const globals={};
let unclassifiedGlobals=0;
for(const name of GLOBAL_INTERFACES){
  const iface=interfaces.get(name);if(!iface)throw new Error(`Pinned typings missing ${name}`);
  globals[name]={};
  for(const member of ownMembers(iface)){
    const capability=GLOBAL_CAPABILITY_MAP[name]?.[member.name];
    const exact=GLOBAL_EXACT_CLASSIFICATION[`${name}.${member.name}`];
    const override=GLOBAL_MEMBER_OVERRIDES[name]?.[member.name];
    const fallback=GLOBAL_INTERFACE_DEFAULTS[name];
    const status=capability?`CAPABILITY:${capability}`:(exact??override??fallback??"UNCLASSIFIED");
    if(status==="UNCLASSIFIED")unclassifiedGlobals++;
    globals[name][member.name]=status;
  }
}
const unmappedMethods=new Set();
let totalNodeMembers=0,supportedNodeMembers=0;
for(const node of Object.values(sceneNodes)){
  for(const member of Object.values(node.members)){
    totalNodeMembers++;
    if(member.status==="UNMAPPED_METHOD")unmappedMethods.add(member.name);
    else supportedNodeMembers++;
  }
}
const actual={
  schema_version:2,
  plugin_typings_version:typingsVersion,
  source:"@figma/plugin-typings/plugin-api.d.ts",
  interfaces:globals,
  auxiliary_interfaces:Object.fromEntries(Object.keys(auxiliaryInterfaces).sort().map(k=>[k,auxiliaryInterfaces[k]])),
  scene_node_types:Object.fromEntries(Object.keys(sceneNodes).sort().map(k=>[k,sceneNodes[k]])),
  generic_property_surface:{
    readable:[...readProperties].sort(),
    writable:[...writeProperties].sort(),
  },
  auxiliary_property_surface:auxPropertySurface,
  summary:{
    global_interfaces:GLOBAL_INTERFACES.length,
    global_members:Object.values(globals).reduce((n,x)=>n+Object.keys(x).length,0),
    auxiliary_interfaces:Object.keys(auxiliaryInterfaces).length,
    auxiliary_method_entries:auxiliaryMethodEntries,
    unclassified_auxiliary_methods:unclassifiedAuxiliaryMethods,
    scene_nodes:Object.keys(sceneNodes).length,
    scene_node_members:totalNodeMembers,
    supported_scene_node_members:supportedNodeMembers,
    unmapped_method_names:[...unmappedMethods].sort(),
  },
};
const generated=[
  "// GENERATED by tools/api_coverage.mjs from pinned @figma/plugin-typings.",
  "// Do not hand-edit. Public property names only; methods are mapped separately.",
  `const SEMWRIGHT_FIGMA_NODE_READ_PROPERTIES = new Set<string>(${JSON.stringify([...readProperties].sort(),null,2)});`,
  `const SEMWRIGHT_FIGMA_NODE_WRITE_PROPERTIES = new Set<string>(${JSON.stringify([...writeProperties].sort(),null,2)});`,
  `const SEMWRIGHT_FIGMA_AUX_READ_PROPERTIES: Record<string, Set<string>> = Object.fromEntries(Object.entries(${JSON.stringify(auxPropertySurface)}).map(([kind,surface]: any)=>[kind,new Set(surface.readable)]));`,
  `const SEMWRIGHT_FIGMA_AUX_WRITE_PROPERTIES: Record<string, Set<string>> = Object.fromEntries(Object.entries(${JSON.stringify(auxPropertySurface)}).map(([kind,surface]: any)=>[kind,new Set(surface.writable)]));`,
  "",
].join("\n");

const json=JSON.stringify(actual,null,2)+"\n";
const write=process.argv.includes("--write");
if(write){
  fs.writeFileSync(coveragePath,json);
  fs.writeFileSync(generatedPath,generated);
}else{
  if(!fs.existsSync(coveragePath)||fs.readFileSync(coveragePath,"utf8")!==json){
    console.error("API coverage inventory drifted; run node tools/api_coverage.mjs --write and classify changes.");
    process.exit(1);
  }
  if(!fs.existsSync(generatedPath)||fs.readFileSync(generatedPath,"utf8")!==generated){
    console.error("Generated Figma semantic property allowlist drifted; run node tools/api_coverage.mjs --write.");
    process.exit(1);
  }
}
if(unclassifiedGlobals){
  console.error(`UNCLASSIFIED global Figma API members: ${unclassifiedGlobals}`);
  process.exit(1);
}
if(unclassifiedAuxiliaryMethods){
  console.error(`UNCLASSIFIED auxiliary Figma API methods: ${unclassifiedAuxiliaryMethods}`);
  for(const [iface,members] of Object.entries(auxiliaryInterfaces)){
    for(const [method,entry] of Object.entries(members)){
      if(entry.status==="UNCLASSIFIED")console.error(`  ${iface}.${method}`);
    }
  }
  process.exit(1);
}
console.log(`PASS typings=${typingsVersion} globals=${actual.summary.global_members} auxiliary_methods=${auxiliaryMethodEntries} scene_nodes=${actual.summary.scene_nodes} node_members=${totalNodeMembers} supported=${supportedNodeMembers} method_gaps=${unmappedMethods.size}`);
