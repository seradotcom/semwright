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
const sourceText=fs.readFileSync(typingsPath,"utf8");
const source=ts.createSourceFile(typingsPath,sourceText,ts.ScriptTarget.Latest,true);
const typingsVersion=JSON.parse(fs.readFileSync(packagePath,"utf8")).version;

const INTERFACES=[
  "PluginAPI","VariablesAPI","TeamLibraryAPI","MotionAPI","AnnotationsAPI",
  "BuzzAPI","TimerAPI","ViewportAPI","CodegenAPI",
];
function memberName(member){
  const n=member.name;
  if(!n)return null;
  if(ts.isIdentifier(n)||ts.isStringLiteral(n)||ts.isNumericLiteral(n))return String(n.text);
  return n.getText(source);
}
function interfaceMembers(name){
  const names=new Set();
  for(const stmt of source.statements){
    if(ts.isInterfaceDeclaration(stmt)&&stmt.name.text===name){
      for(const m of stmt.members){const n=memberName(m);if(n)names.add(n)}
    }
  }
  return [...names].sort();
}
function sceneNodeTypes(){
  for(const stmt of source.statements){
    if(ts.isTypeAliasDeclaration(stmt)&&stmt.name.text==="SceneNode"&&ts.isUnionTypeNode(stmt.type)){
      return stmt.type.types.map(t=>{
        if(ts.isTypeReferenceNode(t))return t.typeName.getText(source).replace(/Node$/,"").toUpperCase();
        return t.getText(source);
      }).sort();
    }
  }
  throw new Error("SceneNode union not found");
}
function classify(iface,name){
  if(iface==="VariablesAPI"||iface==="TeamLibraryAPI"||iface==="AnnotationsAPI"||iface==="BuzzAPI"||iface==="TimerAPI"||iface==="ViewportAPI")return "SUPPORTED";
  if(iface==="MotionAPI")return "BETA_SUPPORTED";
  if(iface==="CodegenAPI")return "SEPARATE_EDITOR_MODE";
  if(iface==="PluginAPI"){
    if(["fetch","payments","clientStorage","parameters","ui","showUI","closePlugin","notify"].includes(name))return "INTERNAL_OR_POLICY_EXCLUDED";
    if(["codegen","vscode","textreview"].includes(name))return "SEPARATE_EDITOR_MODE";
    if(name==="motion")return "BETA_SUPPORTED";
    if(["variables","teamLibrary","annotations","buzz","timer","viewport"].includes(name))return "SUPPORTED";
    if(/^on$|^off$|^once$/.test(name))return "SUPPORTED_EVENT";
    if(/^(get|create|load|import|list|group|ungroup|flatten|union|subtract|intersect|exclude|combine|transform|move|set|save)/.test(name))return "SEMANTICALLY_MAPPED_OR_CLASSIFIED";
    return "INVENTORIED_INTERNAL";
  }
  return "UNCLASSIFIED";
}
function buildInventory(){
  const interfaces={};
  for(const iface of INTERFACES){
    interfaces[iface]={};
    for(const name of interfaceMembers(iface))interfaces[iface][name]=classify(iface,name);
  }
  return {
    schema_version:1,
    plugin_typings_version:typingsVersion,
    source:"@figma/plugin-typings/plugin-api.d.ts",
    interfaces,
    scene_node_types:Object.fromEntries(sceneNodeTypes().map(n=>[n,"INVENTORIED"])),
  };
}
const actual=buildInventory();
if(process.argv.includes("--write")){
  fs.writeFileSync(coveragePath,JSON.stringify(actual,null,2)+"\n");
  console.log("WROTE",coveragePath);
  process.exit(0);
}
if(!fs.existsSync(coveragePath))throw new Error("API_COVERAGE.json missing; run --write intentionally");
const expected=JSON.parse(fs.readFileSync(coveragePath,"utf8"));
const problems=[];
if(expected.plugin_typings_version!==actual.plugin_typings_version)problems.push(`typings version expected ${expected.plugin_typings_version} actual ${actual.plugin_typings_version}`);
for(const iface of INTERFACES){
  const want=expected.interfaces?.[iface]??{};
  const have=actual.interfaces[iface]??{};
  for(const name of Object.keys(have))if(!(name in want))problems.push(`UNMAPPED ${iface}.${name}`);
  for(const name of Object.keys(want))if(!(name in have))problems.push(`STALE ${iface}.${name}`);
  for(const [name,status] of Object.entries(want))if(status==="UNCLASSIFIED")problems.push(`UNCLASSIFIED ${iface}.${name}`);
}
const wantNodes=expected.scene_node_types??{};
const haveNodes=actual.scene_node_types;
for(const name of Object.keys(haveNodes))if(!(name in wantNodes))problems.push(`UNMAPPED SceneNode ${name}`);
for(const name of Object.keys(wantNodes))if(!(name in haveNodes))problems.push(`STALE SceneNode ${name}`);
if(problems.length){console.error(problems.join("\n"));process.exit(1)}
const members=INTERFACES.reduce((n,i)=>n+Object.keys(actual.interfaces[i]).length,0);
console.log(`PASS typings=${typingsVersion} interfaces=${INTERFACES.length} members=${members} scene_nodes=${Object.keys(haveNodes).length}`);
