/** First-party Motion Canvas realization. No eval, source payload, shell or URL dispatch.
 * API baseline: @motion-canvas/{core,2d} 3.17.2. Generated data is validated by
 * semwright-motion-authoring before it enters this module; runtime guards remain.
 */
import {Node,Layout,Rect,Circle,Line,Txt,Code,Img,Video,Camera,makeScene2D,lines,LezerHighlighter} from '@motion-canvas/2d';
import {Vector2,all,delay,waitFor,tween,linear,easeInCubic,easeOutCubic,easeInOutCubic,easeOutBack,easeOutExpo,easeInOutSine,easeOutElastic,useTime} from '@motion-canvas/core';
import type {ThreadGenerator,TimingFunction} from '@motion-canvas/core';
import {parser as jsParser} from '@lezer/javascript';
import {parser as pythonParser} from '@lezer/python';
import {parser as rustParser} from '@lezer/rust';

type Json = null|boolean|number|string|Json[]|{[key:string]:Json};
type Rat = {num:string;den:string};
type Point = {x:number;y:number};
type Size = {width:number;height:number};
type Insets = {top:number;right:number;bottom:number;left:number};
type Known<T> = {status:'known';value:T}|{status:'unknown';reason:string};
type Channel = 'y'|'position'|'world_position'|'scale'|'world_scale'|'opacity'|'rotation'|'width'|'height'|'line_start'|'line_end'|'font_size'|'fill'|'letter_spacing'|'camera_zoom'|'code';
type Operand = {value:'number';data:number}|{value:'vector';data:Point}|{value:'color'|'text';data:string}|{value:'original'}|{value:'original_offset';data:Point}|{value:'original_scale';data:number}|{value:'peer';data:{subject:string;channel:Channel}}|{value:'peer_offset';data:{subject:string;offset:Point}}|{value:'exploded';data:{origin:Point;spread:number}};
type NativeOp =
 {operation:'tween';target:string;channel:Channel;from:Operand|null;to:Operand}|
 {operation:'set';target:string;channel:Channel;value:Operand}|
 {operation:'reactive_connection';path:string;from:string;to:string}|
 {operation:'path_follow';path:string;marker:string;from:number;to:number;orient:boolean}|
 {operation:'camera_follow';camera:string;target:string}|
 {operation:'morph_points';target:string;from:Point[];to:Point[];closed:boolean}|
 {operation:'code_selection';target:string;first_line:number;end_line_exclusive:number}|
 {operation:'counter';target:string;from:number;to:number;decimal_places:number;prefix:string;suffix:string}|
 {operation:'hold';targets:string[]}|
 {operation:'local_region';target:string;overlay:string;center:Point;size:Size};
