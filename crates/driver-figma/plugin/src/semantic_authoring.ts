const AUTHORING_META_KEY = "semwright:semantic-authoring-v1";
const AUTHORING_MAX_NODES = 512;
const AUTHORING_MAX_FINDINGS = 1000;

type AuthoringFinding = {
  severity: "error"|"warning"|"info";
  category: string;
  confidence_class: "DETERMINISTIC"|"HEURISTIC"|"AESTHETIC_ASSIST";
  subject_node_id?: string;
  subject_logical_id?: string;
  related_node_ids: string[];
  expected?: unknown;
  actual?: unknown;
  evidence?: unknown;
  suggested_repairs: unknown[];
};

function authoringObject(value: unknown, label: string): Record<string, any> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error(label);
  return value as Record<string, any>;
}
function authoringArray(value: unknown, max: number, label: string): any[] {
  if (!Array.isArray(value) || value.length > max) throw new Error(label);
  return value;
}
function authoringFinite(value: unknown, label: string): number {
  const number = Number(value);
  if (!Number.isFinite(number)) throw new Error(label);
  return number;
}
function authoringWalk(root: BaseNode, max=AUTHORING_MAX_NODES): BaseNode[] {
  const out: BaseNode[] = [];
  const queue: BaseNode[] = [root];
  while (queue.length && out.length < max) {
    const node = queue.shift()!;
    out.push(node);
    if ("children" in node) queue.push(...node.children);
  }
  return out;
}
function authoringMeta(node: BaseNode): Record<string, any>|null {
  if (!("getPluginData" in node)) return null;
  try {
    const raw = (node as any).getPluginData(AUTHORING_META_KEY);
    if (!raw || raw.length > 4096) return null;
    const value = JSON.parse(raw);
    return value && typeof value === "object" && !Array.isArray(value) ? value : null;
  } catch { return null; }
}
function authoringSetMeta(node: BaseNode, value: Record<string, any>) {
  if (!("setPluginData" in node)) return;
  const raw = JSON.stringify(value);
  if (raw.length > 4096) throw new Error("semantic_metadata_limit");
  (node as any).setPluginData(AUTHORING_META_KEY, raw);
}
function authoringBox(node: BaseNode): {x:number;y:number;width:number;height:number}|null {
  const n = node as any;
  const box = n.absoluteBoundingBox;
  if (box && [box.x,box.y,box.width,box.height].every(Number.isFinite)) {
    return {x:Number(box.x),y:Number(box.y),width:Number(box.width),height:Number(box.height)};
  }
  if ([n.x,n.y,n.width,n.height].every(Number.isFinite)) {
    return {x:Number(n.x),y:Number(n.y),width:Number(n.width),height:Number(n.height)};
  }
  return null;
}

