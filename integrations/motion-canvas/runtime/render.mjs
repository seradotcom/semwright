import fs from 'node:fs/promises';
import {constants as fsConstants} from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import process from 'node:process';
import {spawn} from 'node:child_process';
import {once} from 'node:events';
import {fileURLToPath} from 'node:url';
import {build} from 'vite';
import motionCanvasModule from '@motion-canvas/vite-plugin';
const motionCanvas = typeof motionCanvasModule === 'function' ? motionCanvasModule : motionCanvasModule.default;
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
const state={done:false,result:null,frame:config.firstFrame,error:null};
window.__SEMWRIGHT_RENDER__={state,abort:()=>renderer.abort()};
renderer.onFrameChanged.subscribe(frame=>{state.frame=frame;});
const finished=new Promise(resolve=>renderer.onFinished.subscribe(resolve));
(async()=>{
  try {
    renderer.render({
      name:'frames',
      size:new Vector2(config.width,config.height),
      resolutionScale:1,
      colorSpace:config.colorSpace,
      background:config.alpha?null:config.background,
      // Motion Canvas 3.17.2 treats the range end as inclusive; Semwright profiles are half-open.
      range:[config.firstFrame/config.fps,(config.endFrameExclusive-1)/config.fps],
      fps:config.fps,
      exporter:{name:'@semwright/driver/image-sequence',options:{}},
    }).catch(error=>{state.error=String(error);state.done=true;});
    state.result=await finished;
    state.done=true;
  } catch(error) { state.error=String(error); state.done=true; }
})();
`;
    },
  };
}
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
function exited(child) { return child.exitCode !== null || child.signalCode !== null; }
async function stopChromium(browser, child) {
  try {
    const session = await browser?.newBrowserCDPSession();
    await session?.send('Browser.close').catch(() => {});
  } catch {}
  try { await browser?.close(); } catch {}
  if (!child || exited(child)) return;
  child.kill('SIGTERM');
  await Promise.race([once(child, 'exit'), sleep(2000)]).catch(() => {});
  if (!exited(child)) child.kill('SIGKILL');
}
async function startChromium(executable, output, timeoutMs) {
  if (!executable) fail('missing --browser');
  const profile = path.join(output, 'chromium-profile');
  await fs.mkdir(profile, {recursive:true});
  const argv = [
    '--headless=new', '--no-sandbox', '--enable-logging=stderr', '--v=1',
    '--disable-gpu', '--disable-dev-shm-usage',
    '--disable-background-networking', '--disable-background-timer-throttling',
    '--disable-component-update', '--disable-default-apps', '--disable-extensions',
    '--no-default-browser-check', '--no-first-run', '--password-store=basic',
    '--use-mock-keychain', '--hide-scrollbars', '--mute-audio',
    '--remote-debugging-address=127.0.0.1', '--remote-debugging-port=0',
    `--user-data-dir=${profile}`, 'about:blank',
  ];
  const metadata = await fs.stat(executable);
  if (!metadata.isFile() || (metadata.mode & 0o111) === 0) {
    fail(`Pinned Chromium is not executable (mode=${(metadata.mode & 0o777).toString(8)})`);
  }
  try {
    await fs.access(executable, fsConstants.X_OK);
  } catch (error) {
    fail(`Pinned Chromium failed X_OK access (mode=${(metadata.mode & 0o777).toString(8)}): ${String(error)}`);
  }
  const child = spawn(executable, argv, {stdio:['ignore','ignore','pipe']});
  const spawnFailure = once(child, 'error').then(([error]) => error);
  let stderr = '';
  child.stderr?.setEncoding('utf8');
  child.stderr?.on('data', chunk => { stderr = (stderr + chunk).slice(-16384); });
  const activePort = path.join(profile, 'DevToolsActivePort');
  const deadline = Date.now() + Math.min(timeoutMs, 30000);
  let port = 0;
  while (Date.now() < deadline) {
    const spawnError = await Promise.race([spawnFailure, sleep(25).then(() => null)]);
    if (spawnError) {
      const apparmor = await fs.readFile('/proc/self/attr/current', 'utf8').then(v => v.trim()).catch(error => `unavailable:${String(error)}`);
      const probe = executable => new Promise(resolve => {
        let settled = false;
        const finish = value => { if (!settled) { settled = true; resolve(value); } };
        const process = spawn(executable, ['--version'], {stdio:'ignore'});
        process.once('error', error => finish(`error:${String(error)}`));
        process.once('exit', (code, signal) => finish(`exit:${code}:${signal}`));
        setTimeout(() => { if (!settled) { process.kill('SIGKILL'); finish('timeout'); } }, 1500);
      });
      const [trueProbe, nodeProbe] = await Promise.all([probe('/usr/bin/true'), probe(process.execPath)]);
      fail(`Chromium spawn failed after X_OK passed (mode=${(metadata.mode & 0o777).toString(8)}, apparmor=${apparmor}, true_probe=${trueProbe}, node_probe=${nodeProbe}): ${String(spawnError)}`);
    }
    if (exited(child)) fail(`Chromium exited before CDP startup (code=${child.exitCode}, signal=${child.signalCode}): ${stderr}`);
    try {
      const lines = (await fs.readFile(activePort, 'utf8')).trim().split(/\r?\n/);
      if (/^[0-9]{1,5}$/.test(lines[0] || '')) {
        const candidate = Number(lines[0]);
        if (candidate >= 1 && candidate <= 65535) { port = candidate; break; }
      }
    } catch (error) {
      if (error?.code !== 'ENOENT') throw error;
    }
    await sleep(25);
  }
  if (!port) {
    await stopChromium(undefined, child);
    fail(`Chromium did not publish a bounded CDP endpoint: ${stderr}`);
  }
  try {
    const browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`, {timeout:Math.min(timeoutMs, 30000)});
    return {browser, child};
  } catch (error) {
    await stopChromium(undefined, child);
    fail(`CDP attach failed: ${String(error)}; chromium stderr: ${stderr}`);
  }
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
  let browser; let browserProcess; let page; let cancelling = false;
  const cleanup = async () => { await stopChromium(browser, browserProcess); await fs.rm(work, {recursive:true,force:true}).catch(()=>{}); };
  const cancel = async () => { if (cancelling) return; cancelling = true; try { await page?.evaluate(() => window.__SEMWRIGHT_RENDER__?.abort()); } catch {} await cleanup(); process.exitCode = 130; };
  process.once('SIGTERM', cancel); process.once('SIGINT', cancel);
  try {
    await fs.cp(project, work, {recursive:true,dereference:false,errorOnExist:false});
    await fs.rm(path.join(work, 'node_modules'), {recursive:true,force:true});
    await fs.symlink(path.join(runtimeRoot, 'node_modules'), path.join(work, 'node_modules'), 'dir');
    await fs.writeFile(path.join(work, 'semwright-render.html'), '<!doctype html><meta charset="utf-8"><script type="module" src="/semwright-entry.js"></script>');
    await fs.writeFile(path.join(work, 'semwright-entry.js'), "import 'virtual:semwright-render';\n");
    const projectEntry=path.join(work,'src/project.ts');
    const renderEntry=path.join(work,'semwright-render.html');
    await build({root:work,configFile:false,logLevel:'error',base:'/',plugins:[motionCanvas({project:projectEntry,editor:path.join(runtimeRoot,'stub-editor/main.js')}),harnessPlugin(config,renderEntry)],build:{outDir:dist,emptyOutDir:true,rollupOptions:{input:renderEntry}}});
    await fs.mkdir(path.join(output, 'frames'), {recursive:true});
    if (process.env.SEMWRIGHT_DRIVER_SANDBOX !== 'landlock-bwrap-v1') fail('renderer requires the Semwright Driver Host sandbox');
    // Playwright launch() uses a remote-debugging pipe that crashes the pinned Chromium
    // under this outer sandbox. Start the already-attested browser directly and attach over
    // ephemeral loopback CDP. Bubblewrap's network namespace exposes loopback only.
    ({browser, child:browserProcess} = await startChromium(a.browser, output, config.timeoutMs));
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
