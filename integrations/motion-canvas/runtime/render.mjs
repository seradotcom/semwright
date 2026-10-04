import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import process from 'node:process';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
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
  const required = ['project-root', 'project-relative', 'output-root', 'output-relative', 'fontconfig-root', 'config'];
  for (const key of required) if (!out[key]) fail(`missing --${key}`);
  if (Object.keys(out).some(key => !required.includes(key))) fail('unknown argument');
  return out;
}
function safeRelative(value, label) {
  if (!value || value.includes('\\') || path.isAbsolute(value)) fail(`invalid ${label}`);
  const parts = value.split('/');
  if (parts.some(part => !part || part === '.' || part === '..')) fail(`invalid ${label}`);
  return parts;
}
async function childOf(rootValue, relative, label) {
  const root = await fs.realpath(rootValue);
  const target = await fs.realpath(path.join(root, ...safeRelative(relative, label)));
  const relation = path.relative(root, target);
  if (!relation || relation.startsWith('..' + path.sep) || relation === '..' || path.isAbsolute(relation)) {
    fail(`${label} escapes owner-granted root`);
  }
  return {root, target};
}
async function containedFile(root, candidate, label) {
  const canonicalRoot = await fs.realpath(root);
  const canonical = await fs.realpath(candidate);
  const relation = path.relative(canonicalRoot, canonical);
  if (!relation || relation.startsWith('..' + path.sep) || relation === '..' || path.isAbsolute(relation)) {
    fail(`${label} escapes runtime bundle`);
  }
  const stat = await fs.stat(canonical);
  if (!stat.isFile()) fail(`${label} is not a file`);
  return canonical;
}

// Integer frame clock and strict singleton boundary for Motion Canvas 3.17.2.
function stableFrameSeconds(frame, fps) {
  if (!Number.isSafeInteger(frame) || frame < 0 || !Number.isFinite(fps) || fps <= 0) throw new Error('invalid integer frame clock');
  // An interior representative maps to the same integer under core's Math.ceil.
  const seconds = frame === 0 ? 0 : (frame - 0.25) / fps;
  if (!Number.isFinite(seconds) || Math.ceil(seconds * fps) !== frame) throw new Error('integer frame clock roundtrip mismatch');
  return seconds;
}
function installSingletonTailGuard(exporterClass, config, observed) {
  const expected = config.endFrameExclusive - config.firstFrame;
  if (expected !== 1) return;
  if (!exporterClass || typeof exporterClass.create !== 'function') throw new Error('Semwright exporter binding unavailable');
  const originalCreate = exporterClass.create;
  exporterClass.create = async function(...createArguments) {
    const exporter = await originalCreate.apply(this, createArguments);
    if (!exporter || typeof exporter.handleFrame !== 'function') throw new Error('Semwright exporter binding unavailable');
    const originalHandleFrame = exporter.handleFrame;
    const accepted = new Set();
    let tailFiltered = false;
    exporter.handleFrame = async function(canvas, frame, sceneFrame, sceneName, signal) {
      // Core3.17.2 always progresses/exports once before its >=to check.
      // Drop exactly that singleton tail, only after the requested frame was accepted.
      if (!signal.aborted && Number.isSafeInteger(frame) && frame === config.endFrameExclusive
          && accepted.size === expected && accepted.has(config.firstFrame) && !tailFiltered) {
        tailFiltered = true;
        observed.singletonTailFiltered = true;
        return;
      }
      // Every other frame still reaches the unchanged strict HOST binding.
      await originalHandleFrame.call(this, canvas, frame, sceneFrame, sceneName, signal);
      if (!signal.aborted) accepted.add(frame);
    };
    return exporter;
  };
}
function boundedRendererLog(payload) {
  const truncate = (value, maximum) => {
    if (typeof value !== 'string') return '';
    const encoder = new TextEncoder(), decoder = new TextDecoder();
    let text = decoder.decode(encoder.encode(value.slice(0, maximum)).subarray(0, maximum));
    while (encoder.encode(text).length > maximum) text = text.slice(0, -1);
    return text;
  };
  // Error fields are commonly non-enumerable: read these scalars explicitly.
  const read = key => { try { return typeof payload?.[key] === 'string' ? payload[key] : ''; } catch { return ''; } };
  const diagnostic = {name:truncate(read('name'),128),message:truncate(read('message'),2048),stack:truncate(read('stack'),8192)};
  while (new TextEncoder().encode(JSON.stringify(diagnostic)).length > 12 * 1024) {
    if (diagnostic.stack.length) diagnostic.stack = diagnostic.stack.slice(0, Math.floor(diagnostic.stack.length / 2));
    else if (diagnostic.message.length) diagnostic.message = diagnostic.message.slice(0, Math.floor(diagnostic.message.length / 2));
    else break;
  }
  return diagnostic;
}

