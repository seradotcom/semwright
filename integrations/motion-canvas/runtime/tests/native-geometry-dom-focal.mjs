/** Actual Motion Canvas 3.17.2/Firefox fixture; no Driver Host or G2 claim.
 * The frozen 2c87455b and extended observer modules each draw one unchanged frame per
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
const CASES = ['clip-rotated-red', 'clip-rotated-green', 'clip-nested-green', 'clip-rounded-green', 'clip-cached-green', 'camera-original-red', 'camera-original-green', 'camera-replaced-green', 'camera-duplicate-green', 'camera-cached-green'];
const selection = args.get('--case');
assert(selection === 'all' || CASES.includes(selection), 'closed geometry focal case');
const selected = selection === 'all' ? CASES : [selection];
const HISTORICAL = {
 GREEN: '6ea8d223690da0c7f77f6b2ce4cee22857b46a0c9490093646357ccc2f4e9508',
 RED: '629ebdec4e7dd42c3d6a9bf71c27c08997d54aa2cabe50c9daac3072e16e3bec',
};
const OBSERVED = {
 GREEN: 'afc197bacf9d73c007833a315b253b1b901005a2c9ef7506eec57b2683aaf3c7',
 RED: 'ac4df30d0bdeab0a93924d5841ea18a76f269ce99f2903a2cfebc38e254a3c12',
};
// Existing mandatory transformed/clip corner guard. No prior case is rerun or retuned.
const CORNER_AXIS_GUARD_PX = 2;
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
function need(ok,code){if(!ok)throw new Error(code);}
const digest=async value=>[...new Uint8Array(await crypto.subtle.digest('SHA-256',value))].map(x=>x.toString(16).padStart(2,'0')).join('');
const matrix=m=>[m.a,m.b,m.c,m.d,m.e,m.f];
const zero={top:0,right:0,bottom:0,left:0};
function dataFor(name){
 const fixed=(x,y,w,h)=>({kind:'fixed',position:{x,y},size:{width:w,height:h}});
 const font={family:'Instrument Sans Variable',asset_digest:null,fallback:'deny',permitted_fallbacks:[]};
 const camera=name.startsWith('camera-'),nested=name==='clip-nested-green',rounded=name==='clip-rounded-green';
 const subject=(id,parent,content,layout,clip=false)=>({id,role:'primary',parent,layer:'main',content,layout,initially_visible:true,clip_intentional:clip});
 const subjects=[];if(name==='camera-cached-green')subjects.push(subject('cacheRoot',null,{kind:'group'},fixed(0,0,640,360)));
 subjects.push(subject('parent',name==='camera-cached-green'?'cacheRoot':null,camera?{kind:'camera',zoom:1}:rounded?
  {kind:'rectangle',fill:'#183040',stroke:null,radius:16}:{kind:'group'},fixed(0,0,300,180),!camera));
 if(nested)subjects.push(subject('inner','parent',{kind:'group'},fixed(30,0,200,110),true));
 subjects.push(subject('text',nested?'inner':'parent',{kind:'text',runs:[
  {text:'Original ',weight:400,color:'#f4f4f4',emphasis:false},
  {text:'bold\n',weight:700,color:'#d4a62a',emphasis:false},
  {text:'native polygon',weight:400,color:'#f4f4f4',emphasis:false}],
  style:'body',line_height:1.2,direction:'ltr',language:'en',wrap:true,truncate:false},
  fixed(camera?-70:80,camera?-20:0,camera?220:280,80)));
 return {version:1,id:'geometry-focal-'+name,start:{num:'0',den:'1'},end:{num:'1',den:'1'},
  width:640,height:360,aspect:'landscape',safe_area:zero,
  editorial:{font,mono_font:font,type_scale:{body:24},colors:{text:'#f4f4f4'},spacing:{block:0,base:0},stroke:1,corner_radius:0},
  shots:[{id:'shot',archetype:'statement',start:{num:'0',den:'1'},end:{num:'1',den:'1'},subjects,
   layers:[{id:'main',order:0,intentional_overlay:false}],annotations:[],captions:[]}],instructions:[],cues:{}};
}
function signals(reg){return Object.fromEntries([...reg.nodes].map(([id,n])=>[id,{
 position:[n.position().x,n.position().y],scale:[n.scale().x,n.scale().y],rotation:n.rotation(),opacity:n.opacity(),
 size:n instanceof Layout?[n.size().x,n.size().y]:null,layout_root:n instanceof Layout?n.isLayoutRoot():null,
 text:id==='text'?n.text():null,children:n.peekChildren().map(child=>child.key),
 camera_scene:n instanceof Camera?n.scene().key:null,zoom:n instanceof Camera?n.zoom():null,
 camera_draw_own:n instanceof Camera?Object.hasOwn(n,'drawChildren'):null,scene_draw_own:n instanceof Camera?Object.hasOwn(n.scene(),'drawChildren'):null}]));}
function nativeOverlayPolygon(node,drawMatrix){
 // Independent native Layout overlay, supplied the CTM read inside native draw.
 const canvas=document.createElement('canvas');canvas.width=640;canvas.height=360;
 const context=canvas.getContext('2d');need(context,'native overlay Canvas2D');
 const paths=[];let current=null;
 for(const name of ['beginPath','moveTo','lineTo']){
  const original=context[name].bind(context);
  context[name]=(...args)=>{if(name==='beginPath'){current=[];paths.push(current);}else if(current)current.push([args[0],args[1]]);return original(...args);};
 }
 node.drawOverlay(context,new DOMMatrix(drawMatrix));
 need(paths[2]?.length>=4,'native overlay polygon unavailable');return paths[2].slice(0,4);
}
function observeActualLayoutClips(run,rows){
 // Enter the native Layout.draw itself: this oracle does not read producer
 // records, lineages, cached bounds, or its clipping interception state.
 const descriptor=Object.getOwnPropertyDescriptor(Layout.prototype,'draw');need(descriptor?.value,'native Layout.draw descriptor');
 const nativeDraw=descriptor.value,active=[];
 Object.defineProperty(Layout.prototype,'draw',{...descriptor,value:function(ctx){
  active.push(this);const owner=this;let rectangle=null,done=false;
  const saved=[];
  try{
   if(Object.getPrototypeOf(this)===Layout.prototype&&this.clip()){
    for(const method of ['rect','clip']){
     const own=Object.getOwnPropertyDescriptor(ctx,method),original=ctx[method];
     Object.defineProperty(ctx,method,{configurable:true,writable:true,value:function(...args){
      const mine=this===ctx&&active[active.length-1]===owner&&!done;
      const nativeMatrix=mine?matrix(ctx.getTransform()):null;
      const result=original.apply(this,args);
      if(mine&&method==='rect')rectangle={args:[...args],matrix:nativeMatrix};
      if(mine&&method==='clip'){
       done=true;rows.push({owner_key:owner.key,rectangle,clip_matrix:nativeMatrix,
        clip_argument_types:args.map(x=>typeof x==='object'?x?.constructor?.name:typeof x),
        canvas_size:[ctx.canvas.width,ctx.canvas.height],canvas:ctx.canvas,node:owner});
      }
      return result;
     }});saved.push([method,own]);
    }
   }
   return nativeDraw.call(this,ctx);
  }finally{
   for(const [method,own]of saved.reverse()){if(own)Object.defineProperty(ctx,method,own);else delete ctx[method];}
   active.pop();
  }
 }});
 return Promise.resolve().then(run).finally(()=>Object.defineProperty(Layout.prototype,'draw',descriptor));
}
window.runFocal=async({name,mode,arm})=>{
 const loaded=await Promise.all([400,700].map(weight=>document.fonts.load(weight+' 24px "Instrument Sans Variable"')));
 await document.fonts.ready;need(loaded.every(x=>x.length>0),'real bundled font not loaded');
 globalThis.__SEMWRIGHT_TEXT_DIGEST__=text=>digest(new TextEncoder().encode(text));globalThis.__SEMWRIGHT_NATIVE_PROBE__=undefined;
 const data=dataFor(name),module=modules[mode][arm],logger=new Logger();
 const playback=new PlaybackManager();playback.fps=30;playback.state=PlaybackState.Rendering;
 const webgl=new SharedWebGLContext(logger);
 const scene=new Scene2D({...module.createAuthoringScene(data,{}),name:data.id,logger,playback:new PlaybackStatus(playback),
  size:new Vector2(640,360),resolutionScale:1,timeEventsClass:ReadOnlyTimeEvents,sharedWebGLContext:webgl,experimentalFeatures:false});
 try{
  await scene.reset();for(let count=0;!globalThis.__SEMWRIGHT_NATIVE_PROBE__&&count<8;count++){playback.frame++;await scene.next();}
  need(typeof globalThis.__SEMWRIGHT_NATIVE_PROBE__==='function','actual authoring probe not initialized');
  const reg=module.__focalRegistrations.get(data.id),node=reg.nodes.get('text'),parent=reg.nodes.get('parent');
  const camera=parent instanceof Camera,originalScene=camera?parent.scene():null;let extraCamera=null;
  scene.execute(()=>{
   if(!camera){parent.rotation(8);parent.scale([1.05,.95]);}
   if(name==='clip-nested-green'){reg.nodes.get('inner').rotation(-5);reg.nodes.get('inner').scale([.9,1.1]);}
   if(name==='clip-cached-green')parent.cache(true);
   if(name==='camera-cached-green')reg.nodes.get('cacheRoot').cache(true);
   // These are explicit real negative fixture mutations before rendering;
   // the producer observation itself never creates or replaces a scene.
   if(name==='camera-replaced-green'){const replacement=new Node({children:[...originalScene.peekChildren()]});parent.scene(replacement);}
   if(name==='camera-duplicate-green'){extraCamera=new Camera({zoom:1});scene.getView().add(extraCamera);}
  });
  const canvas=document.createElement('canvas');canvas.width=640;canvas.height=360;
  const context=canvas.getContext('2d',{alpha:true,colorSpace:'srgb'});need(context,'real Canvas2D');
  const nativeDraw=node.draw,ownDraw=Object.getOwnPropertyDescriptor(node,'draw'),drawCalls=[],clipCalls=[];
  node.draw=function(ctx){const call={matrix:matrix(ctx.getTransform()),canvas_is_scene:ctx.canvas===canvas,
   canvas_size:[ctx.canvas.width,ctx.canvas.height],completed:false};drawCalls.push(call);
   const result=nativeDraw.call(this,ctx);call.completed=true;return result;};
  try{await observeActualLayoutClips(()=>scene.render(context),clipCalls);}finally{
   if(ownDraw)Object.defineProperty(node,'draw',ownDraw);else delete node.draw;
  }
  const pixels=context.getImageData(0,0,640,360).data,before=scene.execute(()=>signals(reg));
  globalThis.__SEMWRIGHT_NATIVE_FRAME_CLOCK__={frame:playback.frame,fps_num:30,fps_den:1};
  const probe=await globalThis.__SEMWRIGHT_NATIVE_PROBE__(canvas,playback.frame,
   {render_input_digest:await digest(new TextEncoder().encode(JSON.stringify(data))),native_stage_version:'3.17.2',font_evidence:[]});
  const after=scene.execute(()=>signals(reg));need(JSON.stringify(before)===JSON.stringify(after),'probe changed native signals');
  const clipOracle=clipCalls.map(call=>({owner_key:call.owner_key,rectangle:call.rectangle,clip_matrix:call.clip_matrix,
   clip_argument_types:call.clip_argument_types,canvas_size:call.canvas_size,canvas_is_scene:call.canvas===canvas,
   native_overlay_polygon:scene.execute(()=>nativeOverlayPolygon(call.node,call.clip_matrix))}));
  let cameraOracle=null;
  if(camera){
   const keys=[];for(let n=node;n;n=n.parent())keys.push(n.key);
   cameraOracle={native_key:parent.key,original_scene_key:originalScene.key,current_scene_key:parent.scene().key,
    current_scene_is_original_by_reference:parent.scene()===originalScene,
    actual_fixture_camera_keys:[parent,...(extraCamera?[extraCamera]:[])].map(n=>n.key).sort(),
    text_native_lineage_keys:keys,position:[parent.position().x,parent.position().y],rotation:parent.rotation(),zoom:parent.zoom()};
  }
  const ownMethods=['rect','clip','beginPath','closePath'].map(key=>[key,Object.hasOwn(context,key)]);
  return {name,mode,arm,frame:playback.frame,pixel_sha256:await digest(pixels),signals:after,probe,
   subject:probe.subjects.find(x=>x.id==='text'),native_draw_calls:drawCalls,native_clip_calls:clipOracle,
   native_draw_overlay_polygon:drawCalls.length===1&&drawCalls[0].completed?scene.execute(()=>nativeOverlayPolygon(node,drawCalls[0].matrix)):null,
   native_camera_oracle:cameraOracle,context_own_methods_after_render:ownMethods,
   native_ink_pixels:pixels.filter((_,i)=>i%4===3&&pixels[i]>0).length};
 }finally{scene.getView().dispose();webgl.dispose();}
};
`;

let browser, server, output, watchdog, cgroup, phase = 'arguments';
const result = {schema:'semwright.motion-native-geometry-dom-focal/1', selected_cases:selected,
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
      const tested = body + HOOK;
      await fs.writeFile(path.join(work, mode + '-' + arm + '.ts'), tested, {flag:'wx'});
      result.source_modules[mode + '-' + arm] = {original_sha256:sha(body), test_access_only_suffix_sha256:sha(HOOK), tested_sha256:sha(tested)};
    }
  }
  result.dependency_lock_sha256 = sha(lockBytes); result.pinned_native_API_files = API_PINS;
  result.focal_script_sha256=sha(await fs.readFile(fileURLToPath(import.meta.url)));
  result.browser_fixture_source_sha256=sha(BROWSER_SOURCE);
  result.baseline_sdk_sha='2c87455b5b587d7aa3b3018529c8670730e1eb91';
  result.corner_axis_guard_px=CORNER_AXIS_GUARD_PX;result.previous_six_layout_cases_reused=true;
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
  const context=await browser.newContext({viewport:{width:640,height:360},deviceScaleFactor:1,serviceWorkers:'block'});
  await context.route('**/*', route=>new URL(route.request().url()).origin===origin&&route.request().method()==='GET'
    ?route.continue():route.abort());
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
    const geometry=after.subject.native_geometry;
    const cornerDelta=(a,b)=>Array.isArray(a)&&Array.isArray(b)&&a.length===4&&b.length===4
      ?Math.max(...a.flatMap((p,i)=>p.map((v,j)=>Math.abs(v-b[i][j])))):null;
    const polygonDelta=geometry?.status==='known'&&geometry.value.screen_layout_polygon.status==='known'
      ?cornerDelta(geometry.value.screen_layout_polygon.value,after.native_draw_overlay_polygon):null;
    const clipDeltas=after.probe.subjects.filter(row=>row.native_geometry?.status==='known'&&row.native_geometry.value.own_clip.status==='known'
      &&row.native_geometry.value.own_clip.value.kind==='rect').map(row=>{
        const clip=row.native_geometry.value.own_clip.value.clip;
        const native=after.native_clip_calls.filter(call=>call.owner_key===clip.native_key);
        return {subject:row.id,owner:clip.native_key,native_calls:native.length,
          delta:native.length===1?cornerDelta(clip.screen_polygon,native[0].native_overlay_polygon):null};
      });
    // All original rows and numerical differences precede the first verdict.
    await retain(name+'.captured.original.json',{name,arm,original_native_results:rows,
      observed_polygon_minus_native_draw_overlay_max_axis:polygonDelta,
      clip_polygon_native_draw_differences:clipDeltas,corner_axis_guard_px:CORNER_AXIS_GUARD_PX});
    assert.equal(before.pixel_sha256,after.pixel_sha256,'observer changed actual RGBA: '+name);
    assert.deepEqual(before.signals,after.signals,'observer changed native signals: '+name);
    assert.deepEqual(before.native_draw_calls,after.native_draw_calls,'observer changed actual draw CTM: '+name);
    assert.deepEqual(before.native_clip_calls,after.native_clip_calls,'observer changed actual clipping: '+name);
    assert.deepEqual(before.native_camera_oracle,after.native_camera_oracle,'observer changed original Camera.scene: '+name);
    assert.equal(before.frame,after.frame,'same native frame: '+name);
    assert(after.native_ink_pixels>0,'actual native ink missing: '+name);
    assert(after.context_own_methods_after_render.every(([,own])=>own===false),'Canvas descriptors not restored');
    const legacy=structuredClone(after.probe);
    for(const row of legacy.subjects)for(const key of ['native_geometry','camera_scene'])delete row[key];
    assert.deepEqual(legacy,before.probe,'prior typed observations changed: '+name);
    assert.equal(geometry.status,'known');
    assert.equal(geometry.value.lineage.status,'known');
    assert.equal(geometry.value.lineage.value[0].subject_id,'text');
    if(name.includes('cached')){
      assert(after.native_draw_calls.some(call=>call.completed&&!call.canvas_is_scene),'real cache draw missing');
      assert.equal(geometry.value.screen_layout_polygon.status,'unknown','cache polygon must stay unknown');
    }else{
      assert.equal(geometry.value.screen_layout_polygon.status,'known');
      assert(polygonDelta!==null&&polygonDelta<=CORNER_AXIS_GUARD_PX,'native polygon corner discrepancy: '+name);
    }
    if(name.startsWith('clip-')){
      const parent=after.probe.subjects.find(row=>row.id==='parent');
      if(name==='clip-rounded-green'||name==='clip-cached-green'){
        assert.equal(parent.native_geometry.value.own_clip.status,'unknown','unsupported/cache clip must stay unknown');
      }else{
        const expected=name==='clip-nested-green'?['inner','parent']:['parent'];
        assert.deepEqual(clipDeltas.map(row=>row.subject).sort(),expected);
        for(const row of clipDeltas)assert(row.native_calls===1&&row.delta!==null&&row.delta<=CORNER_AXIS_GUARD_PX,'clip owner or polygon differs');
      }
    }else{
      const row=after.probe.subjects.find(row=>row.id==='parent'),camera=row.camera_scene,oracle=after.native_camera_oracle;
      assert.equal(camera.status,'known');const value=camera.value;
      assert.equal(value.native_key,oracle.native_key);assert.equal(value.original_scene_key,oracle.original_scene_key);
      assert.equal(value.current_scene_key,oracle.current_scene_key);
      assert.equal(value.current_scene_is_original,oracle.current_scene_is_original_by_reference);
      assert.equal(value.draw_scene_key,oracle.current_scene_key);
      assert.equal(value.draw_uses_original_scene,oracle.current_scene_is_original_by_reference);
      assert.equal(value.cameras.status,'known');
      assert.deepEqual(value.cameras.value.map(row=>row.native_key).sort(),oracle.actual_fixture_camera_keys);
      assert.deepEqual(value.position,oracle.position);assert.equal(value.rotation,oracle.rotation);assert.equal(value.zoom,oracle.zoom);
      assert.deepEqual(geometry.value.lineage.value.map(row=>row.native_key),oracle.text_native_lineage_keys);
      assert.equal(geometry.value.lineage.value.at(-1).native_key,value.draw_scene_key,'text must actually be under drawn scene');
      if(name==='camera-cached-green')assert.equal(value.scene_transform.status,'unknown','cached camera mapping stays unknown');
      else {assert.equal(value.scene_transform.status,'known');assert.deepEqual(value.scene_transform.value,[1,0,0,1,320,180]);}
      if(name==='camera-replaced-green')assert.equal(value.draw_uses_original_scene,false,'actual replacement must be retained');
      if(name==='camera-duplicate-green')assert.equal(value.cameras.value.length,2,'actual unregistered duplicate must be retained');
    }
    const caseResult={name,arm,status:'REAL_GEOMETRY_DOM_FOCAL_PASSED',original_native_results:rows};
    result.cases.push(caseResult);await retain(name+'.original.json',caseResult);
    process.stdout.write(JSON.stringify({case:name,status:caseResult.status,pixel_sha256:after.pixel_sha256})+'\n');
  }
  assert.equal(sha(await fs.readFile(observedPath)),OBSERVED.GREEN,'producer source changed');
  assert.equal(sha(await fs.readFile(path.join(runtime,'package-lock.json'))),result.dependency_lock_sha256,'lock changed');
  result.status='SELECTED_REAL_GEOMETRY_DOM_FOCALS_PASSED';
  result.cgroup_memory_peak_bytes=Number((await fs.readFile(path.join(cgroup,'memory.peak'),'utf8')).trim());
  result.production_frames_rendered=0;result.focal_actual_Canvas2D_draws=result.cases.length*2;
  await retain('terminal.original.json',result);
  process.stdout.write(JSON.stringify({status:result.status,cases:result.cases.length,G2_accepted:false,F_accepted:false})+'\n');
} catch (error) {
  result.status='REAL_GEOMETRY_DOM_FOCAL_FAILED';result.phase=phase;
  result.error={name:error?.name??'Error',message:String(error?.message??error).slice(0,2000)};
  if(output)try{await retain('terminal.original.json',result);}catch{}
  process.stderr.write(JSON.stringify({status:result.status,phase,error:result.error,automatic_retry:false})+'\n');
  process.exitCode=1;
} finally {
  if(watchdog)clearInterval(watchdog);
  if(browser)await browser.close();
  if(server)await new Promise(resolve=>server.close(resolve));
}
