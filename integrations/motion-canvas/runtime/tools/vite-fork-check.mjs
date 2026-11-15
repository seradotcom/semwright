// Real Vite 6 builds, local editor/meta/HMR contracts; no browser or renderer.
import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';
import {verifyMotionCanvasPlugin, verifyExportedMotionCanvasPlugin} from '../plugin-integrity.mjs';

assert.ok(process.argv.length === 2 || (process.argv.length === 3 && process.argv[2] === '--editor-only'));
const editorOnly = process.argv[2] === '--editor-only';
const runtime = fileURLToPath(new URL('..', import.meta.url));
const lock = await fs.readFile(path.join(runtime, 'package-lock.json'));
const entry = import.meta.resolve('@semwright/motion-canvas-vite-plugin');
const identity = await verifyMotionCanvasPlugin(runtime, lock, entry);
const {build, createServer, normalizePath} = await import('vite');
const {default:module} = await import(entry);
const motionCanvas = typeof module === 'function' ? module : module.default;
const project = await fs.mkdtemp(path.join(os.tmpdir(), 'semwright-vite-build-'));
const previous = process.cwd();
let server;
let socket;
const report = {schema:'semwright.vite-fork-contract/1', scope:editorOnly?'editor-only':'build-and-editor', vite:'6.4.6', identity, render:false, native:false};
async function write(relative, contents) {
  const target = path.join(project, relative);
  await fs.mkdir(path.dirname(target), {recursive:true});
  await fs.writeFile(target, contents, {flag:'wx'});
}
async function outputs(root) {
  const result = [];
  async function visit(directory) {
    for (const item of await fs.readdir(directory, {withFileTypes:true})) {
      const target = path.join(directory, item.name);
      if (item.isDirectory()) await visit(target);
      else {
        assert.ok(item.isFile());
        const bytes = await fs.readFile(target);
        result.push({path:path.relative(root,target).split(path.sep).join('/'),bytes:bytes.length,
          sha256:createHash('sha256').update(bytes).digest('hex')});
      }
    }
  }
  await visit(root);
  assert.ok(result.length > 0 && result.length <= 128);
  assert.ok(result.reduce((sum,item) => sum + item.bytes, 0) <= 32*1024*1024);
  return result.sort((a,b) => a.path.localeCompare(b.path,'en'));
}
function message(predicate, timeout=10000) {
  return new Promise((resolve,reject) => {
    const timer = setTimeout(() => {socket.removeEventListener('message',listener); reject(new Error('Vite event timeout'));},timeout);
    function listener(event) {
      if (typeof event.data !== 'string' || event.data.length > 65536) return;
      const data = JSON.parse(event.data);
      if (predicate(data)) {clearTimeout(timer); socket.removeEventListener('message',listener); resolve(data);}
    }
    socket.addEventListener('message',listener);
  });
}
try {
  for (const relative of ['package.json','package-lock.json','plugin-integrity.mjs',
    'vendor/motion-canvas-vite-plugin.provenance.json',
    'vendor/semwright-motion-canvas-vite-plugin-3.17.2-semwright.1.tgz']) {
    await write(relative, await fs.readFile(path.join(runtime,relative)));
  }
  await write('vite.config.ts', await fs.readFile(path.join(runtime,'project-vite.config.ts')));
  await fs.symlink(path.join(runtime,'node_modules'),path.join(project,'node_modules'),
    process.platform==='win32'?'junction':'dir');
  await write('src/project.ts',"import {makeProject} from '@motion-canvas/core';\nimport main from './scenes/main?scene';\nexport default makeProject({scenes:[main]});\n");
  await write('src/scenes/main.tsx',"import {makeScene2D,Rect} from '@motion-canvas/2d';\nimport {waitFor} from '@motion-canvas/core';\nimport shader from '../shader.glsl';\nif(!shader.includes('contractColor'))throw new Error('GLSL include missing');\nexport default makeScene2D(function*(view){view.add(<Rect width={10} height={10} fill={'#123456'}/>);yield* waitFor(0.1);});\n");
  await write('src/shader.glsl','#include "./shared.glsl"\nvoid main(){gl_FragColor=contractColor;}\n');
  await write('src/shared.glsl','const vec4 contractColor=vec4(1.0);\n');
  await write('src/extension.ts',"export default function(options){return {name:'semwright-contract-extension',options};}\n");
  await write('managed.html','<!doctype html><script type="module" src="/src/managed-entry.ts"></script>');
  await write('src/managed-entry.ts',"import project from './project.ts?project';\nexport default project;\n");
  process.chdir(project);
  assert.equal((await verifyExportedMotionCanvasPlugin(project,lock,entry)).files,51);
  if (!editorOnly) {
  await build({root:project,configFile:path.join(project,'vite.config.ts'),logLevel:'error',
    build:{outDir:path.join(project,'dist-exported'),emptyOutDir:true}});
  report.exported_build = await outputs(path.join(project,'dist-exported'));
  process.stdout.write(JSON.stringify({phase:'exported_build',files:report.exported_build})+'\n');
  let extensionConfig = 0;
  let extensionRuntime = 0;
  const extension = {
    name:'semwright:contract-extension',
    [Symbol.for('@motion-canvas/vite-plugin/PLUGIN_OPTIONS')]: {
      config(value) {extensionConfig++; return value;},
      runtimeConfig() {extensionRuntime++; return {verified:true};},
      entryPoint:normalizePath(path.join(project,'src/extension.ts')),
    },
  };
  await build({root:project,configFile:false,logLevel:'error',
    plugins:[motionCanvas({project:normalizePath(path.join(project,'src/project.ts')),settingsFile:'.semwright-settings.json',
      editor:path.join(runtime,'stub-editor/main.js'),buildForEditor:true}),extension,
      {name:'semwright:contract-managed-entry',enforce:'post',
        config() {return {build:{rollupOptions:{input:normalizePath(path.join(project,'managed.html'))}}};}}],
    build:{outDir:path.join(project,'dist-managed'),emptyOutDir:true}});
  assert.ok(extensionConfig > 0 && extensionRuntime > 0);
  report.managed_editor_build = await outputs(path.join(project,'dist-managed'));
  process.stdout.write(JSON.stringify({phase:'managed_editor_build',files:report.managed_editor_build})+'\n');
  report.extension = {config:extensionConfig,runtimeConfig:extensionRuntime,literal_symbol_preserved:true};
  }
  server = await createServer({root:project,configFile:path.join(project,'vite.config.ts'),logLevel:'error',
    server:{host:'127.0.0.1',port:0,strictPort:true},optimizeDeps:{noDiscovery:true,include:[]}});
  await server.listen();
  const address = server.httpServer.address();
  assert.ok(address && typeof address !== 'string');
  const origin = 'http://127.0.0.1:'+address.port;
  const html = await (await fetch(origin+'/')).text();
  assert.ok(html.includes('virtual:editor'));
  async function moduleText(relative) {
    const response = await fetch(origin + relative);
    assert.equal(response.status, 200, 'module status: ' + relative);
    return response.text();
  }
  const editor = await moduleText('/@id/__x00__virtual:editor');
  assert.ok(editor.includes('editor(') && editor.includes('?project'));
  const projectModule = await moduleText('/src/project.ts?project');
  const settingsImport = projectModule.match(/from\s+["']([^"']*virtual:settings\.meta[^"']*)["']/);
  assert.ok(settingsImport, 'the transformed project imports its settings module');
  const settingsUrl = settingsImport[1];
  const settings = await moduleText(settingsUrl);
  assert.ok(settings.includes('MetaFile') && settings.includes('import.meta.hot.accept()'));
  const shader = await moduleText('/src/shader.glsl?import');
  assert.ok(shader.includes('contractColor') && !shader.includes('#include'));
  await moduleText('/src/scenes/main.tsx?scene');
  assert.equal(typeof server.config.webSocketToken,'string');
  socket = new WebSocket('ws://127.0.0.1:'+address.port+'/?token='+encodeURIComponent(server.config.webSocketToken),'vite-hmr');
  await message(value=>value.type==='connected');
  let ack = message(value=>value.type==='custom' && value.event==='motion-canvas:meta-ack' && value.data.source==='\0virtual:settings.meta');
  socket.send(JSON.stringify({type:'custom',event:'motion-canvas:meta',data:{source:'\0virtual:settings.meta',data:{theme:'contract'}}}));
  await ack;
  assert.deepEqual(JSON.parse(await fs.readFile(path.join(project,'.semwright-settings.json'),'utf8')),{theme:'contract'});
  const refreshed = await moduleText(settingsUrl);
  assert.ok(refreshed.includes('"theme":"contract"'));
  const metaPath = path.join(project,'src/project.meta').split(path.sep).join('/');
  ack = message(value=>value.type==='custom' && value.event==='motion-canvas:meta-ack' && value.data.source===metaPath);
  socket.send(JSON.stringify({type:'custom',event:'motion-canvas:meta',data:{source:metaPath,data:{version:1,contract:'retained'}}}));
  await ack;
  assert.equal(JSON.parse(await fs.readFile(metaPath,'utf8')).contract,'retained');
  const hot = message(value=>value.type==='update' && value.updates.some(item=>item.path.includes('main.tsx')));
  await fs.appendFile(path.join(project,'src/scenes/main.tsx'),'\n// physical HMR edit\n');
  const update = await hot;
  report.editor = {html:true,virtual_module:true,settings_ack_and_readback:true,
    settings_module_invalidated:true,project_meta_ack_and_readback:true,
    glsl_include:true,hmr_updates:update.updates.map(item=>({type:item.type,path:item.path}))};
  report.closed = true;
} finally {
  socket?.close();
  await server?.close();
  process.chdir(previous);
  await fs.rm(project,{recursive:true,force:true});
}
process.stdout.write(JSON.stringify(report)+'\n');
