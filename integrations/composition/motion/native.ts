/** First-party Motion Canvas realization. No eval, source payload, shell or URL dispatch.
 * API baseline: @motion-canvas/{core,2d} 3.17.2. Generated data is validated by
 * semwright-motion-authoring before it enters this module; runtime guards remain.
 */
import {Node,Layout,Rect,Circle,Line,Txt,Code,Img,Video,Camera,makeScene2D,lines,LezerHighlighter} from '@motion-canvas/2d';
import {Vector2,all,delay,waitFor,tween,linear,easeInCubic,easeOutCubic,easeInOutCubic,easeOutBack,easeOutExpo,easeInOutSine,easeOutElastic,useTime,useThread,usePlayback} from '@motion-canvas/core';
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
type Content = {kind:'group'}|{kind:'text';runs:Run[];style:string;line_height?:number|null;direction:'auto'|'ltr'|'rtl';language:string;wrap:boolean;truncate:boolean}|{kind:'rectangle';fill:string;stroke:string|null;radius:number}|{kind:'circle';fill:string;stroke:string|null}|{kind:'path';points:Point[];closed:boolean;stroke:string;stroke_width:number}|{kind:'image';asset_id:string;fit:string;ratio:number}|{kind:'video';asset_id:string;fit:string;ratio:number;source_offset:Rat}|{kind:'code';source:string;language:string;font_style:string}|{kind:'camera';zoom:number};
type Spatial = {kind:'flow';grow:number;align:string}|{kind:'fixed';position:Point;size:Size}|{kind:'stack';axis:string;gap:number;padding:Insets;align:string}|{kind:'split';ratio:number;gap:number;portrait_axis:string}|{kind:'grid';columns:number;gap:number;portrait_columns:number}|{kind:'overlay';anchor:string;offset:Point;intentional:boolean};
type Subject={id:string;role:string;parent:string|null;layer:string;content:Content;layout:Spatial;initially_visible:boolean;clip_intentional:boolean};
type NativeShot={id:string;archetype:string;start:Rat;end:Rat;subjects:Subject[];layers:{id:string;order:number;intentional_overlay:boolean}[];annotations:{id:string;subject:string;anchor_subject:string;offset:Point}[];captions:{id:string;subject:string;cue_id:string;language:string;text:string}[]};
type FontSpec={family:string;asset_digest:string|null;fallback:string;permitted_fallbacks:string[]};
type Editorial={font:FontSpec;mono_font:FontSpec;type_scale:Record<string,number>;colors:Record<string,string>;spacing:Record<string,number>;stroke:number;corner_radius:number};
type ResolvedCue={status:'resolved';start:Rat;end:Rat}|{status:'unknown';reason:string};
export type NativeSceneData={version:1;id:string;start:Rat;end:Rat;width:number;height:number;aspect:string;safe_area:Insets;editorial:Editorial;shots:NativeShot[];instructions:Instruction[];cues:Record<string,ResolvedCue>};
export type AssetUrls=Readonly<Record<string,string>>;
export type FontEvidence={family:string;sha256:string;weights:number[];codepoints:number[][];face_loaded:boolean};
export type FrameBinding={render_input_digest:string;native_stage_version:'3.17.2';font_evidence:FontEvidence[];
 text_pixel_observation_frames?:number[];text_pixel_background?:string|null};
