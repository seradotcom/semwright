/** Actual Motion Canvas 3.17.2/Firefox fixture; no Driver Host or G2 claim.
 * The frozen ec546358 and text-pixel observer modules each draw one unchanged frame per
 * selected case. Exact RGBA equality checks that observation does not edit it.
 */
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import path from 'node:path';
import http from 'node:http';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';

const sha = value => createHash('sha256').update(value).digest('hex');
const runtime = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const sdk = path.resolve(runtime, '../../..');
const args = new Map();
for (let i = 2; i < process.argv.length; i += 2) {
  assert(i + 1 < process.argv.length && !args.has(process.argv[i]), 'closed arguments');
  args.set(process.argv[i], process.argv[i + 1]);
}
assert.deepEqual([...args.keys()].sort(), ['--baseline', '--case', '--output']);
const CASES = ['paint-red', 'paint-green', 'absent-profile', 'unselected-frame', 'cached', 'unsupported-effects', 'same-color', 'operation-overflow', 'inactive-second'];
const selection = args.get('--case');
assert(selection === 'all' || selection === 'other-eight' || CASES.includes(selection), 'closed text pixel focal case');
const selected = selection === 'all' ? CASES : selection === 'other-eight' ? CASES.filter(name=>name!=='paint-red') : [selection];
const HISTORICAL = {
 GREEN: 'afc197bacf9d73c007833a315b253b1b901005a2c9ef7506eec57b2683aaf3c7',
 RED: 'ac4df30d0bdeab0a93924d5841ea18a76f269ce99f2903a2cfebc38e254a3c12',
};
const OBSERVED = {
 GREEN: '0aa5b9bd414f3c7d88e9b871ee96e3523729c02caddd8d420d63d9f14c2c794d',
 RED: '2d8ab6983d0845fe0290d6f25bef277759cb530ed85e6f3bd5e94f263a5f2bb3',
};
const RED = '   if(parent instanceof Camera){let scene=parent.scene();if(!scene){scene=new Node({});parent.scene(scene);}scene.add(n);}else parent.add(n);\n';
const GREEN = `   // Fixed Txt keeps its DOM-enabled inline runs. A non-layout parent makes
   // its own position signal authoritative while preserving the logical Txt.
   // The attachment follows the later layer assignment and any zIndex change.
   const attachment=s.layout.kind==='fixed'&&n instanceof Txt
    ?new Layout({layout:false,position:[0,0],rotation:0,scale:[1,1],
      opacity:1,clip:false,zIndex:()=>n.zIndex(),children:[n]}):n;
   if(parent instanceof Camera){let scene=parent.scene();if(!scene){scene=new Node({});parent.scene(scene);}scene.add(attachment);}else parent.add(attachment);
`;
// Only expose an existing map in the test copies; production bytes stay intact.
const HOOK = '\nexport {sceneRegistrations as __focalRegistrations};\n';
const PACKAGE_PINS = {
  '@motion-canvas/2d': ['3.17.2', 'sha512-Q2DMYSmJMTN8vvLcpofHEMuYEjx60z4hjlwbWJnWx3xnb7DU5suovJ47qM2sk46al/RDXJIz6rfALTr33brKaw=='],
  '@motion-canvas/core': ['3.17.2', 'sha512-9M4NxNVR9LLxZ+ykTznMcbMlKjbe2NkmDIlFDl2uOyVkK5wFiRwF7k13a1I/n3BVxJJoKPt9UOQF/wn3LrBXJA=='],
  playwright: ['1.61.1', 'sha512-DWnY5o3YbLWK4GovuAVwpqL+1VwGNdUGrRr++8j8PtQQzvAVZUIMjKQ90fY689sEJZJBbZVw1rXaOKSTitkzPQ=='],
};
// Exact files already verified inside the original npm SRI-checked tarballs.
const API_PINS = {
  '@motion-canvas/2d/lib/components/TxtLeaf.js': 'aa6d3a7a0895f83349c34fcf87821d3c90dbf34a06e8cae36fd00517b5ec988f',
  '@motion-canvas/2d/lib/scenes/Scene2D.js': '919d95179fdac20110b2b45926e700bc196e8078f42b6e618c044a559e46b854',
  '@motion-canvas/core/lib/scenes/GeneratorScene.js': '881d3c16a61a94b2cbd231f09d1e878f7c9acb963b0c94c8482b6c3396a9eea6',
  '@motion-canvas/core/lib/utils/usePlayback.js': '78edbac1495f4750b06f51baa298b08c30c5473ea2964c5852c08cb304aac2e6',
  '@motion-canvas/core/lib/app/PlaybackStatus.js': '55dff2614c7b8d0a932d202005708983c690d2dc6a20e829d9106de4f143cff9',
  '@motion-canvas/core/lib/app/Renderer.js': '3afde003d9f52b98772978f11b5b22c70d73e5411fe0280508f07934242b9483',
  '@motion-canvas/2d/lib/components/Node.js': '1db56cdcbfb642a4dd304be91bb029c940ee3142c9a35ad5304f9b708f9b759d',
  '@motion-canvas/2d/lib/components/Layout.js': '7094ab99d3daa882efc98017f0f7959a5185d0a16bb943bb453874e25f31ac20',
  '@motion-canvas/2d/lib/components/Txt.js': 'a29ea3fa1bbf0497b3f23727eb2da5b62040fd6ab12c106fdcca9d359b497448',
  '@motion-canvas/2d/lib/components/Camera.js': '044676178bbb8e0114e66a8e2cc3ad7a105f767d4fc4cbc20ce1f5a1e092c1a6',
  '@motion-canvas/2d/lib/components/View2D.js': 'dd62fee25ec7e8cfa6cdc4ee7e47d4cd82235c4258d571b3ca43f789c67c932e',
  '@motion-canvas/core/lib/app/Stage.js': '685ab2aa34c37e05932cbafea9aae6e8e5c5e101f72afc72fb475fdf90dbef23',
  '@motion-canvas/core/lib/types/BBox.js': '7c4f159e3b0e89bf886d773a9d63384bbbb5066d4a4e0521cf483df47f9653a7',
  '@motion-canvas/core/lib/types/Vector.js': '5b45d059aee4c84f03b4bfa2c2b1609c6ed735d20bef9cc96e7bab6b0cc4f291',
  '@motion-canvas/core/lib/types/Matrix2D.js': 'c6c3f2afe7da1e5c6b415e6d03bf8f73f6923d6eb514d93d155f4aceb9f743cd',
};