// BEGIN PINNED FONT READINESS V04 (browser API mock tests extract exactly these functions).
async function loadPinnedFontFaces(fonts, timeoutMs) {
  if (!fonts || typeof fonts.load !== 'function' || typeof fonts.check !== 'function'
      || !fonts.ready || typeof fonts.ready.then !== 'function' || !Number.isSafeInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 10000) {
    throw new Error('pinned font readiness API or deadline unavailable');
  }
  // These exact faces are imported by both compiler routes. CSS pins remain unchanged.
  // Instrument Sans index.css declares 400..700; IBM Plex Mono400.css declares 400.
  const specs = [
    {family:'Instrument Sans Variable',weights:[400,500,600,700]},
    {family:'IBM Plex Mono',weights:[400]},
  ];
  const sample = 'SEMWRIGHT 0123456789';
  const normalizeFamily = value => typeof value === 'string' ? value.replace(/^(['"])(.*)\1$/, '$2') : '';
  let timer;
  const deadline = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error('pinned font readiness deadline exceeded')), timeoutMs);
  });
  const work = (async () => {
    const requests = await Promise.all(specs.flatMap(spec => spec.weights.map(async weight => {
      const descriptor = String(weight) + ' 16px "' + spec.family + '"';
      const loaded = await fonts.load(descriptor, sample);
      if (!Array.isArray(loaded) || loaded.length === 0 || loaded.length > 16) {
        throw new Error('pinned font load returned an empty or unbounded face set');
      }
      const faces = loaded.map(face => {
        if (normalizeFamily(face.family) !== spec.family || face.status !== 'loaded'
            || typeof face.weight !== 'string' || face.weight.length > 64
            || face.style !== 'normal') {
          throw new Error('pinned font face readback differs');
        }
        if (!/^\d{1,4}(?: \d{1,4})?$/.test(face.weight)) throw new Error('pinned font weight readback differs');
        const range=face.weight.split(' ').map(Number);
        if (range[0] < 1 || range[range.length-1] > 1000 || range[0] > weight || range[range.length-1] < weight) {
          throw new Error('pinned font weight readback differs');
        }
        return {family:normalizeFamily(face.family),status:face.status,weight:face.weight,style:face.style};
      });
      if (fonts.check(descriptor, sample) !== true) throw new Error('pinned font check failed after load');
      return {family:spec.family,requested_weight:weight,loaded_face_count:faces.length,faces,check_after_load:true};
    })));
    await fonts.ready;
    if (fonts.status !== 'loaded') throw new Error('pinned font set remains pending');
    for (const request of requests) {
      const descriptor = String(request.requested_weight) + ' 16px "' + request.family + '"';
      if (fonts.check(descriptor, sample) !== true) throw new Error('pinned font check failed after ready');
      request.check_after_ready = true;
    }
    return {version:1,method:'actual browser FontFaceSet.load/ready/check and FontFace scalar readbacks',
      observed_before_renderer_render:true,sample,timeout_ms:timeoutMs,status_after_ready:fonts.status,requests};
  })();
  try { return await Promise.race([work, deadline]); }
  finally { clearTimeout(timer); }
}
// END PINNED FONT READINESS V04.

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
${stableFrameSeconds.toString()}
${installSingletonTailGuard.toString()}
${boundedRendererLog.toString()}
${loadPinnedFontFaces.toString()}
const state={done:false,result:null,frame:config.firstFrame,error:null,errorClass:null,rendererLogClass:null,rendererLogDiagnostic:null,typeErrorDetail:null,singletonTailFiltered:false,fontReadiness:null,phase:'created'};
const desiredRange=[stableFrameSeconds(config.firstFrame,config.fps),stableFrameSeconds(config.endFrameExclusive-1,config.fps)];
installSingletonTailGuard(project.meta.rendering.exporter.exporters.find(candidate=>candidate.id==='@semwright/driver/image-sequence'),config,state);
const renderer=new Renderer(project);
function classifyAuthoringMessage(message){
  if(typeof message!=='string'||message.length===0)return null;
  if(/^(invalid align|unsupported subject kind|unannounced overlap|overlay anchor unavailable|split requires exactly two layout children|unknown archetype|duplicate logical id|unknown layer|native parent graph cannot be resolved|annotation binding missing|node limit|invalid rational|unsafe color|native scene bounds|caption requires native Txt)$/.test(message))return 'authoring_model';
  if(/^(non-finite |width requires layout|height requires layout|line start requires Line|line end requires Line|font size requires Layout|tracking requires Layout|fill requires shape|zoom requires Camera|vector operand required|unknown easing|connection requires Line|trace requires native Line|follow requires Camera|incompatible morph topology|selection requires Code|counter requires text|region requires Layout)/.test(message))return 'authoring_model';
  if(/^missing subject /.test(message)||/^original value unavailable: /.test(message))return 'authoring_model';
  if(message==='native signal not available'||message==='stage compositor baseline changed'||message==='native frame clock not supplied by exporter'||message==='native authoring probe unavailable'||message==='native text digest binding unavailable'||message==='native text digest binding returned invalid hash'||message==='Semwright exporter binding unavailable'||message==='asset must be a generated local import')return 'authoring_protocol';
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
  if(message.length>512)return 'other';
  let match=/^Cannot read properties of (?:undefined|null) \(reading '([A-Za-z_$][A-Za-z0-9_$]{0,63})'\)$/.exec(message);
  if(match)return 'read:'+match[1];
  match=/^Cannot set properties of (?:undefined|null) \(setting '([A-Za-z_$][A-Za-z0-9_$]{0,63})'\)$/.exec(message);
  if(match)return 'set:'+match[1];
  match=/^(?:can't|Can't) access property ["']([A-Za-z_$][A-Za-z0-9_$]{0,63})["'], .+ is (?:undefined|null)$/.exec(message);
  if(match)return 'read:'+match[1];
  match=/^(?:can't|Can't) assign to property ["']([A-Za-z_$][A-Za-z0-9_$]{0,63})["'] on .*(?:undefined|null).*$/.exec(message);
  if(match)return 'set:'+match[1];
  match=/^([A-Za-z_$][A-Za-z0-9_$]{0,63}) is not a function$/.exec(message);
  if(match)return 'not_function:'+match[1];
  match=/\.([A-Za-z_$][A-Za-z0-9_$]{0,63}) is not a function$/.exec(message);
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
project.logger.onLogged.subscribe(payload=>{const classified=classifyRendererLog(payload);if(classified){state.rendererLogClass=classified;state.rendererLogDiagnostic=boundedRendererLog(payload);}});
if(config.authoring){globalThis.__SEMWRIGHT_NATIVE_CONFIG__={fps_num:config.fpsNum,fps_den:config.fpsDen,render_input_digest:config.renderInputDigest,native_stage_version:'3.17.2',font_evidence:config.fontEvidence??[]};}
window.__SEMWRIGHT_RENDER__={state,abort:()=>renderer.abort()};
renderer.onFrameChanged.subscribe(frame=>{state.frame=frame;state.phase='frame';});
renderer.onFinished.subscribe(result=>{state.result=result;});
(async()=>{
  try {
    state.phase='font_loading';
    state.fontReadiness=await loadPinnedFontFaces(document.fonts,Math.min(config.timeoutMs,10000));
    if(config.authoring){
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
      range:desiredRange,
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
// Keep local failure details bounded and separate from the public classification.
const DIAGNOSTIC_PHASES = new Set(['startup','arguments','runtime_module_load','font_evidence','project_stage','vite_build','frame_export','browser_launch','page_load','render_wait','render_wait_timeout','renderer_state_authoring_model','renderer_state_authoring_protocol','renderer_state_webgl_unavailable','renderer_state_playback_protocol','renderer_state_invalid_scene','renderer_state_type_error','renderer_state_range_error','renderer_state_semwright_native','renderer_state_semwright_exporter','renderer_state_motion_core','renderer_state_motion_2d','renderer_state_before_first_frame','renderer_state_after_first_frame','renderer_state_error','renderer_log_authoring_model','renderer_log_authoring_protocol','renderer_log_webgl_unavailable','renderer_log_playback_protocol','renderer_log_invalid_scene','renderer_log_type_error','renderer_log_range_error','renderer_log_exporter_missing','renderer_log_async_property','renderer_log_error','render_result_error','render_result_aborted','render_result_unknown','render_nonzero','observation','finalize']);
let failureOutput = null;
let failureModule = null;
const FAILURE_JSON_MAX_BYTES = 64 * 1024;
const FAILURE_STACK_MAX_BYTES = 16 * 1024;
function boundedUtf8(value, maximum) {
  let bounded = Buffer.from(String(value ?? ''), 'utf8').subarray(0, maximum).toString('utf8');
  while (Buffer.byteLength(bounded, 'utf8') > maximum) bounded = bounded.slice(0, -1);
  return bounded;
}
function failureReceipt(error, errorClass, binding, module) {
  if (!DIAGNOSTIC_PHASES.has(errorClass) || !/^[a-f0-9]{64}$/.test(binding)) fail('invalid finite failure classification or binding');
  const names = new Set(['Error','TypeError','RangeError','SyntaxError','AggregateError','TimeoutError']);
  const codes = new Set(['ERR_MODULE_NOT_FOUND','MODULE_NOT_FOUND','ERR_DLOPEN_FAILED','EACCES','EPERM','ENOMEM']);
  const modules = new Set(['vite','motion_canvas_vite_plugin','playwright']);
  const receipt = {version:2,ok:false,render_input_digest:binding,error_class:errorClass,
    runtime_module:modules.has(module)?module:null,
    exception_name:names.has(error?.name)?error.name:'OtherError',
    exception_code:codes.has(error?.code)?error.code:null,
    local_stack_only:boundedUtf8(error?.stack ?? error, FAILURE_STACK_MAX_BYTES)};
  let bytes = Buffer.from(JSON.stringify(receipt)+'\n', 'utf8');
  while (bytes.length > FAILURE_JSON_MAX_BYTES && receipt.local_stack_only.length) {
    receipt.local_stack_only = receipt.local_stack_only.slice(0, Math.floor(receipt.local_stack_only.length / 2));
    bytes = Buffer.from(JSON.stringify(receipt)+'\n', 'utf8');
  }
  if (bytes.length > FAILURE_JSON_MAX_BYTES) fail('native failure receipt exceeds byte budget');
  return bytes;
}
async function writeFailureReceipt(error, errorClass, selected=failureOutput, module=failureModule) {
  if (!selected) return false;
  if (!/^[a-f0-9]{64}$/.test(selected.binding) || !/^render-[a-f0-9]{32}$/.test(selected.relative)) {
    fail('invalid native failure output binding');
  }
  const checked = await childOf(selected.root, selected.relative, 'native failure output');
  if (checked.root !== selected.root || checked.target !== selected.target
      || checked.target !== path.join(checked.root, selected.relative)
      || !(await fs.lstat(checked.target)).isDirectory()) fail('native failure output alias');
  const bytes = failureReceipt(error,errorClass,selected.binding,module);
  const handle = await fs.open(path.join(checked.target,'native-failure-receipt.json'),'wx',0o600);
  try { await handle.writeFile(bytes); await handle.sync(); } finally { await handle.close(); }
  return true;
}

async function main() {
  failurePhase = 'arguments';
  const a = args();
  const config = globalThis.__SEMWRIGHT_RENDER_INPUT__;
  if(!config||config.renderInputDigest!==a.config||!/^[a-f0-9]{64}$/.test(a.config))fail('render input binding absent or changed');
  const {root: outputRoot,target: output} = await childOf(a['output-root'], a['output-relative'], 'output');
  if (!/^render-[a-f0-9]{32}$/.test(a['output-relative'])
      || output !== path.join(outputRoot,a['output-relative'])
      || !(await fs.lstat(output)).isDirectory()) fail('native render output is not an ordinary unique child');
  failureOutput = {root:outputRoot,target:output,relative:a['output-relative'],binding:a.config};
  const runtimeRoot = await fs.realpath(process.cwd());
  // Driver Host intentionally clears ambient environment. Pin Playwright's
  // browser registry to this owner-granted runtime bundle before importing it.
  process.env.PLAYWRIGHT_BROWSERS_PATH = '0';
  failurePhase = 'runtime_module_load';
  failureModule = 'vite';
  const {build} = await import('vite');
  failureModule = 'motion_canvas_vite_plugin';
  const {default:motionCanvasModule} = await import('@motion-canvas/vite-plugin');
  const motionCanvas = typeof motionCanvasModule === 'function' ? motionCanvasModule : motionCanvasModule.default;
  failureModule = 'playwright';
  const {firefox} = await import('playwright');
  failureModule = null;
  failurePhase = 'font_evidence';
  const lockBytes=await fs.readFile(await containedFile(runtimeRoot,path.join(runtimeRoot,'package-lock.json'),'dependency lock'));
  if(createHash('sha256').update(lockBytes).digest('hex')!==config.dependencyLockDigest)fail('dependency lock binding changed');
  if(!Array.isArray(config.fontResourcePins)||config.fontResourcePins.length===0||config.fontResourcePins.length>128)fail('font resource pin budget');
  const resourceHash=createHash('sha256');
  for(const pin of config.fontResourcePins){
    safeRelative(pin.path,'font resource');
    const bytes=await fs.readFile(await containedFile(runtimeRoot,path.join(runtimeRoot,pin.path),'font resource'));
    if(bytes.length===0||bytes.length>16*1024*1024||createHash('sha256').update(bytes).digest('hex')!==pin.sha256)fail('font resource binding changed');
    resourceHash.update(pin.path);resourceHash.update(Buffer.from([0]));resourceHash.update(pin.sha256);resourceHash.update(Buffer.from([0]));
  }
  if(resourceHash.digest('hex')!==config.fontResourcesDigest)fail('font resource digest binding changed');
  if(config.authoring) config.fontEvidence=await pinnedFontEvidence(runtimeRoot);
  const {target: project} = await childOf(a['project-root'], a['project-relative'], 'project');
  const fontconfigRoot = await fs.realpath(a['fontconfig-root']);
  if (!(await fs.stat(fontconfigRoot)).isDirectory()) fail('fontconfig root is not a directory');
  const browser = await containedFile(runtimeRoot, firefox.executablePath(), 'Firefox executable');
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
      executablePath:browser,
      viewport:{width:config.width,height:config.height},
      serviceWorkers:'block',
      firefoxUserPrefs:{'dom.ipc.forkserver.enable':false},
      env:{
        ...process.env,
        FONTCONFIG_PATH:fontconfigRoot,
        FONTCONFIG_FILE:path.join(fontconfigRoot,'fonts.conf'),
        TMPDIR:output,
        TMP:output,
        TEMP:output,
        XDG_CACHE_HOME:path.join(output,'.cache'),
        XDG_CONFIG_HOME:path.join(output,'.config'),
        XDG_DATA_HOME:path.join(output,'.data'),
        MOZ_ASSUME_USER_NS:'0',
        MOZ_DISABLE_CONTENT_SANDBOX:'1'
      },
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
    await page.exposeBinding('__SEMWRIGHT_TEXT_DIGEST__', async (_source, text) => {
      if (typeof text !== 'string' || Buffer.byteLength(text, 'utf8') > 65_536) fail('invalid text digest payload');
      return createHash('sha256').update(Buffer.from(text, 'utf8')).digest('hex');
    });
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
      fail(`renderer result ${state.result}; renderer_log_local=${JSON.stringify(state.rendererLogDiagnostic)}; state=${JSON.stringify(state)} diagnostics=${JSON.stringify(diagnostics)}`);
    }
    failurePhase = 'observation';
    if (!state.fontReadiness || state.fontReadiness.observed_before_renderer_render !== true
        || state.fontReadiness.status_after_ready !== 'loaded' || state.fontReadiness.requests?.length !== 5) fail('native font readiness observation incomplete');
    const fontReceipt = {version:1,render_input_digest:config.renderInputDigest,
      font_resources_sha256:config.fontResourcesDigest,first_frame:config.firstFrame,
      end_frame_exclusive:config.endFrameExclusive,fps_num:config.fpsNum,fps_den:config.fpsDen,
      authoring:config.authoring === true,readiness:state.fontReadiness,
      scope:'actual native browser readiness; local receipt only, no reference bitmap identity claim'};
    const fontReceiptBytes=Buffer.from(JSON.stringify(fontReceipt)+'\n','utf8');
    if(fontReceiptBytes.length>32768)fail('native font readiness receipt exceeds bounds');
    const fontReceiptFile=await fs.open(path.join(output,'font-readiness-receipt.json'),'wx',0o600);
    try { await fontReceiptFile.writeFile(fontReceiptBytes); await fontReceiptFile.sync(); }
    finally { await fontReceiptFile.close(); }
    if(config.authoring){
      if(observationCount!==config.endFrameExclusive-config.firstFrame)fail('native observation count incomplete');
      await fs.writeFile(path.join(output,'native-observations-receipt.json'),JSON.stringify({version:1,render_input_digest:config.renderInputDigest,font_resources_sha256:config.fontResourcesDigest,sha256:observationHash.digest('hex'),bytes:observationBytes,frames:observationCount,fps_num:config.fpsNum,fps_den:config.fpsDen}),{flag:'wx'});
    }
    failurePhase = 'finalize';
    const files = (await fs.readdir(path.join(output,'frames'))).sort();
    process.stdout.write(JSON.stringify({ok:true,renderer:'motion-canvas-core-renderer-v3.17.2-firefox',lastFrame:state.frame,files:files.map(file=>`frames/${file}`)})+'\n');
  } finally { await cleanup(); }
}
main().catch(async error => {
  const errorClass = DIAGNOSTIC_PHASES.has(failurePhase) ? failurePhase : 'startup';
  let diagnosticPersisted = false;
  try { diagnosticPersisted = await writeFailureReceipt(error,errorClass); } catch {}
  process.stdout.write(JSON.stringify({ok:false,errorClass,detail:failureDetail,diagnosticPersisted})+'\n');
  process.stderr.write(JSON.stringify({ok:false,errorClass,diagnosticPersisted})+'\n');
  process.exitCode = 1;
});
