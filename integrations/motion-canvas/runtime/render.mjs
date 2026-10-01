import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import process from 'node:process';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {build} from 'vite';
import motionCanvasModule from '@motion-canvas/vite-plugin';
import {firefox} from 'playwright';

const motionCanvas = typeof motionCanvasModule === 'function' ? motionCanvasModule : motionCanvasModule.default;
function fail(message) { throw new Error(message); }
function unicodeRanges(css) {
  const ranges=[];
  for(const match of css.matchAll(/unicode-range:\s*([^;]+);/gi)){
    for(const token of match[1].split(',').map(x=>x.trim())){
      const value=token.replace(/^U\+/i,'');
      let start;let end;
      if(value.includes('?')){start=parseInt(value.replaceAll('?','0'),16);end=parseInt(value.replaceAll('?','F'),16);}
      else if(value.includes('-')){const parts=value.split('-',2);start=parseInt(parts[0],16);end=parseInt(parts[1],16);}
      else{start=parseInt(value,16);end=start;}
      if(!Number.isSafeInteger(start)||!Number.isSafeInteger(end)||start<0||end<start||end>0x10ffff)fail('invalid pinned font unicode-range');
      ranges.push([start,end]);
    }
  }
  return ranges;
}
async function pinnedFontEvidence(runtimeRoot){
  const specs=[
    {family:'Instrument Sans Variable',css:'node_modules/@fontsource-variable/instrument-sans/index.css',weights:[100,200,300,400,500,600,700,800,900]},
    {family:'IBM Plex Mono',css:'node_modules/@fontsource/ibm-plex-mono/400.css',weights:[400]},
  ];
  const evidence=[];
  for(const spec of specs){
    const cssPath=path.join(runtimeRoot,spec.css);const css=await fs.readFile(cssPath,'utf8');
    const refs=[...css.matchAll(/url\((?:['"]?)([^)'"]+\.woff2)(?:['"]?)\)/g)].map(m=>m[1]);
    if(refs.length===0)fail(`pinned font ${spec.family} exposes no WOFF2 files`);
    const hash=createHash('sha256');hash.update(Buffer.from(spec.css+'\n','utf8'));hash.update(Buffer.from(css,'utf8'));
    for(const rel of [...new Set(refs)].sort()){
      if(rel.includes('..')||path.isAbsolute(rel)||rel.includes('\\'))fail('invalid pinned font path');
      const bytes=await fs.readFile(path.resolve(path.dirname(cssPath),rel));
      if(bytes.length===0||bytes.length>16*1024*1024)fail('pinned font file exceeds bounds');
      hash.update(Buffer.from(rel+'\n','utf8'));hash.update(bytes);
    }
    const codepoints=unicodeRanges(css);if(codepoints.length===0)fail(`pinned font ${spec.family} has no Unicode coverage`);
    evidence.push({family:spec.family,sha256:hash.digest('hex'),weights:spec.weights,codepoints,face_loaded:false});
  }
  return evidence;
}
function args() {
  const out = {};
  for (let i = 2; i < process.argv.length; i += 2) {
    const key = process.argv[i]; const value = process.argv[i + 1];
    if (!key?.startsWith('--') || value === undefined) fail('invalid arguments');
    out[key.slice(2)] = value;
  }
  for (const key of ['project', 'output', 'config', 'browser']) if (!out[key]) fail(`missing --${key}`);
  return out;
}
function harnessPlugin(config, entry) {
  const id = '\0semwright-render-entry';
  return {
    name: 'semwright:controlled-render-harness',
    enforce: 'post',
    config() { return {build:{rollupOptions:{input:entry}}}; },
    resolveId(source) { if (source === 'virtual:semwright-render') return id; },
    load(source) {
      if (source !== id) return;
      return `
import project from '/src/project.ts?project';
import {Renderer, Vector2} from '@motion-canvas/core';
const config=${JSON.stringify(config)};
const renderer=new Renderer(project);
const state={done:false,result:null,frame:config.firstFrame,error:null,errorClass:null,rendererLogClass:null,typeErrorDetail:null,phase:'created'};
function classifyAuthoringMessage(message){
  if(typeof message!=='string'||message.length===0)return null;
  if(/^(invalid align|unsupported subject kind|unannounced overlap|overlay anchor unavailable|split requires exactly two layout children|unknown archetype|duplicate logical id|unknown layer|native parent graph cannot be resolved|annotation binding missing|node limit|invalid rational|unsafe color|native scene bounds|caption requires native Txt)$/.test(message))return 'authoring_model';
  if(/^(non-finite |width requires layout|height requires layout|line start requires Line|line end requires Line|font size requires Layout|tracking requires Layout|fill requires shape|zoom requires Camera|vector operand required|unknown easing|connection requires Line|trace requires native Line|follow requires Camera|incompatible morph topology|selection requires Code|counter requires text|region requires Layout)/.test(message))return 'authoring_model';
  if(/^missing subject /.test(message)||/^original value unavailable: /.test(message))return 'authoring_model';
  if(message==='native signal not available'||message==='stage compositor baseline changed'||message==='native frame clock not supplied by exporter'||message==='native authoring probe unavailable'||message==='Semwright exporter binding unavailable'||message==='asset must be a generated local import')return 'authoring_protocol';
  if(message==='Failed to initialize WebGL.'||message==='Failed to initialize the shader program.'||message==='Unknown shader compilation error.')return 'webgl_unavailable';
  if(message==='PlaybackManager has not been properly initialized')return 'playback_protocol';
  if(message==='Invalid scene.')return 'invalid_scene';
  return null;
}
function classifyLogStack(payload){
  const stack=typeof payload?.stack==='string'?payload.stack:'';
  // Vite development/build stack URLs may preserve source paths or rewrite scoped
  // package names into dependency chunk names. Only classify to an allowlisted
  // module family; never surface the URL, frame, line, stack, or message.
  if(stack.includes('semwright-authoring-native')||stack.includes('/src/semwright-authoring-native'))return 'semwright_native';
  if(stack.includes('semwright-exporter')||stack.includes('/src/semwright-exporter'))return 'semwright_exporter';
  if(stack.includes('@motion-canvas/core')||stack.includes('@motion-canvas_core'))return 'motion_core';
  if(stack.includes('@motion-canvas/2d')||stack.includes('@motion-canvas_2d'))return 'motion_2d';
  return null;
}
function classifyTypeErrorDetail(message){
  if(typeof message!=='string')return null;
  let match=/^Cannot read properties of (?:undefined|null) \(reading '([A-Za-z_$][A-Za-z0-9_$]{0,63})'\)$/.exec(message);
  if(match)return 'read:'+match[1];
  match=/^Cannot set properties of (?:undefined|null) \(setting '([A-Za-z_$][A-Za-z0-9_$]{0,63})'\)$/.exec(message);
  if(match)return 'set:'+match[1];
  match=/^([A-Za-z_$][A-Za-z0-9_$]{0,63}) is not a function$/.exec(message);
  if(match)return 'not_function:'+match[1];
  if(message.includes(' is not iterable'))return 'not_iterable';
  if(message==='Cannot convert undefined or null to object')return 'null_object';
  return 'other';
}
function classifyRendererLog(payload){
  if(!payload||payload.level!=='error')return null;
  const name=typeof payload.name==='string'?payload.name:'';
  const message=typeof payload.message==='string'?payload.message:'';
  const authoring=classifyAuthoringMessage(message);
  if(authoring==='authoring_model')return 'renderer_log_authoring_model';
  if(authoring==='authoring_protocol')return 'renderer_log_authoring_protocol';
  if(authoring==='webgl_unavailable')return 'renderer_log_webgl_unavailable';
  if(authoring==='playback_protocol')return 'renderer_log_playback_protocol';
  if(authoring==='invalid_scene')return 'renderer_log_invalid_scene';
  if(name==='TypeError'){
    state.typeErrorDetail=classifyTypeErrorDetail(message);
    const origin=classifyLogStack(payload);
    if(origin==='semwright_native')return 'renderer_state_semwright_native';
    if(origin==='semwright_exporter')return 'renderer_state_semwright_exporter';
    if(origin==='motion_core')return 'renderer_state_motion_core';
    if(origin==='motion_2d')return 'renderer_state_motion_2d';
    return 'renderer_log_type_error';
  }
  if(name==='RangeError')return 'renderer_log_range_error';
  if(message.startsWith('Could not find the \"')&&message.endsWith('\" exporter.'))return 'renderer_log_exporter_missing';
  if(message.includes('Tried to access an asynchronous property before the node was ready.'))return 'renderer_log_async_property';
  return 'renderer_log_error';
}
function classifyRendererStack(error){
  const stack=typeof error?.stack==='string'?error.stack:'';
  if(stack.includes('semwright-authoring-native'))return 'renderer_state_semwright_native';
  if(stack.includes('semwright-exporter'))return 'renderer_state_semwright_exporter';
  if(stack.includes('@motion-canvas/core'))return 'renderer_state_motion_core';
  if(stack.includes('@motion-canvas/2d'))return 'renderer_state_motion_2d';
  return null;
}
project.logger.onLogged.subscribe(payload=>{const classified=classifyRendererLog(payload);if(classified)state.rendererLogClass=classified;});
if(config.authoring){globalThis.__SEMWRIGHT_NATIVE_CONFIG__={fps_num:config.fpsNum,fps_den:config.fpsDen,render_input_digest:config.renderInputDigest,native_stage_version:'3.17.2',font_evidence:config.fontEvidence??[]};}
window.__SEMWRIGHT_RENDER__={state,abort:()=>renderer.abort()};
renderer.onFrameChanged.subscribe(frame=>{state.frame=frame;state.phase='frame';});
renderer.onFinished.subscribe(result=>{state.result=result;});
(async()=>{
  try {
    if(config.authoring){
      await document.fonts.ready;
      const binding=globalThis.__SEMWRIGHT_NATIVE_CONFIG__;
      binding.font_evidence=binding.font_evidence.map(face=>({...face,face_loaded:face.weights.some(weight=>document.fonts.check(String(weight)+' 16px "'+face.family+'"'))}));
    }
    state.phase='rendering';
    await renderer.render({
      name:'frames',
      size:new Vector2(config.width,config.height),
      resolutionScale:1,
      colorSpace:config.colorSpace,
      background:config.alpha?null:config.background,
      // Motion Canvas 3.17.2 treats the range end as inclusive; Semwright profiles are half-open.
      range:[config.firstFrame/config.fps,(config.endFrameExclusive-1)/config.fps],
      fps:config.fps,
      exporter:{name:'@semwright/driver/image-sequence',options:{}},
    });
    state.phase='finished';
    state.done=true;
  } catch(error) {
    const text=String(error?.message ?? error ?? '');
    let errorClass='renderer_state_error';
    const authoring=classifyAuthoringMessage(text);
    if(authoring==='authoring_model')errorClass='renderer_state_authoring_model';
    else if(authoring==='authoring_protocol')errorClass='renderer_state_authoring_protocol';
    else if(authoring==='webgl_unavailable')errorClass='renderer_state_webgl_unavailable';
    else if(authoring==='playback_protocol')errorClass='renderer_state_playback_protocol';
    else if(authoring==='invalid_scene')errorClass='renderer_state_invalid_scene';
    else if(error?.name==='TypeError'){state.typeErrorDetail=classifyTypeErrorDetail(text);errorClass='renderer_state_type_error';}
    else if(error?.name==='RangeError')errorClass='renderer_state_range_error';
    else {
      const stackClass=classifyRendererStack(error);
      if(stackClass)errorClass=stackClass;
      else if(state.frame<=config.firstFrame)errorClass='renderer_state_before_first_frame';
      else errorClass='renderer_state_after_first_frame';
    }
    state.error=String(error);state.errorClass=errorClass;state.phase='error';state.done=true;
  }
})();
`;
    },
  };
}
function contentType(file) {
  const ext = path.extname(file).toLowerCase();
  return ({'.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.mjs':'text/javascript; charset=utf-8','.css':'text/css; charset=utf-8','.json':'application/json','.svg':'image/svg+xml','.png':'image/png','.jpg':'image/jpeg','.jpeg':'image/jpeg','.woff2':'font/woff2','.woff':'font/woff','.mp4':'video/mp4','.webm':'video/webm','.wav':'audio/wav','.mp3':'audio/mpeg'})[ext] || 'application/octet-stream';
}
let failurePhase = 'startup';
let failureDetail = null;
async function main() {
  failurePhase = 'arguments';
  const a = args();
  const runtimeRoot = path.dirname(fileURLToPath(import.meta.url));
  const config = JSON.parse(Buffer.from(a.config, 'base64url').toString('utf8'));
  failurePhase = 'font_evidence';
  if(config.authoring) config.fontEvidence=await pinnedFontEvidence(runtimeRoot);
  const project = path.resolve(a.project); const output = path.resolve(a.output);
  const work = await fs.mkdtemp(path.join(os.tmpdir(), 'semwright-motion-render-'));
  const dist = path.join(work, '.semwright-render-dist');
  let context; let page; let cancelling = false;
  const diagnostics = [];
  const note = (kind, message) => { if (diagnostics.length < 32) diagnostics.push({kind,message:String(message).slice(0,512)}); };
  const cleanup = async () => { try { await context?.close(); } catch {} await fs.rm(work, {recursive:true,force:true}).catch(()=>{}); };
  const cancel = async () => { if (cancelling) return; cancelling = true; try { await page?.evaluate(() => window.__SEMWRIGHT_RENDER__?.abort()); } catch {} await cleanup(); process.exitCode = 130; };
  process.once('SIGTERM', cancel); process.once('SIGINT', cancel);
  try {
    failurePhase = 'project_stage';
    await fs.cp(project, work, {recursive:true,dereference:false,errorOnExist:false});
    await fs.rm(path.join(work, 'node_modules'), {recursive:true,force:true});
    await fs.symlink(path.join(runtimeRoot, 'node_modules'), path.join(work, 'node_modules'), 'dir');
    await fs.writeFile(path.join(work, 'semwright-render.html'), '<!doctype html><meta charset="utf-8"><script type="module" src="/semwright-entry.js"></script>');
    await fs.writeFile(path.join(work, 'semwright-entry.js'), "import 'virtual:semwright-render';\n");
    const projectEntry=path.join(work,'src/project.ts');
    const renderEntry=path.join(work,'semwright-render.html');
    failurePhase = 'vite_build';
    await build({root:work,configFile:false,logLevel:'error',base:'/',plugins:[motionCanvas({project:projectEntry,editor:path.join(runtimeRoot,'stub-editor/main.js'),buildForEditor:true}),harnessPlugin(config,renderEntry)],build:{outDir:dist,emptyOutDir:true,rollupOptions:{input:renderEntry}}});
    failurePhase = 'frame_export';
    await fs.mkdir(path.join(output, 'frames'), {recursive:true});
    if (process.env.SEMWRIGHT_DRIVER_SANDBOX !== 'landlock-bwrap-v1') fail('renderer requires the Semwright Driver Host sandbox');
    const profile = path.join(work, '.semwright-firefox-profile');
    failurePhase = 'browser_launch';
    context = await firefox.launchPersistentContext(profile, {
      headless:true,
      executablePath:a.browser,
      viewport:{width:config.width,height:config.height},
      serviceWorkers:'block',
      firefoxUserPrefs:{'dom.ipc.forkserver.enable':false},
      env:{...process.env,MOZ_ASSUME_USER_NS:'0',MOZ_DISABLE_CONTENT_SANDBOX:'1'},
    });
    page = context.pages()[0];
    if (!page) fail('Firefox persistent context exposed no startup page');
    page.on('pageerror', error => note('pageerror', error));
    page.on('console', message => { if (['error','warning'].includes(message.type())) note(`console:${message.type()}`, message.text()); });
    const written = new Set();
    let observationBytes=0;
    const observationHash=createHash('sha256');
    let observationCount=0;
    if(config.authoring) await fs.writeFile(path.join(output,'native-observations.ndjson'),'',{flag:'wx'});
    await page.exposeBinding('__SEMWRIGHT_EXPORT_FRAME__', async (_source, payload) => {
      if (!payload || !Number.isSafeInteger(payload.frame) || payload.frame < config.firstFrame || payload.frame >= config.endFrameExclusive || typeof payload.data !== 'string' || !payload.data.startsWith('data:image/png;base64,')) fail('invalid frame payload');
      if (written.has(payload.frame)) fail('duplicate frame payload');
      const bytes = Buffer.from(payload.data.slice('data:image/png;base64,'.length), 'base64');
      if (bytes.length < 8 || bytes.length > 32 * 1024 * 1024 || bytes.subarray(0,8).toString('hex') !== '89504e470d0a1a0a') fail('invalid PNG payload');
      written.add(payload.frame);
      if(config.authoring){
        if(!payload.observation||payload.observation.frame!==payload.frame)fail('native observation/frame mismatch');
        const line=Buffer.from(JSON.stringify(payload.observation)+'\n','utf8');
        observationBytes+=line.length;
        if(line.length>524288||observationBytes>64*1024*1024)fail('native observation byte budget exceeded');
        observationHash.update(line);observationCount++;
        await fs.appendFile(path.join(output,'native-observations.ndjson'),line);
      }
      await fs.writeFile(path.join(output, 'frames', `${String(payload.frame).padStart(6,'0')}.png`), bytes, {flag:'wx'});
    });
    await page.route('**/*', async route => {
      const url = new URL(route.request().url());
      if (url.hostname !== 'semwright.invalid') return route.abort('blockedbyclient');
      let rel = decodeURIComponent(url.pathname.replace(/^\/+/, '')) || 'semwright-render.html';
      if (rel.includes('..') || rel.includes('\\') || path.isAbsolute(rel)) return route.abort('blockedbyclient');
      const file = path.resolve(dist, rel);
      if (!file.startsWith(path.resolve(dist) + path.sep) && file !== path.resolve(dist, 'semwright-render.html')) return route.abort('blockedbyclient');
      try { const body = await fs.readFile(file); await route.fulfill({status:200,body,contentType:contentType(file)}); }
      catch { await route.fulfill({status:404,body:'not found',contentType:'text/plain'}); }
    });
    failurePhase = 'page_load';
    await page.goto('http://semwright.invalid/semwright-render.html', {waitUntil:'domcontentloaded',timeout:config.timeoutMs});
    failurePhase = 'render_wait';
    try {
      await page.waitForFunction(() => window.__SEMWRIGHT_RENDER__?.state?.done === true, undefined, {timeout:config.timeoutMs});
    } catch (error) {
      const state = await page.evaluate(() => window.__SEMWRIGHT_RENDER__?.state ?? null).catch(()=>null);
      failurePhase='render_wait_timeout';
      fail(`render wait failed: ${error}; state=${JSON.stringify(state)} diagnostics=${JSON.stringify(diagnostics)}`);
    }
    const state = await page.evaluate(() => window.__SEMWRIGHT_RENDER__.state);
    if (state.error) {
      const allowedStateClasses=new Set(['renderer_state_authoring_model','renderer_state_authoring_protocol','renderer_state_webgl_unavailable','renderer_state_playback_protocol','renderer_state_invalid_scene','renderer_state_type_error','renderer_state_range_error','renderer_state_semwright_native','renderer_state_semwright_exporter','renderer_state_motion_core','renderer_state_motion_2d','renderer_state_before_first_frame','renderer_state_after_first_frame','renderer_state_error']);
      const allowedLogClasses=new Set(['renderer_log_authoring_model','renderer_log_authoring_protocol','renderer_log_webgl_unavailable','renderer_log_playback_protocol','renderer_log_invalid_scene','renderer_log_type_error','renderer_state_semwright_native','renderer_state_semwright_exporter','renderer_state_motion_core','renderer_state_motion_2d','renderer_log_range_error','renderer_log_exporter_missing','renderer_log_async_property','renderer_log_error']);
      const stateClass=allowedStateClasses.has(state.errorClass)?state.errorClass:'renderer_state_error';
      const logClass=allowedLogClasses.has(state.rendererLogClass)?state.rendererLogClass:null;
      failurePhase=stateClass==='renderer_state_error'&&logClass?logClass:stateClass;
      failureDetail=typeof state.typeErrorDetail==='string'?state.typeErrorDetail:null;
      fail(`renderer failed: ${state.error}`);
    }
    if (state.result !== 0) {
      if(state.result===2)failurePhase='render_result_aborted';
      else if(state.result===1)failurePhase=state.rendererLogClass??'render_result_error';
      else failurePhase='render_result_unknown';
      failureDetail=typeof state.typeErrorDetail==='string'?state.typeErrorDetail:null;
      fail(`renderer result ${state.result}; state=${JSON.stringify(state)} diagnostics=${JSON.stringify(diagnostics)}`);
    }
    failurePhase = 'observation';
    if(config.authoring){
      if(observationCount!==config.endFrameExclusive-config.firstFrame)fail('native observation count incomplete');
      await fs.writeFile(path.join(output,'native-observations-receipt.json'),JSON.stringify({version:1,render_input_digest:config.renderInputDigest,font_resources_sha256:config.fontResourcesDigest,sha256:observationHash.digest('hex'),bytes:observationBytes,frames:observationCount,fps_num:config.fpsNum,fps_den:config.fpsDen}),{flag:'wx'});
    }
    failurePhase = 'finalize';
    const files = (await fs.readdir(path.join(output,'frames'))).sort();
    process.stdout.write(JSON.stringify({ok:true,renderer:'motion-canvas-core-renderer-v3.17.2-firefox',lastFrame:state.frame,files:files.map(file=>`frames/${file}`)})+'\n');
  } finally { await cleanup(); }
}
main().catch(error => {
  const allowed = new Set(['arguments','font_evidence','project_stage','vite_build','frame_export','browser_launch','page_load','render_wait','render_wait_timeout','renderer_state_authoring_model','renderer_state_authoring_protocol','renderer_state_webgl_unavailable','renderer_state_playback_protocol','renderer_state_invalid_scene','renderer_state_type_error','renderer_state_range_error','renderer_state_semwright_native','renderer_state_semwright_exporter','renderer_state_motion_core','renderer_state_motion_2d','renderer_state_before_first_frame','renderer_state_after_first_frame','renderer_state_error','renderer_log_authoring_model','renderer_log_authoring_protocol','renderer_log_webgl_unavailable','renderer_log_playback_protocol','renderer_log_invalid_scene','renderer_log_type_error','renderer_log_range_error','renderer_log_exporter_missing','renderer_log_async_property','renderer_log_error','render_result_error','render_result_aborted','render_result_unknown','render_nonzero','observation','finalize']);
  const errorClass = allowed.has(failurePhase) ? failurePhase : 'startup';
  process.stdout.write(JSON.stringify({ok:false,errorClass,detail:failureDetail})+'\n');
  process.stderr.write(String(error?.stack || error) + '\n');
  process.exitCode = 1;
});
