import {describe, expect, it} from "vitest";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import ts from "typescript";

type AnyNode = Record<string, any>;

function harness(editorType = "figma") {
  const semantic = fs.readFileSync(path.join(process.cwd(), "src/semantic_complete.ts"), "utf8");
  const more = fs.readFileSync(path.join(process.cwd(), "src/semantic_more.ts"), "utf8");
  const code = fs.readFileSync(path.join(process.cwd(), "src/code.ts"), "utf8");
  const source = semantic + "\n" + more + "\n" + code;
  const javascript = ts.transpileModule(source, {
    compilerOptions: {target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None},
  }).outputText;
  const posted: any[] = [];
  const nodes = new Map<string, AnyNode>();
  const collections = new Map<string, AnyNode>();
  const variables = new Map<string, AnyNode>();
  const images = new Map<string, AnyNode>();
  const videos = new Map<string, AnyNode>();
  const eventHandlers = new Map<string, Array<(event: any) => void>>();
  let thumbnail: AnyNode | null = null;
  let nextNode = 2, nextCollection = 1, nextVariable = 1, nextMedia = 1;

  function scene(type: string, name = type): AnyNode {
    const node: AnyNode = {
      id: `1:${nextNode++}`, type, name, visible: true, locked: false,
      x: 0, y: 0, width: 100, height: 100, rotation: 0, opacity: 1,
      layoutMode: "NONE", itemSpacing: 0, paddingTop: 0, paddingRight: 0,
      paddingBottom: 0, paddingLeft: 0, reactions: [], children: [],
      fills: [], strokes: [], effects: [], strokeWeight: 1,
      animationStyles: [], manualKeyframeTracks: [], animations: [], timelines: [],
      resize(w: number, h: number) { this.width = w; this.height = h; },
      remove() { this.parent?.children.splice(this.parent.children.indexOf(this), 1); nodes.delete(this.id); },
      clone() { const copy = scene(this.type, this.name + " copy"); copy.x=this.x; copy.y=this.y; page.appendChild(copy); return copy; },
      async setReactionsAsync(value: unknown[]) { this.reactions = value; },
      applyAnimationStyle(styleId: string, config: unknown) { this.animationStyles.push({styleId, config}); },
      removeAnimationStyle(styleId: string) { this.animationStyles = this.animationStyles.filter((x:any)=>x.styleId!==styleId); },
      applyManualKeyframeTrack(field: unknown, track: unknown) { this.manualKeyframeTracks.push({field, ...track as object}); },
      removeManualKeyframeTrack(field: unknown) { this.manualKeyframeTracks = this.manualKeyframeTracks.filter((x:any)=>JSON.stringify(x.field)!==JSON.stringify(field)); },
      setTimelineDuration(timelineId: string, duration: number) {
        const current=this.timelines.find((x:any)=>x.id===timelineId);
        if(current) current.duration=duration; else this.timelines.push({id:timelineId,duration});
      },
      setBoundVariable(field: string, variable: AnyNode) { this.boundVariables ??= {}; this.boundVariables[field]=variable.id; },
      _pluginData: {} as Record<string,string>, _relaunchData: {} as Record<string,string>,
      getPluginData(key:string){ return this._pluginData[key] ?? ""; },
      setPluginData(key:string,value:string){ this._pluginData[key]=value; },
      getRelaunchData(){ return {...this._relaunchData}; },
      setRelaunchData(value:Record<string,string>){ this._relaunchData={...value}; },
      appendChild(child:AnyNode){ if(child.parent) child.parent.children.splice(child.parent.children.indexOf(child),1); child.parent=this; this.children.push(child); },
      insertChild(index:number,child:AnyNode){ if(child.parent) child.parent.children.splice(child.parent.children.indexOf(child),1); child.parent=this; this.children.splice(index,0,child); },
    };
    nodes.set(node.id, node);
    return node;
  }

  const page: AnyNode = {
    id:"0:1", type:"PAGE", name:"Page 1", children:[], selection:[], flowStartingPoints:[],
    async loadAsync(){},
    appendChild(node:AnyNode){ node.parent=this; this.children.push(node); },
    insertChild(index:number,node:AnyNode){ if(node.parent) node.parent.children.splice(node.parent.children.indexOf(node),1); node.parent=this; this.children.splice(index,0,node); },
  };
  nodes.set(page.id, page);
  const root: AnyNode = {id:"0:0",type:"DOCUMENT",name:"Document",children:[page]};
  nodes.set(root.id, root);

  function component(): AnyNode {
    const n=scene("COMPONENT","Component");
    n.key="component-key-"+n.id; n.description=""; n.componentPropertyDefinitions={};
    n.createInstance=()=>{ const i=scene("INSTANCE",n.name); i.mainComponent=n; i.componentProperties={}; i.scaleFactor=1; i.getMainComponentAsync=async()=>i.mainComponent; i.swapComponent=(c:AnyNode)=>{i.mainComponent=c}; i.detachInstance=()=>scene("FRAME",i.name); page.appendChild(i); return i; };
    page.appendChild(n); return n;
  }

  const figma:any = {
    root, currentPage: page, editorType, mixed: Symbol("mixed"),
    ui: {onmessage: undefined, postMessage: (message:any)=>posted.push(message)},
    showUI(){},
    on(type:string, callback:(event:any)=>void){
      const list=eventHandlers.get(type)??[];
      list.push(callback);
      eventHandlers.set(type,list);
    },
    async loadAllPagesAsync(){},
    async getNodeByIdAsync(id:string){ return nodes.get(id) ?? null; },
    async setCurrentPageAsync(p:AnyNode){ this.currentPage=p; },
    createPage(){ const p:any={...page,id:`0:${nextNode++}`,name:"Page",children:[],selection:[],flowStartingPoints:[]}; nodes.set(p.id,p); root.children.push(p); return p; },
    createPageDivider(name="---"){ const p:any={...page,id:`0:${nextNode++}`,name,isPageDivider:true,children:[],selection:[],flowStartingPoints:[]};nodes.set(p.id,p);root.children.push(p);return p; },
    createFrame(){ const n=scene("FRAME","Frame"); page.appendChild(n); return n; },
    createSlice(){ const n=scene("SLICE","Slice"); page.appendChild(n); return n; },
    group(items:AnyNode[],parent:AnyNode,index?:number){ const g=scene("GROUP","Group");g.children=[];(parent??page).insertChild(index??(parent??page).children.length,g);for(const item of items)g.appendChild(item);return g; },
    ungroup(group:AnyNode){ const parent=group.parent??page;const at=parent.children.indexOf(group);const children=[...group.children];parent.children.splice(at,1,...children);for(const c of children)c.parent=parent;nodes.delete(group.id);return children; },
    async getFileThumbnailNodeAsync(){return thumbnail;},
    async setFileThumbnailNodeAsync(node:AnyNode|null){thumbnail=node;},
    commitUndo(){}, triggerUndo(){}, async loadBrushesAsync(){},
    createSection(){ const n=scene("SECTION","Section"); page.appendChild(n); return n; },
    createRectangle(){ const n=scene("RECTANGLE","Rectangle"); page.appendChild(n); return n; },
    createEllipse(){ const n=scene("ELLIPSE","Ellipse"); page.appendChild(n); return n; },
    createLine(){ const n=scene("LINE","Line"); page.appendChild(n); return n; },
    createPolygon(){ const n=scene("POLYGON","Polygon"); page.appendChild(n); return n; },
    createStar(){ const n=scene("STAR","Star"); page.appendChild(n); return n; },
    createText(){
      const n=scene("TEXT","Text");
      n.characters="";
      n.fontName={family:"Inter",style:"Regular"};
      n.fontSize=16;
      n.textAlignHorizontal="LEFT";
      n.textAlignVertical="TOP";
      n.textAutoResize="WIDTH_AND_HEIGHT";
      n.getRangeAllFontNames=()=>[n.fontName];
      page.appendChild(n);
      return n;
    },
    createComponent: component,
    createComponentFromNode(){ return component(); },
    combineAsVariants(cs:AnyNode[]){ const n=scene("COMPONENT_SET","Variants"); n.children=cs; for(const c of cs)c.parent=n; page.appendChild(n); return n; },
    createSticky(){ const n=scene("STICKY","Sticky"); page.appendChild(n); return n; },
    createShapeWithText(){ const n=scene("SHAPE_WITH_TEXT","Shape"); page.appendChild(n); return n; },
    createConnector(){ const n=scene("CONNECTOR","Connector"); page.appendChild(n); return n; },
    createCodeBlock(){ const n=scene("CODE_BLOCK","Code"); n.code=""; page.appendChild(n); return n; },
    createImage(data:Uint8Array){ const hash=`image:${nextMedia++}`;const bytes=new Uint8Array(data);const image={hash,async getBytesAsync(){return bytes},async getSizeAsync(){return {width:1,height:1}}};images.set(hash,image);return image; },
    getImageByHash(hash:string){return images.get(hash)??null;},
    async createVideoAsync(data:Uint8Array){const hash=`video:${nextMedia++}`;const video={hash,bytes:new Uint8Array(data)};videos.set(hash,video);return video;},
    async loadFontAsync(){},
    async getLocalPaintStylesAsync(){return[];}, async getLocalTextStylesAsync(){return[];},
    async getLocalEffectStylesAsync(){return[];}, async getLocalGridStylesAsync(){return[];},
    variables: {
      async getLocalVariableCollectionsAsync(){return [...collections.values()];},
      async getLocalVariablesAsync(){return [...variables.values()];},
      createVariableCollection(name:string){
        const id=`vc:${nextCollection++}`; const c:any={id,name,modes:[{modeId:"m:1",name:"Mode 1"}],defaultModeId:"m:1",variableIds:[],addMode(n:string){const id=`m:${this.modes.length+1}`;this.modes.push({modeId:id,name:n});return id;}};
        collections.set(id,c); return c;
      },
      createVariable(name:string,c:AnyNode,resolvedType:string){
        const id=`v:${nextVariable++}`; const v:any={id,name,resolvedType,valuesByMode:{},setValueForMode(mode:string,value:unknown){this.valuesByMode[mode]=value;}};
        variables.set(id,v); c.variableIds.push(id); return v;
      },
      async getVariableCollectionByIdAsync(id:string){return collections.get(id)??null;},
      async getVariableByIdAsync(id:string){return variables.get(id)??null;},
      createVariableAlias(v:AnyNode){return {type:"VARIABLE_ALIAS",id:v.id};},
    },
    motion: {
      figmaAnimationStyles(){return [{id:"spring",name:"Spring"}];},
      physicalSpringToNormalized(){return 0.5;},
      playheadPosition: 0.75,
    },
  };

  vm.runInNewContext(javascript, {
    figma, __html__:"", console, setTimeout, clearTimeout,
    atob: globalThis.atob, btoa: globalThis.btoa, crypto: globalThis.crypto,
  });
  if (typeof figma.ui.onmessage !== "function") throw new Error("plugin did not install UI message handler");

  async function call(operation:string,args:Record<string,unknown>={},expectedRevision?:number){
    posted.length=0;
    await figma.ui.onmessage({type:"bridge-request",request:{id:"req",sessionId:"s",generation:1,expectedRevision,operation,args}});
    const message=posted.find(x=>x.type==="bridge-response");
    if(!message) throw new Error("plugin did not respond");
    return message.response;
  }
  function emit(type:string,event:any){
    for(const callback of eventHandlers.get(type)??[]) callback(event);
  }
  return {call, figma, nodes, page, posted, emit};
}