function authoringDesignCandidates(kind:"component"|"text_style"|"variable"): Promise<any[]> {
  if (kind === "component") {
    return Promise.resolve(authoringWalk(figma.currentPage, 2000).filter(n => n.type === "COMPONENT"));
  }
  if (kind === "text_style") return figma.getLocalTextStylesAsync();
  return figma.variables.getLocalVariablesAsync();
}
type AuthoringDesignCandidates = {
  component: any[];
  text_style: any[];
  variable: any[];
};
async function authoringDesignSnapshot(): Promise<AuthoringDesignCandidates> {
  const [component,text_style,variable]=await Promise.all([
    authoringDesignCandidates("component"),
    authoringDesignCandidates("text_style"),
    authoringDesignCandidates("variable"),
  ]);
  return {component,text_style,variable};
}
function authoringResolveDesignRef(
  kind:"component"|"text_style"|"variable",
  ref:any,
  candidates:AuthoringDesignCandidates,
):any|null {
  if (!ref) return null;
  const id = typeof ref.id === "string" ? ref.id : null;
  const key = typeof ref.key === "string" ? ref.key : null;
  const name = typeof ref.name === "string" ? ref.name : null;
  if (!id && !key && !name) throw new Error(kind + "_reference_required");
  const matches = candidates[kind].filter((item:any) =>
    (!id || item.id === id) &&
    (!key || item.key === key) &&
    (!name || item.name === name)
  );
  if (!matches.length) throw new Error(kind + "_not_found");
  const unique = [...new Map(matches.map((x:any)=>[x.id,x])).values()];
  if (unique.length !== 1) throw new Error("ambiguous_" + kind);
  return unique[0];
}
async function authoringResolveNodeBindings(
  node:any,
  candidates:AuthoringDesignCandidates,
) {
  const resolved:any = {
    component_id: null,
    text_style_id: null,
    fill_variable_id: null,
  };
  if (node.component?.component) {
    resolved.component_id = authoringResolveDesignRef(
      "component",
      node.component.component,
      candidates,
    )?.id ?? null;
  }
  if (node.visual?.text_style) {
    resolved.text_style_id = authoringResolveDesignRef(
      "text_style",
      node.visual.text_style,
      candidates,
    )?.id ?? null;
  }
  if (node.visual?.fill?.kind === "variable") {
    resolved.fill_variable_id = authoringResolveDesignRef(
      "variable",
      node.visual.fill.variable,
      candidates,
    )?.id ?? null;
  }
  return resolved;
}
function authoringKindCompatible(kind:string,node:BaseNode):boolean {
  if(kind==="text") return node.type==="TEXT";
  if(kind==="component_instance") return node.type==="INSTANCE";
  if(kind==="section") return node.type==="SECTION";
  if(kind==="media") return ["RECTANGLE","FRAME","COMPONENT","INSTANCE"].includes(node.type);
  if(kind==="shape") {
    return ["RECTANGLE","ELLIPSE","LINE","POLYGON","STAR","VECTOR","BOOLEAN_OPERATION"].includes(node.type);
  }
  return ["FRAME","COMPONENT","SECTION","INSTANCE"].includes(node.type);
}
function authoringDepth(id:string, byId:Map<string,any>):number {
  let depth=0, current=byId.get(id), seen=new Set<string>();
  while(current?.parent) {
    if(seen.has(current.id)) throw new Error("composition_parent_cycle");
    seen.add(current.id);
    depth++;
    if(depth>24) throw new Error("composition_depth");
    current=byId.get(current.parent);
  }
  return depth;
}
async function authoringDraftChangeSet(spec:any) {
  const nodes=authoringArray(spec.nodes,AUTHORING_MAX_NODES,"composition_node_limit");
  const byId=new Map(nodes.map((node:any)=>[String(node.id),node]));
  if(byId.size!==nodes.length) throw new Error("duplicate_composition_id");
  const ordered=[...nodes].sort((a:any,b:any)=>{
    const delta=authoringDepth(String(a.id),byId)-authoringDepth(String(b.id),byId);
    return delta || Number(a.order??0)-Number(b.order??0) || String(a.id).localeCompare(String(b.id));
  });
  const candidates=await authoringDesignSnapshot();
  const creates:any[]=[];
  const modifies:any[]=[];
  for(const node of ordered){
    const resolved=await authoringResolveNodeBindings(node,candidates);
    if(node.existing_node_id!=null){
      const existing=asScene(await nodeById(String(node.existing_node_id)));
      if(!authoringKindCompatible(String(node.kind),existing)){
        throw new Error("existing_node_kind_mismatch");
      }
      if(node.parent!=null){
        const parentSpec=byId.get(String(node.parent));
        if(!parentSpec?.existing_node_id){
          throw new Error("existing_reparent_not_supported");
        }
        if(existing.parent?.id!==String(parentSpec.existing_node_id)){
          throw new Error("existing_parent_mismatch");
        }
      }
      modifies.push({
        node_id:existing.id,
        logical_id:String(node.id),
        resolved,
        action:{kind:"apply_intent"},
      });
    } else {
      creates.push({
        logical_id:String(node.id),
        parent_logical_id:node.parent==null?null:String(node.parent),
        resolved,
      });
    }
  }
  return {
    version:1,
    creates,
    modifies,
    deletes:[],
    expected_effects:[
      "native_figma_nodes",
      "native_text_for_copy",
      "auto_layout_when_declared",
      "post_write_measurement",
    ],
    postconditions:["document_identity_unchanged","revision_advanced_only_by_authorized_apply"],
    required_scopes:["driver:figma"],
    risk:"mutating_reversible",
  };
}
function authoringLayoutMode(node:any):"NONE"|"HORIZONTAL"|"VERTICAL"|"GRID" {
  const kind=String(node.kind);
  if(kind==="stack") return "VERTICAL";
  if(kind==="row"||kind==="split") return "HORIZONTAL";
  if(kind==="grid") return "GRID";
  if(kind==="overlay") return "NONE";
  const direction=String(node.layout?.direction??"none");
  return direction==="vertical"?"VERTICAL":direction==="horizontal"?"HORIZONTAL":direction==="grid"?"GRID":"NONE";
}
function authoringApplyLayout(target:any, spec:any){
  if(!("layoutMode" in target)) return;
  target.layoutMode=authoringLayoutMode(spec);
  const layout=spec.layout;
  if(!layout) return;
  target.itemSpacing=Math.max(0,authoringFinite(layout.gap??0,"invalid_gap"));
  const pad=layout.padding??{};
  target.paddingTop=Math.max(0,authoringFinite(pad.top??0,"invalid_padding"));
  target.paddingRight=Math.max(0,authoringFinite(pad.right??0,"invalid_padding"));
  target.paddingBottom=Math.max(0,authoringFinite(pad.bottom??0,"invalid_padding"));
  target.paddingLeft=Math.max(0,authoringFinite(pad.left??0,"invalid_padding"));
  const align=String(layout.align??"start");
  target.counterAxisAlignItems=align==="center"?"CENTER":align==="end"?"MAX":align==="baseline"?"BASELINE":"MIN";
  const distribute=String(layout.distribute??"start");
  target.primaryAxisAlignItems=distribute==="center"?"CENTER":distribute==="end"?"MAX":distribute==="space_between"?"SPACE_BETWEEN":"MIN";
  if("layoutWrap" in target) target.layoutWrap=layout.wrap?"WRAP":"NO_WRAP";
}
function authoringApplySizing(target:any, sizing:any){
  if(!sizing) return;
  const axes:[string,any][]=[["Horizontal",sizing.width],["Vertical",sizing.height]];
  for(const [suffix,axis] of axes){
    if(!axis) continue;
    const mode=String(axis.mode);
    const property="layoutSizing"+suffix;
    if(property in target && (mode==="fill"||mode==="hug")) target[property]=mode==="fill"?"FILL":"HUG";
    if(axis.min!=null){
      const key=suffix==="Horizontal"?"minWidth":"minHeight";
      if(key in target) target[key]=authoringFinite(axis.min,"invalid_min");
    }
    if(axis.max!=null){
      const key=suffix==="Horizontal"?"maxWidth":"maxHeight";
      if(key in target) target[key]=authoringFinite(axis.max,"invalid_max");
    }
  }
  let width=Number(target.width??100),height=Number(target.height??100);
  if(sizing.width?.mode==="fixed") width=authoringFinite(sizing.width.value,"fixed_width_required");
  if(sizing.height?.mode==="fixed") height=authoringFinite(sizing.height.value,"fixed_height_required");
  if(typeof target.resize==="function" && width>0 && height>0) target.resize(width,height);
}
async function authoringApplyText(target:any, spec:any, resolved:any){
  const text=spec.text;
  if(!text) return;
  const font={
    family:String(text.font_family??target.fontName?.family??"Inter"),
    style:String(text.font_style??target.fontName?.style??"Regular"),
  };
  await figma.loadFontAsync(font);
  target.fontName=font;
  target.characters=String(text.characters??"").slice(0,65536);
  if(text.font_size!=null) target.fontSize=authoringFinite(text.font_size,"invalid_font_size");
  if(text.line_height!=null) target.lineHeight={unit:"PIXELS",value:authoringFinite(text.line_height,"invalid_line_height")};
  if(text.letter_spacing!=null) target.letterSpacing={unit:"PIXELS",value:authoringFinite(text.letter_spacing,"invalid_letter_spacing")};
  const fixedWidth=spec.sizing?.width?.mode==="fixed";
  const fit=String(text.fit??"grow_height");
  if(fit==="bounded_shrink") throw new Error("bounded_shrink_requires_explicit_bounds");
  target.textAutoResize=fixedWidth?"HEIGHT":"WIDTH_AND_HEIGHT";
  if(text.max_lines!=null && "maxLines" in target) target.maxLines=Math.max(1,Math.min(1000,Number(text.max_lines)));
  if(fit==="truncate" && "textTruncation" in target) target.textTruncation="ENDING";
  if(resolved.text_style_id && typeof target.setTextStyleIdAsync==="function") {
    await target.setTextStyleIdAsync(resolved.text_style_id);
  }
}
async function authoringApplyVisual(target:any, spec:any, resolved:any){
  const visual=spec.visual;
  if(!visual) return;
  if(visual.opacity!=null && "opacity" in target) {
    target.opacity=Math.max(0,Math.min(1,authoringFinite(visual.opacity,"invalid_opacity")));
  }
  if(visual.radius!=null && "cornerRadius" in target) {
    target.cornerRadius=Math.max(0,authoringFinite(visual.radius,"invalid_radius"));
  }
  if(!("fills" in target)||!visual.fill) return;
  if(visual.fill.kind==="solid"){
    target.fills=[{
      type:"SOLID",
      color:{r:Number(visual.fill.r),g:Number(visual.fill.g),b:Number(visual.fill.b)},
      opacity:Number(visual.fill.a??1),
    }];
  } else if(visual.fill.kind==="variable"){
    const variable=await figma.variables.getVariableByIdAsync(resolved.fill_variable_id);
    if(!variable) throw new Error("resolved_variable_missing");
    const base:any={type:"SOLID",color:{r:0,g:0,b:0},opacity:1};
    target.fills=[figma.variables.setBoundVariableForPaint(base,"color",variable)];
  }
}
async function authoringApplyDeclaredIntent(
  node:SceneNode,
  spec:any,
  resolved:any,
){
  const target:any=node;
  target.name=String(spec.name??spec.id).slice(0,256);
  if(String(spec.kind)==="media"){
    const media=authoringObject(spec.media,"media_intent_required");
    const image=figma.getImageByHash(String(media.image_hash));
    if(!image) throw new Error("image_hash_not_found");
    target.fills=[{
      type:"IMAGE",
      imageHash:String(media.image_hash),
      scaleMode:String(media.scale_mode??"FILL").toUpperCase(),
    }];
  }
  authoringApplyLayout(target,spec);
  authoringApplySizing(target,spec.sizing);
  if(target.type==="TEXT") await authoringApplyText(target,spec,resolved);
  await authoringApplyVisual(target,spec,resolved);
  if(target.type==="INSTANCE"){
    if(resolved.component_id){
      const component=await nodeById(String(resolved.component_id));
      if(component.type!=="COMPONENT") throw new Error("resolved_component_invalid");
      if(target.mainComponent?.id!==component.id){
        if(typeof target.swapComponent!=="function") throw new Error("instance_swap_unavailable");
        target.swapComponent(component);
      }
    }
    if(spec.component?.variant_properties && typeof target.setProperties==="function"){
      target.setProperties(spec.component.variant_properties);
    }
  }
  authoringSetMeta(target,{
    version:1,
    logicalId:String(spec.id),
    kind:String(spec.kind),
    role:spec.role??null,
    profile:spec.profile??null,
    aspectRatio:spec.sizing?.aspect_ratio??null,
    maxLines:spec.text?.max_lines??null,
    textFit:spec.text?.fit??null,
  });
}
async function authoringCreateNode(spec:any,resolved:any):Promise<SceneNode>{
  let node:any;
  switch(String(spec.kind)){
    case "section": node=figma.createSection(); break;
    case "text": node=figma.createText(); break;
    case "shape": node=figma.createRectangle(); break;
    case "media": node=figma.createRectangle(); break;
    case "component_instance": {
      const component=await nodeById(String(resolved.component_id));
      if(component.type!=="COMPONENT") throw new Error("resolved_component_invalid");
      node=component.createInstance();
      break;
    }
    default: node=figma.createFrame(); break;
  }
  return node as SceneNode;
}
async function authoringTargetParent(spec:any):Promise<BaseNode & ChildrenMixin>{
  if(spec.target?.parent_node_id){
    const parent=await nodeById(String(spec.target.parent_node_id));
    if(!("appendChild" in parent)) throw new Error("target_parent_not_container");
    return parent as BaseNode & ChildrenMixin;
  }
  if(spec.target?.page_id) return await pageById(String(spec.target.page_id));
  return figma.currentPage;
}
async function authoringApplyComposition(plan:any){
  const spec=authoringObject(plan.spec,"plan_spec_required");
  const changeSet=authoringObject(plan.changeset,"changeset_required");
  if(authoringArray(changeSet.deletes??[],1,"delete_limit").length) {
    throw new Error("semantic_delete_forbidden");
  }
  const nodes=authoringArray(spec.nodes,AUTHORING_MAX_NODES,"composition_node_limit");
  const byId=new Map(nodes.map((node:any)=>[String(node.id),node]));
  const createChanges=authoringArray(
    changeSet.creates??[],
    AUTHORING_MAX_NODES,
    "create_limit",
  );
  const modifyChanges=authoringArray(
    changeSet.modifies??[],
    AUTHORING_MAX_NODES,
    "modify_limit",
  );
  const createByLogical=new Map(
    createChanges.map((change:any)=>[String(change.logical_id),change])
  );
  const modifyByLogical=new Map(
    modifyChanges.map((change:any)=>[String(change.logical_id),change])
  );
  const ordered=[...nodes].sort((a:any,b:any)=>{
    const delta=authoringDepth(String(a.id),byId)-authoringDepth(String(b.id),byId);
    return delta || Number(a.order??0)-Number(b.order??0);
  });
  const needsTarget=ordered.some(
    (entry:any)=>entry.existing_node_id==null&&entry.parent==null,
  );
  const target=needsTarget?await authoringTargetParent(spec):null;
  const made=new Map<string,SceneNode>();
  const created:any[]=[];
  const modified:any[]=[];
  for(const entry of ordered){
    const logicalId=String(entry.id);
    if(entry.existing_node_id!=null){
      const change=authoringObject(
        modifyByLogical.get(logicalId),
        "missing_declared_modify",
      );
      if(
        String(change.node_id)!==String(entry.existing_node_id) ||
        String(change.action?.kind)!=="apply_intent"
      ){
        throw new Error("semantic_modify_binding_mismatch");
      }
      const node=asScene(await nodeById(String(entry.existing_node_id)));
      if(!authoringKindCompatible(String(entry.kind),node)){
        throw new Error("existing_node_kind_mismatch");
      }
      await authoringApplyDeclaredIntent(node,entry,change.resolved??{});
      made.set(logicalId,node);
      modified.push({logicalId,nodeId:node.id,type:node.type,name:node.name});
      continue;
    }

    const change=authoringObject(
      createByLogical.get(logicalId),
      "missing_declared_create",
    );
    const node=await authoringCreateNode(entry,change.resolved??{});
    const parentId=entry.parent==null?null:String(entry.parent);
    const parent=parentId?made.get(parentId):target;
    if(!parent || !("appendChild" in parent)) throw new Error("resolved_parent_unavailable");
    (parent as any).appendChild(node);
    await authoringApplyDeclaredIntent(node,entry,change.resolved??{});
    made.set(logicalId,node);
    created.push({logicalId,nodeId:node.id,type:node.type,name:node.name});
  }
  const logicalToNode=Object.fromEntries([...made.entries()].map(([key,value])=>[key,value.id]));
  const rootNodeIds=ordered
    .filter((n:any)=>n.parent==null)
    .map((n:any)=>made.get(String(n.id))!.id);
  return {
    applied:true,
    rootNodeIds,
    created,
    modified,
    logicalToNode,
    effects:changeSet.expected_effects??[],
  };
}
function authoringMeasuredNode(node:BaseNode){
  const n:any=node;
  const meta=authoringMeta(node);
  const box=authoringBox(node);
  const result:any={
    nodeId:node.id,
    logicalId:meta?.logicalId??null,
    type:node.type,
    name:node.name,
    box,
    visible:"visible" in n?n.visible:null,
    locked:"locked" in n?n.locked:null,
    layoutMode:n.layoutMode??null,
    itemSpacing:n.itemSpacing??null,
    padding:n.layoutMode&&n.layoutMode!=="NONE"?{
      top:n.paddingTop,
      right:n.paddingRight,
      bottom:n.paddingBottom,
      left:n.paddingLeft,
    }:null,
    boundVariables:n.boundVariables??null,
    componentId:n.type==="INSTANCE"?(n.mainComponent?.id??null):null,
  };
  if(node.type==="TEXT"){
    result.text={
      characters:n.characters,
      fontSize:n.fontSize===figma.mixed?null:n.fontSize,
      lineHeight:n.lineHeight===figma.mixed?null:n.lineHeight,
      textAutoResize:n.textAutoResize,
      maxLines:n.maxLines??null,
      textTruncation:n.textTruncation??null,
    };
  }
  return result;
}
async function authoringMeasure(root:BaseNode,max=AUTHORING_MAX_NODES){
  const bounded=Math.max(1,Math.min(AUTHORING_MAX_NODES,max));
  const all=authoringWalk(root,bounded);
  return {
    rootNodeId:root.id,
    nodes:all.map(authoringMeasuredNode),
    truncated:all.length>=bounded,
  };
}
function authoringFinding(
  category:string,
  node:BaseNode|undefined,
  logicalId:string|undefined,
  expected:any,
  actual:any,
  evidence:any,
  severity:"error"|"warning"|"info"="warning",
  repairs:any[]=[],
  confidenceClass:"DETERMINISTIC"|"HEURISTIC"|"AESTHETIC_ASSIST"="DETERMINISTIC",
):AuthoringFinding{
  return {
    severity,
    category,
    confidence_class:confidenceClass,
    subject_node_id:node?.id,
    subject_logical_id:logicalId,
    related_node_ids:[],
    expected,
    actual,
    evidence,
    suggested_repairs:repairs,
  };
}
function authoringGap(
  a:{x:number;y:number;width:number;height:number},
  b:{x:number;y:number;width:number;height:number},
){
  const horizontal=Math.max(
    b.x-(a.x+a.width),
    a.x-(b.x+b.width),
  );
  const vertical=Math.max(
    b.y-(a.y+a.height),
    a.y-(b.y+b.height),
  );
  return Math.max(horizontal,vertical,0);
}
function authoringOverlap(a:any,b:any){
  return a.x < b.x+b.width &&
    b.x < a.x+a.width &&
    a.y < b.y+b.height &&
    b.y < a.y+a.height;
}
async function authoringValidate(
  root:BaseNode,
  spec:any,
  maxFindings=AUTHORING_MAX_FINDINGS,
){
  const limit=Math.max(1,Math.min(AUTHORING_MAX_FINDINGS,maxFindings));
  const all=authoringWalk(root,AUTHORING_MAX_NODES);
  const byLogical=new Map<string,BaseNode>();
  for(const node of all){
    const meta=authoringMeta(node);
    if(meta?.logicalId) byLogical.set(String(meta.logicalId),node);
  }
  const findings:AuthoringFinding[]=[];
  let unknown=0;
  const add=(finding:AuthoringFinding)=>{
    if(findings.length<limit) findings.push(finding);
  };
  for(const node of all){
    const n:any=node;
    const meta=authoringMeta(node);
    const box=authoringBox(node);
    if(node.type==="TEXT"){
      if(figma.hasMissingFont){
        add(authoringFinding(
          "missing_fonts",node,meta?.logicalId,false,true,
          {hasMissingFont:true},"error",
        ));
      }
      if(typeof n.getRangeBoundingBox==="function" && n.characters?.length){
        try{
          const textBox=await n.getRangeBoundingBox(0,n.characters.length);
          if(
            textBox && box &&
            textBox.height>box.height+0.5 &&
            n.textTruncation!=="ENDING"
          ){
            add(authoringFinding(
              "text_clipping",
              node,
              meta?.logicalId,
              {fits:true},
              {textHeight:textBox.height,nodeHeight:box.height},
              {method:"getRangeBoundingBox"},
              "error",
              [{kind:"grow_text_height"}],
            ));
          }
        } catch {
          unknown++;
        }
      }
      const foreground=extraSolidColor(n.fills);
      const parent=n.parent as any;
      const background=parent&&"fills" in parent
        ? extraSolidColor(parent.fills)
        : null;
      if(foreground&&background){
        const ratio=extraContrast(foreground,background);
        const size=n.fontSize===figma.mixed?null:Number(n.fontSize);
        const threshold=size!==null&&size>=24?3:4.5;
        if(ratio+0.001<threshold){
          add(authoringFinding(
            "contrast",
            node,
            meta?.logicalId,
            {minimumRatio:threshold},
            {ratio},
            {method:"solid-text-on-solid-parent",fontSize:size},
            "warning",
          ));
        }
      }
    }
    if(
      meta?.role &&
      /button|control|input|cta/i.test(String(meta.role)) &&
      box &&
      (box.width<44||box.height<44)
    ){
      add(authoringFinding(
        "minimum_touch_target",
        node,
        meta.logicalId,
        {width:44,height:44},
        box,
        {role:meta.role},
      ));
    }
    const parent=node.parent as any;
    const parentBox=parent?authoringBox(parent):null;
    if(
      box &&
      parentBox &&
      parent?.type!=="PAGE" &&
      parent?.type!=="DOCUMENT"
    ){
      const local=node as any;
      if(
        Number.isFinite(local.x) &&
        Number.isFinite(local.y) &&
        (
          local.x < -0.5 ||
          local.y < -0.5 ||
          local.x+box.width > parentBox.width+0.5 ||
          local.y+box.height > parentBox.height+0.5
        )
      ){
        add(authoringFinding(
          "parent_bounds_overflow",
          node,
          meta?.logicalId,
          "inside parent",
          {x:local.x,y:local.y,width:box.width,height:box.height},
          {
            parentNodeId:parent.id,
            parentWidth:parentBox.width,
            parentHeight:parentBox.height,
          },
          "error",
        ));
        if(parent.clipsContent===true){
          add(authoringFinding(
            "hidden_overflow_state",
            node,
            meta?.logicalId,
            {clipped:false},
            {clipped:true},
            {parentNodeId:parent.id,clipsContent:true},
            "warning",
          ));
        }
      }
    }
  }

  for(const parent of all){
    if(!("children" in parent)) continue;
    const p:any=parent;
    const pmeta=authoringMeta(parent);
    if(p.layoutMode!=="NONE" || pmeta?.kind==="overlay") continue;
    const children=parent.children.filter((c:any)=>c.visible!==false);
    for(let i=0;i<children.length;i++){
      for(let j=i+1;j<children.length;j++){
        const first=authoringBox(children[i]);
        const second=authoringBox(children[j]);
        if(first&&second&&authoringOverlap(first,second)){
          add(authoringFinding(
            "sibling_overlap",
            children[i],
            authoringMeta(children[i])?.logicalId,
            false,
            true,
            {relatedNodeId:children[j].id,parentNodeId:parent.id},
            "warning",
          ));
        }
      }
    }
  }

  if(spec){
    const declared=authoringArray(
      spec.nodes??[],
      AUTHORING_MAX_NODES,
      "composition_node_limit",
    );
    for(const declaredNode of declared){
      const logicalId=String(declaredNode.id);
      const actual=byLogical.get(logicalId);
      if(!actual){
        add(authoringFinding(
          "missing_declared_node",
          undefined,
          logicalId,
          true,
          false,
          {logicalId},
          "error",
        ));
        continue;
      }
      const n:any=actual;
      const expectedMode=authoringLayoutMode(declaredNode);
      if(expectedMode!=="NONE" && n.layoutMode!==expectedMode){
        add(authoringFinding(
          "invalid_auto_layout",
          actual,
          logicalId,
          expectedMode,
          n.layoutMode,
          {kind:declaredNode.kind},
          "error",
        ));
      }
      if(expectedMode!=="NONE"&&declaredNode.layout?.align){
        const expectedAlign=String(declaredNode.layout.align)==="center"
          ?"CENTER"
          :String(declaredNode.layout.align)==="end"
            ?"MAX"
            :String(declaredNode.layout.align)==="baseline"
              ?"BASELINE"
              :"MIN";
        if(n.counterAxisAlignItems!==expectedAlign){
          add(authoringFinding(
            "alignment",
            actual,
            logicalId,
            expectedAlign,
            n.counterAxisAlignItems??null,
            {source:"native-auto-layout-counter-axis"},
            "warning",
          ));
        }
      }
      if(String(declaredNode.kind)==="text" && actual.type!=="TEXT"){
        add(authoringFinding(
          "native_text_violation",
          actual,
          logicalId,
          "TEXT",
          actual.type,
          {declaredKind:"text"},
          "error",
        ));
      }
      if(
        String(declaredNode.kind)==="component_instance" &&
        actual.type!=="INSTANCE"
      ){
        add(authoringFinding(
          "component_relationship",
          actual,
          logicalId,
          "INSTANCE",
          actual.type,
          {},
          "error",
        ));
      }
      const hasVariableFill=Boolean(
        n.boundVariables?.fills ||
        (Array.isArray(n.fills) && n.fills.some((paint:any)=>paint?.boundVariables?.color))
      );
      if(
        declaredNode.visual?.fill?.kind==="variable" &&
        !hasVariableFill
      ){
        add(authoringFinding(
          "required_binding",
          actual,
          logicalId,
          "variable-bound fill",
          n.boundVariables??null,
          {},
          "error",
        ));
      }
      const declaredRatio=declaredNode.sizing?.aspect_ratio;
      const actualBox=authoringBox(actual);
      if(declaredRatio!=null&&actualBox){
        const expected=Number(declaredRatio);
        const observed=actualBox.height===0?Infinity:actualBox.width/actualBox.height;
        if(!Number.isFinite(observed)||Math.abs(observed-expected)>0.01){
          add(authoringFinding(
            String(declaredNode.kind)==="media"?"media_deformation":"aspect_ratio",
            actual,
            logicalId,
            expected,
            observed,
            {source:"declared_node_sizing",tolerance:0.01},
            "warning",
            [{kind:"restore_aspect_ratio",ratio:expected}],
          ));
        }
      }
    }

    for(const relation of authoringArray(
      spec.relationships??[],
      1024,
      "relationship_limit",
    )){
      const subject=byLogical.get(String(relation.subject));
      if(!subject){
        unknown++;
        continue;
      }
      const object=relation.object==null
        ? null
        : byLogical.get(String(relation.object));
      const subjectBox=authoringBox(subject);
      const objectBox=object?authoringBox(object):null;
      const kind=String(relation.kind);
      if(kind==="aspect_ratio"&&subjectBox&&relation.value!=null){
        const expected=Number(relation.value);
        const actual=subjectBox.height===0
          ? Infinity
          : subjectBox.width/subjectBox.height;
        const tolerance=Number(relation.tolerance??0.01);
        if(!Number.isFinite(actual)||Math.abs(actual-expected)>tolerance){
          add(authoringFinding(
            "aspect_ratio",
            subject,
            String(relation.subject),
            expected,
            actual,
            {tolerance},
            "warning",
            [{kind:"restore_aspect_ratio",ratio:expected}],
          ));
        }
      } else if(
        (kind==="minimum_gap"||kind==="maximum_gap") &&
        subjectBox &&
        objectBox &&
        object &&
        relation.value!=null
      ){
        const expected=Number(relation.value);
        const sameParent=subject.parent&&subject.parent===object.parent;
        const parent=sameParent?subject.parent as BaseNode:undefined;
        const auto=Boolean(
          parent &&
          "layoutMode" in parent &&
          (parent as any).layoutMode!=="NONE"
        );
        const actual=auto && parent
          ? Number((parent as any).itemSpacing)
          : authoringGap(subjectBox,objectBox);
        const failed=kind==="minimum_gap"
          ? actual+0.5<expected
          : actual-0.5>expected;
        if(failed){
          add(authoringFinding(
            "declared_spacing",
            parent??subject,
            String(relation.subject),
            {kind,value:expected},
            actual,
            {
              subjectNodeId:subject.id,
              objectNodeId:object.id,
              parentNodeId:parent?.id,
              autoLayout:auto,
            },
            "warning",
            auto?[{kind:"set_auto_layout_gap",gap:expected}]:[],
          ));
        }
      }
      else if(
        kind==="same_width_as" &&
        subjectBox &&
        objectBox &&
        Math.abs(subjectBox.width-objectBox.width)>0.5
      ){
        add(authoringFinding(
          "same_width",
          subject,
          String(relation.subject),
          objectBox.width,
          subjectBox.width,
          {objectNodeId:object?.id},
        ));
      } else if(
        kind==="same_height_as" &&
        subjectBox &&
        objectBox &&
        Math.abs(subjectBox.height-objectBox.height)>0.5
      ){
        add(authoringFinding(
          "same_height",
          subject,
          String(relation.subject),
          objectBox.height,
          subjectBox.height,
          {objectNodeId:object?.id},
        ));
      } else if(
        (kind==="before"||kind==="after") &&
        subjectBox &&
        objectBox
      ){
        const pass=kind==="before"
          ? (
              subjectBox.x+subjectBox.width<=objectBox.x+0.5 ||
              subjectBox.y+subjectBox.height<=objectBox.y+0.5
            )
          : (
              objectBox.x+objectBox.width<=subjectBox.x+0.5 ||
              objectBox.y+objectBox.height<=subjectBox.y+0.5
            );
        if(!pass){
          add(authoringFinding(
            "relative_order",
            subject,
            String(relation.subject),
            kind,
            "violated",
            {objectNodeId:object?.id},
          ));
        }
      } else if(kind==="centered_in"&&subjectBox&&objectBox){
        const dx=Math.abs(
          (subjectBox.x+subjectBox.width/2)-
          (objectBox.x+objectBox.width/2)
        );
        const dy=Math.abs(
          (subjectBox.y+subjectBox.height/2)-
          (objectBox.y+objectBox.height/2)
        );
        const tolerance=Number(relation.tolerance??1);
        if(dx>tolerance||dy>tolerance){
          add(authoringFinding(
            "centered_in",
            subject,
            String(relation.subject),
            {tolerance},
            {dx,dy},
            {objectNodeId:object?.id},
          ));
        }
      } else if(
        ["aligned_with","baseline_with","anchored_to"].includes(kind)
      ){
        unknown++;
      }
    }

    for(const profile of authoringArray(
      spec.profiles??[],
      8,
      "profile_limit",
    )){
      const profileRoot=byLogical.get(String(profile.root_id));
      const box=profileRoot?authoringBox(profileRoot):null;
      if(!profileRoot||!box){
        unknown++;
        continue;
      }
      if(Math.abs(box.width-Number(profile.width))>0.5){
        add(authoringFinding(
          "responsive_profile",
          profileRoot,
          String(profile.root_id),
          Number(profile.width),
          box.width,
          {profile:profile.name},
          "error",
        ));
      }
    }
  }

  const prototypeScope=new Set(all.map(node=>node.id));
  const prototypeValidation=await extraValidatePrototype();
  for(const finding of prototypeValidation.findings??[]){
    if(!prototypeScope.has(String(finding.nodeId))) continue;
    const subject=all.find(node=>node.id===String(finding.nodeId));
    add(authoringFinding(
      "invalid_prototype_reference",
      subject,
      subject?authoringMeta(subject)?.logicalId:undefined,
      {destinationExists:true},
      {destinationId:finding.destinationId},
      {
        sourceRule:finding.rule,
        reactionIndex:finding.reactionIndex,
      },
      "error",
    ));
  }
  if(prototypeValidation.truncated===true) unknown++;

  const fontSizes=[...new Set(
    all
      .filter(node=>node.type==="TEXT")
      .map(node=>(node as any).fontSize)
      .filter(value=>value!==figma.mixed&&Number.isFinite(Number(value)))
      .map(value=>Math.round(Number(value)*100)/100)
  )].sort((a,b)=>a-b);
  if(fontSizes.length>6){
    add(authoringFinding(
      "type_scale_complexity",
      root,
      authoringMeta(root)?.logicalId,
      {distinctFontSizesAtMost:6},
      {distinctFontSizes:fontSizes.length,values:fontSizes.slice(0,32)},
      {method:"observed-distinct-font-sizes"},
      "info",
      [],
      "HEURISTIC",
    ));
  }
  const deterministicFailures=findings.filter(f =>
    f.confidence_class==="DETERMINISTIC" &&
    (f.severity==="error"||f.severity==="warning")
  ).length;
  const status=deterministicFailures
    ? "FAIL"
    : unknown
      ? "UNKNOWN"
      : "PASS";
  return {
    status,
    findings,
    summary:{
      deterministicFailures,
      unknown,
      checkedNodes:all.length,
      truncated:all.length>=AUTHORING_MAX_NODES,
    },
  };
}