const BROWSER_SOURCE = String.raw`
import {Scene2D,Node,Layout,Camera} from '@motion-canvas/2d';
import {Logger,PlaybackManager,PlaybackStatus,PlaybackState,SharedWebGLContext,Vector2} from '@motion-canvas/core';
import {ReadOnlyTimeEvents} from '@motion-canvas/core/lib/scenes/timeEvents/ReadOnlyTimeEvents';
import '@fontsource-variable/instrument-sans/index.css';
import * as historicalGreen from './historical-GREEN.ts';
import * as historicalRed from './historical-RED.ts';
import * as observedGreen from './observed-GREEN.ts';
import * as observedRed from './observed-RED.ts';
const modules={historical:{GREEN:historicalGreen,RED:historicalRed},observed:{GREEN:observedGreen,RED:observedRed}};
const W=1920,H=1080;
function need(ok,code){if(!ok)throw new Error(code);}
const digest=async value=>[...new Uint8Array(await crypto.subtle.digest('SHA-256',value))].map(x=>x.toString(16).padStart(2,'0')).join('');
const matrix=m=>[m.a,m.b,m.c,m.d,m.e,m.f];
const zero={top:0,right:0,bottom:0,left:0};
function dataFor(name){
 const fixed=(x,y,w,h)=>({kind:'fixed',position:{x,y},size:{width:w,height:h}});
 const font={family:'Instrument Sans Variable',asset_digest:null,fallback:'deny',permitted_fallbacks:[]};
 const subject=(id,parent,content,layout,layer='main',clip=false)=>({id,role:'primary',parent,layer,content,layout,initially_visible:true,clip_intentional:clip});
 const overflow=name==='operation-overflow',same=name==='same-color';
 const content=runs=>({kind:'text',runs,style:overflow?'small':'body',line_height:1.2,direction:'ltr',language:'en',wrap:true,truncate:false});
 const run=(text,color='#f4f4f4',weight=400)=>({text,weight,color,emphasis:false});
 const subjects=[
  subject('background',null,{kind:'rectangle',fill:same?'#f4f4f4':'#101020',stroke:null,radius:0},fixed(0,0,W,H),'under'),
  ...(name==='cached'?[subject('cacheRoot',null,{kind:'group'},fixed(0,0,W,H))]:[]),
  subject('camera',name==='cached'?'cacheRoot':null,{kind:'camera',zoom:1},fixed(0,0,W,H)),
  subject('clip','camera',{kind:'group'},fixed(-220,0,overflow?900:720,overflow?1000:340),'main',true),
  subject('text','clip',content(overflow?[run(Array(65).fill('X').join('\n'))]:[
   run('Real native '),run('styled\n','#d4a62a',700),run('antialiased glyphs')]),
   fixed(0,0,overflow?880:690,overflow?980:200)),
 ];
 if(!overflow)subjects.push(subject('second','camera',content([run('Independent second subject')]),fixed(400,-280,720,100)));
 return {version:1,id:'text-pixel-focal-'+name,start:{num:'0',den:'1'},end:{num:'6',den:'1'},
  width:W,height:H,aspect:'landscape',safe_area:zero,
  editorial:{font,mono_font:font,type_scale:{body:48,small:10},colors:{text:'#f4f4f4'},spacing:{block:0,base:0},stroke:1,corner_radius:0},
  shots:[{id:'shot',archetype:'statement',start:{num:'0',den:'1'},end:{num:'6',den:'1'},subjects,
   layers:[{id:'under',order:0,intentional_overlay:true},{id:'main',order:1,intentional_overlay:true}],annotations:[],captions:[]}],instructions:[],cues:{}};
}
function signals(reg){return Object.fromEntries([...reg.nodes].map(([id,n])=>[id,{
 position:[n.position().x,n.position().y],scale:[n.scale().x,n.scale().y],rotation:n.rotation(),opacity:n.opacity(),
 size:n instanceof Layout?[n.size().x,n.size().y]:null,layout_root:n instanceof Layout?n.isLayoutRoot():null,
 text:id==='text'||id==='second'?n.text():null,children:n.peekChildren().map(child=>child.key),
 camera_scene:n instanceof Camera?n.scene().key:null,zoom:n instanceof Camera?n.zoom():null}]));}
function linearRuns(pixels){
 const runs=[];
 for(let index=0;index<pixels.length;){
  if(!pixels[index]){index++;continue;}
  const start=index;while(index<pixels.length&&pixels[index])index++;
  need(runs.length<16384,'independent pixel oracle transport bound');runs.push([start,index-start]);
 }
 return runs;
}

function imageDataShape(image){
 const present='colorSpace' in image,value=present?image.colorSpace:undefined;
 return {width:image.width,height:image.height,data_type:image.data.constructor.name,data_bytes:image.data.length,
  image_data_color_space:{attribute_present:present,value:value===undefined?null:value,value_type:typeof value}};
}
window.probeNativeReadbackApi=()=>{
 const canvas=document.createElement('canvas');canvas.width=1;canvas.height=1;
 const context=canvas.getContext('2d',{alpha:true,colorSpace:'srgb'}),attributes=context.getContextAttributes();
 const image=context.getImageData(0,0,1,1),actual=imageDataShape(image),check=observedGreen.__focalReadbackInSrgb;
 const accepted=check(image,1,1,attributes.colorSpace);
 const descriptor=Object.getOwnPropertyDescriptor(image,'colorSpace');
 let explicit_other_rejected=false,explicit_undefined_rejected=false;
 try{
  Object.defineProperty(image,'colorSpace',{configurable:true,value:'display-p3'});
  explicit_other_rejected=check(image,1,1,attributes.colorSpace)===false;
  Object.defineProperty(image,'colorSpace',{configurable:true,value:undefined});
  explicit_undefined_rejected=check(image,1,1,attributes.colorSpace)===false;
 }finally{if(descriptor)Object.defineProperty(image,'colorSpace',descriptor);else delete image.colorSpace;}
 return {context_attributes:attributes,actual_native_image_data:actual,actual_readback_admitted:accepted,
  synthetic_metadata_negative_checks:{explicit_other_rejected,explicit_undefined_rejected,
   unobserved_context_rejected:check(image,1,1,undefined)===false,wrong_dimensions_rejected:check(image,2,1,attributes.colorSpace)===false},
  original_image_data_metadata_restored:JSON.stringify(imageDataShape(image))===JSON.stringify(actual),
  getImageData_calls:1,Txt_draw_calls:0,scene_render_calls:0,canvas_paint_calls:0};
};
window.runFocal=async({name,mode,arm})=>{
 const loaded=await Promise.all([400,700].map(weight=>document.fonts.load(weight+' 48px "Instrument Sans Variable"')));
 await document.fonts.ready;need(loaded.every(x=>x.length>0),'real bundled font not loaded');
 globalThis.__SEMWRIGHT_TEXT_DIGEST__=text=>digest(new TextEncoder().encode(text));
 globalThis.__SEMWRIGHT_NATIVE_PROBE__=undefined;globalThis.__SEMWRIGHT_NATIVE_CONFIG__=undefined;
 const data=dataFor(name),module=modules[mode][arm],logger=new Logger();
 const playback=new PlaybackManager();playback.fps=30;playback.state=PlaybackState.Rendering;
 const webgl=new SharedWebGLContext(logger);
 const scene=new Scene2D({...module.createAuthoringScene(data,{}),name:data.id,logger,playback:new PlaybackStatus(playback),
  size:new Vector2(W,H),resolutionScale:1,timeEventsClass:ReadOnlyTimeEvents,sharedWebGLContext:webgl,experimentalFeatures:false});
 try{
  await scene.reset();for(let count=0;!globalThis.__SEMWRIGHT_NATIVE_PROBE__&&count<8;count++){playback.frame++;await scene.next();}
  need(typeof globalThis.__SEMWRIGHT_NATIVE_PROBE__==='function','actual authoring probe not initialized');
  const reg=module.__focalRegistrations.get(data.id),ids=data.shots[0].subjects.filter(s=>s.content.kind==='text').map(s=>s.id);
  const camera=reg.nodes.get('camera'),originalScene=camera.scene();
  scene.execute(()=>{
   if(name!=='operation-overflow'){reg.nodes.get('clip').rotation(8);reg.nodes.get('clip').scale([1.02,.98]);}
   if(name==='cached')reg.nodes.get('cacheRoot').cache(true);
   if(name==='unsupported-effects'){
    reg.nodes.get('text').filters.blur(2);
    reg.nodes.get('second').compositeOperation('multiply');
    reg.nodes.get('second').shadowColor('#101010');reg.nodes.get('second').shadowBlur(3);
   }
   if(name==='inactive-second')reg.nodes.get('second').opacity(0);
  });
  const admitted=await digest(new TextEncoder().encode(JSON.stringify({data,selected:[playback.frame],name})));
  const binding={render_input_digest:admitted,native_stage_version:'3.17.2',font_evidence:[]};
  if(name!=='absent-profile'){
   binding.text_pixel_observation_frames=[name==='unselected-frame'?playback.frame+1:playback.frame];
   binding.text_pixel_background='#101020';
  }
  globalThis.__SEMWRIGHT_NATIVE_CONFIG__={...binding,fps_num:30,fps_den:1};
  const canvas=document.createElement('canvas');canvas.width=W;canvas.height=H;
  const context=canvas.getContext('2d',{alpha:true,colorSpace:'srgb'});need(context,'real Canvas2D');
  const proto=CanvasRenderingContext2D.prototype;
  const saved=new Map(['fillText','strokeText','getImageData'].map(key=>[key,Object.getOwnPropertyDescriptor(proto,key)]));
  need([...saved.values()].every(d=>d&&typeof d.value==='function'&&d.configurable),'native Canvas method descriptors');
  const nativeRead=saved.get('getImageData').value;
  const oracle=Object.fromEntries(ids.map(id=>[id,{bits:new Uint8Array(W*H),calls:[],offscreen_calls:0}]));
  const owners=[],drawRestore=[],initialOwn=['fillText','strokeText'].map(key=>[key,Object.hasOwn(context,key)]);
  let producerReads=0,drawCount=0,viewDraws=0,firstProducerReadbackShape=null;
  const view=scene.getView(),viewDescriptor=Object.getOwnPropertyDescriptor(view,'render'),viewRender=view.render;
  try{
   Object.defineProperty(view,'render',{configurable:true,writable:true,value:function(ctx){viewDraws++;return viewRender.call(this,ctx);}});
   Object.defineProperty(proto,'getImageData',{...saved.get('getImageData'),value:function(...args){
    producerReads++;const image=nativeRead.apply(this,args);
    if(firstProducerReadbackShape===null)firstProducerReadbackShape=imageDataShape(image);return image;
   }});
   for(const id of ids){
    const node=reg.nodes.get(id),descriptor=Object.getOwnPropertyDescriptor(node,'draw'),original=node.draw;
    Object.defineProperty(node,'draw',{configurable:true,writable:true,value:function(ctx){
     owners.push({id,node,ctx});drawCount++;
     try{return original.call(this,ctx);}finally{owners.pop();}
    }});drawRestore.push([node,descriptor]);
   }
   for(const method of ['fillText','strokeText']){
    const original=saved.get(method).value;
    Object.defineProperty(proto,method,{...saved.get(method),value:function(...args){
     const owner=owners.at(-1);need(owner,'independent semantic Txt.draw owner missing');
     const row=oracle[owner.id],root=this.canvas===canvas;
     const call={method,root_canvas:root,text:args[0],font:this.font,matrix:matrix(this.getTransform()),
      global_alpha:this.globalAlpha,composite:this.globalCompositeOperation,filter:this.filter,
      fill:typeof this.fillStyle==='string'?this.fillStyle:null,changed_pixels:0};
     if(!root)row.offscreen_calls++;
     // Oracle bypasses the producer and its read counter, observing the native
     // operation itself. It never consults producer masks, geometry or stack.
     const before=root?nativeRead.call(this,0,0,W,H).data:null;
     const result=original.apply(this,args);
     if(root){
      const after=nativeRead.call(this,0,0,W,H).data;
      for(let at=0;at<after.length;at+=4){
       if(before[at]!==after[at]||before[at+1]!==after[at+1]||before[at+2]!==after[at+2]||before[at+3]!==after[at+3]){
        row.bits[at/4]=1;call.changed_pixels++;
       }
      }
     }
     row.calls.push(call);return result;
    }});
   }
   const start=performance.now();await scene.render(context);var renderMs=performance.now()-start;
  }finally{
   if(viewDescriptor)Object.defineProperty(view,'render',viewDescriptor);else delete view.render;
   for(const [node,descriptor]of drawRestore){if(descriptor)Object.defineProperty(node,'draw',descriptor);else delete node.draw;}
   for(const [key,descriptor]of saved)Object.defineProperty(proto,key,descriptor);
  }
  const pixels=nativeRead.call(context,0,0,W,H).data,beforeSignals=scene.execute(()=>signals(reg));
  globalThis.__SEMWRIGHT_NATIVE_FRAME_CLOCK__={frame:playback.frame,fps_num:30,fps_den:1};
  const probe=await globalThis.__SEMWRIGHT_NATIVE_PROBE__(canvas,playback.frame,binding);
  const mismatches=[];
  if(name==='paint-green'||name==='paint-red'){
   for(const [label,wrong]of [
    ['digest',{...binding,render_input_digest:'cd'.repeat(32)}],
    ['roster',{...binding,text_pixel_observation_frames:[playback.frame+1]}],
   ]){
    const rejected=await globalThis.__SEMWRIGHT_NATIVE_PROBE__(canvas,playback.frame,wrong);
    mismatches.push({label,observations:rejected.subjects.filter(row=>ids.includes(row.id)).map(row=>({id:row.id,pixels:row.native_text_pixels}))});
   }
   globalThis.__SEMWRIGHT_NATIVE_FRAME_CLOCK__={frame:playback.frame,fps_num:60,fps_den:1};
   const rejected=await globalThis.__SEMWRIGHT_NATIVE_PROBE__(canvas,playback.frame,binding);
   mismatches.push({label:'export-rate',observations:rejected.subjects.filter(row=>ids.includes(row.id)).map(row=>({id:row.id,pixels:row.native_text_pixels}))});
   globalThis.__SEMWRIGHT_NATIVE_FRAME_CLOCK__={frame:playback.frame,fps_num:30,fps_den:1};
  }
  const afterSignals=scene.execute(()=>signals(reg));need(JSON.stringify(beforeSignals)===JSON.stringify(afterSignals),'probe changed native signals');
  const independent={};
  for(const [id,row]of Object.entries(oracle)){
   independent[id]={calls:row.calls,offscreen_calls:row.offscreen_calls,linear_runs:linearRuns(row.bits),
    mask_sha256:await digest(row.bits),changed_pixels:row.bits.reduce((n,v)=>n+v,0)};
   row.bits.fill(0);
  }
  const finalPixels=nativeRead.call(context,0,0,W,H).data;
  const capture=reg.textPixels;const producerCapture={present:!!capture,failure:capture?.failure??null,
   completed:capture?.completed??null,restored:capture?.restored??null,operations:capture?.operations??null,
   calls_metadata_bytes:capture?.callBytes??null,subjects:capture?[...capture.subjects].map(([id,s])=>({id,visits:s.visits,completed:s.completed,calls:s.calls.length})):[]};
  const methods=['fillText','strokeText'].map(key=>[key,Object.hasOwn(context,key)]);
  return {name,mode,arm,frame:playback.frame,width:W,height:H,input_binding:binding,
   pixel_sha256:await digest(pixels),post_probe_pixel_sha256:await digest(finalPixels),signals:afterSignals,probe,
   native_camera_scene_unchanged:camera.scene()===originalScene,independent_glyph_operations:independent,
   producer_getImageData_calls:producerReads,first_producer_readback_shape:firstProducerReadbackShape,producer_capture:producerCapture,semantic_draw_calls:drawCount,view_render_calls:viewDraws,scene_render_calls:1,render_elapsed_ms:renderMs,
   context_own_methods_before:initialOwn,context_own_methods_after:methods,
   prototypes_restored:[...saved].every(([key,descriptor])=>Object.getOwnPropertyDescriptor(proto,key).value===descriptor.value),
   mismatched_export_bindings:mismatches};
 }finally{scene.getView().dispose();webgl.dispose();}
};
`;

