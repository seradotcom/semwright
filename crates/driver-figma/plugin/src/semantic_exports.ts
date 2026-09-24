function sxSlug(value: string): string {
  return value.trim().replace(/[^a-zA-Z0-9_-]+/g, "-").replace(/^-+|-+$/g, "").toLowerCase() || "token";
}
function sxColor(value: any): string | null {
  if (!value || typeof value !== "object") return null;
  if (![value.r, value.g, value.b].every(Number.isFinite)) return null;
  const r=Math.round(Math.max(0,Math.min(1,value.r))*255);
  const g=Math.round(Math.max(0,Math.min(1,value.g))*255);
  const b=Math.round(Math.max(0,Math.min(1,value.b))*255);
  const a=value.a===undefined?1:Math.max(0,Math.min(1,Number(value.a)));
  return a >= 0.999 ? `#${[r,g,b].map(x=>x.toString(16).padStart(2,"0")).join("")}`
    : `rgba(${r}, ${g}, ${b}, ${Number(a.toFixed(4))})`;
}
function sxCssValue(value: any, variables: Map<string, Variable>): string {
  if (value && typeof value === "object" && value.type === "VARIABLE_ALIAS") {
    const target=variables.get(String(value.id));
    return target ? `var(--${sxSlug(target.name)})` : `var(--missing-${sxSlug(String(value.id))})`;
  }
  const color=sxColor(value); if(color) return color;
  if(typeof value==="number"||typeof value==="boolean") return String(value);
  if(typeof value==="string") return JSON.stringify(value);
  return JSON.stringify(JSON.stringify(value));
}
function sxTextArtifact(text:string,name:string,mediaType:string){
  const bytes=new TextEncoder().encode(text);
  return extraStoreArtifact(bytes,mediaType,name);
}
async function sxVariables() {
  const [collections, list]=await Promise.all([
    figma.variables.getLocalVariableCollectionsAsync(),
    figma.variables.getLocalVariablesAsync(),
  ]);
  return {
    collections:new Map(collections.map(c=>[c.id,c])),
    variables:new Map(list.map(v=>[v.id,v])),
    list,
  };
}
async function sxCss(modeName?:string):Promise<string>{
  const {collections,variables,list}=await sxVariables();
  const lines=[":root {"];
  for(const variable of [...list].sort((a,b)=>a.name.localeCompare(b.name))){
    const collection=collections.get(variable.variableCollectionId);
    if(!collection)continue;
    const mode=collection.modes.find(m=>m.name===modeName) ??
      collection.modes.find(m=>m.modeId===collection.defaultModeId) ?? collection.modes[0];
    if(!mode)continue;
    const value=variable.valuesByMode[mode.modeId];
    if(value===undefined)continue;
    lines.push(`  --${sxSlug(variable.name)}: ${sxCssValue(value,variables)};`);
  }
  lines.push("}");
  return lines.join("\n")+"\n";
}
function sxTailwindGroup(variable:Variable):"colors"|"spacing"|"borderRadius"|"fontSize"|null{
  if(variable.resolvedType==="COLOR")return "colors";
  if(variable.resolvedType!=="FLOAT")return null;
  const scopes=(variable.scopes??[]).map(String);
  if(scopes.some(x=>x.includes("CORNER_RADIUS")))return "borderRadius";
  if(scopes.some(x=>x.includes("FONT_SIZE")))return "fontSize";
  if(scopes.some(x=>/GAP|PADDING|WIDTH|HEIGHT/.test(x)))return "spacing";
  return null;
}
async function sxTailwind(modeName?:string):Promise<string>{
  const {collections,variables,list}=await sxVariables();
  const groups:Record<string,Record<string,string>>={colors:{},spacing:{},borderRadius:{},fontSize:{}};
  for(const variable of [...list].sort((a,b)=>a.name.localeCompare(b.name))){
    const group=sxTailwindGroup(variable); if(!group)continue;
    const collection=collections.get(variable.variableCollectionId); if(!collection)continue;
    const mode=collection.modes.find(m=>m.name===modeName) ??
      collection.modes.find(m=>m.modeId===collection.defaultModeId) ?? collection.modes[0];
    if(!mode)continue;
    const value=variable.valuesByMode[mode.modeId]; if(value===undefined)continue;
    const key=sxSlug(variable.name);
    groups[group][key]=variable.resolvedType==="COLOR"
      ? `var(--${key})`
      : sxCssValue(value,variables);
  }
  return [
    "export default {",
    "  theme: {",
    "    extend: "+JSON.stringify(groups,null,2).replace(/^/gm,"    ").trimStart(),
    "  }",
    "};",
    "",
  ].join("\n");
}
function sxProp(name:string,value:unknown):string{
  return `${name}={${JSON.stringify(value)}}`;
}
function sxTag(type:string):string{
  return ({FRAME:"Frame",SECTION:"Section",GROUP:"Group",COMPONENT:"Component",
    COMPONENT_SET:"ComponentSet",INSTANCE:"Instance",TEXT:"Text",RECTANGLE:"Rect",
    ELLIPSE:"Ellipse",LINE:"Line",VECTOR:"Vector",POLYGON:"Polygon",STAR:"Star",
    SLOT:"Slot"} as Record<string,string>)[type] ?? "Node";
}
async function sxJsx(node:BaseNode,depth=0,budget={count:0}):Promise<string>{
  if(depth>12||budget.count++>=512)return "";
  const n=node as any, tag=sxTag(node.type);
  const props=[sxProp("name",node.name),sxProp("figmaId",node.id),sxProp("type",node.type)];
  for(const [key,value] of [["x",n.x],["y",n.y],["w",n.width],["h",n.height],["rotation",n.rotation],["opacity",n.opacity]] as const){
    if(typeof value==="number"&&Number.isFinite(value))props.push(sxProp(key,value));
  }
  if(typeof n.layoutMode==="string"&&n.layoutMode!=="NONE")props.push(sxProp("layout",n.layoutMode));
  if(node.type==="TEXT")props.push(sxProp("text",(node as TextNode).characters.slice(0,65536)));
  if(!("children" in node)||node.children.length===0)return `<${tag} ${props.join(" ")} />`;
  const children:string[]=[];
  for(const child of node.children.slice(0,128)){
    const rendered=await sxJsx(child,depth+1,budget);if(rendered)children.push(rendered);
  }
  const indent="  ".repeat(depth+1);
  return `<${tag} ${props.join(" ")}>\n${children.map(x=>indent+x.replace(/\n/g,"\n"+indent)).join("\n")}\n${"  ".repeat(depth)}</${tag}>`;
}
function sxStyle(node:BaseNode):Record<string,string|number>{
  const n=node as any, style:Record<string,string|number>={};
  if(typeof n.width==="number")style.width=n.width;
  if(typeof n.height==="number")style.height=n.height;
  if(typeof n.opacity==="number")style.opacity=n.opacity;
  if(typeof n.rotation==="number"&&n.rotation!==0)style.transform=`rotate(${n.rotation}deg)`;
  const fill=extraSolidColor(n.fills);if(fill)style.backgroundColor=sxColor(fill)??"transparent";
  if(typeof n.cornerRadius==="number")style.borderRadius=n.cornerRadius;
  if(typeof n.layoutMode==="string"&&n.layoutMode!=="NONE"){
    style.display="flex";style.flexDirection=n.layoutMode==="VERTICAL"?"column":"row";
    if(typeof n.itemSpacing==="number")style.gap=n.itemSpacing;
  }
  return style;
}
function sxHtmlText(value:string):string{
  return value.replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;");
}
async function sxReact(node:BaseNode,depth=0,budget={count:0}):Promise<string>{
  if(depth>12||budget.count++>=512)return "";
  const n=node as any, style=sxStyle(node);
  const attrs=`data-figma-id=${JSON.stringify(node.id)} data-figma-type=${JSON.stringify(node.type)} style={${JSON.stringify(style)}}`;
  if(node.type==="TEXT"){
    return `<span ${attrs}>${sxHtmlText((node as TextNode).characters.slice(0,65536))}</span>`;
  }
  const children:string[]=[];
  if("children" in node){
    for(const child of node.children.slice(0,128)){
      const rendered=await sxReact(child,depth+1,budget);if(rendered)children.push(rendered);
    }
  }
  if(!children.length)return `<div ${attrs} />`;
  const indent="  ".repeat(depth+1);
  return `<div ${attrs}>\n${children.map(x=>indent+x.replace(/\n/g,"\n"+indent)).join("\n")}\n${"  ".repeat(depth)}</div>`;
}
async function sxStorybook(node:BaseNode):Promise<string>{
  const body=await sxReact(node,2);
  const title=JSON.stringify(`Figma/${node.name || node.type}`);
  return [
    'import type { Meta, StoryObj } from "@storybook/react";',
    'import React from "react";',
    "",
    "function FigmaSnapshot() {",
    "  return (",
    "    "+body.replace(/\n/g,"\n    "),
    "  );",
    "}",
    `const meta = { title: ${title}, component: FigmaSnapshot } satisfies Meta<typeof FigmaSnapshot>;`,
    "export default meta;",
    "type Story = StoryObj<typeof meta>;",
    "export const Snapshot: Story = {};",
    "",
  ].join("\n");
}
async function handleSemanticExports(request:BridgeRequest,a:any):Promise<BridgeResponse|null>{
  switch(request.operation){
    case "design_system.export.css": {
      const text=await sxCss(a.modeName===undefined?undefined:String(a.modeName));
      return ok(request.id,sxTextArtifact(text,String(a.name??"figma-tokens.css"),"text/css"));
    }
    case "design_system.export.tailwind": {
      const text=await sxTailwind(a.modeName===undefined?undefined:String(a.modeName));
      return ok(request.id,sxTextArtifact(text,String(a.name??"figma-tailwind.ts"),"text/typescript"));
    }
    case "node.export.jsx": {
      const node=await nodeById(String(a.nodeId));
      const text=(await sxJsx(node))+"\n";
      return ok(request.id,sxTextArtifact(text,String(a.name??sxSlug(node.name)+".figma.jsx"),"text/jsx"));
    }
    case "node.export.storybook": {
      const node=await nodeById(String(a.nodeId));
      const text=await sxStorybook(node);
      return ok(request.id,sxTextArtifact(text,String(a.name??sxSlug(node.name)+".stories.tsx"),"text/typescript"));
    }
    default:return null;
  }
}