async function authoringRepairChangeSet(plan:any, findings:any[]){
  const max=Math.max(
    0,
    Math.min(64,Number(plan.spec?.budgets?.max_repair_operations??32)),
  );
  const modifies:any[]=[];
  for(const finding of findings.slice(0,1000)){
    if(modifies.length>=max) break;
    if(String(finding.confidence_class)!=="DETERMINISTIC") continue;
    const nodeId=typeof finding.subject_node_id==="string"
      ? finding.subject_node_id
      : null;
    if(!nodeId) continue;
    const repairs=Array.isArray(finding.suggested_repairs)
      ? finding.suggested_repairs
      : [];
    const repair=repairs.length===1?repairs[0]:null;
    if(!repair) continue;
    if(repair.kind==="grow_text_height"){
      modifies.push({
        node_id:nodeId,
        logical_id:finding.subject_logical_id??null,
        action:{kind:"grow_text_height"},
      });
    } else if(repair.kind==="set_auto_layout_gap"){
      modifies.push({
        node_id:nodeId,
        logical_id:finding.subject_logical_id??null,
        action:{
          kind:"set_auto_layout_gap",
          gap:Number(repair.gap),
        },
      });
    } else if(repair.kind==="restore_aspect_ratio"){
      modifies.push({
        node_id:nodeId,
        logical_id:finding.subject_logical_id??null,
        action:{
          kind:"restore_aspect_ratio",
          ratio:Number(repair.ratio),
        },
      });
    } else if(repair.kind==="bind_variable"){
      modifies.push({
        node_id:nodeId,
        logical_id:finding.subject_logical_id??null,
        action:{
          kind:"bind_variable",
          field:String(repair.field),
          variable_id:String(repair.variable_id),
        },
      });
    }
  }
  return {
    version:1,
    creates:[],
    modifies,
    deletes:[],
    expected_effects:modifies.map(
      (m:any)=>"repair:"+m.action.kind+":"+m.node_id,
    ),
    postconditions:[
      "fresh_measurement_required",
      "fresh_validation_required",
    ],
    required_scopes:["driver:figma"],
    risk:"mutating_reversible",
  };
}
async function authoringApplyRepairs(plan:any){
  const changes=authoringArray(
    plan.changeset?.modifies??[],
    64,
    "repair_limit",
  );
  const modified:any[]=[];
  for(const change of changes){
    const node=asScene(await nodeById(String(change.node_id))) as any;
    const action=authoringObject(
      change.action,
      "repair_action_required",
    );
    if(action.kind==="grow_text_height"){
      if(node.type!=="TEXT") throw new Error("repair_target_not_text");
      node.textAutoResize=Number.isFinite(node.width)
        ? "HEIGHT"
        : "WIDTH_AND_HEIGHT";
    } else if(action.kind==="set_auto_layout_gap"){
      if(!("layoutMode" in node)||node.layoutMode==="NONE"){
        throw new Error("repair_target_not_auto_layout");
      }
      node.itemSpacing=Math.max(
        0,
        authoringFinite(action.gap,"invalid_repair_gap"),
      );
    } else if(action.kind==="restore_aspect_ratio"){
      if(typeof node.resize!=="function"){
        throw new Error("repair_target_not_resizable");
      }
      const ratio=authoringFinite(
        action.ratio,
        "invalid_repair_ratio",
      );
      if(ratio<=0) throw new Error("invalid_repair_ratio");
      node.resize(node.width,node.width/ratio);
    } else if(action.kind==="bind_variable"){
      const variable=await figma.variables.getVariableByIdAsync(
        String(action.variable_id),
      );
      if(!variable) throw new Error("repair_variable_missing");
      if(
        action.field==="fill_color" &&
        Array.isArray(node.fills) &&
        node.fills[0]?.type==="SOLID"
      ){
        const paints=[...node.fills];
        paints[0]=figma.variables.setBoundVariableForPaint(
          paints[0],
          "color",
          variable,
        );
        node.fills=paints;
      } else {
        throw new Error("unsupported_binding_repair");
      }
    } else {
      throw new Error("unsupported_repair_action");
    }
    modified.push({
      nodeId:node.id,
      action:action.kind,
    });
  }
  return {applied:true,modified};
}
async function authoringInspect(a:any){
  const root=a.root_node_id
    ? await nodeById(String(a.root_node_id))
    : figma.currentPage;
  const max=Math.max(
    1,
    Math.min(AUTHORING_MAX_NODES,Number(a.max_nodes??128)),
  );
  const measured=await authoringMeasure(root,max);
  const designSystem:any={
    components:[],
    componentSets:[],
    variableCollections:[],
    variables:[],
    textStyles:[],
    paintStyles:[],
    effectStyles:[],
    gridStyles:[],
  };
  if(a.include_design_system!==false){
    designSystem.components=authoringWalk(
      figma.currentPage,
      1000,
    )
      .filter(n=>n.type==="COMPONENT")
      .slice(0,100)
      .map((n:any)=>({
        id:n.id,
        key:n.key,
        name:n.name,
      }));
    designSystem.componentSets=authoringWalk(
      figma.currentPage,
      1000,
    )
      .filter(n=>n.type==="COMPONENT_SET")
      .slice(0,100)
      .map((n:any)=>({
        id:n.id,
        key:n.key,
        name:n.name,
        componentPropertyDefinitions:n.componentPropertyDefinitions??{},
      }));
    designSystem.variableCollections=(
      await figma.variables.getLocalVariableCollectionsAsync()
    )
      .slice(0,100)
      .map((c:any)=>({
        id:c.id,
        key:c.key??null,
        name:c.name,
        defaultModeId:c.defaultModeId,
        modes:(c.modes??[]).slice(0,32),
        variableCount:Array.isArray(c.variableIds)?c.variableIds.length:0,
      }));
    designSystem.variables=(
      await figma.variables.getLocalVariablesAsync()
    )
      .slice(0,200)
      .map((v:any)=>({
        id:v.id,
        key:v.key??null,
        name:v.name,
        resolvedType:v.resolvedType,
        collectionId:v.variableCollectionId,
        scopes:Array.isArray(v.scopes)?v.scopes.slice(0,32):[],
      }));
    const mapStyle=(s:any)=>({
      id:s.id,
      key:s.key??null,
      name:s.name,
    });
    designSystem.textStyles=(
      await figma.getLocalTextStylesAsync()
    ).slice(0,100).map(mapStyle);
    designSystem.paintStyles=(
      await figma.getLocalPaintStylesAsync()
    ).slice(0,100).map(mapStyle);
    designSystem.effectStyles=(
      await figma.getLocalEffectStylesAsync()
    ).slice(0,100).map(mapStyle);
    designSystem.gridStyles=(
      await figma.getLocalGridStylesAsync()
    ).slice(0,100).map(mapStyle);
  }
  return {
    editorType:figma.editorType,
    root:measured,
    designSystem,
    limits:{
      maxNodes:AUTHORING_MAX_NODES,
      maxFindings:AUTHORING_MAX_FINDINGS,
    },
  };
}