function decodeObservedMask(mask,width,height){
  assert.equal(mask.encoding,'row-runs-u16le-base64');
  assert.equal(typeof mask.data,'string');assert(mask.data.length<=87376&&mask.data.length%8===0);
  const raw=Buffer.from(mask.data,'base64');assert.equal(raw.toString('base64'),mask.data);
  assert.equal(raw.length,mask.runs*6);assert(width*height<=1920*1080);
  const bitmap=new Uint8Array(width*height);let count=0,oldY=-1,oldEnd=-1;
  for(let at=0;at<raw.length;at+=6){
    const y=raw.readUInt16LE(at),x=raw.readUInt16LE(at+2),length=raw.readUInt16LE(at+4);
    assert(y<height&&length>0&&x+length<=width&&(y>oldY||(y===oldY&&x>oldEnd)));
    bitmap.fill(1,y*width+x,y*width+x+length);count+=length;oldY=y;oldEnd=x+length;
  }
  assert.equal(count,mask.changed_pixels);return bitmap;
}

let browser, server, output, watchdog, cgroup, phase = 'arguments';
const result = {schema:'semwright.motion-native-text-pixels-dom-focal/1', selected_cases:selected,
  production_Native_calls:0, Driver_Host_calls:0, production_frames_rendered:0,
  G2_accepted:false, F_accepted:false, automatic_retry:false, cases:[]};
