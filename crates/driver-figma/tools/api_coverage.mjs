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
  "BuzzAPI","TimerAPI","ViewportAPI","CodegenAPI",
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
  getPluginData:"node.plugin_data.get", setPluginData:"node.plugin_data.set",
  getRelaunchData:"node.relaunch_data.get", setRelaunchData:"node.relaunch_data.set",
  exportAsync:"export.node", getCSSAsync:"dev.css",
  getDevResourcesAsync:"dev.resources.list", addDevResourceAsync:"dev.resources.add",
  editDevResourceAsync:"dev.resources.edit", deleteDevResourceAsync:"dev.resources.remove",
  setBoundVariable:"variable.bind", setExplicitVariableModeForCollection:"variable.mode.set_explicit",
  clearExplicitVariableModeForCollection:"variable.mode.clear_explicit",
  getStyledTextSegments:"text.runs.inspect", setRangeHyperlink:"text.hyperlink.set",
  setRangeBoundVariable:"text.variable.bind_range", createInstance:"instance.create",
  getMainComponentAsync:"instance.inspect", swapComponent:"instance.swap",
  detachInstance:"instance.detach", addComponentProperty:"component.property.add",
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
  setEffectStyleIdAsync:"style.apply", setFillStyleIdAsync:"style.apply",
  setGridStyleIdAsync:"style.apply", setStrokeStyleIdAsync:"style.apply",
  setTextStyleIdAsync:"style.apply", setFillsAsync:"node.properties.patch",
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
const previous=fs.existsSync(coveragePath)?JSON.parse(fs.readFileSync(coveragePath,"utf8")):{};
const globals={};
let unclassifiedGlobals=0;
for(const name of GLOBAL_INTERFACES){
  const iface=interfaces.get(name);if(!iface)throw new Error(`Pinned typings missing ${name}`);
  globals[name]={};
  for(const member of ownMembers(iface)){
    const old=previous.interfaces?.[name]?.[member.name];
    const status=typeof old==="string"?old:(old?.status??"UNCLASSIFIED");
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
  scene_node_types:Object.fromEntries(Object.keys(sceneNodes).sort().map(k=>[k,sceneNodes[k]])),
  generic_property_surface:{
    readable:[...readProperties].sort(),
    writable:[...writeProperties].sort(),
  },
  summary:{
    global_interfaces:GLOBAL_INTERFACES.length,
    global_members:Object.values(globals).reduce((n,x)=>n+Object.keys(x).length,0),
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
console.log(`PASS typings=${typingsVersion} globals=${actual.summary.global_members} scene_nodes=${actual.summary.scene_nodes} node_members=${totalNodeMembers} supported=${supportedNodeMembers} method_gaps=${unmappedMethods.size}`);
