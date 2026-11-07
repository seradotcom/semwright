/** Actual Motion Canvas 3.17.2/Firefox fixture; no Driver Host or G2 claim.
 * The historical and observer-only modules each draw one unchanged frame per
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
const CASES = ['enabled-red', 'enabled-green', 'disabled-control', 'transformed', 'camera', 'cached'];
const selection = args.get('--case');
assert(selection === 'all' || selection === 'camera-and-cached' || CASES.includes(selection), 'closed focal case');
const selected = selection === 'all' ? CASES : selection === 'camera-and-cached' ? ['camera','cached'] : [selection];
const HISTORICAL = {
  GREEN: 'a400b38608e169a20e7ade099aa8632c11629152951b9e62a0fb66858b530f36',
  RED: 'dfe6c0fc226d7e9017eb718c7e745d942c4067a54f968d4483b80bbdd51b4fa8',
};
const OBSERVED = {
  GREEN: '6ea8d223690da0c7f77f6b2ce4cee22857b46a0c9490093646357ccc2f4e9508',
  RED: '629ebdec4e7dd42c3d6a9bf71c27c08997d54aa2cabe50c9daac3072e16e3bec',
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
import {Scene2D} from '@motion-canvas/2d';
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
const zero={top:0,right:0,bottom:0,left:0};
function dataFor(name){
 const fixed=(x,y,w,h)=>({kind:'fixed',position:{x,y},size:{width:w,height:h}});
 const font={family:'Instrument Sans Variable',asset_digest:null,fallback:'deny',permitted_fallbacks:[]};
 const subjects=[];const enabled=name.startsWith('enabled-');
 if(!enabled)subjects.push({id:'parent',role:'group',parent:null,layer:'main',
  content:name==='camera'?{kind:'camera',zoom:1.2}:{kind:'group'},
  layout:fixed(0,0,640,360),initially_visible:true,clip_intentional:false});
 subjects.push({id:'text',role:'label',parent:enabled?null:'parent',layer:'main',
  content:{kind:'text',runs:[{text:'Original ',weight:400,color:'#f4f4f4',emphasis:false},
   {text:'bold\n',weight:700,color:'#d4a62a',emphasis:false},
   {text:'native layout',weight:400,color:'#f4f4f4',emphasis:false}],style:'body',line_height:1.2,
   direction:'ltr',language:'en',wrap:true,truncate:false},
  layout:fixed(-90,-25,220,80),initially_visible:true,clip_intentional:false});
 return {version:1,id:'dom-focal-'+name,start:{num:'0',den:'1'},end:{num:'1',den:'1'},
  width:640,height:360,aspect:'landscape',safe_area:zero,
  editorial:{font,mono_font:font,type_scale:{body:24},colors:{text:'#f4f4f4'},spacing:{block:0,base:0},stroke:1,corner_radius:0},
  shots:[{id:'shot',archetype:'statement',start:{num:'0',den:'1'},end:{num:'1',den:'1'},subjects,
   layers:[{id:'main',order:0,intentional_overlay:false}],annotations:[],captions:[]}],instructions:[],cues:{}};
}
function signals(node){return {position:[node.position().x,node.position().y],size:[node.size().x,node.size().y],
 scale:[node.scale().x,node.scale().y],rotation:node.rotation(),offset:[node.offset().x,node.offset().y],
 layout_root:node.isLayoutRoot(),text:node.text(),children:node.children().map(x=>x.key)};}
function nativeOverlayBox(scene,node,drawMatrix=null){
 // Observe the real 3.17.2 Layout overlay polygon through a real 2D context.
 // Retain the editor's world/camera path separately from the actual draw CTM.
 // Vector2.transformAsPoint quantizes its supplied matrix to Float32Array;
 // the editor's composed DOMMatrix and the native draw CTM can round differently.
 const canvas=document.createElement('canvas');canvas.width=640;canvas.height=360;
 const context=canvas.getContext('2d');need(context,'overlay real Canvas2D');
 const paths=[],matrices=[];let current=null;
 for(const name of ['beginPath','moveTo','lineTo']){
  const original=context[name].bind(context);
  context[name]=(...args)=>{if(name==='beginPath'){current=[];paths.push(current);}
   else if(current)current.push([args[0],args[1]]);return original(...args);};
 }
 const nativeDrawOverlay=node.drawOverlay;
 node.drawOverlay=function(ctx,matrix){matrices.push([matrix.a,matrix.b,matrix.c,matrix.d,matrix.e,matrix.f]);return nativeDrawOverlay.call(this,ctx,matrix);};
 try{
  if(drawMatrix===null)scene.drawOverlay(node.key,new DOMMatrix(),context);
  else node.drawOverlay(context,new DOMMatrix(drawMatrix));
 }finally{node.drawOverlay=nativeDrawOverlay;}
 const corners=paths[2];need(corners&&corners.length>=4,'native overlay layout polygon unavailable');
 const xs=corners.map(p=>p[0]),ys=corners.map(p=>p[1]);
 const x=Math.min(...xs),y=Math.min(...ys);
 return {box:{x,y,width:Math.max(...xs)-x,height:Math.max(...ys)-y},matrices};
}
window.runFocal=async({name,mode,arm})=>{
 const loaded=await Promise.all([400,700].map(weight=>document.fonts.load(weight+' 24px "Instrument Sans Variable"')));
 await document.fonts.ready;need(loaded.every(x=>x.length>0),'real bundled font not loaded');
 globalThis.__SEMWRIGHT_TEXT_DIGEST__=text=>digest(new TextEncoder().encode(text));
 globalThis.__SEMWRIGHT_NATIVE_PROBE__=undefined;
 const data=dataFor(name),module=modules[mode][arm],logger=new Logger();
 const playback=new PlaybackManager();playback.fps=30;playback.state=PlaybackState.Rendering;
 const webgl=new SharedWebGLContext(logger);
 const scene=new Scene2D({...module.createAuthoringScene(data,{}),name:data.id,
  logger,playback:new PlaybackStatus(playback),size:new Vector2(640,360),resolutionScale:1,
  timeEventsClass:ReadOnlyTimeEvents,sharedWebGLContext:webgl,experimentalFeatures:false});
 try{
  await scene.reset();
  for(let count=0;!globalThis.__SEMWRIGHT_NATIVE_PROBE__&&count<8;count++){
   playback.frame++;await scene.next();
  }
  need(typeof globalThis.__SEMWRIGHT_NATIVE_PROBE__==='function','actual authoring probe not initialized');
  const reg=module.__focalRegistrations.get(data.id);need(reg,'original scene registration missing');
  const node=reg.nodes.get('text');need(node,'original logical text node missing');
  // Fixture-authored transforms are applied before either baseline or observed draw.
  scene.execute(()=>{
   if(name==='transformed'){reg.nodes.get('parent').rotation(17);reg.nodes.get('parent').scale([1.2,.8]);node.offset([.5,-.25]);}
   if(name==='camera'){reg.nodes.get('parent').position([13,-9]);reg.nodes.get('parent').rotation(11);}
   if(name==='cached')reg.nodes.get('parent').cache(true);
  });
  const canvas=document.createElement('canvas');canvas.width=640;canvas.height=360;
  const context=canvas.getContext('2d',{alpha:true,colorSpace:'srgb'});need(context,'real native Canvas2D unavailable');
  // Observe the native Txt.draw entry after Node.render has transformed Canvas.
  // This oracle never reads reg.draws, probe geometry, or localToParent.
  const nativeDraw=node.draw,ownDraw=Object.getOwnPropertyDescriptor(node,'draw'),drawCalls=[];
  node.draw=function(ctx){
   const m=ctx.getTransform();const call={matrix:[m.a,m.b,m.c,m.d,m.e,m.f],
    canvas_is_scene:ctx.canvas===canvas,canvas_size:[ctx.canvas.width,ctx.canvas.height],completed:false};
   drawCalls.push(call);const value=nativeDraw.call(this,ctx);call.completed=true;return value;
  };
  try{await scene.render(context);}finally{
   if(ownDraw)Object.defineProperty(node,'draw',ownDraw);else delete node.draw;
  }
  const pixels=context.getImageData(0,0,640,360).data;
  const before=scene.execute(()=>signals(node));
  globalThis.__SEMWRIGHT_NATIVE_FRAME_CLOCK__={frame:playback.frame,fps_num:30,fps_den:1};
  const probe=await globalThis.__SEMWRIGHT_NATIVE_PROBE__(canvas,playback.frame,
   {render_input_digest:await digest(new TextEncoder().encode(JSON.stringify(data))),native_stage_version:'3.17.2',font_evidence:[]});
  const after=scene.execute(()=>signals(node));
  need(JSON.stringify(before)===JSON.stringify(after),'probe changed native signals');
  const row=probe.subjects.find(x=>x.id==='text');need(row,'original serialized text observation missing');
  const rect=node.element.getBoundingClientRect();
  const overlay=scene.execute(()=>nativeOverlayBox(scene,node));
  const drawOverlay=drawCalls.length===1&&drawCalls[0].completed
   ?scene.execute(()=>nativeOverlayBox(scene,node,drawCalls[0].matrix)):null;
  return {name,mode,arm,frame:playback.frame,pixel_sha256:await digest(pixels),
   actual_canvas:context.getContextAttributes(),native_dom:{connected:node.element.isConnected,width:rect.width,height:rect.height},
   signals:after,overlay_box:overlay.box,overlay_matrices:overlay.matrices,
   native_draw_calls:drawCalls,native_draw_overlay:drawOverlay,probe,subject:row,
   native_ink_pixels:pixels.filter((_,i)=>i%4===3&&pixels[i]>0).length,
   root_native_class:node.constructor.name,actual_draw_observed:reg.rendered.has('text'),
   cache_canvas_observed:!!reg.draws.get('text')&&reg.draws.get('text').canvas!==reg.canvas};
 }finally{scene.getView().dispose();webgl.dispose();}
};
`;

let browser, server, output, watchdog, cgroup, phase = 'arguments';
const result = {schema:'semwright.motion-native-layout-dom-focal/1', selected_cases:selected,
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
    phase = 'case:' + name; const arm=name==='enabled-red'?'RED':'GREEN'; const rows=[];
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
    const actualBox=after.subject.screen_layout_box;
    const geometryDelta=actualBox?.status==='known'
      ?Object.fromEntries(['x','y','width','height'].map(key=>[key,actualBox.value[key]-after.overlay_box[key]])):null;
    const actualMatrix=after.subject.transform;
    const matrixDelta=actualMatrix?.status==='known'&&after.overlay_matrices.length===1
      ?actualMatrix.value.map((value,index)=>value-after.overlay_matrices[0][index]):null;
    const drawGeometryDelta=actualBox?.status==='known'&&after.native_draw_overlay
      ?Object.fromEntries(['x','y','width','height'].map(key=>[key,actualBox.value[key]-after.native_draw_overlay.box[key]])):null;
    // Preserve actual rows and differences before any verdict, including failures.
    await retain(name+'.captured.original.json',{name,arm,original_native_results:rows,
      observed_minus_native_overlay_box:geometryDelta,observed_minus_native_overlay_matrix:matrixDelta,
      observed_minus_native_draw_overlay_box:drawGeometryDelta});
    assert.equal(before.pixel_sha256,after.pixel_sha256,'observer changed actual RGBA: '+name);
    assert.deepEqual(before.signals,after.signals,'observer changed native node signals: '+name);
    assert.deepEqual(before.native_draw_calls,after.native_draw_calls,'observer changed actual draw CTM: '+name);
    assert.deepEqual(before.native_draw_overlay,after.native_draw_overlay,'observer changed native draw overlay: '+name);
    assert.equal(before.frame,after.frame,'same native frame: '+name);
    assert(after.native_ink_pixels>0,'actual native ink missing: '+name);
    assert.equal(after.subject.native_type.status,'known');assert.equal(after.subject.native_type.value,'Txt');
    const legacy=structuredClone(after.probe);
    for(const row of legacy.subjects)for(const key of ['native_type','layout_root','screen_layout_box'])delete row[key];
    assert.deepEqual(legacy,before.probe,'old observations changed: '+name);
    if(name==='cached') {
      assert.equal(after.native_draw_calls.length,1,'one actual cached Txt.draw');
      assert(after.native_draw_calls[0].completed&&!after.native_draw_calls[0].canvas_is_scene&&after.cache_canvas_observed,'actual offscreen cache draw required');
      assert.equal(after.subject.screen_layout_box.status,'unknown','cache transform must remain unknown');
    } else {
      assert(after.native_dom.connected&&after.native_dom.width>0&&after.native_dom.height>0,'real native DOM dimensions');
      assert.equal(after.subject.layout_root.status,'known');
      assert.equal(after.subject.layout_root.value,after.signals.layout_root,'actual Layout.isLayoutRoot');
      assert.equal(after.subject.layout_root.value,name!=='enabled-red','real RED/GREEN layout root distinction');
      assert.equal(after.subject.screen_layout_box.status,'known');
      assert.equal(after.native_draw_calls.length,1,'one actual native Txt.draw');
      assert(after.native_draw_calls[0].completed&&after.native_draw_calls[0].canvas_is_scene,'completed draw on the admitted scene canvas');
      assert.deepEqual(after.native_draw_calls[0].canvas_size,[640,360],'native draw canvas dimensions');
      assert(after.native_draw_overlay,'actual native draw overlay unavailable');
      for(const key of ['x','y','width','height'])assert(Math.abs(after.subject.screen_layout_box.value[key]-after.native_draw_overlay.box[key])<1e-5,'native draw overlay mismatch '+name+':'+key);
      assert(Math.abs(after.native_dom.width-after.signals.size[0])<1e-5&&Math.abs(after.native_dom.height-after.signals.size[1])<1e-5,'size getter differs from real DOM');
    }
    const caseResult={name,arm,status:'REAL_DOM_FOCAL_PASSED',original_native_results:rows};
    result.cases.push(caseResult);await retain(name+'.original.json',caseResult);
    process.stdout.write(JSON.stringify({case:name,status:caseResult.status,pixel_sha256:after.pixel_sha256})+'\n');
  }
  assert.equal(sha(await fs.readFile(observedPath)),OBSERVED.GREEN,'producer source changed');
  assert.equal(sha(await fs.readFile(path.join(runtime,'package-lock.json'))),result.dependency_lock_sha256,'lock changed');
  result.status='SELECTED_REAL_DOM_FOCALS_PASSED';
  result.cgroup_memory_peak_bytes=Number((await fs.readFile(path.join(cgroup,'memory.peak'),'utf8')).trim());
  result.production_frames_rendered=0;result.focal_actual_Canvas2D_draws=result.cases.length*2;
  await retain('terminal.original.json',result);
  process.stdout.write(JSON.stringify({status:result.status,cases:result.cases.length,G2_accepted:false,F_accepted:false})+'\n');
} catch (error) {
  result.status='REAL_DOM_FOCAL_FAILED';result.phase=phase;
  result.error={name:error?.name??'Error',message:String(error?.message??error).slice(0,2000)};
  if(output)try{await retain('terminal.original.json',result);}catch{}
  process.stderr.write(JSON.stringify({status:result.status,phase,error:result.error,automatic_retry:false})+'\n');
  process.exitCode=1;
} finally {
  if(watchdog)clearInterval(watchdog);
  if(browser)await browser.close();
  if(server)await new Promise(resolve=>server.close(resolve));
}