async function retain(name, value) {
  const raw = Buffer.from(JSON.stringify(value, null, 2) + '\n');
  assert(raw.length <= 2 * 1024 * 1024, 'receipt size');
  await fs.writeFile(path.join(output, name), raw, {flag:'wx', mode:0o600});
}
try {
  output = path.resolve(args.get('--output'));
  assert(output !== sdk && !output.startsWith(sdk + path.sep), 'private focal output outside SDK checkout');
  await fs.mkdir(output); // Exclusive new execution, never overwrite an uncertain run.
  assert.equal(process.platform, 'linux', 'reviewed CI resource controller');
  const groupLine=(await fs.readFile('/proc/self/cgroup','utf8')).trim().split('\n').find(line=>line.startsWith('0::'));
  assert(groupLine&&/^0::\/system.slice\/semwright-layout-focal-[0-9]+-[0-9]+\.service$/.test(groupLine),'owned cgroup required');
  cgroup=path.join('/sys/fs/cgroup',groupLine.slice(3));
  const limits={memory_max:Number((await fs.readFile(path.join(cgroup,'memory.max'),'utf8')).trim()),
    swap_max:Number((await fs.readFile(path.join(cgroup,'memory.swap.max'),'utf8')).trim()),
    pids_max:Number((await fs.readFile(path.join(cgroup,'pids.max'),'utf8')).trim())};
  assert.equal(limits.memory_max,1536*1024*1024);assert.equal(limits.swap_max,0);assert.equal(limits.pids_max,256);
  const available=async()=>Number((await fs.readFile('/proc/meminfo','utf8')).match(/^MemAvailable:\s+(\d+) kB$/m)?.[1])*1024;
  assert(await available()>=2048*1024*1024,'physical memory not admitted');
  const disk=await fs.statfs(output);assert(disk.bavail*disk.bsize>=10*1024**3,'disk floor not admitted');
  result.resource_limits={...limits,physical_reserve_bytes:512*1024*1024,service_deadline_seconds:240};
  let resourceStopped=false;
  watchdog=setInterval(async()=>{
    if(resourceStopped)return;
    try {
      const disk=await fs.statfs(output);
      assert(await available()>=512*1024*1024&&disk.bavail*disk.bsize>=10*1024**3,'resource reserve lost');
    } catch(error) {
      resourceStopped=true;
      try{await retain('resource-stop.original.json',{status:'RESOURCE_STOP_NO_RETRY',phase,message:String(error.message).slice(0,200)});}catch{}
      process.exit(3); // systemd KillMode=control-group closes every owned child.
    }
  },500);
  const lockBytes = await fs.readFile(path.join(runtime, 'package-lock.json'));
  const lock = JSON.parse(lockBytes);
  for (const [name, [version, integrity]] of Object.entries(PACKAGE_PINS)) {
    const pin = lock.packages['node_modules/' + name];
    assert.equal(pin.version, version); assert.equal(pin.integrity, integrity);
    assert.equal(JSON.parse(await fs.readFile(path.join(runtime, 'node_modules', name, 'package.json'))).version, version);
  }
  for (const [file, expected] of Object.entries(API_PINS)) {
    assert.equal(sha(await fs.readFile(path.join(runtime, 'node_modules', file))), expected, file);
  }
  const observedPath = path.join(sdk, 'integrations/composition/motion/native.ts');
  const historical = await fs.readFile(path.resolve(args.get('--baseline')));
  const observed = await fs.readFile(observedPath);
  assert.equal(sha(historical), HISTORICAL.GREEN); assert.equal(sha(observed), OBSERVED.GREEN);
  const work = path.join(output, 'work'); await fs.mkdir(work);
  await fs.symlink(path.join(runtime, 'node_modules'), path.join(work, 'node_modules'), 'dir');
  result.source_modules = {};
  for (const [mode, green, expected] of [['historical', historical, HISTORICAL], ['observed', observed, OBSERVED]]) {
    const text = green.toString('utf8'); assert.equal(text.split(GREEN).length, 2, 'exact causal attachment delta');
    for (const [arm, body] of [['GREEN', text], ['RED', text.replace(GREEN, RED)]]) {
      assert.equal(sha(body), expected[arm], 'preserved arm source');
      const access=HOOK+(mode==='observed'?'\nexport {textPixelReadbackInSrgb as __focalReadbackInSrgb};\n':'');
      const tested = body + access;
      await fs.writeFile(path.join(work, mode + '-' + arm + '.ts'), tested, {flag:'wx'});
      result.source_modules[mode + '-' + arm] = {original_sha256:sha(body), test_access_only_suffix_sha256:sha(access), tested_sha256:sha(tested)};
    }
  }
  result.dependency_lock_sha256 = sha(lockBytes); result.pinned_native_API_files = API_PINS;
  result.focal_script_sha256=sha(await fs.readFile(fileURLToPath(import.meta.url)));
  result.browser_fixture_source_sha256=sha(BROWSER_SOURCE);
  result.baseline_sdk_sha='ec546358dd330ecb2b2abf058145eb3e0aeb11d0';
  result.prior_six_layout_and_twenty_geometry_draws_reused=true;
  result.pixel_oracle="independent Canvas prototype interception at actual semantic Txt.draw";
  await retain('intake.original.json', result);
  await fs.writeFile(path.join(work, 'index.html'), '<!doctype html><meta charset="utf-8"><script type="module" src="/focal.js"></script>', {flag:'wx'});
  await fs.writeFile(path.join(work, 'focal.js'), BROWSER_SOURCE, {flag:'wx'});
  phase = 'build_exact_native_modules';
  const {build} = await import('vite');
  await build({root:work, configFile:false, logLevel:'error',
    build:{outDir:path.join(work,'dist'),emptyOutDir:false,minify:false,sourcemap:false,assetsInlineLimit:0}});
  const dist = path.join(work, 'dist'), files = new Map(); let total = 0;
  async function inventory(dir) {
    for (const item of await fs.readdir(dir, {withFileTypes:true})) {
      assert(files.size < 256, 'bounded focal bundle roster');
      const file = path.join(dir,item.name); assert(!item.isSymbolicLink(), 'no served symlink');
      if (item.isDirectory()) await inventory(file);
      else {assert(item.isFile());const stat=await fs.stat(file);total+=stat.size;
        assert(stat.size<=32*1024*1024&&total<=64*1024*1024,'bounded focal bundle bytes');
        files.set('/'+path.relative(dist,file).split(path.sep).join('/'),file);}
    }
  }
  await inventory(dist);
  server = http.createServer(async (request,response)=>{
    try {
      const url=new URL(request.url,'http://127.0.0.1');const key=url.pathname==='/'?'/index.html':url.pathname;
      assert(request.method==='GET'&&files.has(key));
      const mime={'.js':'text/javascript','.css':'text/css','.html':'text/html','.woff2':'font/woff2'}[path.extname(key)]??'application/octet-stream';
      response.writeHead(200,{'content-type':mime,'cache-control':'no-store'});response.end(await fs.readFile(files.get(key)));
    } catch {response.writeHead(404);response.end();}
  });
  await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve));
  const origin='http://127.0.0.1:'+server.address().port;
  phase = 'real_firefox_fixture';
  const {firefox}=await import('playwright');
  result.browser_executable_sha256=sha(await fs.readFile(firefox.executablePath()));
  browser=await firefox.launch({headless:true});result.browser_version=browser.version();
  const context=await browser.newContext({viewport:{width:1920,height:1080},deviceScaleFactor:1,serviceWorkers:'block'});
  await context.route('**/*', route=>new URL(route.request().url()).origin===origin&&route.request().method()==='GET'
    ?route.continue():route.abort());
  phase='native-api-readback-without-draw';
  const apiPage=await context.newPage();
  try{
    await apiPage.goto(origin,{waitUntil:'networkidle',timeout:30000});
    result.native_readback_API=await apiPage.evaluate(()=>window.probeNativeReadbackApi());
  }finally{await apiPage.close();}
  assert.equal(result.native_readback_API.actual_readback_admitted,true,'native sRGB readback API unavailable before drawing');
  assert.equal(result.native_readback_API.original_image_data_metadata_restored,true);
  assert(Object.values(result.native_readback_API.synthetic_metadata_negative_checks).every(value=>value===true));
  for (const name of selected) {
    phase = 'case:' + name; const arm=name.endsWith('-red')?'RED':'GREEN'; const rows=[];
    for (const mode of ['historical','observed']) {
      const page=await context.newPage();
      try {
        await page.goto(origin,{waitUntil:'networkidle',timeout:30000});
        let timer;
        const actual=await Promise.race([page.evaluate(value=>window.runFocal(value),{name,mode,arm}),
          new Promise((_,reject)=>{timer=setTimeout(()=>reject(new Error('focal DOM deadline')),30000);})])
          .finally(()=>clearTimeout(timer));
        rows.push(actual);
      } finally {await page.close();}
    }
    const [before,after]=rows;
    // Preserve both real draws before every verdict, including authentic negatives.
    await retain(name+'.captured.original.json',{name,arm,native_readback_API:result.native_readback_API,original_native_results:rows});
    assert.equal(before.pixel_sha256,after.pixel_sha256,'observer changed actual RGBA: '+name);
    for(const row of rows){
      assert.equal(row.pixel_sha256,row.post_probe_pixel_sha256,'probe changed actual RGBA');
      assert.equal(row.native_camera_scene_unchanged,true,'original Camera.scene reference changed');
      assert.equal(row.prototypes_restored,true,'native Canvas prototypes not restored');
      assert.deepEqual(row.context_own_methods_before,row.context_own_methods_after,'Canvas instance descriptor changed');
    }
    assert.deepEqual(before.signals,after.signals,'observer changed native signals: '+name);
    assert.equal(before.frame,after.frame,'native playback frame changed');
    assert.equal(before.semantic_draw_calls,after.semantic_draw_calls,'observer added an actual native draw');
    assert.equal(before.view_render_calls,after.view_render_calls,'observer repeated a native view render');
    assert.deepEqual(before.independent_glyph_operations,after.independent_glyph_operations,
      'observer changed actual native glyph calls or contribution');
    const legacy=structuredClone(after.probe);
    for(const row of legacy.subjects)delete row.native_text_pixels;
    assert.deepEqual(legacy,before.probe,'prior native geometry or text observations changed');
    const text=after.probe.subjects.filter(row=>Object.hasOwn(after.independent_glyph_operations,row.id));
    if(name==='absent-profile'){
      assert(text.every(row=>!Object.hasOwn(row,'native_text_pixels')),'absent profile gained pixel evidence');
      assert.equal(after.producer_getImageData_calls,before.producer_getImageData_calls,'absent profile read pixels');
    }else if(['unselected-frame','cached','unsupported-effects','same-color','operation-overflow'].includes(name)){
      assert(text.every(row=>row.native_text_pixels?.status==='unknown'),'unsupported observation became known');
      if(name==='unselected-frame')assert.equal(after.producer_getImageData_calls,before.producer_getImageData_calls,'unselected frame read pixels');
      if(name==='cached'||name==='unsupported-effects')assert(Object.values(after.independent_glyph_operations).some(row=>row.offscreen_calls>0),'actual offscreen negative missing');
      if(name==='operation-overflow')assert(Object.values(after.independent_glyph_operations).reduce((n,row)=>n+row.calls.length,0)>64,'actual operation budget not exceeded');
    }else{
      for(const row of text){
        assert.equal(row.native_text_pixels?.status,'known','real selected glyph contribution not observed');
        const observed=row.native_text_pixels.value,oracle=after.independent_glyph_operations[row.id];
        const bitmap=decodeObservedMask(observed.mask,after.width,after.height);
        try{
          assert.equal(sha(bitmap),oracle.mask_sha256,'native mask differs from independent real glyph changes');
          assert.equal(observed.mask.changed_pixels,oracle.changed_pixels,'native changed-pixel count differs');
          assert.equal(observed.calls.length,oracle.calls.length,'glyph operation roster differs');
          assert.equal(observed.frame,after.frame);assert.deepEqual(observed.selected_frames,[after.frame]);
          assert.equal(observed.render_input_digest,after.input_binding.render_input_digest);
          for(let i=0;i<observed.calls.length;i++){
            assert.equal(observed.calls[i].text,oracle.calls[i].text);
            assert.equal(observed.calls[i].font,oracle.calls[i].font);
            assert.equal(observed.calls[i].changed_pixels,oracle.calls[i].changed_pixels);
            assert.deepEqual(observed.calls[i].matrix,oracle.calls[i].matrix);
          }
        }finally{bitmap.fill(0);}
      }
      if(name==='inactive-second')assert.equal(text.find(row=>row.id==='second').native_text_pixels.value.mask.changed_pixels,0);
      else {
        assert(text.every(row=>row.native_text_pixels.value.mask.changed_pixels>32),'positive actual ink absent');
        for(const wrong of after.mismatched_export_bindings){
          assert(wrong.observations.every(row=>row.pixels?.status==='unknown'),'foreign export binding was admitted');
        }
        assert.equal(after.mismatched_export_bindings.length,3);
      }
    }
    const caseResult={name,arm,status:'REAL_TEXT_PIXEL_DOM_FOCAL_PASSED',
      captured_original:name+'.captured.original.json',pixel_sha256:after.pixel_sha256,
      producer_getImageData_calls:after.producer_getImageData_calls,
      render_elapsed_ms:{historical:before.render_elapsed_ms,observed:after.render_elapsed_ms}};
    caseResult.actual_view_render_calls=rows.reduce((count,row)=>count+row.view_render_calls,0);
    result.cases.push(caseResult);await retain(name+'.original.json',caseResult);
    process.stdout.write(JSON.stringify(caseResult)+'\n');
  }
  assert.equal(sha(await fs.readFile(observedPath)),OBSERVED.GREEN,'producer source changed');
  assert.equal(sha(await fs.readFile(path.join(runtime,'package-lock.json'))),result.dependency_lock_sha256,'lock changed');
  result.status='SELECTED_REAL_TEXT_PIXEL_DOM_FOCALS_PASSED';
  result.cgroup_memory_peak_bytes=Number((await fs.readFile(path.join(cgroup,'memory.peak'),'utf8')).trim());
  result.production_frames_rendered=0;result.focal_actual_Canvas2D_draws=result.cases.reduce((count,row)=>count+row.actual_view_render_calls,0);result.explicit_scene_render_calls=result.cases.length*2;
  await retain('terminal.original.json',result);
  process.stdout.write(JSON.stringify({status:result.status,cases:result.cases.length,G2_accepted:false,F_accepted:false})+'\n');
} catch (error) {
  result.status='REAL_TEXT_PIXEL_DOM_FOCAL_FAILED';result.phase=phase;
  result.error={name:error?.name??'Error',message:String(error?.message??error).slice(0,2000)};
  if(output)try{await retain('terminal.original.json',result);}catch{}
  process.stderr.write(JSON.stringify({status:result.status,phase,error:result.error,automatic_retry:false})+'\n');
  process.exitCode=1;
} finally {
  if(watchdog)clearInterval(watchdog);
  if(browser)await browser.close();
  if(server)await new Promise(resolve=>server.close(resolve));
}