async function handleSemanticAuthoring(
  request:BridgeRequest,
  a:any,
):Promise<BridgeResponse|null>{
  switch(request.operation){
    case "composition.inspect":
      return ok(request.id,await authoringInspect(a));
    case "composition.plan":
      return ok(
        request.id,
        await authoringDraftChangeSet(
          authoringObject(a.spec,"composition_spec_required"),
        ),
      );
    case "composition.apply": {
      try {
        const result=await authoringApplyComposition(
          authoringObject(a.plan,"plan_required"),
        );
        return ok(
          request.id,
          {...result,observedRevision:revision+1},
          true,
        );
      } catch(error) {
        revision++;
        return fail(
          request.id,
          "semantic_apply_partial_or_unknown",
          error instanceof Error?error.message:"semantic apply failed",
          false,
        );
      }
    }
    case "composition.measure": {
      const root=await nodeById(String(a.root_node_id));
      const measurement=await authoringMeasure(
        root,
        Number(a.max_nodes??AUTHORING_MAX_NODES),
      );
      return ok(
        request.id,
        {...measurement,observedRevision:revision},
      );
    }
    case "composition.validate": {
      const root=await nodeById(String(a.root_node_id));
      const validation=await authoringValidate(
        root,
        a.spec??null,
        Number(a.max_findings??AUTHORING_MAX_FINDINGS),
      );
      return ok(
        request.id,
        {...validation,observedRevision:revision},
      );
    }
    case "composition.repair.plan":
      return ok(
        request.id,
        await authoringRepairChangeSet(
          authoringObject(a.plan,"plan_required"),
          authoringArray(
            a.findings,
            AUTHORING_MAX_FINDINGS,
            "finding_limit",
          ),
        ),
      );
    case "composition.repair.apply": {
      try {
        const result=await authoringApplyRepairs(
          authoringObject(a.plan,"plan_required"),
        );
        return ok(
          request.id,
          {...result,observedRevision:revision+1},
          true,
        );
      } catch(error) {
        revision++;
        return fail(
          request.id,
          "semantic_repair_partial_or_unknown",
          error instanceof Error?error.message:"semantic repair failed",
          false,
        );
      }
    }
    case "composition.verify": {
      const root=await nodeById(String(a.root_node_id));
      const measurement=await authoringMeasure(
        root,
        AUTHORING_MAX_NODES,
      );
      const validation=await authoringValidate(
        root,
        a.spec??null,
        Number(a.max_findings??AUTHORING_MAX_FINDINGS),
      );
      const artifact:any=await semanticVerifyNode(
        {
          nodeId:root.id,
          scale:a.scale??1,
          name:a.name,
        },
        request.id,
      );
      if(validation.findings.length<Number(a.max_findings??AUTHORING_MAX_FINDINGS)){
        validation.findings.push(authoringFinding(
          "visual_judgment_required",
          root,
          authoringMeta(root)?.logicalId,
          "model-or-human visual review",
          "verification artifact available",
          {
            artifactToken:artifact.token,
            mediaType:artifact.mediaType,
            source:"artifact-backed-png",
          },
          "info",
          [],
          "AESTHETIC_ASSIST",
        ));
      }
      return ok(request.id,{
        token:artifact.token,
        bytes:artifact.bytes,
        mediaType:artifact.mediaType,
        name:artifact.name,
        nodeId:root.id,
        scale:artifact.scale,
        measurement,
        validation,
        observedRevision:revision,
      });
    }
    default:
      return null;
  }
}
