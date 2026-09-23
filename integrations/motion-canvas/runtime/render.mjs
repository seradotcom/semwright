import fs from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import process from 'node:process';
import {fileURLToPath} from 'node:url';
import {build} from 'vite';
import motionCanvas from '@motion-canvas/vite-plugin';
import {chromium} from 'playwright';

function fail(message) { throw new Error(message); }
function args() {
  const out = {};
  for (let i = 2; i < process.argv.length; i += 2) {
    const key = process.argv[i]; const value = process.argv[i + 1];
    if (!key?.startsWith('--') || value === undefined) fail('invalid arguments');
    out[key.slice(2)] = value;
  }
  for (const key of ['project', 'output', 'config']) if (!out[key]) fail(`missing --${key}`);
  return out;
}
function harnessPlugin(config) {
  const id = '\0semwright-render-entry';
  return {
    name: 'semwright:controlled-render-harness',
    resolveId(source) { if (source === 'virtual:semwright-render') return id; },
    load(source) {
      if (source !== id) return;
      return `
import project from '/src/project.ts?project';
import {Renderer, Vector2} from '@motion-canvas/core';
const config=${JSON.stringify(config)};
const renderer=new Renderer(project);
const state={done:false,result:null,frame:config.firstFrame,error:null};
window.__SEMWRIGHT_RENDER__={state,abort:()=>renderer.abort()};
renderer.onFrameChanged.subscribe(frame=>{state.frame=frame;});
const finished=new Promise(resolve=>renderer.onFinished.subscribe(resolve));
try {
  renderer.render({
    name:'frames',
    size:new Vector2(config.width,config.height),
    resolutionScale:1,
    colorSpace:config.colorSpace,
    background:config.alpha?null:config.background,
    range:[config.firstFrame/config.fps,config.endFrameExclusive/config.fps],
    fps:config.fps,
    exporter:{name:'@semwright/driver/image-sequence',options:{}},
  }).catch(error=>{state.error=String(error);state.done=true;});
  state.result=await finished;
  state.done=true;
} catch(error) { state.error=String(error); state.done=true; }
`;
    },
  };
}
function contentType(file) {
  const ext = path.extname(file).toLowerCase();
  return ({'.html':'text/html; charset=utf-8','.js':'text/javascript; charset=utf-8','.mjs':'text/javascript; charset=utf-8','.css':'text/css; charset=utf-8','.json':'application/json','.svg':'image/svg+xml','.png':'image/png','.jpg':'image/jpeg','.jpeg':'image/jpeg','.woff2':'font/woff2','.woff':'font/woff','.mp4':'video/mp4','.webm':'video/webm','.wav':'audio/wav','.mp3':'audio/mpeg'})[ext] || 'application/octet-stream';
}
async function main() {
  const a = args();
  const runtimeRoot = path.dirname(fileURLToPath(import.meta.url));
  const config = JSON.parse(Buffer.from(a.config, 'base64url').toString('utf8'));
  const project = path.resolve(a.project); const output = path.resolve(a.output);
  const work = await fs.mkdtemp(path.join(os.tmpdir(), 'semwright-motion-render-'));
  const dist = path.join(work, '.semwright-render-dist');
  let browser; let page; let cancelling = false;
  const cleanup = async () => { try { await browser?.close(); } catch {} await fs.rm(work, {recursive:true,force:true}).catch(()=>{}); };
  const cancel = async () => { if (cancelling) return; cancelling = true; try { await page?.evaluate(() => window.__SEMWRIGHT_RENDER__?.abort()); } catch {} await cleanup(); process.exitCode = 130; };
  process.once('SIGTERM', cancel); process.once('SIGINT', cancel);
  try {
    await fs.cp(project, work, {recursive:true,dereference:false,errorOnExist:false});
    await fs.rm(path.join(work, 'node_modules'), {recursive:true,force:true});
    await fs.symlink(path.join(runtimeRoot, 'node_modules'), path.join(work, 'node_modules'), 'dir');
    await fs.writeFile(path.join(work, 'semwright-render.html'), '<!doctype html><meta charset="utf-8"><script type="module" src="/semwright-entry.js"></script>');
    await fs.writeFile(path.join(work, 'semwright-entry.js'), "import 'virtual:semwright-render';\n");
    await build({root:work,configFile:false,logLevel:'error',base:'/',plugins:[motionCanvas({project:'./src/project.ts',editor:path.join(runtimeRoot,'stub-editor/main.js')}),harnessPlugin(config)],build:{outDir:dist,emptyOutDir:true,rollupOptions:{input:path.join(work,'semwright-render.html')}}});
    await fs.mkdir(path.join(output, 'frames'), {recursive:true});
    const launch = {headless:true,chromiumSandbox:true,args:['--disable-background-networking','--disable-component-update','--no-first-run']};
    if (a.browser) launch.executablePath = a.browser;
    browser = await chromium.launch(launch);
    const context = await browser.newContext({viewport:{width:config.width,height:config.height},serviceWorkers:'block'});
    page = await context.newPage();
    const written = new Set();
    await page.exposeBinding('__SEMWRIGHT_EXPORT_FRAME__', async (_source, payload) => {
      if (!payload || !Number.isSafeInteger(payload.frame) || payload.frame < config.firstFrame || payload.frame >= config.endFrameExclusive || typeof payload.data !== 'string' || !payload.data.startsWith('data:image/png;base64,')) fail('invalid frame payload');
      if (written.has(payload.frame)) fail('duplicate frame payload');
      const bytes = Buffer.from(payload.data.slice('data:image/png;base64,'.length), 'base64');
      if (bytes.length < 8 || bytes.length > 32 * 1024 * 1024 || bytes.subarray(0,8).toString('hex') !== '89504e470d0a1a0a') fail('invalid PNG payload');
      written.add(payload.frame);
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
    await page.goto('http://semwright.invalid/semwright-render.html', {waitUntil:'domcontentloaded',timeout:config.timeoutMs});
    await page.waitForFunction(() => window.__SEMWRIGHT_RENDER__?.state?.done === true, {timeout:config.timeoutMs});
    const state = await page.evaluate(() => window.__SEMWRIGHT_RENDER__.state);
    if (state.error) fail(`renderer failed: ${state.error}`);
    if (state.result !== 0) fail(`renderer result ${state.result}`);
    const files = (await fs.readdir(path.join(output,'frames'))).sort();
    process.stdout.write(JSON.stringify({ok:true,renderer:'motion-canvas-core-renderer-v3.17.2',lastFrame:state.frame,files:files.map(file=>`frames/${file}`)})+'\n');
  } finally { await cleanup(); }
}
main().catch(error => { process.stderr.write(String(error?.stack || error) + '\n'); process.exitCode = 1; });