describe("plugin runtime behavior",()=>{
  it("mutates layout and enforces revision preconditions",async()=>{
    const h=harness();
    const created=await h.call("frame.create",{name:"Card",width:320,height:180});
    expect(created.ok).toBe(true); expect(created.revision).toBe(1);
    const id=created.value.id;
    const layout=await h.call("layout.patch",{nodeId:id,layoutMode:"HORIZONTAL",itemSpacing:16},1);
    expect(layout.ok).toBe(true); expect(layout.revision).toBe(2);
    expect(layout.value.layoutMode).toBe("HORIZONTAL");
    const stale=await h.call("node.get",{nodeId:id},1);
    expect(stale.ok).toBe(false); expect(stale.error.code).toBe("conflict");
    const fresh=await h.call("node.get",{nodeId:id},2);
    expect(fresh.ok).toBe(true); expect(fresh.value.name).toBe("Card");
  });

  it("creates variables and extracts a design system",async()=>{
    const h=harness();
    const c=await h.call("variable.collection.create",{name:"Theme"});
    const cid=c.value.id;
    const v=await h.call("variable.create",{collectionId:cid,name:"brand/primary",resolvedType:"COLOR"},1);
    const vid=v.value.id;
    const set=await h.call("variable.set_value",{variableId:vid,modeId:"m:1",value:{r:1,g:0,b:0,a:1}},2);
    expect(set.ok).toBe(true);
    const ds=await h.call("design_system.extract",{},3);
    expect(ds.ok).toBe(true);
    expect(ds.value.collections[0].name).toBe("Theme");
    expect(ds.value.variables[0].name).toBe("brand/primary");
  });

  it("executes prototype and Motion handlers",async()=>{
    const h=harness();
    const created=await h.call("frame.create",{name:"Interactive"});
    const id=created.value.id;
    const reactions=[{trigger:{type:"ON_CLICK"},actions:[]}];
    expect((await h.call("prototype.reaction.set",{nodeId:id,reactions},1)).ok).toBe(true);
    expect((await h.call("prototype.reaction.list",{nodeId:id},2)).value).toHaveLength(1);
    expect((await h.call("motion.style.apply",{nodeId:id,styleId:"spring",duration:0.4},2)).ok).toBe(true);
    expect((await h.call("motion.keyframe.apply",{nodeId:id,field:{type:"x"},track:{keyframes:[{t:0,value:0},{t:1,value:100}]}},3)).ok).toBe(true);
    expect((await h.call("motion.timeline.set_duration",{nodeId:id,timelineId:"main",duration:1.2},4)).ok).toBe(true);
    const inspected=await h.call("motion.node.inspect",{nodeId:id},5);
    expect(inspected.ok).toBe(true);
    expect(inspected.value.animationStyles).toHaveLength(1);
    expect(inspected.value.manualKeyframeTracks).toHaveLength(1);
    expect(inspected.value.timelines[0].duration).toBe(1.2);
  });

  it("creates semantic FigJam nodes and connectors",async()=>{
    const h=harness("figjam");
    const a=await h.call("figjam.sticky.create",{name:"Agent"});
    const b=await h.call("figjam.shape.create",{name:"Semwright"},1);
    const c=await h.call("figjam.connector.create",{from:a.value.id,to:b.value.id},2);
    expect(c.ok).toBe(true);
    const connector=h.nodes.get(c.value.id)!;
    expect(connector.connectorStart.endpointNodeId).toBe(a.value.id);
    expect(connector.connectorEnd.endpointNodeId).toBe(b.value.id);
  });

  it("exercises text, styling, components, variants and instances",async()=>{
    const h=harness();
    const text=await h.call("text.create",{name:"Headline",characters:"Semwright"});
    expect(text.ok).toBe(true);
    expect(text.value.characters).toBe("Semwright");
    const patched=await h.call("text.patch",{nodeId:text.value.id,characters:"Semantic I/O"},1);
    expect(patched.ok).toBe(true);
    expect(patched.value.characters).toBe("Semantic I/O");

    const rect=await h.call("rect.create",{name:"Card"},2);
    const id=rect.value.id;
    expect((await h.call("paint.patch",{nodeId:id,r:0.1,g:0.2,b:0.3,opacity:0.9},3)).ok).toBe(true);
    expect((await h.call("stroke.patch",{nodeId:id,r:1,g:1,b:1,weight:2},4)).ok).toBe(true);
    expect((await h.call("effects.patch",{nodeId:id,effects:[]},5)).ok).toBe(true);

    const primary=await h.call("component.create",{name:"Button / Primary"},6);
    const secondary=await h.call("component.create",{name:"Button / Secondary"},7);
    const set=await h.call("component_set.create",{name:"Button",componentIds:[primary.value.id,secondary.value.id]},8);
    expect(set.ok).toBe(true);
    const variants=await h.call("variant.list",{nodeId:set.value.id},9);
    expect(variants.value).toHaveLength(2);

    const instance=await h.call("instance.create",{componentId:primary.value.id},9);
    expect(instance.ok).toBe(true);
    expect((await h.call("instance.swap",{nodeId:instance.value.id,componentId:secondary.value.id},10)).ok).toBe(true);
    const inspected=await h.call("instance.inspect",{nodeId:instance.value.id},11);
    expect(inspected.value.mainComponent.id).toBe(secondary.value.id);
    expect((await h.call("instance.detach",{nodeId:instance.value.id},11)).ok).toBe(true);
  });

  it("invalidates stale plans on remote collaborator changes only",async()=>{
    const h=harness();
    await new Promise(resolve=>setTimeout(resolve,0));
    expect((await h.call("document.status")).value.revision).toBe(0);

    h.emit("documentchange",{documentChanges:[{origin:"LOCAL",id:"1:2",type:"PROPERTY_CHANGE"}]});
    expect((await h.call("document.status")).value.revision).toBe(0);

    h.emit("documentchange",{documentChanges:[{origin:"REMOTE",id:"1:2",type:"PROPERTY_CHANGE"}]});
    expect(h.posted.some(message=>message.type==="event"&&message.kind==="documentchange"&&message.revision===1)).toBe(true);
    const status=await h.call("document.status");
    expect(status.value.revision).toBe(1);

    const stale=await h.call("frame.create",{name:"Stale"},0);
    expect(stale.ok).toBe(false);
    expect(stale.error.code).toBe("conflict");
    const fresh=await h.call("frame.create",{name:"Fresh"},1);
    expect(fresh.ok).toBe(true);
    expect(fresh.revision).toBe(2);
  });
});