export type NativeProbe={frame:number;subjects:unknown[];transitions:unknown[];captions:unknown[];unresolved_cues:string[]};
declare global {
 var __SEMWRIGHT_NATIVE_PROBE__: undefined|((canvas:HTMLCanvasElement,frame:number,binding:FrameBinding)=>Promise<NativeProbe>);
 var __SEMWRIGHT_NATIVE_FRAME_CLOCK__: undefined|{frame:number;fps_num:number;fps_den:number};
 var __SEMWRIGHT_NATIVE_CONFIG__: undefined|(FrameBinding&{fps_num:number;fps_den:number});
 var __SEMWRIGHT_TEXT_DIGEST__: undefined|((text:string)=>Promise<string>);
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
   const ratio=c.line_height;
   if(ratio!=null)requireValue(typeof ratio==='number'&&Number.isFinite(ratio)&&ratio>=0.1&&ratio<=10,'invalid authored line-height ratio');
   const props={fontFamily:e.font.family,fontSize:e.type_scale[c.style],textDirection:c.direction==='auto'?'inherit':c.direction,textWrap:c.wrap,fill:color(textColor)} as const;
   // Public nested Txt nodes are Motion Canvas' supported API for styled complex text.
   // Keep the parent layout-enabled so its DOM tree can shape/wrap the inline runs.
   const text=new Txt({...props,children:c.runs.map(r=>new Txt({text:r.text,fontWeight:r.weight,fill:color(r.color??textColor),fontStyle:r.emphasis?'italic':'normal'}))});
   if(ratio!=null)text.lineHeight(()=>finite(ratio*text.fontSize(),'native line height'));
   n=text;break;
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
function createShot(shot:NativeShot,d:NativeSceneData,urls:AssetUrls,nodes:Map<string,Node>,reg:ReturnType<typeof registration>):Layout {
 const a=archetypes[shot.archetype];requireValue(a,'unknown archetype');
 const portrait=d.aspect==='portrait';const gap=d.editorial.spacing.block??d.editorial.spacing.base??24;
 const inset=d.safe_area;
 // Motion Canvas can export frame 0 before a delay(0) task gets its first turn.
 // Materialize any shot already active at the scene boundary before the first
 // renderer yield so native frame evidence matches the Film half-open schedule.
 const initiallyActive=sec(shot.start)<=sec(d.start)+1e-9&&sec(d.start)<sec(shot.end)-1e-9;
 const root=new Layout({layout:true,size:[d.width,d.height],padding:[inset.top,inset.right,inset.bottom,inset.left],direction:portrait?'column':a.direction,justifyContent:a.justify,alignItems:a.align,gap,opacity:initiallyActive?1:0});
 nodes.set(`sw-shot-${shot.id}`,root);
 const byId=new Map(shot.subjects.map(s=>[s.id,s]));
 const pending=new Map(byId);let count=0;
 while(pending.size){let progress=false;
  for(const [id,s] of pending){if(s.parent&&!nodes.has(s.parent))continue;
   requireValue(!nodes.has(id),'duplicate logical id');const n=newSubject(s,d,urls);nodes.set(id,n);
   // Instrument before attaching the node. Motion Canvas may populate a render
   // cache as soon as a subtree is attached; wrapping after view.add can miss
   // the first native frame and make deterministic bounds appear UNKNOWN.
   instrument(n,id,reg);
   const parent=s.parent?nodes.get(s.parent)!:root;
   // A camera renders its scene via the native scene signal, not ordinary children.
   // Fixed Txt keeps its DOM-enabled inline runs. A non-layout parent makes
   // its own position signal authoritative while preserving the logical Txt.
   // The attachment follows the later layer assignment and any zIndex change.
   const attachment=s.layout.kind==='fixed'&&n instanceof Txt
    ?new Layout({layout:false,position:[0,0],rotation:0,scale:[1,1],
      opacity:1,clip:false,zIndex:()=>n.zIndex(),children:[n]}):n;
   if(parent instanceof Camera){let scene=parent.scene();if(!scene){scene=new Node({});parent.scene(scene);}scene.add(attachment);}else parent.add(attachment);
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
const sceneRegistrations=new Map<string,ReturnType<typeof registration>>();
// This extension records native layout separately from drawable/cache bounds.
// The public 3.17.2 Layout size signal reads computed DOM layout; it is not the
// authored desiredSize and may differ from the intended size in the RED arm.
type NativeNodeType='Node'|'Layout'|'Rect'|'Circle'|'Line'|'Txt'|'Code'|'Img'|'Video'|'Camera';
type LayoutDraw={root:boolean;box:{x:number;y:number;width:number;height:number}};
type DrawRecord={canvas:HTMLCanvasElement;matrix:DOMMatrix;bounds:{x:number;y:number;width:number;height:number};opacity:number;serial:number;clip:boolean;layout?:LayoutDraw};
function nativeNodeType(n:Node):NativeNodeType{
 if(n instanceof Txt)return 'Txt';if(n instanceof Code)return 'Code';
 if(n instanceof Video)return 'Video';if(n instanceof Img)return 'Img';
 if(n instanceof Circle)return 'Circle';if(n instanceof Line)return 'Line';
 if(n instanceof Rect)return 'Rect';if(n instanceof Camera)return 'Camera';
 if(n instanceof Layout)return 'Layout';return 'Node';
}
function readNativeLayout(n:Node):LayoutDraw|undefined{
 if(!(n instanceof Layout))return undefined;
 // Public getters run the native layout resolution used by Layout.drawOverlay.
 // No position/size/layout signal is assigned by this observation.
 const size=n.size();const width=finite(size.width,'observed layout width');
 const height=finite(size.height,'observed layout height');
 requireValue(width>=0&&height>=0&&width<=1e6&&height<=1e6,'observed layout size bounds');
 const root=n.isLayoutRoot();requireValue(typeof root==='boolean','observed layout root');
 return {root,box:{x:-width/2,y:-height/2,width,height}};
}
// Native draw observations. These records do not assign authored signals.
type Matrix6=[number,number,number,number,number,number];
type ScreenQuad=[[number,number],[number,number],[number,number],[number,number]];
type NativeLineageNode={native_key:string;subject_id:string|null;native_type:NativeNodeType;
 parent_key:string|null;local_to_parent:Matrix6;opacity:number;layout_enabled:Known<boolean>;clip:Known<boolean>};
type NativeRectClip={native_key:string;local_box:LayoutDraw['box'];matrix:Matrix6;screen_polygon:ScreenQuad};
type NativeOwnClip={kind:'none'}|{kind:'rect';clip:NativeRectClip};
type NativeGeometry={native_key:string;screen_layout_polygon:Known<ScreenQuad>;
 lineage:Known<NativeLineageNode[]>;own_clip:Known<NativeOwnClip>};
type NativeCameraIdentity={native_key:string;subject_id:string|null};
type NativeCameraScene={native_key:string;original_scene_key:string;current_scene_key:string;
 current_scene_is_original:boolean;draw_scene_key:string;draw_uses_original_scene:boolean;
 scene_transform:Known<Matrix6>;position:[number,number];rotation:number;zoom:number;
 cameras:Known<NativeCameraIdentity[]>};
type NativeClipRecord={canvas:HTMLCanvasElement;value:Known<NativeOwnClip>};
type NativeSceneDraw={scene:Node;canvas:HTMLCanvasElement;matrix:Matrix6;completed:boolean};
type NativeMethod=(this:unknown,...args:unknown[])=>unknown;

// Read-only contribution observation of real Canvas glyph paint operations.
// No isolated text render, generated GOLD, ROI proxy or RED/GREEN difference.
type TextPixelCall={serial:number;operation:'fill_text'|'stroke_text';matrix:Matrix6;
 text:string;text_digest:string;font:string;color:[number,number,number];x:number;y:number;
 max_width:number|null;changed_pixels:number;global_alpha:1;composite:'source-over';filter:'none'};
type TextPixelMask={encoding:'row-runs-u16le-base64';data:string;runs:number;changed_pixels:number};
type NativeTextPixels={native_key:string;frame:number;width:number;height:number;playback_fps:number;
 fps_num:number;fps_den:number;selected_frames:number[];render_input_digest:string;
 output_background:string|null;calls:TextPixelCall[];mask:TextPixelMask};
type PendingTextCall=Omit<TextPixelCall,'text_digest'>;
type TextPixelSubject={node:Txt;visits:number;completed:boolean;calls:PendingTextCall[];bits:Uint8Array};
type TextPixelFrame={frame:number;fps:number;fps_num:number;fps_den:number;samples:number[];
 digest:string;background:string|null;canvas:HTMLCanvasElement;context:CanvasRenderingContext2D;
 width:number;height:number;subjects:Map<string,TextPixelSubject>;union:Uint8Array;
 operations:number;callBytes:number;completed:boolean;restored:boolean;failure:string|null};
const TEXT_PIXEL_AREA=1920*1080,TEXT_PIXEL_MAX_CALLS=64,TEXT_PIXEL_RLE_BYTES=65532;
function textPixelRgb(value:unknown):[number,number,number]|null{
 if(typeof value!=='string')return null;
 const found=/^#([0-9a-f]{6})$/i.exec(value);
 if(!found)return null;
 return [0,2,4].map(at=>parseInt(found[1].slice(at,at+2),16)) as [number,number,number];
}
function textPixelBit(bits:Uint8Array,index:number){return (bits[index>>3]&(1<<(index&7)))!==0;}
function textPixelSet(bits:Uint8Array,index:number){bits[index>>3]|=1<<(index&7);}
function textPixelRle(bits:Uint8Array,width:number,height:number):TextPixelMask{
 const encoded=new Uint8Array(TEXT_PIXEL_RLE_BYTES);const view=new DataView(encoded.buffer);
 let used=0,changed=0;
 for(let y=0;y<height;y++)for(let x=0;x<width;){
  if(!textPixelBit(bits,y*width+x)){x++;continue;}
  const start=x;while(x<width&&textPixelBit(bits,y*width+x))x++;
  requireValue(used+6<=encoded.length,'native text pixel RLE budget');
  view.setUint16(used,y,true);view.setUint16(used+2,start,true);view.setUint16(used+4,x-start,true);
  used+=6;changed+=x-start;
 }
 let bytes='';for(let i=0;i<used;i+=1024)bytes+=String.fromCharCode(...encoded.subarray(i,Math.min(i+1024,used)));
 const data=btoa(bytes);encoded.fill(0);
 requireValue(data.length<=87376&&data.length%8===0,'native text pixel transport bound');
 return {encoding:'row-runs-u16le-base64',data,runs:used/6,changed_pixels:changed};
}
function beginTextPixelFrame(reg:ReturnType<typeof registration>,context:CanvasRenderingContext2D):TextPixelFrame|undefined{
 const config=globalThis.__SEMWRIGHT_NATIVE_CONFIG__,samples=config?.text_pixel_observation_frames;
 if(!samples?.length)return undefined;
 const playback=usePlayback();
 const width=context.canvas.width,height=context.canvas.height;
 requireValue(Array.isArray(samples)&&samples.length<=7&&samples.every((v,i)=>Number.isSafeInteger(v)&&v>=0&&(i===0||samples[i-1]<v)),'native text pixel sample roster');
 requireValue(Number.isSafeInteger(playback.frame)&&playback.frame>=0&&Number.isSafeInteger(config.fps_num)&&config.fps_num>0&&Number.isSafeInteger(config.fps_den)&&config.fps_den>0&&playback.fps===config.fps_num/config.fps_den,'native text pixel playback binding');
 if(!samples.includes(playback.frame))return undefined;
 requireValue(width>0&&height>0&&width<=4096&&height<=4096&&width*height<=TEXT_PIXEL_AREA,'native text pixel canvas budget');
 requireValue(config.text_pixel_background===null||textPixelRgb(config.text_pixel_background)!==null,'native text pixel background observation');
 const subjects=new Map<string,TextPixelSubject>();
 for(const shot of reg.data.shots)for(const subject of shot.subjects)if(subject.content.kind==='text'){
  const node=reg.nodes.get(subject.id);
  requireValue(node instanceof Txt&&!subjects.has(subject.id),'native text pixel subject identity');
  requireValue(subjects.size<2,'native text pixel subject budget');
  subjects.set(subject.id,{node,visits:0,completed:false,calls:[],bits:new Uint8Array(Math.ceil(width*height/8))});
 }
 requireValue(/^[0-9a-f]{64}$/.test(config.render_input_digest),'native text pixel render binding');
 return {frame:playback.frame,fps:playback.fps,fps_num:config.fps_num,fps_den:config.fps_den,
  samples:[...samples],digest:config.render_input_digest,background:config.text_pixel_background,
  canvas:context.canvas,context,width,height,subjects,union:new Uint8Array(Math.ceil(width*height/8)),
  operations:0,callBytes:0,completed:false,restored:false,failure:null};
}
function paintTextPixels(frame:TextPixelFrame,reg:ReturnType<typeof registration>,method:'fillText'|'strokeText',
 original:Function,receiver:unknown,args:unknown[]){
 const forward=()=>Reflect.apply(original,receiver,args);
 if(frame.failure)return forward();
 const native=reg.renderStack[reg.renderStack.length-1],id=native?reg.semanticIds.get(native):undefined;
 const subject=id?frame.subjects.get(id):undefined;
 let call:PendingTextCall|undefined,before:ImageData|undefined;
 try{
  requireValue(receiver===frame.context&&subject&&native===subject.node&&frame.canvas===reg.canvas,'native glyph paint owner or canvas unavailable');
  requireValue(++frame.operations<=TEXT_PIXEL_MAX_CALLS,'native glyph paint operation budget');
  const context=frame.context;
  requireValue(args.length>=3&&args.length<=4&&typeof args[0]==='string'&&new TextEncoder().encode(args[0]).length<=8192,
   'native glyph paint arguments');
  requireValue(args.slice(1).every(v=>typeof v==='number'&&Number.isFinite(v)&&Math.abs(v)<=2e6)
   &&(args.length===3||(args[3] as number)>0),'native glyph paint coordinates');
  requireValue(context.globalAlpha===1&&context.globalCompositeOperation==='source-over'&&context.filter==='none'
   &&context.shadowBlur===0&&context.shadowOffsetX===0&&context.shadowOffsetY===0
   &&context.shadowColor==='rgba(0, 0, 0, 0)','native glyph paint context unsupported');
  const color=textPixelRgb(method==='fillText'?context.fillStyle:context.strokeStyle);
  requireValue(color!==null,'native glyph paint opaque solid style unavailable');
  const authored=reg.data.shots.flatMap(s=>s.subjects).find(s=>s.id===id)!;
  requireValue(authored.content.kind==='text','native glyph source subject type');
  const fallback=reg.data.editorial.colors.text??Object.values(reg.data.editorial.colors)[0]??'#202020';
  const allowed=authored.content.runs.map(r=>textPixelRgb(r.color??fallback));
  requireValue(allowed.every(v=>v!==null)&&allowed.some(v=>v!.every((c,i)=>c===color[i])),'native glyph style differs from observed source runs');
  const background=frame.background===null?null:textPixelRgb(frame.background);
  requireValue(background===null||background.some((c,i)=>c!==color[i]),'native glyph and output background have identical color');
  requireValue(typeof context.font==='string'&&context.font.length>0&&new TextEncoder().encode(context.font).length<=512,'native glyph font style unavailable');
  const attributes=context.getContextAttributes();
  requireValue(attributes.colorSpace==='srgb','native glyph readback color space unsupported');
  call={serial:frame.operations-1,operation:method==='fillText'?'fill_text':'stroke_text',matrix:nativeMatrix(context.getTransform()),
   text:args[0],font:context.font,color,x:args[1] as number,y:args[2] as number,max_width:args.length===4?args[3] as number:null,
   changed_pixels:0,global_alpha:1,composite:'source-over',filter:'none'};
  before=context.getImageData(0,0,frame.width,frame.height);
  requireValue(before.width===frame.width&&before.height===frame.height&&before.colorSpace==='srgb'
   &&before.data instanceof Uint8ClampedArray&&before.data.length===frame.width*frame.height*4,'native glyph before readback incomplete');
  // Equal opaque destination color could hide glyph coverage. Existing observed
  // text is already in the frame union; other equal-color pixels deny this proof.
  for(let p=0;p<frame.width*frame.height;p++){
   const at=p*4;
   requireValue(before.data[at+3]===0||textPixelBit(frame.union,p)
    ||color.some((c,i)=>before!.data[at+i]!==c),'native glyph contrast is not independently decidable');
  }
 }catch{
  frame.failure='native glyph operation, style, contrast or readback unsupported';
  before=undefined;
 }
 // The original operation executes exactly once even if observation is denied.
 let result:unknown;
 try{result=forward();}catch(error){frame.failure='original native glyph operation did not complete';throw error;}
 if(!frame.failure&&before&&call&&subject){
  try{
   const after=frame.context.getImageData(0,0,frame.width,frame.height);
   requireValue(after.width===frame.width&&after.height===frame.height&&after.colorSpace==='srgb'
    &&after.data instanceof Uint8ClampedArray&&after.data.length===before.data.length,'native glyph after readback incomplete');
   for(let p=0;p<frame.width*frame.height;p++){
    const at=p*4;
    if(before.data[at]!==after.data[at]||before.data[at+1]!==after.data[at+1]
      ||before.data[at+2]!==after.data[at+2]||before.data[at+3]!==after.data[at+3]){
     textPixelSet(subject.bits,p);textPixelSet(frame.union,p);call.changed_pixels++;
    }
   }
   // This bound is inside the existing 512 KiB observation-line and 8 MiB
   // FixedTxt stream budgets. Overflow denies observation, never truncates it.
   frame.callBytes+=new TextEncoder().encode(JSON.stringify(call)).length+82;
   requireValue(frame.callBytes<=65536,'native glyph call metadata budget');
   subject.calls.push(call);
  }catch{frame.failure='native glyph after readback unavailable';}
 }
 before=undefined;
 return result;
}
function withTextPixelFrame<T>(reg:ReturnType<typeof registration>,context:CanvasRenderingContext2D,run:()=>T):T{
 if(reg.textPixels){
  reg.textPixels.union.fill(0);
  for(const subject of reg.textPixels.subjects.values()){subject.bits.fill(0);subject.calls.length=0;}
 }
 reg.textPixels=undefined;
 let frame:TextPixelFrame|undefined;
 try{frame=beginTextPixelFrame(reg,context);reg.textPixels=frame;}catch{/* requested observation remains UNKNOWN */}
 if(!frame)return run();
 frame.restored=true; // No method has been replaced yet.
 const installed:{name:'fillText'|'strokeText';descriptor:PropertyDescriptor|undefined;original:Function}[]=[];
 function restore(){
  if(installed.length===0)return;
  let okay=true;
  for(const row of installed.reverse())try{
   if(row.descriptor)Object.defineProperty(context,row.name,row.descriptor);else delete (context as unknown as Record<string,unknown>)[row.name];
   if(context[row.name]!==row.original)okay=false;
  }catch{okay=false;}
  installed.length=0;frame!.restored=okay;
  if(!okay)frame!.failure='native glyph method restoration failed';
 }
 try{
  for(const name of ['fillText','strokeText'] as const){
   const descriptor=Object.getOwnPropertyDescriptor(context,name),original=context[name];
   requireValue(typeof original==='function'&&(!descriptor||descriptor.configurable),'native glyph method cannot be safely observed');
   Object.defineProperty(context,name,{configurable:true,writable:true,enumerable:descriptor?.enumerable??false,value:function(this:unknown,...args:unknown[]){
    return paintTextPixels(frame!,reg,name,original,this,args);
   }});
   installed.push({name,descriptor,original});
   frame.restored=false;
  }
 }catch{frame.failure='native glyph observation wrapper unavailable';restore();}
 try{const result=run();frame.completed=true;return result;}finally{restore();}
}
async function nativeTextPixelObservation(reg:ReturnType<typeof registration>,id:string,canvas:HTMLCanvasElement,frame:number,binding:FrameBinding){
 if(!binding.text_pixel_observation_frames?.length)return {};
 let observed:Known<NativeTextPixels>=unknown('native glyph contribution not selected or not observed');
 try{
  const capture=reg.textPixels,subject=capture?.subjects.get(id),node=reg.nodes.get(id);
  requireValue(capture&&subject&&node instanceof Txt&&subject.node===node&&!capture.failure
   &&capture.completed&&capture.restored,'native glyph capture incomplete or unsupported');
  const invisible=node.absoluteOpacity()===0;
  requireValue((subject.visits===1&&subject.completed)||(subject.visits===0&&invisible&&subject.calls.length===0),
   'native glyph subject draw incomplete or repeated');
  requireValue(capture.frame===frame&&capture.samples.length===binding.text_pixel_observation_frames.length
   &&capture.samples.every((v,i)=>v===binding.text_pixel_observation_frames![i])&&capture.digest===binding.render_input_digest
   &&capture.background===binding.text_pixel_background,'native glyph frame or renderer binding changed');
  const exported=globalThis.__SEMWRIGHT_NATIVE_FRAME_CLOCK__;
  requireValue(exported&&exported.frame===capture.frame&&exported.fps_num===capture.fps_num
   &&exported.fps_den===capture.fps_den,'native glyph playback/exporter clock differs');
  requireValue(capture.canvas===reg.canvas&&capture.width===canvas.width&&capture.height===canvas.height,
   'native glyph canvas reference or exported dimensions changed');
  requireValue(subject.calls.length>0||invisible||node.text()==='',
   'native glyph root-canvas calls absent, possibly cached or offscreen');
  const calls:TextPixelCall[]=[];
  for(const call of subject.calls)calls.push({...call,text_digest:await textHash(call.text)});
  observed=known({native_key:nativeKey(node),frame,width:capture.width,height:capture.height,
   playback_fps:capture.fps,fps_num:capture.fps_num,fps_den:capture.fps_den,selected_frames:capture.samples,
   render_input_digest:capture.digest,output_background:capture.background,calls,
   mask:textPixelRle(subject.bits,capture.width,capture.height)});
 }catch{/* No cropped mask, guessed ink, repeated paint or threshold fallback. */}
 return {native_text_pixels:observed};
}

function nativeKey(n:Node):string{
 const key=n.key;requireValue(typeof key==='string'&&key.length>0&&key.length<=512&&/^[\x20-\x7e]+$/.test(key),'native node key unavailable');return key;
}
function nativeMatrix(m:DOMMatrix):Matrix6{
 const a:Matrix6=[m.a,m.b,m.c,m.d,m.e,m.f];
 requireValue(m.is2D&&a.every(v=>Number.isFinite(v)&&Math.abs(v)<=1e6),'native matrix unavailable');return a;
}
function nativeQuad(box:LayoutDraw['box'],m:DOMMatrix):ScreenQuad{
 // Preserve ordered native corners before any axis-aligned reduction.
 const points=[[box.x,box.y],[box.x+box.width,box.y],[box.x+box.width,box.y+box.height],[box.x,box.y+box.height]];
 const result=points.map(([x,y])=>{const p=m.transformPoint({x,y});return[p.x,p.y];});
 requireValue(result.every(p=>p.every(v=>Number.isFinite(v)&&Math.abs(v)<=2e6)),'native polygon unavailable');
 return result as ScreenQuad;
}
function withNativeMethods<T>(target:object,names:string[],wrap:(name:string,original:NativeMethod)=>NativeMethod,run:()=>T):T{
 const saved:{name:string;descriptor:PropertyDescriptor|undefined}[]=[];
 const restore=()=>{for(const {name,descriptor}of saved.reverse()){
  if(descriptor)Object.defineProperty(target,name,descriptor);
  else requireValue(Reflect.deleteProperty(target,name),'native observer method restoration failed');
 }};
 try{
  for(const name of names){
   const original=Reflect.get(target,name);requireValue(typeof original==='function','native observation method unavailable');
   const descriptor=Object.getOwnPropertyDescriptor(target,name);
   Object.defineProperty(target,name,{configurable:true,writable:true,enumerable:descriptor?.enumerable??false,value:wrap(name,original as NativeMethod)});
   saved.push({name,descriptor});
  }
 }catch{restore();return run();}
 try{return run();}finally{restore();}
}
function observeRectClip<T>(n:Layout,context:CanvasRenderingContext2D,reg:ReturnType<typeof registration>,run:()=>T):T{
 const record:NativeClipRecord={canvas:context.canvas,value:unknown('native rectangular clip call unavailable')};reg.nativeClips.set(n,record);
 let open=false,closed=false,invalid=false,done=false;
 let rectangle:{box:LayoutDraw['box'];matrix:Matrix6}|null=null;
 return withNativeMethods(context,['beginPath','rect','closePath','clip'],(method,original)=>function(...args){
  const own=this===context&&reg.renderStack[reg.renderStack.length-1]===n&&!done;
  let matrix:Matrix6|undefined;
  if(own&&(method==='rect'||method==='clip')){try{matrix=nativeMatrix(context.getTransform());}catch{invalid=true;}}
  const value=original.apply(this,args); // exactly the real Canvas call
  if(own){try{
   if(method==='beginPath'){open=true;closed=false;invalid=false;rectangle=null;}
   else if(method==='rect'){
    if(!open||rectangle!==null||args.length!==4||!args.every(v=>typeof v==='number'&&Number.isFinite(v)&&Math.abs(v)<=1e6)||!matrix){invalid=true;}
    else{
     const [x,y,width,height]=args as number[];
     if(width<=0||height<=0||Math.abs(x)>32768||Math.abs(y)>32768)invalid=true;
     rectangle={box:{x,y,width,height},matrix};
    }
   }else if(method==='closePath'){closed=true;}
   else if(method==='clip'){
    done=true;
    if(!invalid&&open&&closed&&rectangle&&matrix&&args.length===0&&matrix.every((v,i)=>v===rectangle!.matrix[i])){
     record.value=known({kind:'rect',clip:{native_key:nativeKey(n),local_box:rectangle.box,matrix,
      screen_polygon:nativeQuad(rectangle.box,new DOMMatrix(matrix))}});
    }
   }
  }catch{invalid=true;record.value=unknown('native rectangular clip observation unavailable');}}
  return value;
 },run);
}
function observeCameraDraw<T>(camera:Camera,reg:ReturnType<typeof registration>,run:()=>T):T{
 return withNativeMethods(camera,['drawChildren'],(_method,originalCamera)=>function(...args){
  let scene:Node;
  try{scene=camera.scene();requireValue(scene instanceof Node,'native camera scene unavailable');}
  catch{return originalCamera.apply(this,args);}
  return withNativeMethods(scene,['drawChildren'],(_name,originalScene)=>function(...sceneArgs){
   const ctx=sceneArgs[0] as CanvasRenderingContext2D;
   const own=this===scene&&reg.renderStack[reg.renderStack.length-1]===camera;
   let draw:NativeSceneDraw|undefined;
   if(own){try{draw={scene,canvas:ctx.canvas,matrix:nativeMatrix(ctx.getTransform()),completed:false};}catch{/* unavailable stays UNKNOWN */}}
   if(own){const prior=reg.cameraDraws.get(camera);reg.cameraDraws.set(camera,{count:(prior?.count??0)+1,draw:prior?.draw??draw});}
   const value=originalScene.apply(this,sceneArgs);
   if(draw)draw.completed=true;
   return value;
  },()=>originalCamera.apply(this,args));
 },run);
}
function observeNativeRender<T>(n:Node,context:CanvasRenderingContext2D,reg:ReturnType<typeof registration>,run:()=>T):T{
 reg.renderStack.push(n);
 try{
  if(n instanceof Camera)return observeCameraDraw(n,reg,run);
  // Only the exact 3.17.2 Layout path has this rectangular clipping contract.
  const draw=Reflect.get(n,'draw');
  if(Object.getPrototypeOf(n)===Layout.prototype&&draw===Reflect.get(Layout.prototype,'draw')){
   const layout=n as Layout;
   if(layout.clip())return observeRectClip(layout,context,reg,run);
   const value=run();
   if(!layout.clip())reg.nativeClips.set(n,{canvas:context.canvas,value:known({kind:'none'})});
   return value;
  }
  return run();
 }finally{reg.renderStack.pop();}
}
function nativeLineage(n:Node,reg:ReturnType<typeof registration>):Known<NativeLineageNode[]>{
 try{
  const result:NativeLineageNode[]=[],seen=new Set<Node>();let current:Node|null=n;
  while(current){
   requireValue(result.length<64&&!seen.has(current),'native lineage bound or cycle');seen.add(current);
   const parent=current.parent(),opacity=current.opacity();
   requireValue((parent===null||parent instanceof Node)&&Number.isFinite(opacity)&&opacity>=0&&opacity<=1,'native lineage values');
   result.push({native_key:nativeKey(current),subject_id:reg.semanticIds.get(current)??null,native_type:nativeNodeType(current),
    parent_key:parent?nativeKey(parent):null,local_to_parent:nativeMatrix(current.localToParent()),opacity,
    layout_enabled:current instanceof Layout?known(current.layoutEnabled()):unknown('node is not a native Layout'),
    clip:current instanceof Layout?known(current.clip()):unknown('node is not a native Layout')});
   current=parent;
  }
  return known(result);
 }catch{return unknown('native parent lineage unavailable or ambiguous');}
}
function nativeCameraInventory(reg:ReturnType<typeof registration>):Known<NativeCameraIdentity[]>{
 try{
  requireValue(reg.nativeView instanceof Node,'native view unavailable');
  const seen=new Set<Node>(),active=new Set<Node>(),keys=new Set<string>();
  const stack:{node:Node;exit:boolean}[]=[{node:reg.nativeView,exit:false}],result:NativeCameraIdentity[]=[];
  while(stack.length){
   const {node,exit}=stack.pop()!;
   if(exit){active.delete(node);continue;}
   requireValue(!active.has(node),'native scene graph cycle');if(seen.has(node))continue;
   requireValue(seen.size<4096,'native scene graph bound');seen.add(node);active.add(node);
   const key=nativeKey(node);requireValue(!keys.has(key),'native node keys ambiguous');keys.add(key);
   stack.push({node,exit:true});const children=node.peekChildren();
   requireValue(Array.isArray(children)&&children.length<=1024,'native child roster bound');
   for(const child of children){requireValue(child instanceof Node,'native child type');stack.push({node:child,exit:false});}
   if(node instanceof Camera){
    requireValue(result.length<64,'native camera roster bound');result.push({native_key:key,subject_id:reg.semanticIds.get(node)??null});
    const scene=node.scene();requireValue(scene instanceof Node,'native camera scene type');stack.push({node:scene,exit:false});
   }
  }
  return known(result.sort((a,b)=>a.native_key<b.native_key?-1:a.native_key>b.native_key?1:0));
 }catch{return unknown('native camera graph inventory unavailable or ambiguous');}
}
function nativeGeometryObservation(n:Node,draw:DrawRecord|undefined,coordinateKnown:boolean,layoutBoxKnown:boolean,reg:ReturnType<typeof registration>):Known<NativeGeometry>{
 try{
  let polygon:Known<ScreenQuad>=unknown('native layout polygon or output transform unavailable');
  if(coordinateKnown&&layoutBoxKnown&&draw?.layout){try{polygon=known(nativeQuad(draw.layout.box,draw.matrix));}catch{/* keep UNKNOWN */}}
  const clip=reg.nativeClips.get(n);
  return known({native_key:nativeKey(n),screen_layout_polygon:polygon,
   lineage:draw?nativeLineage(n,reg):unknown('native node was not observed in a completed draw'),
   own_clip:coordinateKnown&&clip&&clip.canvas===draw?.canvas?clip.value:unknown('native own clip path or output transform unavailable')});
 }catch{return unknown('native geometry identity unavailable');}
}
function nativeCameraObservation(n:Node,reg:ReturnType<typeof registration>,canvas:HTMLCanvasElement,inventory:Known<NativeCameraIdentity[]>):Known<NativeCameraScene>{
 if(!(n instanceof Camera))return unknown('node is not a native Camera');
 try{
  const original=reg.cameraOrigins.get(n),current=n.scene(),record=reg.cameraDraws.get(n);
  requireValue(original instanceof Node&&current instanceof Node&&record?.count===1&&record.draw?.completed,'native original camera scene draw unavailable');
  const draw=record.draw,position=n.position(),rotation=n.rotation(),zoom=n.zoom();
  requireValue([position.x,position.y,rotation,zoom].every(v=>Number.isFinite(v)&&Math.abs(v)<=1e6)&&zoom>0,'native camera signals unavailable');
  const mapped=draw.canvas===reg.canvas&&draw.canvas.width===canvas.width&&draw.canvas.height===canvas.height;
  return known({native_key:nativeKey(n),original_scene_key:nativeKey(original),current_scene_key:nativeKey(current),
   current_scene_is_original:current===original,draw_scene_key:nativeKey(draw.scene),draw_uses_original_scene:draw.scene===original,
   scene_transform:mapped?known(draw.matrix):unknown('native camera scene draw has an unmodeled output transform'),
   position:[position.x,position.y],rotation,zoom,cameras:inventory});
 }catch{return unknown('native original Camera.scene draw or identity unavailable');}
}

function nativeLayoutObservation(n:Node,draw:DrawRecord|undefined,coordinateKnown:boolean){
 const native_type=known(nativeNodeType(n));
 const layout_root:Known<boolean>=draw?.layout?known(draw.layout.root):unknown('native layout root unavailable after completed draw');
 let screen_layout_box:Known<LayoutDraw['box']>=unknown('native layout box or output transform unavailable');
 if(coordinateKnown&&draw?.layout){
  try{
   // The captured parent context includes the actual camera/ancestor transforms.
   // Stage 3.17.2 copies the admitted scene buffer at (0,0), without rescaling.
   const box=transformed(draw.layout.box,draw.matrix);
   requireValue(Object.values(box).every(Number.isFinite)&&Math.abs(box.x)<=32768&&Math.abs(box.y)<=32768&&box.width>=0&&box.height>=0&&box.width<=1e6&&box.height<=1e6,'observed screen layout bounds');
   screen_layout_box=known(box);
  }catch{/* unavailable native geometry remains UNKNOWN */}
 }
 return {native_type,layout_root,screen_layout_box};
}
function clipAncestor(n:Node){for(let a:Node|null=n;a;a=a.parent()){if(a instanceof Layout&&a.clip())return true;}return false;}
function instrument(n:Node,id:string,reg:ReturnType<typeof registration>){
 reg.semanticIds.set(n,id);
 if(n instanceof Camera){try{const original=n.scene();if(original instanceof Node)reg.cameraOrigins.set(n,original);}catch{/* unavailable scene identity stays UNKNOWN */}}
 const render=n.render.bind(n);
 n.render=(context:CanvasRenderingContext2D)=>{
  const pixelSubject=reg.textPixels?.subjects.get(id);
  if(pixelSubject)pixelSubject.visits++;
  const result=observeNativeRender(n,context,reg,()=>render(context));
  if(pixelSubject)pixelSubject.completed=true;
  if(n.absoluteOpacity()>0){
   reg.rendered.add(id);
   // Layout/Txt transforms and cache bounds can require the native DOM layout
   // pass. Observation must happen only after Motion Canvas completes its draw,
   // and any unavailable geometry remains UNKNOWN rather than failing rendering.
   try{
    const matrix=context.getTransform().multiply(n.localToParent());
    const box=n.cacheBBox();
    const draw:DrawRecord={canvas:context.canvas,matrix,bounds:{x:box.x,y:box.y,width:box.width,height:box.height},opacity:n.absoluteOpacity(),serial:reg.drawSerial++,clip:clipAncestor(n)};
    reg.draws.set(id,draw);
    try{const layout=readNativeLayout(n);if(layout)draw.layout=layout;}catch{/* keep the independent drawable bounds; layout remains UNKNOWN */}
   }catch{/* geometry remains UNKNOWN */}
  }
  return result;
 };
}
function registration(data:NativeSceneData,nodes:Map<string,Node>){return {data,nodes,draws:new Map<string,DrawRecord>(),rendered:new Set<string>(),canvas:null as HTMLCanvasElement|null,drawSerial:0,finished:new Set<string>(),captionActive:new Map<string,boolean>(),
 nativeView:null as Node|null,semanticIds:new WeakMap<Node,string>(),renderStack:[] as Node[],nativeClips:new Map<Node,NativeClipRecord>(),
 textPixels:undefined as TextPixelFrame|undefined,cameraOrigins:new WeakMap<Camera,Node>(),cameraDraws:new Map<Camera,{count:number;draw?:NativeSceneDraw}>()};}
function transformed(box:DrawRecord['bounds'],m:DOMMatrix){const p=[[box.x,box.y],[box.x+box.width,box.y],[box.x,box.y+box.height],[box.x+box.width,box.y+box.height]].map(([x,y])=>m.transformPoint({x,y}));const x=Math.min(...p.map(x=>x.x)),y=Math.min(...p.map(x=>x.y));return{x,y,width:Math.max(...p.map(x=>x.x))-x,height:Math.max(...p.map(x=>x.y))-y};}
async function textHash(text:string){const digest=globalThis.__SEMWRIGHT_TEXT_DIGEST__;requireValue(typeof digest==='function','native text digest binding unavailable');const hash=await digest(text);requireValue(/^[0-9a-f]{64}$/.test(hash),'native text digest binding returned invalid hash');return hash;}
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
 const output:unknown[]=[];const cameras=nativeCameraInventory(reg);
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
    const computed=getComputedStyle(el);const range=document.createRange();range.selectNodeContents(el);const rects=Array.from(range.getClientRects()).filter(r=>r.width>0&&r.height>0);
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
  const observedLayout=nativeLayoutObservation(n,draw,coordinateKnown);
  const observedPixels=c.kind==='text'?await nativeTextPixelObservation(reg,subject.id,canvas,frame,binding):{};
  output.push({id:subject.id,...observedLayout,...observedPixels,native_geometry:nativeGeometryObservation(n,draw,coordinateKnown,observedLayout.screen_layout_box.status==='known',reg),camera_scene:nativeCameraObservation(n,reg,canvas,cameras),local_size:draw?known({width:draw.bounds.width,height:draw.bounds.height}):unknown('native layout size unavailable'),bounds:coordinateKnown?known(transformed(draw!.bounds,draw!.matrix)):unknown('not drawn in the native scene buffer or unmodeled cache transform'),transform:coordinateKnown?known([draw!.matrix.a,draw!.matrix.b,draw!.matrix.c,draw!.matrix.d,draw!.matrix.e,draw!.matrix.f]):unknown('output transform unavailable'),opacity:known(n.absoluteOpacity()),drawn:rendered,z_order:draw?known(draw.serial):unknown('no native draw order'),clip_active:known(clipAncestor(n)),pixel_visibility:unknown('draw and alpha do not prove pixel contribution after overlap'),text,asset_ready});
 }
 const transitions=[...new Set(reg.data.instructions.map(i=>i.invocation))].map(id=>({invocation_id:id,finished:known(reg.finished.has(id))}));
 const captions=reg.data.shots.flatMap(s=>s.captions.map(c=>{const cue=reg.data.cues[c.cue_id];return{id:c.id,active:known((reg.captionActive.get(c.id)??false)&&reg.draws.has(c.subject)&&reg.nodes.get(c.subject)!.absoluteOpacity()>0&&(reg.nodes.get(c.subject) as Txt).text()===c.text),cue_start:cue?.status==='resolved'?known(cue.start):unknown('unresolved cue'),cue_end:cue?.status==='resolved'?known(cue.end):unknown('unresolved cue')};}));
 return {frame,subjects:output,transitions,captions,unresolved_cues:Object.entries(reg.data.cues).filter(([,v])=>v.status!=='resolved').map(([k])=>k)};
}
/**
 * Motion Canvas 3.17.2 waitFor intentionally resumes generators as much as
 * one frame ahead of the fixed animation clock. Authored subtitle opacity,
 * unlike tween precomputation, MUST NOT change before the true cue boundary.
 * Use the engine's own fixed frame time to gate the actual paint mutation.
 * There is no fabricated observation or parallel timeline.
 */
function* captionAtNativeFrame(absoluteTime:number):ThreadGenerator{
 const thread=useThread();const at=finite(absoluteTime,'native caption cue frame');
 if(at>thread.time())yield* waitFor(at-thread.time());
 // Sub-nanosecond tolerance only absorbs IEEE-754 frame accumulation error.
 // The authoritative native frame is still required to reach the cue time.
 while(thread.fixed+1e-9<at)yield;
}
export function createAuthoringScene(data:NativeSceneData,urls:AssetUrls){
 requireValue(data.version===1&&data.shots.length<=128&&data.instructions.length<=8192,'native scene bounds');
 return makeScene2D(function*(view){
  const nodes=new Map<string,Node>();const reg=registration(data,nodes);sceneRegistrations.set(data.id,reg);
  // Capture the stage canvas before any authored subtree is attached, then
  // instrument each subject inside createShot before parent.add/view.add can
  // prime Motion Canvas caches.
  reg.nativeView=view;const originalRender=view.render.bind(view);
  view.render=(ctx:CanvasRenderingContext2D)=>{reg.canvas=ctx.canvas;reg.draws.clear();reg.rendered.clear();reg.drawSerial=0;reg.nativeClips.clear();reg.cameraDraws.clear();reg.renderStack.length=0;return withTextPixelFrame(reg,ctx,()=>originalRender(ctx));};
  for(const shot of data.shots)view.add(createShot(shot,data,urls,nodes,reg));
  // Native dependency resolution participates in renderer startup; fonts and
  // media are awaited by native promises, never set ready from the desired model.
  yield view.toPromise();yield document.fonts.ready;
  const start=sec(data.start),end=sec(data.end);const sceneThreadBase=useTime();const tasks:ThreadGenerator[]=[];
  for(const shot of data.shots){
   const root=nodes.get(`sw-shot-${shot.id}`)!;
   tasks.push(delay(sec(shot.start)-start,(function*(){root.opacity(1);for(const s of shot.subjects){const n=nodes.get(s.id);if(n instanceof Video)n.play();else n?.findAll(x=>x instanceof Video).forEach(x=>(x as Video).play());}yield* waitFor(sec(shot.end)-sec(shot.start));if(Math.abs(sec(shot.end)-end)>1e-9)root.opacity(0);for(const s of shot.subjects){const n=nodes.get(s.id);if(n instanceof Video)n.pause();else n?.findAll(x=>x instanceof Video).forEach(x=>(x as Video).pause());}})()));
   for(const caption of shot.captions){const cue=data.cues[caption.cue_id];const node=nodes.get(caption.subject);requireValue(node instanceof Txt,'caption requires native Txt');node.opacity(0);
    if(cue?.status==='resolved')tasks.push((function*(){
     const begin=sceneThreadBase+sec(cue.start)-start;
     const finish=sceneThreadBase+sec(cue.end)-start;
     requireValue(finish>begin,'invalid native caption span');
     yield* captionAtNativeFrame(begin);
     node.text(caption.text);node.opacity(1);reg.captionActive.set(caption.id,true);
     yield* captionAtNativeFrame(finish);
     node.opacity(0);reg.captionActive.set(caption.id,false);
    })());
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
