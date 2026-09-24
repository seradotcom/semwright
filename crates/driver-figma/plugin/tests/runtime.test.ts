import {describe, expect, it} from "vitest";
import fs from "node:fs";
import path from "node:path";
import vm from "node:vm";
import ts from "typescript";

type AnyNode = Record<string, any>;

function harness(editorType = "figma") {
  const generated = fs.readFileSync(path.join(process.cwd(), "src/generated_api_surface.ts"), "utf8");
  const properties = fs.readFileSync(path.join(process.cwd(), "src/semantic_properties.ts"), "utf8");
  const semantic = fs.readFileSync(path.join(process.cwd(), "src/semantic_complete.ts"), "utf8");
  const more = fs.readFileSync(path.join(process.cwd(), "src/semantic_more.ts"), "utf8");
  const product = fs.readFileSync(path.join(process.cwd(), "src/semantic_product.ts"), "utf8");
  const exports = fs.readFileSync(path.join(process.cwd(), "src/semantic_exports.ts"), "utf8");
  const admin = fs.readFileSync(path.join(process.cwd(), "src/semantic_admin.ts"), "utf8");
  const verification = fs.readFileSync(path.join(process.cwd(), "src/semantic_verification.ts"), "utf8");
  const code = fs.readFileSync(path.join(process.cwd(), "src/code.ts"), "utf8");
  const source = generated + "\n" + properties + "\n" + semantic + "\n" + more + "\n" + product + "\n" + exports + "\n" + admin + "\n" + verification + "\n" + code;
  const javascript = ts.transpileModule(source, {
    compilerOptions: {target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.None},
  }).outputText;
  const posted: any[] = [];
  const nodes = new Map<string, AnyNode>();
  const collections = new Map<string, AnyNode>();
  const variables = new Map<string, AnyNode>();
  const images = new Map<string, AnyNode>();
  const videos = new Map<string, AnyNode>();
  const styles = new Map<string, AnyNode>();
  const annotationCategories = new Map<string, AnyNode>();
  const loadedFonts: AnyNode[] = [];
  const eventHandlers = new Map<string, Array<(event: any) => void>>();
  let thumbnail: AnyNode | null = null;
  let slideGrid: AnyNode[][] = [];
  let paymentStatus:{type:"UNPAID"|"PAID"|"NOT_SUPPORTED"}={type:"UNPAID"};
  let checkoutRequests=0;
  let nextNode = 2, nextCollection = 1, nextVariable = 1, nextMedia = 1, nextStyle = 1, nextComponentProperty = 1;

  function scene(type: string, name = type): AnyNode {
    const node: AnyNode = {
      id: `1:${nextNode++}`, type, name, visible: true, locked: false,
      x: 0, y: 0, width: 100, height: 100, rotation: 0, opacity: 1,
      layoutMode: "NONE", itemSpacing: 0, paddingTop: 0, paddingRight: 0,
      paddingBottom: 0, paddingLeft: 0, clipsContent: false, reactions: [], children: [],
      fills: [], strokes: [], effects: [], strokeWeight: 1,
      animationStyles: [], manualKeyframeTracks: [], animations: [], timelines: [],
      resize(w: number, h: number) { this.width = w; this.height = h; },
      remove() { this.parent?.children.splice(this.parent.children.indexOf(this), 1); nodes.delete(this.id); },
      clone() {
        const copy = scene(this.type, this.name + " copy");
        copy.x=this.x; copy.y=this.y; copy.width=this.width; copy.height=this.height;
        copy.fills=JSON.parse(JSON.stringify(this.fills)); copy.strokes=JSON.parse(JSON.stringify(this.strokes));
        copy.effects=JSON.parse(JSON.stringify(this.effects)); page.appendChild(copy); return copy;
      },
      async exportAsync() { return new Uint8Array([137,80,78,71,13,10,26,10,1,2,3]); },
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
    id:"0:1", type:"PAGE", name:"Page 1", children:[], selection:[], flowStartingPoints:[], focusedNode:null,
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
    n.addComponentProperty=(name:string,type:string,defaultValue:any,options:AnyNode={})=>{
      const id=`${name}#${nextComponentProperty++}`;
      n.componentPropertyDefinitions[id]={type,defaultValue,...JSON.parse(JSON.stringify(options))};
      return id;
    };
    n.editComponentProperty=(propertyName:string,patch:AnyNode)=>{
      const current=n.componentPropertyDefinitions[propertyName];
      if(!current)throw new Error("component_property_not_found");
      n.componentPropertyDefinitions[propertyName]={...current,...JSON.parse(JSON.stringify(patch))};
      return propertyName;
    };
    n.deleteComponentProperty=(propertyName:string)=>{delete n.componentPropertyDefinitions[propertyName];};
    n.createSlot=()=>{
      const slot=scene("SLOT","Slot");
      const propertyName=n.addComponentProperty(slot.name,"SLOT","",{});
      slot.componentPropertyReferences={slotContentId:propertyName};
      slot.limitViolations=[];
      slot.resetSlot=()=>{};
      n.appendChild(slot);
      return slot;
    };
    n.createInstance=()=>{ const i=scene("INSTANCE",n.name); i.mainComponent=n; i.componentProperties={}; i.scaleFactor=1; i.getMainComponentAsync=async()=>i.mainComponent; i.swapComponent=(c:AnyNode)=>{i.mainComponent=c}; i.detachInstance=()=>scene("FRAME",i.name); page.appendChild(i); return i; };
    page.appendChild(n); return n;
  }

  function style(type:"PAINT"|"TEXT"|"EFFECT"|"GRID"):AnyNode{
    const id="S:"+(nextStyle++);
    const value:any={id,key:"style-key-"+id,name:type+" Style",type,remote:false,description:"",descriptionMarkdown:""};
    if(type==="PAINT")value.paints=[];
    if(type==="TEXT"){value.fontName={family:"Inter",style:"Regular"};value.fontSize=16;value.letterSpacing={unit:"PIXELS",value:0};value.lineHeight={unit:"AUTO"};}
    if(type==="EFFECT")value.effects=[];
    if(type==="GRID")value.layoutGrids=[];
    value.remove=()=>styles.delete(id);
    styles.set(id,value);
    return value;
  }

  annotationCategories.set("cat:1",{id:"cat:1",label:"Review",color:"yellow",isPreset:false});
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
    async getStyleByIdAsync(id:string){ return styles.get(id) ?? null; },
    async setCurrentPageAsync(p:AnyNode){ this.currentPage=p; },
    mode: editorType==="dev"?"codegen":"default", command:"",
    currentUser:{id:"user:1",name:"Sergio Test",photoUrl:null,color:"#ff0000",sessionId:1},
    activeUsers:[{id:"user:1",name:"Sergio Test",photoUrl:null,color:"#ff0000",sessionId:1,position:{x:10,y:20},viewport:{x:0,y:0,width:800,height:600},selection:[]}],
    payments:{
      get status(){return paymentStatus;},
      setPaymentStatusInDevelopment(status:{type:"UNPAID"|"PAID"|"NOT_SUPPORTED"}){paymentStatus={...status};},
      getUserFirstRanSecondsAgo(){return 123;},
      async initiateCheckoutAsync(){paymentStatus={type:"PAID"};},
      requestCheckout(){checkoutRequests++;},
      async getPluginPaymentTokenAsync(){return "TEST_ONLY_INTERNAL_PAYMENT_TOKEN";},
    },
    codegen:{
      preferences:{unit:"PIXEL",scaleFactor:undefined,customSettings:{}},
      refresh(){figma.codegenRefreshes++;},
      on(type:string,callback:(event:any)=>unknown){figma.codegenHandlers.set(type,callback);}
    },
    codegenRefreshes:0,
    codegenHandlers:new Map<string,(event:any)=>unknown>(),
    textreview:{
      isEnabled:false,
      async requestToBeEnabledAsync(){this.isEnabled=true;},
      async requestToBeDisabledAsync(){this.isEnabled=false;}
    },
    annotations:{
      async getAnnotationCategoriesAsync(){return [...annotationCategories.values()];},
      async getAnnotationCategoryByIdAsync(id:string){return annotationCategories.get(id)??null;},
      async addAnnotationCategoryAsync(input:any){
        const id="cat:"+(annotationCategories.size+1);
        const c:any={id,label:input.label,color:input.color,isPreset:false};
        c.remove=()=>annotationCategories.delete(id);c.setColor=(color:string)=>{c.color=color};c.setLabel=(label:string)=>{c.label=label};
        annotationCategories.set(id,c);return c;
      },
    },
    createPage(){ const p:any={...page,id:`0:${nextNode++}`,name:"Page",children:[],selection:[],flowStartingPoints:[],focusedNode:null}; nodes.set(p.id,p); root.children.push(p); return p; },
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
    loadedFonts,
    async loadFontAsync(font:AnyNode){loadedFonts.push(JSON.parse(JSON.stringify(font)));},
    async listAvailableFontsAsync(){return [{fontName:{family:"Inter",style:"Regular"}}];},
    getFontFamilyVariationAxes(family:string){return family==="Inter"?["wght","slnt"]:null;},
    createPaintStyle(){return style("PAINT");}, createTextStyle(){return style("TEXT");},
    createEffectStyle(){return style("EFFECT");}, createGridStyle(){return style("GRID");},
    async getLocalPaintStylesAsync(){return [...styles.values()].filter(s=>s.type==="PAINT");},
    async getLocalTextStylesAsync(){return [...styles.values()].filter(s=>s.type==="TEXT");},
    async getLocalEffectStylesAsync(){return [...styles.values()].filter(s=>s.type==="EFFECT");},
    async getLocalGridStylesAsync(){return [...styles.values()].filter(s=>s.type==="GRID");},
    moveLocalPaintStyleAfter(){},moveLocalTextStyleAfter(){},moveLocalEffectStyleAfter(){},moveLocalGridStyleAfter(){},
    moveLocalPaintFolderAfter(){},moveLocalTextFolderAfter(){},moveLocalEffectFolderAfter(){},moveLocalGridFolderAfter(){},
    createSlide(){const n=scene("SLIDE","Slide");page.appendChild(n);slideGrid.push([n]);return n;},
    createSlideRow(){const n=scene("SLIDEROW","Slide Row");page.appendChild(n);return n;},
    getSlideGrid(){return slideGrid;},
    setSlideGrid(value:AnyNode[][]){slideGrid=value.map(row=>[...row]);},
    variables: {
      async getLocalVariableCollectionsAsync(){return [...collections.values()];},
      async getLocalVariablesAsync(){return [...variables.values()];},
      createVariableCollection(name:string){
        const id=`vc:${nextCollection++}`; const c:any={id,name,modes:[{modeId:"m:1",name:"Mode 1"}],defaultModeId:"m:1",variableIds:[],addMode(n:string){const id=`m:${this.modes.length+1}`;this.modes.push({modeId:id,name:n});return id;}};
        collections.set(id,c); return c;
      },
      createVariable(name:string,c:AnyNode,resolvedType:string){
        const id=`v:${nextVariable++}`; const v:any={
          id,name,resolvedType,variableCollectionId:c.id,valuesByMode:{},scopes:[],
          setValueForMode(mode:string,value:unknown){this.valuesByMode[mode]=value;}
        };
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
    TextEncoder: globalThis.TextEncoder, TextDecoder: globalThis.TextDecoder,
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
  return {call, figma, nodes, page, posted, emit, scene};
}

async function readArtifact(h:ReturnType<typeof harness>,token:string):Promise<string>{
  let offset=0;
  const parts:number[]=[];
  while(true){
    const result=await h.call("artifact.read",{token,offset,length:4096});
    if(!result.ok)throw new Error("artifact read failed");
    const raw=globalThis.atob(String(result.value.base64));
    for(let i=0;i<raw.length;i++)parts.push(raw.charCodeAt(i));
    offset=Number(result.value.nextOffset);
    if(result.value.eof)break;
  }
  return new TextDecoder().decode(new Uint8Array(parts));
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

  it("supports Update 139 composed colors and COLOR_OPACITY scope with strict validation",async()=>{
    const h=harness();
    const collection=await h.call("variable.collection.create",{name:"Theme"});
    const opacity=await h.call("variable.create",{
      collectionId:collection.value.id,name:"opacity/disabled",resolvedType:"FLOAT"
    },1);
    const color=await h.call("variable.create",{
      collectionId:collection.value.id,name:"color/disabled",resolvedType:"COLOR"
    },2);

    const composed=await h.call("variable.set_value",{
      variableId:color.value.id,modeId:"m:1",
      value:{color:{r:0.2,g:0.4,b:0.8},opacity:{type:"VARIABLE_ALIAS",id:opacity.value.id}}
    },3);
    expect(composed.ok).toBe(true);

    const invalid=await h.call("variable.set_value",{
      variableId:color.value.id,modeId:"m:1",
      value:{color:{r:0.2,g:0.4,b:0.8},opacity:0.5}
    },4);
    expect(invalid.ok).toBe(false);
    expect(invalid.error.message).toContain("variable_value_type_mismatch");

    const scoped=await h.call("variable.scopes.set",{
      variableId:color.value.id,scopes:["COLOR_OPACITY","ALL_FILLS"]
    },4);
    expect(scoped.ok).toBe(true);
    expect(scoped.value.scopes).toContain("COLOR_OPACITY");

    const badScope=await h.call("variable.scopes.set",{
      variableId:color.value.id,scopes:["MADE_UP_SCOPE"]
    },5);
    expect(badScope.ok).toBe(false);
    expect(badScope.error.message).toContain("invalid_variable_scope");
  });

  it("executes prototype and official Figma Motion keyframe handlers",async()=>{
    const h=harness();
    const created=await h.call("frame.create",{name:"Interactive"});
    const id=created.value.id;
    const reactions=[{trigger:{type:"ON_CLICK"},actions:[]}];
    expect((await h.call("prototype.reaction.set",{nodeId:id,reactions},1)).ok).toBe(true);
    expect((await h.call("prototype.reaction.list",{nodeId:id},2)).value).toHaveLength(1);
    expect((await h.call("motion.style.apply",{nodeId:id,styleId:"spring",duration:0.4},2)).ok).toBe(true);
    const keyframe=await h.call("motion.keyframe.apply",{
      nodeId:id,
      field:{type:"PROPERTY",name:"TRANSLATION_X"},
      track:{keyframes:[
        {timelinePosition:0,value:{type:"FLOAT",value:0}},
        {timelinePosition:1,value:{type:"FLOAT",value:100},easing:{type:"EASE_OUT"}}
      ]}
    },3);
    expect(keyframe.ok).toBe(true);
    expect(keyframe.value.field).toEqual({type:"PROPERTY",name:"TRANSLATION_X"});
    expect(keyframe.value.end).toBe(1);

    const legacy=await h.call("motion.keyframe.apply",{
      nodeId:id,field:{type:"x"},track:{keyframes:[{t:0,value:0}]}
    },4);
    expect(legacy.ok).toBe(false);
    expect(legacy.error.message).toContain("invalid_motion_field");

    expect((await h.call("motion.timeline.set_duration",{nodeId:id,timelineId:"main",duration:1.2},4)).ok).toBe(true);
    const inspected=await h.call("motion.node.inspect",{nodeId:id},5);
    expect(inspected.ok).toBe(true);
    expect(inspected.value.animationStyles).toHaveLength(1);
    expect(inspected.value.manualKeyframeTracks).toHaveLength(1);
    expect(inspected.value.timelines[0].duration).toBe(1.2);
  });

  it("provides typed Motion presets, multi-track apply and stagger without eval",async()=>{
    const h=harness();
    const a=await h.call("frame.create",{name:"A"});
    const b=await h.call("frame.create",{name:"B"},1);

    const preset=await h.call("motion.preset.apply",{
      nodeId:a.value.id,preset:"fade-up",duration:0.4,at:0.1,easing:"ease-out",distance:32
    },2);
    expect(preset.ok).toBe(true);
    expect(preset.value.fields).toHaveLength(2);
    expect(h.nodes.get(a.value.id)?.manualKeyframeTracks).toHaveLength(2);

    const stagger=await h.call("motion.stagger",{
      nodeIds:[a.value.id,b.value.id],preset:"pop",duration:0.3,step:0.12,easing:"quick"
    },3);
    expect(stagger.ok).toBe(true);
    expect(stagger.value.results).toHaveLength(2);
    expect(stagger.value.results[1].offset).toBeCloseTo(0.12);

    const applied=await h.call("motion.apply",{tracks:[{
      nodeId:b.value.id,
      field:{type:"PROPERTY",name:"OPACITY"},
      track:{baseValue:{type:"FLOAT",value:0},keyframes:[
        {timelinePosition:0,value:{type:"FLOAT",value:0}},
        {timelinePosition:0.25,value:{type:"FLOAT",value:1},easing:{type:"QUICK"}}
      ]}
    }]},4);
    expect(applied.ok).toBe(true);
    expect(applied.value.results[0].field).toEqual({type:"PROPERTY",name:"OPACITY"});
  });

  it("converts frames into native slot properties and patches SlotSettings",async()=>{
    const h=harness();
    const component=await h.call("component.create",{name:"Card"});
    const owner=h.nodes.get(component.value.id)!;
    const frame=h.scene("FRAME","Content");
    owner.appendChild(frame);

    const converted=await h.call("slot.convert",{
      nodeId:frame.id,
      name:"Content",
      componentKeys:["remote-component-key"],
      description:"Replaceable body content",
      slotSettings:{minChildren:1,maxChildren:4,stretchChildOnInsert:true,allowPreferredValuesOnly:true}
    },1);
    expect(converted.ok).toBe(true);
    const propertyName=converted.value.propertyName;
    expect(frame.componentPropertyReferences.slotContentId).toBe(propertyName);
    expect(owner.componentPropertyDefinitions[propertyName].type).toBe("SLOT");
    expect(owner.componentPropertyDefinitions[propertyName].slotSettings.minChildren).toBe(1);

    const patched=await h.call("slot.settings.patch",{
      nodeId:frame.id,
      slotSettings:{maxChildren:6,displayEmptyByDefault:true}
    },2);
    expect(patched.ok).toBe(true);
    expect(patched.value.slotSettings).toMatchObject({
      minChildren:1,maxChildren:6,displayEmptyByDefault:true,stretchChildOnInsert:true
    });

    const invalid=await h.call("slot.settings.patch",{
      nodeId:frame.id,slotSettings:{minChildren:10,maxChildren:2}
    },3);
    expect(invalid.ok).toBe(false);
    expect(invalid.error.message).toContain("invalid_slot_limits");
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

  it("sets an instance main component explicitly and attaches FigJam stickable nodes",async()=>{
    const h=harness();
    const primary=await h.call("component.create",{name:"Primary"});
    const secondary=await h.call("component.create",{name:"Secondary"},1);
    const instance=await h.call("instance.create",{componentId:primary.value.id},2);
    const reassigned=await h.call("instance.main_component.set",{
      nodeId:instance.value.id,componentId:secondary.value.id
    },3);
    expect(reassigned.ok).toBe(true);
    expect(reassigned.value).toEqual({
      nodeId:instance.value.id,
      mainComponentId:secondary.value.id,
      overridesCleared:true,
    });
    expect(h.nodes.get(instance.value.id)!.mainComponent.id).toBe(secondary.value.id);
    const genericMainComponent=await h.call("node.properties.patch",{
      nodeId:instance.value.id,properties:{mainComponent:primary.value.id}
    },4);
    expect(genericMainComponent.ok).toBe(false);
    expect(genericMainComponent.error.message).toContain("property_not_writable");

    const figjam=harness("figjam");
    const stamp=figjam.scene("STAMP","Stamp");
    stamp.stuckTo=null;
    figjam.page.appendChild(stamp);
    const target=await figjam.call("figjam.shape.create",{name:"Target"});
    const attached=await figjam.call("figjam.stuck_to.set",{
      nodeId:stamp.id,targetNodeId:target.value.id
    },1);
    expect(attached.ok).toBe(true);
    expect(attached.value.targetNodeId).toBe(target.value.id);
    expect(figjam.nodes.get(stamp.id)!.stuckTo.id).toBe(target.value.id);
    const detached=await figjam.call("figjam.stuck_to.set",{
      nodeId:stamp.id,targetNodeId:null
    },2);
    expect(detached.ok).toBe(true);
    expect(detached.value.targetNodeId).toBeNull();
    expect(figjam.nodes.get(stamp.id)!.stuckTo).toBeNull();
    const genericStuckTo=await figjam.call("node.properties.patch",{
      nodeId:stamp.id,properties:{stuckTo:target.value.id}
    },3);
    expect(genericStuckTo.ok).toBe(false);
    expect(genericStuckTo.error.message).toContain("property_not_writable");

    const sticky=await figjam.call("figjam.sticky.create",{name:"Not Stickable"},3);
    const denied=await figjam.call("figjam.stuck_to.set",{
      nodeId:sticky.value.id,targetNodeId:target.value.id
    },4);
    expect(denied.ok).toBe(false);
    expect(denied.error.message).toContain("node_not_stickable");
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

  it("queries the scene graph with bounded semantic predicates instead of XPath",async()=>{
    const h=harness();
    const frame=await h.call("frame.create",{name:"Pricing Card"});
    const text=await h.call("text.create",{name:"Title",characters:"Semantic Figma"},1);
    expect((await h.call("node.reparent",{nodeId:text.value.id,parentId:frame.value.id},2)).ok).toBe(true);

    const result=await h.call("node.query",{
      rootId:frame.value.id,
      types:["TEXT"],
      textContains:"semantic",
      predicates:[{field:"fontSize",op:"gte",value:16}],
      maxDepth:4,
      limit:20
    },3);
    expect(result.ok).toBe(true);
    expect(result.value.matches).toHaveLength(1);
    expect(result.value.matches[0].id).toBe(text.value.id);
    expect(result.value.visited).toBeGreaterThanOrEqual(2);

    const denied=await h.call("node.query",{
      predicates:[{field:"__proto__",op:"exists"}]
    },3);
    expect(denied.ok).toBe(false);
    expect(denied.error.message).toContain("query_property_not_allowlisted");
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

  it("reads and patches generated public property surfaces",async()=> {
    const h=harness();
    const frame=await h.call("frame.create",{name:"Property Surface"});
    const patched=await h.call("node.properties.patch",{
      nodeId:frame.value.id,
      properties:{itemSpacing:24,clipsContent:true}
    },1);
    expect(patched.ok).toBe(true);
    expect(patched.value.values.itemSpacing).toBe(24);

    const inspected=await h.call("node.properties.inspect",{
      nodeId:frame.value.id,
      properties:["id","name","itemSpacing","clipsContent"]
    },2);
    expect(inspected.ok).toBe(true);
    expect(inspected.value.values.id).toBe(frame.value.id);
    expect(inspected.value.values.itemSpacing).toBe(24);

    const readonly=await h.call("node.properties.patch",{
      nodeId:frame.value.id,properties:{id:"forbidden"}
    },2);
    expect(readonly.ok).toBe(false);
  });

  it("exports CSS Tailwind JSX and Storybook through bounded artifacts",async()=> {
    const h=harness();
    const collection=await h.call("variable.collection.create",{name:"Theme"});
    const variable=await h.call("variable.create",{
      collectionId:collection.value.id,name:"brand/primary",resolvedType:"COLOR"
    },1);
    await h.call("variable.set_value",{
      variableId:variable.value.id,modeId:"m:1",value:{r:1,g:0,b:0,a:1}
    },2);
    const frame=await h.call("frame.create",{name:"Hero",width:320,height:180},3);
    await h.call("text.create",{name:"Title",characters:"Semantic Figma"},4);

    const css=await h.call("design_system.export.css",{},5);
    expect(css.ok).toBe(true);
    expect(await readArtifact(h,css.value.token)).toContain("--brand-primary: #ff0000;");

    const tailwind=await h.call("design_system.export.tailwind",{},5);
    expect(await readArtifact(h,tailwind.value.token)).toContain('"brand-primary": "var(--brand-primary)"');

    const jsx=await h.call("node.export.jsx",{nodeId:frame.value.id},5);
    const jsxText=await readArtifact(h,jsx.value.token);
    expect(jsxText).toContain("<Frame");
    expect(jsxText).toContain('name={"Hero"}');

    const story=await h.call("node.export.storybook",{nodeId:frame.value.id},5);
    const storyText=await readArtifact(h,story.value.token);
    expect(storyText).toContain("StoryObj");
    expect(storyText).toContain("Figma/Hero");
  });
});

describe("semantic admin and editor-gated runtime",()=> {
  it("reorders local styles and style folders without arbitrary code",async()=> {
    const h=harness();
    const first=await h.call("style.create",{styleType:"PAINT",name:"A"});
    const second=await h.call("style.create",{styleType:"PAINT",name:"B"});
    const moved=await h.call("style.order.after",{styleId:second.value.id,referenceStyleId:first.value.id});
    expect(moved.ok).toBe(true);
    expect(moved.value).toEqual({moved:true,targetId:second.value.id,referenceId:first.value.id});
    const folder=await h.call("style.folder.order.after",{styleType:"PAINT",targetFolder:"Brand",referenceFolder:null});
    expect(folder.ok).toBe(true);
    expect(folder.value.targetFolder).toBe("Brand");
  });

  it("round-trips the official Slides grid through explicit slide refs",async()=> {
    const h=harness("slides");
    const a=await h.call("slides.slide.create",{name:"One"});
    const b=await h.call("slides.slide.create",{name:"Two"});
    const set=await h.call("slides.grid.set",{rows:[[a.value.id,b.value.id]]});
    expect(set.ok).toBe(true);
    expect(set.value).toEqual({rows:1,slides:2});
    const grid=await h.call("slides.grid.inspect");
    expect(grid.ok).toBe(true);
    expect(grid.value[0].map((x:any)=>x.name)).toEqual(["One","Two"]);
  });
  it("inspects annotation categories and loads static and variable fonts explicitly",async()=> {
    const h=harness();
    const category=await h.call("annotation.category.inspect",{id:"cat:1"});
    expect(category.ok).toBe(true);
    expect(category.value).toEqual({id:"cat:1",label:"Review",color:"yellow",isPreset:false});
    const missing=await h.call("annotation.category.inspect",{id:"cat:404"});
    expect(missing.ok).toBe(true);
    expect(missing.value).toBeNull();

    const loaded=await h.call("font.load",{family:"Inter",style:"Regular"});
    expect(loaded.ok).toBe(true);
    expect(loaded.value).toEqual({loaded:true,family:"Inter",style:"Regular",variationSettings:null});

    const variable=await h.call("font.load",{family:"Inter",variationSettings:{wght:650,slnt:-5}});
    expect(variable.ok).toBe(true);
    expect(variable.value).toEqual({
      loaded:true,family:"Inter",style:null,variationSettings:{wght:650,slnt:-5}
    });
    expect(h.figma.loadedFonts.at(-1)).toEqual({family:"Inter",variationSettings:{wght:650,slnt:-5}});

    const text=await h.call("text.create",{characters:"Variable"});
    const patched=await h.call("node.properties.patch",{
      nodeId:text.value.id,
      properties:{fontName:{family:"Inter",variationSettings:{wght:725}}}
    });
    expect(patched.ok).toBe(true);
    expect(h.nodes.get(text.value.id)?.fontName).toEqual({family:"Inter",variationSettings:{wght:725}});

    const invalid=await h.call("font.load",{family:"Inter",variationSettings:{"bad-axis!":500}});
    expect(invalid.ok).toBe(false);
    expect(invalid.error.message).toContain("invalid_font_variation_axis");
  });

  it("gates Dev Mode codegen semantics to the dev editor",async()=> {
    const dev=harness("dev");
    const status=await dev.call("codegen.status");
    expect(status.ok).toBe(true);
    expect(status.value.editorType).toBe("dev");
    const refresh=await dev.call("codegen.refresh");
    expect(refresh.ok).toBe(true);
    expect(dev.figma.codegenRefreshes).toBe(1);

    const normal=harness();
    const denied=await normal.call("codegen.status");
    expect(denied.ok).toBe(false);
    expect(denied.error.message).toContain("unsupported_editor");
  });
  it("maps PaymentsAPI without exposing plugin payment tokens",async()=> {
    const h=harness();
    const initial=await h.call("payments.status");
    expect(initial.ok).toBe(true);
    expect(initial.value).toEqual({type:"UNPAID"});

    const age=await h.call("payments.first_run_age");
    expect(age.ok).toBe(true);
    expect(age.value).toEqual({seconds:123});

    const dev=await h.call("payments.dev.status.set",{status:"PAID"});
    expect(dev.ok).toBe(true);
    expect(dev.value).toEqual({type:"PAID"});

    const requested=await h.call("payments.checkout.request");
    expect(requested.ok).toBe(true);
    expect(requested.value).toEqual({requested:true});

    const checkout=await h.call("payments.checkout",{interstitial:"PAID_FEATURE"});
    expect(checkout.ok).toBe(true);
    expect(checkout.value).toEqual({type:"PAID"});

    const serialized=JSON.stringify([initial,age,dev,requested,checkout]);
    expect(serialized).not.toContain("TEST_ONLY_INTERNAL_PAYMENT_TOKEN");
  });

  it("exposes collaboration context only through explicit semantic operations",async()=> {
    const h=harness("figjam");
    const current=await h.call("user.current");
    expect(current.ok).toBe(true);
    expect(current.value.name).toBe("Sergio Test");
    const active=await h.call("figjam.active_users");
    expect(active.ok).toBe(true);
    expect(active.value).toHaveLength(1);
    expect(active.value[0].selection).toEqual([]);

    const design=harness("figma");
    const wrongEditor=await design.call("figjam.active_users");
    expect(wrongEditor.ok).toBe(false);
    expect(wrongEditor.error.message).toContain("unsupported_editor");
  });

  it("controls text-review enablement without arbitrary text execution",async()=> {
    const h=harness("figma");
    const initial=await h.call("textreview.status");
    expect(initial.ok).toBe(true);
    expect(initial.value.enabled).toBe(false);
    const enabled=await h.call("textreview.enable");
    expect(enabled.ok).toBe(true);
    expect(enabled.value.enabled).toBe(true);
    const disabled=await h.call("textreview.disable");
    expect(disabled.ok).toBe(true);
    expect(disabled.value.enabled).toBe(false);
  });

  it("registers a deterministic Dev Codegen result outside the generate callback",async()=> {
    const dev=harness("dev");
    const generate=dev.figma.codegenHandlers.get("generate");
    expect(typeof generate).toBe("function");
    const node=dev.figma.createFrame();
    const result=await generate({node});
    expect(result).toHaveLength(1);
    expect(result[0].language).toBe("JSON");
    expect(JSON.parse(result[0].code).id).toBe(node.id);
  });
});


describe("semantic verification and color-vision workflows",()=>{
  it("analyzes color-vision simulations without mutating the document",async()=>{
    const h=harness();
    const a=await h.call("rect.create",{name:"Red"});
    const b=await h.call("rect.create",{name:"Green"},1);
    h.nodes.get(a.value.id)!.fills=[{type:"SOLID",color:{r:1,g:0,b:0},opacity:1}];
    h.nodes.get(b.value.id)!.fills=[{type:"SOLID",color:{r:0,g:1,b:0},opacity:1}];
    const result=await h.call("a11y.vision.analyze",{
      modes:["protanopia","deuteranopia"],threshold:0.25,minOriginalDistance:0.2,maxPairs:10
    },2);
    expect(result.ok).toBe(true);
    expect(result.revision).toBe(2);
    expect(result.value.model).toBe("machado-2009-full-severity");
    expect(result.value.results.map((x:any)=>x.mode)).toEqual(["protanopia","deuteranopia"]);
    expect(result.value.colorCount).toBe(2);
  });

  it("creates reversible color-vision preview clones and leaves the source unchanged",async()=>{
    const h=harness();
    const source=await h.call("rect.create",{name:"Brand",x:10,y:20,width:120,height:60});
    const original=h.nodes.get(source.value.id)!;
    original.fills=[{type:"SOLID",color:{r:1,g:0,b:0},opacity:1}];
    original.effects=[{type:"DROP_SHADOW",color:{r:1,g:0,b:0,a:0.42},offset:{x:0,y:2},radius:4,spread:0,visible:true,blendMode:"NORMAL"}];
    const preview=await h.call("a11y.vision.preview",{
      nodeId:source.value.id,modes:["protanopia"],gap:40,namePrefix:"Preview"
    },1);
    expect(preview.ok).toBe(true);
    expect(preview.revision).toBe(2);
    expect(preview.value.previews).toHaveLength(1);
    const cloneId=preview.value.previews[0].node.id;
    expect(cloneId).not.toBe(source.value.id);
    expect(preview.value.previews[0].transformedPaints).toBe(2);
    expect(h.nodes.get(source.value.id)!.fills[0].color).toEqual({r:1,g:0,b:0});
    expect(h.nodes.get(cloneId)!.fills[0].color).not.toEqual({r:1,g:0,b:0});
    expect(h.nodes.get(cloneId)!.effects[0].color.a).toBe(0.42);
    expect(h.nodes.get(cloneId)!.effects[0].color).not.toEqual({r:1,g:0,b:0,a:0.42});
    const removed=await h.call("node.remove",{nodeId:cloneId},2);
    expect(removed.ok).toBe(true);
    expect(h.nodes.has(cloneId)).toBe(false);
  });

  it("verifies a node with a bounded PNG artifact plus structural measurements",async()=>{
    const h=harness();
    const frame=await h.call("frame.create",{name:"Verification",width:320,height:180});
    const verified=await h.call("verify.node",{nodeId:frame.value.id,scale:1,name:"verify.png"},1);
    expect(verified.ok).toBe(true);
    expect(verified.revision).toBe(1);
    expect(verified.value.mediaType).toBe("image/png");
    expect(verified.value.nodeId).toBe(frame.value.id);
    expect(verified.value.nodeCount).toBeGreaterThanOrEqual(1);
    expect(verified.value.structure.name).toBe("Verification");
    const chunk=await h.call("artifact.read",{token:verified.value.token,offset:0,length:64},1);
    expect(chunk.ok).toBe(true);
    const raw=globalThis.atob(String(chunk.value.base64));
    expect([...raw.slice(0,8)].map(x=>x.charCodeAt(0))).toEqual([137,80,78,71,13,10,26,10]);
  });
});