describe("semantic completeness runtime",()=>{
  it("builds a nested declarative composition without executable JSX",async()=>{
    const h=harness();
    const result=await h.call("compose.apply",{root:{
      type:"frame",name:"Card",width:320,height:180,
      layout:{layoutMode:"VERTICAL",itemSpacing:12},
      children:[{type:"text",name:"Title",text:"Semantic Figma"}]
    }});
    expect(result.ok).toBe(true);
    expect(result.value.name).toBe("Card");
    expect(result.value.children).toHaveLength(1);
    expect(result.value.children[0].name).toBe("Title");
    expect(result.revision).toBe(1);
  });

  it("snapshots the document and produces a bounded semantic diff",async()=>{
    const h=harness();
    await h.call("frame.create",{name:"Before"});
    const snapshot=await h.call("document.snapshot",{},1);
    expect(snapshot.ok).toBe(true);
    const after=JSON.parse(JSON.stringify(snapshot.value));
    after.name="Changed";
    const diff=await h.call("document.diff",{before:snapshot.value,after,limit:20},1);
    expect(diff.ok).toBe(true);
    expect(diff.value.changes.some((x:any)=>x.path==="/name")).toBe(true);
  });

  it("runs deterministic validation and creates a FigJam graph",async()=>{
    const h=harness();
    const small=await h.call("rect.create",{name:"Tiny",width:8,height:8});
    expect(small.ok).toBe(true);
    const a11y=await h.call("validate.a11y",{minTouchTarget:44},1);
    expect(a11y.ok).toBe(true);
    expect(a11y.value.findings.some((x:any)=>x.rule==="a11y.touch_target")).toBe(true);

    const j=harness("figjam");
    const graph=await j.call("figjam.diagram.create",{
      nodes:[{id:"a",kind:"sticky",name:"Agent"},{id:"b",kind:"shape",name:"Semwright"}],
      edges:[{from:"a",to:"b"}]
    });
    expect(graph.ok).toBe(true);
    expect(graph.value.nodes).toHaveLength(2);
    expect(graph.value.edges).toHaveLength(1);
  });
});