export type Instruction = {id:string;invocation:string;sequence:string;shot:string;start:Rat;duration:Rat;easing:string;operation:NativeOp};
type Run={text:string;weight:number;color:string|null;emphasis:boolean};
type Content = {kind:'group'}|{kind:'text';runs:Run[];style:string;direction:'auto'|'ltr'|'rtl';language:string;wrap:boolean;truncate:boolean}|{kind:'rectangle';fill:string;stroke:string|null;radius:number}|{kind:'circle';fill:string;stroke:string|null}|{kind:'path';points:Point[];closed:boolean;stroke:string;stroke_width:number}|{kind:'image';asset_id:string;fit:string;ratio:number}|{kind:'video';asset_id:string;fit:string;ratio:number;source_offset:Rat}|{kind:'code';source:string;language:string;font_style:string}|{kind:'camera';zoom:number};
type Spatial = {kind:'flow';grow:number;align:string}|{kind:'fixed';position:Point;size:Size}|{kind:'stack';axis:string;gap:number;padding:Insets;align:string}|{kind:'split';ratio:number;gap:number;portrait_axis:string}|{kind:'grid';columns:number;gap:number;portrait_columns:number}|{kind:'overlay';anchor:string;offset:Point;intentional:boolean};
type Subject={id:string;role:string;parent:string|null;layer:string;content:Content;layout:Spatial;initially_visible:boolean;clip_intentional:boolean};
type NativeShot={id:string;archetype:string;start:Rat;end:Rat;subjects:Subject[];layers:{id:string;order:number;intentional_overlay:boolean}[];annotations:{id:string;subject:string;anchor_subject:string;offset:Point}[];captions:{id:string;subject:string;cue_id:string;language:string;text:string}[]};
type FontSpec={family:string;asset_digest:string|null;fallback:string;permitted_fallbacks:string[]};
type Editorial={font:FontSpec;mono_font:FontSpec;type_scale:Record<string,number>;colors:Record<string,string>;spacing:Record<string,number>;stroke:number;corner_radius:number};
type ResolvedCue={status:'resolved';start:Rat;end:Rat}|{status:'unknown';reason:string};
export type NativeSceneData={version:1;id:string;start:Rat;end:Rat;width:number;height:number;aspect:string;safe_area:Insets;editorial:Editorial;shots:NativeShot[];instructions:Instruction[];cues:Record<string,ResolvedCue>};
export type AssetUrls=Readonly<Record<string,string>>;
export type FontEvidence={family:string;sha256:string;weights:number[];codepoints:number[][];face_loaded:boolean};
export type FrameBinding={render_input_digest:string;native_stage_version:'3.17.2';font_evidence:FontEvidence[]};
export type NativeProbe={frame:number;subjects:unknown[];transitions:unknown[];captions:unknown[];unresolved_cues:string[]};
declare global {
 var __SEMWRIGHT_NATIVE_PROBE__: undefined|((canvas:HTMLCanvasElement,frame:number,binding:FrameBinding)=>Promise<NativeProbe>);
 var __SEMWRIGHT_NATIVE_FRAME_CLOCK__: undefined|{frame:number;fps_num:number;fps_den:number};
}
const known=<T>(value:T):Known<T>=>({status:'known',value});
const unknown=<T>(reason:string):Known<T>=>({status:'unknown',reason});
function requireValue(v:unknown,message:string):asserts v{if(!v)throw new Error(message);}
function finite(v:number,name:string){requireValue(Number.isFinite(v),`non-finite ${name}`);return v;}
function sec(q:Rat):number {requireValue(/^-?(0|[1-9][0-9]*)$/.test(q.num)&&/^[1-9][0-9]*$/.test(q.den),'invalid rational');return finite(Number(BigInt(q.num))/Number(BigInt(q.den)),'rational seconds');}
function vec(p:Point){return new Vector2(finite(p.x,'x'),finite(p.y,'y'));}
function align(v:string){switch(v){case 'start':return 'start';case 'end':return 'end';case 'stretch':return 'stretch';case 'baseline':return 'baseline';case 'center':return 'center';default:throw new Error('invalid align');}}
function color(s:string){requireValue(/^#(?:[0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/.test(s),'unsafe color');return s;}
function asset(urls:AssetUrls,id:string){const url=urls[id];requireValue(typeof url==='string' && /^(\/(?!\/)|\.\/|data:)/.test(url),'asset must be a generated local import');return url;}
const timings:Record<string,TimingFunction>={linear,in_cubic:easeInCubic,out_cubic:easeOutCubic,in_out_cubic:easeInOutCubic,out_back:easeOutBack,out_expo:easeOutExpo,in_out_sine:easeInOutSine,out_elastic:easeOutElastic};
const archetypes:Record<string,{direction:'row'|'column';justify:'start'|'center'|'space-between';align:'start'|'center'|'stretch';columns:number;titleGrow:number}>={
 statement:{direction:'column',justify:'center',align:'start',columns:1,titleGrow:0},
 split_explanation:{direction:'row',justify:'start',align:'stretch',columns:2,titleGrow:0},
 architecture_reveal:{direction:'column',justify:'space-between',align:'stretch',columns:3,titleGrow:0},
 comparison:{direction:'row',justify:'center',align:'stretch',columns:2,titleGrow:0},
 metric:{direction:'column',justify:'center',align:'center',columns:1,titleGrow:1},
 timeline:{direction:'row',justify:'space-between',align:'center',columns:4,titleGrow:0},
 code_focus:{direction:'column',justify:'start',align:'stretch',columns:1,titleGrow:0},
 diagram_build:{direction:'column',justify:'center',align:'stretch',columns:3,titleGrow:0},
 object_spotlight:{direction:'column',justify:'center',align:'center',columns:1,titleGrow:0},
 evidence_frame:{direction:'column',justify:'space-between',align:'stretch',columns:1,titleGrow:0},
 product_proof:{direction:'row',justify:'start',align:'stretch',columns:2,titleGrow:0},
 endcard:{direction:'column',justify:'center',align:'center',columns:1,titleGrow:0},
};
function newSubject(s:Subject,d:NativeSceneData,urls:AssetUrls):Node {
 const c=s.content;const e=d.editorial;let n:Node;
 const textColor=e.colors.text??Object.values(e.colors)[0]??'#202020';
 switch(c.kind){
  case 'group':n=new Layout({});break;
  case 'text':{
   const props={fontFamily:e.font.family,fontSize:e.type_scale[c.style],textDirection:c.direction==='auto'?'inherit':c.direction,textWrap:c.wrap,fill:color(textColor)} as const;
   // Public nested Txt nodes are Motion Canvas' supported API for styled complex text.
   // Keep the parent layout-enabled so its DOM tree can shape/wrap the inline runs.
   n=new Txt({...props,children:c.runs.map(r=>new Txt({text:r.text,fontWeight:r.weight,fill:color(r.color??textColor),fontStyle:r.emphasis?'italic':'normal'}))});break;
  }
  case 'rectangle':n=new Rect({fill:color(c.fill),stroke:c.stroke?color(c.stroke):null,lineWidth:e.stroke,radius:c.radius});break;
  case 'circle':n=new Circle({fill:color(c.fill),stroke:c.stroke?color(c.stroke):null,lineWidth:e.stroke});break;
  case 'path':n=new Line({points:c.points.map(vec),closed:c.closed,stroke:color(c.stroke),lineWidth:c.stroke_width});break;
  case 'image':case 'video':{
   const outer=new Rect({clip:c.fit==='cover'});
   const media=c.kind==='image'?new Img({src:asset(urls,c.asset_id),ratio:c.ratio}):new Video({src:asset(urls,c.asset_id),ratio:c.ratio,time:sec(c.source_offset),play:false});
   media.layout(false);outer.add(media);
   media.width(()=>c.fit==='stretch'?outer.width():c.fit==='cover'?Math.max(outer.width(),outer.height()*c.ratio):Math.min(outer.width(),outer.height()*c.ratio));
   media.height(()=>c.fit==='stretch'?outer.height():media.width()/c.ratio);n=outer;break;
  }
  case 'code':{
   const parser=c.language==='python'?pythonParser:c.language==='rust'?rustParser:jsParser;
   n=new Code({code:c.source,fontFamily:e.mono_font.family,fontSize:e.type_scale[c.font_style],fill:color(textColor),highlighter:c.language==='plain'?null:new LezerHighlighter(parser)});break;
  }
  case 'camera':n=new Camera({zoom:c.zoom});break;
  default:throw new Error('unsupported subject kind');
 }
 n.opacity(s.initially_visible?1:0);
 if(n instanceof Layout){n.clip(n.clip()||s.clip_intentional);layoutSubject(n,s,d);}
 return n;
}
function layoutSubject(n:Layout,s:Subject,d:NativeSceneData){
 const l=s.layout;
 switch(l.kind){
  case 'flow':n.layout(true);n.grow(l.grow);n.alignSelf(align(l.align));break;
  case 'fixed':if(!(n instanceof Txt))n.layout(false);n.position(vec(l.position));n.size([l.size.width,l.size.height]);break;
  case 'stack':n.layout(true);n.direction(l.axis==='horizontal'?'row':'column');n.gap(l.gap);n.padding([l.padding.top,l.padding.right,l.padding.bottom,l.padding.left]);n.alignItems(align(l.align));break;
  case 'split':n.layout(true);n.direction(d.aspect==='portrait'?(l.portrait_axis==='horizontal'?'row':'column'):'row');n.gap(l.gap);break;
  case 'grid':n.layout(true);n.direction('row');n.wrap('wrap');n.gap(l.gap);break;
  case 'overlay':requireValue(l.intentional,'unannounced overlap');n.layout(false);break;
 }
}
function finishLayouts(subjects:Subject[],nodes:Map<string,Node>,d:NativeSceneData){
 for(const s of subjects){
  const n=nodes.get(s.id)!;const l=s.layout;
  if(l.kind==='overlay'){const anchor=nodes.get(l.anchor);requireValue(anchor,'overlay anchor unavailable');n.absolutePosition(()=>anchor.absolutePosition().add(vec(l.offset)));}
  if((l.kind==='split'||l.kind==='grid') && n instanceof Layout){
   const children=subjects.filter(c=>c.parent===s.id).map(c=>nodes.get(c.id)!).filter((x):x is Layout=>x instanceof Layout);
   if(l.kind==='split'){
    requireValue(children.length===2,'split requires exactly two layout children');
    children[0].grow(l.ratio);children[1].grow(1-l.ratio);children.forEach(c=>{c.basis(0);c.shrink(1);});
   }else{
    const columns=d.aspect==='portrait'?l.portrait_columns:l.columns;
    children.forEach(c=>{c.width(()=>Math.max(0,(n.width()-(columns-1)*l.gap)/columns));c.shrink(0);});
   }
  }
 }
}
function createShot(shot:NativeShot,d:NativeSceneData,urls:AssetUrls,nodes:Map<string,Node>):Layout {
 const a=archetypes[shot.archetype];requireValue(a,'unknown archetype');
 const portrait=d.aspect==='portrait';const gap=d.editorial.spacing.block??d.editorial.spacing.base??24;
 const inset=d.safe_area;
 const root=new Layout({layout:true,size:[d.width,d.height],padding:[inset.top,inset.right,inset.bottom,inset.left],direction:portrait?'column':a.direction,justifyContent:a.justify,alignItems:a.align,gap,opacity:0});
 nodes.set(`sw-shot-${shot.id}`,root);
 const byId=new Map(shot.subjects.map(s=>[s.id,s]));
 const pending=new Map(byId);let count=0;
 while(pending.size){let progress=false;
  for(const [id,s] of pending){if(s.parent&&!nodes.has(s.parent))continue;
   requireValue(!nodes.has(id),'duplicate logical id');const n=newSubject(s,d,urls);nodes.set(id,n);
   const parent=s.parent?nodes.get(s.parent)!:root;
   // A camera renders its scene via the native scene signal, not ordinary children.
   if(parent instanceof Camera){let scene=parent.scene();if(!scene){scene=new Node({});parent.scene(scene);}scene.add(n);}else parent.add(n);
   const layer=shot.layers.find(l=>l.id===s.layer);requireValue(layer,'unknown layer');n.zIndex(layer.order);
   if(!s.parent&&n instanceof Layout&&s.layout.kind==='flow'){
    if(['primary','secondary','media','evidence','code','metric'].includes(s.role)){n.grow(s.role==='metric'?Math.max(1,a.titleGrow):1);n.shrink(1);n.basis(0);}
    if(s.role==='caption'||s.role==='annotation')n.grow(0);
   }
   pending.delete(id);progress=true;if(++count>512)throw new Error('node limit');
  }
  requireValue(progress,'native parent graph cannot be resolved');
 }
 finishLayouts(shot.subjects,nodes,d);
 for(const a of shot.annotations){const label=nodes.get(a.subject);const target=nodes.get(a.anchor_subject);requireValue(label&&target,'annotation binding missing');label.absolutePosition(()=>target.absolutePosition().add(vec(a.offset)));}
 return root;
}
// The following is a closed mapping. A model cannot address arbitrary signal names.
type RuntimeValue=number|string|Vector2;
type SignalAdapter={get:()=>RuntimeValue;set:(v:RuntimeValue)=>void;tween:(v:RuntimeValue,t:number,f:TimingFunction)=>ThreadGenerator};
function sig(n:Node,c:Channel):SignalAdapter {
 const wrap=(s:unknown):SignalAdapter=>{
  requireValue(typeof s==='function','native signal not available');
  const f=s as ((...a:unknown[])=>unknown);
  return {get:()=>f() as RuntimeValue,set:v=>{f(v);},tween:(v,t,e)=>f(v,t,e) as ThreadGenerator};
 };
 switch(c){
  case 'y':return wrap(n.y);case 'position':return wrap(n.position);case 'world_position':return wrap(n.absolutePosition);
  case 'scale':return wrap(n.scale);case 'world_scale':return wrap(n.absoluteScale);case 'opacity':return wrap(n.opacity);case 'rotation':return wrap(n.rotation);
  case 'width':requireValue(n instanceof Layout,'width requires layout');return wrap(n.width);
  case 'height':requireValue(n instanceof Layout,'height requires layout');return wrap(n.height);
  case 'line_start':requireValue(n instanceof Line,'line start requires Line');return wrap(n.start);
  case 'line_end':requireValue(n instanceof Line,'line end requires Line');return wrap(n.end);
  case 'font_size':requireValue(n instanceof Layout,'font size requires Layout');return wrap(n.fontSize);
  case 'letter_spacing':requireValue(n instanceof Layout,'tracking requires Layout');return wrap(n.letterSpacing);
  case 'fill':requireValue(n instanceof Rect||n instanceof Txt||n instanceof Code||n instanceof Circle,'fill requires shape');return wrap(n.fill);
  case 'camera_zoom':requireValue(n instanceof Camera,'zoom requires Camera');return wrap(n.zoom);
  case 'code':requireValue(n instanceof Code,'code requires native Code');return {get:()=>n.parsed(),set:v=>n.code(String(v)),tween:(v,t,e)=>n.code(String(v),t,e)};
 }
}
function cloned(v:RuntimeValue):RuntimeValue{return v instanceof Vector2?new Vector2(v.x,v.y):v;}
function asVector(v:RuntimeValue){requireValue(v instanceof Vector2,'vector operand required');return v;}
function plus(v:RuntimeValue,p:Point){return asVector(v).add(vec(p));}
function times(v:RuntimeValue,k:number){return v instanceof Vector2?v.scale(k):finite(Number(v)*k,'scaled operand');}
const channels:Channel[]=['y','position','world_position','scale','world_scale','opacity','rotation','width','height','line_start','line_end','font_size','fill','letter_spacing','camera_zoom','code'];
function snapshot(nodes:Map<string,Node>):Map<string,RuntimeValue>{
 const out=new Map<string,RuntimeValue>();for(const [id,n] of nodes)for(const c of channels){try{out.set(`${id}:${c}`,cloned(sig(n,c).get()));}catch{/* unsupported channels are not measured as zero */}}
 return out;
}
function resolve(o:Operand,target:string,c:Channel,initial:Map<string,RuntimeValue>):RuntimeValue {
 const read=(id:string,ch:Channel)=>{const v=initial.get(`${id}:${ch}`);requireValue(v!==undefined,`original value unavailable: ${id}:${ch}`);return cloned(v);};
 switch(o.value){case 'number':return finite(o.data,'operand');case 'text':return o.data;case 'color':return color(o.data);case 'vector':return vec(o.data);case 'original':return read(target,c);case 'original_offset':return plus(read(target,c),o.data);case 'original_scale':return times(read(target,c),o.data);case 'peer':return read(o.data.subject,o.data.channel);case 'peer_offset':return plus(read(o.data.subject,'world_position'),o.data.offset);case 'exploded':return vec(o.data.origin).add(asVector(read(target,c)).sub(vec(o.data.origin)).scale(o.data.spread));}
}
function* executeInstruction(i:Instruction,nodes:Map<string,Node>,initial:Map<string,RuntimeValue>):ThreadGenerator {
 const op=i.operation;const duration=sec(i.duration);const easing=timings[i.easing];requireValue(easing,'unknown easing');
 const node=(id:string)=>{const n=nodes.get(id);requireValue(n,`missing subject ${id}`);return n;};
 switch(op.operation){
  case 'tween':{const signal=sig(node(op.target),op.channel);if(op.from)signal.set(resolve(op.from,op.target,op.channel,initial));yield* signal.tween(resolve(op.to,op.target,op.channel,initial),duration,easing);break;}
  case 'set':sig(node(op.target),op.channel).set(resolve(op.value,op.target,op.channel,initial));break;
  case 'reactive_connection':{const path=node(op.path);requireValue(path instanceof Line,'connection requires Line');path.points(()=>[node(op.from).absolutePosition().transformAsPoint(path.worldToLocal()),node(op.to).absolutePosition().transformAsPoint(path.worldToLocal())]);break;}
  case 'path_follow':{const path=node(op.path);const marker=node(op.marker);requireValue(path instanceof Line,'trace requires native Line');yield* tween(duration,p=>{const sample=path.getPointAtPercentage(op.from+(op.to-op.from)*easing(p));marker.absolutePosition(sample.position.transformAsPoint(path.localToWorld()));if(op.orient){const tangent=sample.tangent.transform(path.localToWorld());marker.absoluteRotation(Math.atan2(tangent.y,tangent.x)*180/Math.PI);}});break;}
  case 'camera_follow':{const cam=node(op.camera);requireValue(cam instanceof Camera,'follow requires Camera');cam.absolutePosition(()=>node(op.target).absolutePosition());yield* waitFor(duration);cam.absolutePosition(cloned(cam.absolutePosition()) as Vector2);break;}
  case 'morph_points':{const target=node(op.target);requireValue(target instanceof Line&&op.from.length===op.to.length&&op.from.length>=2,'incompatible morph topology');target.closed(op.closed);target.points(op.from.map(vec));yield* target.points(op.to.map(vec),duration,easing);break;}
  case 'code_selection':{const code=node(op.target);requireValue(code instanceof Code,'selection requires Code');yield* code.selection(lines(op.first_line,op.end_line_exclusive-1),duration,easing);break;}
  case 'counter':{const text=node(op.target);requireValue(text instanceof Txt,'counter requires text');yield* tween(duration,p=>{text.text(op.prefix+(op.from+(op.to-op.from)*easing(p)).toFixed(op.decimal_places)+op.suffix);});break;}
  case 'hold':yield* waitFor(duration);break;
  case 'local_region':{const target=node(op.target);const overlay=node(op.overlay);requireValue(overlay instanceof Layout,'region requires Layout');overlay.absolutePosition(()=>vec(op.center).transformAsPoint(target.localToWorld()));overlay.size([op.size.width,op.size.height]);break;}
 }
}
const sceneRegistrations=new Map<string,{data:NativeSceneData;nodes:Map<string,Node>;draws:Map<string,DrawRecord>;rendered:Set<string>;canvas:HTMLCanvasElement|null;drawSerial:number;finished:Set<string>;captionActive:Map<string,boolean>}>();
type DrawRecord={canvas:HTMLCanvasElement;matrix:DOMMatrix;bounds:{x:number;y:number;width:number;height:number};opacity:number;serial:number;clip:boolean};
function clipAncestor(n:Node){for(let a:Node|null=n;a;a=a.parent()){if(a instanceof Layout&&a.clip())return true;}return false;}
function instrument(n:Node,id:string,reg:ReturnType<typeof registration>){
 const render=n.render.bind(n);
 n.render=(context:CanvasRenderingContext2D)=>{
  const matrix=context.getTransform().multiply(n.localToParent());
  const result=render(context);
  if(n.absoluteOpacity()>0){
   reg.rendered.add(id);
   // Layout/Txt cache bounds can require the native DOM layout pass. Never invoke
   // that pass before Motion Canvas renders the node, and do not turn missing
   // observation geometry into a render failure.
   try{const box=n.cacheBBox();reg.draws.set(id,{canvas:context.canvas,matrix,bounds:{x:box.x,y:box.y,width:box.width,height:box.height},opacity:n.absoluteOpacity(),serial:reg.drawSerial++,clip:clipAncestor(n)});}catch{/* geometry remains UNKNOWN */}
  }
  return result;
 };
}
function registration(data:NativeSceneData,nodes:Map<string,Node>){return {data,nodes,draws:new Map<string,DrawRecord>(),rendered:new Set<string>(),canvas:null as HTMLCanvasElement|null,drawSerial:0,finished:new Set<string>(),captionActive:new Map<string,boolean>()};}
function transformed(box:DrawRecord['bounds'],m:DOMMatrix){const p=[[box.x,box.y],[box.x+box.width,box.y],[box.x,box.y+box.height],[box.x+box.width,box.y+box.height]].map(([x,y])=>m.transformPoint({x,y}));const x=Math.min(...p.map(x=>x.x)),y=Math.min(...p.map(x=>x.y));return{x,y,width:Math.max(...p.map(x=>x.x))-x,height:Math.max(...p.map(x=>x.y))-y};}
async function textHash(text:string){const bytes=new TextEncoder().encode(text);const hash=await crypto.subtle.digest('SHA-256',bytes);return [...new Uint8Array(hash)].map(b=>b.toString(16).padStart(2,'0')).join('');}
function hasGlyph(face:FontEvidence,cp:number){return face.codepoints.some(([a,b])=>a<=cp&&cp<=b);}
function fontMeasurement(spec:FontSpec,text:string,weight:number,evidence:FontEvidence[]){
 const primary=evidence.find(f=>f.family===spec.family&&(!spec.asset_digest||f.sha256===spec.asset_digest)&&f.face_loaded&&f.weights.includes(weight));
 const cps=[...text].map(c=>c.codePointAt(0)!).filter(c=>c>32&&c!==0x200c&&c!==0x200d&&c!==0xfe0f);
 if(primary&&cps.every(c=>hasGlyph(primary,c)))return {font_family:known(spec.family),font_ready:known(true),fallback_used:known(false)};
 const fallback=spec.fallback==='allow_and_report'?evidence.find(f=>spec.permitted_fallbacks.includes(f.family)&&f.face_loaded&&f.weights.includes(weight)&&cps.every(c=>hasGlyph(f,c))):undefined;
 if(fallback)return {font_family:known(fallback.family),font_ready:known(true),fallback_used:known(true)};
 return {font_family:unknown<string>('verified font face/cmap does not cover every required code point'),font_ready:known(false),fallback_used:unknown<boolean>('browser per-glyph fallback not observable')};
}
async function probeScene(reg:ReturnType<typeof registration>,canvas:HTMLCanvasElement,frame:number,binding:FrameBinding){
 requireValue(binding.native_stage_version==='3.17.2','stage compositor baseline changed');
 const output:unknown[]=[];
 for(const shot of reg.data.shots)for(const subject of shot.subjects){
  const n=reg.nodes.get(subject.id)!;const draw=reg.draws.get(subject.id);
  // 3.17.2 Stage copies same-sized current/previous buffers at (0,0). A cache
  // canvas has no such identity guarantee and is deliberately UNKNOWN.
  const coordinateKnown=!!draw&&draw.canvas===reg.canvas&&draw.canvas.width===canvas.width&&draw.canvas.height===canvas.height;
  let text:unknown=null;const c=subject.content;
  if(c.kind==='text'&&n instanceof Txt){
   const observed=n.text();const expected=c.runs.map(r=>r.text).join('');
   const el=n.element;const meaningful=el instanceof HTMLElement&&el.isConnected&&el.clientWidth>0&&el.clientHeight>0;
   let clipped:Known<boolean>=unknown('native DOM text metrics unavailable');
   let lineCount:Known<number>=unknown('native line rectangles unavailable');
   let direction:Known<string>=unknown('native DOM text direction unavailable');
   if(meaningful){
    const computed=getComputedStyle(el);const range=document.createRange();range.selectNodeContents(el);const rects=[...range.getClientRects()].filter(r=>r.width>0&&r.height>0);
    const tops=new Set(rects.map(r=>Math.round(r.top*100)/100));
    clipped=known(el.scrollWidth>el.clientWidth+0.5||el.scrollHeight>el.clientHeight+0.5);lineCount=known(tops.size);direction=known(computed.direction);
   }
   const perRun=c.runs.map(r=>fontMeasurement(reg.data.editorial.font,r.text,r.weight,binding.font_evidence));
   const font=perRun.find(f=>f.font_ready.status==='unknown'||!f.font_ready.value)??perRun.find(f=>f.fallback_used.status==='known'&&f.fallback_used.value)??perRun[0];
   text={logical_text_digest:await textHash(expected),observed_text_digest:await textHash(observed),truncated:clipped,lines:lineCount,...font,direction};
  }
  if(c.kind==='code'&&n instanceof Code){
   const observed=n.parsed();const boxes=n.getSelectionBBox(lines(0,Math.max(0,observed.split('\n').length-1)));
   const width=n.width(),height=n.height();
   const clipped=n.clip()&&boxes.some(b=>b.x < -width/2-0.5||b.y < -height/2-0.5||b.x+b.width>width/2+0.5||b.y+b.height>height/2+0.5);
   text={logical_text_digest:await textHash(c.source),observed_text_digest:await textHash(observed),truncated:known(clipped),lines:known(observed.split('\n').length),...fontMeasurement(reg.data.editorial.mono_font,observed,400,binding.font_evidence),direction:known('ltr')};
  }
  const rendered=reg.rendered.has(subject.id);
  let asset_ready:Known<boolean>=known(true);
  if(c.kind==='image'||c.kind==='video')asset_ready=rendered?known(true):unknown('asset was not observed in a completed native draw');
  output.push({id:subject.id,local_size:draw?known({width:draw.bounds.width,height:draw.bounds.height}):unknown('native layout size unavailable'),bounds:coordinateKnown?known(transformed(draw!.bounds,draw!.matrix)):unknown('not drawn in the native scene buffer or unmodeled cache transform'),transform:coordinateKnown?known([draw!.matrix.a,draw!.matrix.b,draw!.matrix.c,draw!.matrix.d,draw!.matrix.e,draw!.matrix.f]):unknown('output transform unavailable'),opacity:known(n.absoluteOpacity()),drawn:rendered,z_order:draw?known(draw.serial):unknown('no native draw order'),clip_active:known(clipAncestor(n)),pixel_visibility:unknown('draw and alpha do not prove pixel contribution after overlap'),text,asset_ready});
 }
 const transitions=[...new Set(reg.data.instructions.map(i=>i.invocation))].map(id=>({invocation_id:id,finished:known(reg.finished.has(id))}));
 const captions=reg.data.shots.flatMap(s=>s.captions.map(c=>{const cue=reg.data.cues[c.cue_id];return{id:c.id,active:known((reg.captionActive.get(c.id)??false)&&reg.draws.has(c.subject)&&reg.nodes.get(c.subject)!.absoluteOpacity()>0&&(reg.nodes.get(c.subject) as Txt).text()===c.text),cue_start:cue?.status==='resolved'?known(cue.start):unknown('unresolved cue'),cue_end:cue?.status==='resolved'?known(cue.end):unknown('unresolved cue')};}));
 return {frame,subjects:output,transitions,captions,unresolved_cues:Object.entries(reg.data.cues).filter(([,v])=>v.status!=='resolved').map(([k])=>k)};
}
export function createAuthoringScene(data:NativeSceneData,urls:AssetUrls){
 requireValue(data.version===1&&data.shots.length<=128&&data.instructions.length<=8192,'native scene bounds');
 return makeScene2D(function*(view){
  const nodes=new Map<string,Node>();const reg=registration(data,nodes);sceneRegistrations.set(data.id,reg);
  for(const shot of data.shots)view.add(createShot(shot,data,urls,nodes));
  const originalRender=view.render.bind(view);
  view.render=(ctx:CanvasRenderingContext2D)=>{reg.canvas=ctx.canvas;reg.draws.clear();reg.rendered.clear();reg.drawSerial=0;return originalRender(ctx);};
  for(const [id,n] of nodes)instrument(n,id,reg);
  // Native dependency resolution participates in renderer startup; fonts and
  // media are awaited by native promises, never set ready from the desired model.
  yield view.toPromise();yield document.fonts.ready;
  const start=sec(data.start),end=sec(data.end);const tasks:ThreadGenerator[]=[];
  for(const shot of data.shots){
   const root=nodes.get(`sw-shot-${shot.id}`)!;
   tasks.push(delay(sec(shot.start)-start,(function*(){root.opacity(1);for(const s of shot.subjects){const n=nodes.get(s.id);if(n instanceof Video)n.play();else n?.findAll(x=>x instanceof Video).forEach(x=>(x as Video).play());}yield* waitFor(sec(shot.end)-sec(shot.start));root.opacity(0);for(const s of shot.subjects){const n=nodes.get(s.id);if(n instanceof Video)n.pause();else n?.findAll(x=>x instanceof Video).forEach(x=>(x as Video).pause());}})()));
   for(const caption of shot.captions){const cue=data.cues[caption.cue_id];const node=nodes.get(caption.subject);requireValue(node instanceof Txt,'caption requires native Txt');node.opacity(0);
    if(cue?.status==='resolved')tasks.push(delay(sec(cue.start)-start,(function*(){node.text(caption.text);node.opacity(1);reg.captionActive.set(caption.id,true);yield* waitFor(sec(cue.end)-sec(cue.start));node.opacity(0);reg.captionActive.set(caption.id,false);})()));
   }
  }
  const groups=new Map<string,Instruction[]>();for(const i of data.instructions){const values=groups.get(i.invocation)??[];values.push(i);groups.set(i.invocation,values);}
  for(const [id,group]of groups){group.sort((a,b)=>sec(a.start)-sec(b.start)||(a.id<b.id?-1:a.id>b.id?1:0));const first=sec(group[0].start);
   tasks.push(delay(first-start,(function*(){
    // One coherent snapshot per invocation prevents swap and multi-stage settle
    // from reading values already overwritten by a sibling suboperation.
    const initial=snapshot(nodes);
    yield* all(...group.map(i=>delay(sec(i.start)-first,executeInstruction(i,nodes,initial))));reg.finished.add(id);
   })()));
  }
  globalThis.__SEMWRIGHT_NATIVE_PROBE__=async(canvas,frame,binding)=>{
   const at=globalThis.__SEMWRIGHT_NATIVE_FRAME_CLOCK__;
   requireValue(at&&at.frame===frame,'native frame clock not supplied by exporter');
   const time=frame*at.fps_den/at.fps_num;
   const active=[...sceneRegistrations.values()].filter(r=>time>=sec(r.data.start)-1e-9&&time<sec(r.data.end)-1e-9);
   const probes=await Promise.all(active.map(r=>probeScene(r,canvas,frame,binding)));
   return {frame,subjects:probes.flatMap(p=>p.subjects),transitions:probes.flatMap(p=>p.transitions),captions:probes.flatMap(p=>p.captions),unresolved_cues:[...new Set(probes.flatMap(p=>p.unresolved_cues))]};
  };
  yield* all(waitFor(end-start),...tasks);
 });
}

export type NativeProbeFunction=(canvas:HTMLCanvasElement,frame:number,binding:FrameBinding)=>Promise<NativeProbe>;
export declare const nativeProbeType: NativeProbeFunction;