describe("extended semantic runtime",()=> {
  it("groups and ungroups explicit nodes with revision tracking",async()=> {
    const h=harness();
    const a=await h.call("rect.create",{name:"A"});
    const b=await h.call("ellipse.create",{name:"B"},1);
    const grouped=await h.call("group.create",{nodeIds:[a.value.id,b.value.id],name:"Pair"},2);
    expect(grouped.ok).toBe(true);
    expect(grouped.value.type).toBe("GROUP");
    expect(grouped.revision).toBe(3);
    const ungrouped=await h.call("group.ungroup",{nodeId:grouped.value.id},3);
    expect(ungrouped.ok).toBe(true);
    expect(ungrouped.revision).toBe(4);
  });

  it("uploads bounded bytes, creates an image and applies it semantically",async()=> {
    const h=harness();
    const upload=await h.call("artifact.upload.begin",{name:"pixel.bin",mediaType:"application/octet-stream"});
    expect(upload.ok).toBe(true);
    expect(upload.revision).toBe(0);
    const chunk=await h.call("artifact.upload.append",{token:upload.value.token,offset:0,base64:"AQIDBA=="});
    expect(chunk.value.bytes).toBe(4);
    expect(chunk.revision).toBe(0);
    const image=await h.call("image.create",{token:upload.value.token});
    expect(image.ok).toBe(true);
    expect(image.value.hash).toMatch(/^image:/);
    expect(image.revision).toBe(1);
    const rect=await h.call("rect.create",{name:"Media"},1);
    const applied=await h.call("media.fill.apply",{nodeId:rect.value.id,mediaType:"IMAGE",hash:image.value.hash},2);
    expect(applied.ok).toBe(true);
    expect(applied.revision).toBe(3);
    expect(h.nodes.get(rect.value.id)!.fills[0].imageHash).toBe(image.value.hash);
  });

  it("exposes Motion playhead and bounded Semwright plugin metadata",async()=> {
    const h=harness();
    const frame=await h.call("frame.create",{name:"Metadata"});
    const playhead=await h.call("motion.playhead.get",{},1);
    expect(playhead.value.position).toBe(0.75);
    const set=await h.call("node.plugin_data.set",{nodeId:frame.value.id,key:"semantic-role",value:"hero"},1);
    expect(set.ok).toBe(true);
    const get=await h.call("node.plugin_data.get",{nodeId:frame.value.id,key:"semantic-role"},2);
    expect(get.value.value).toBe("hero");
  });
});
