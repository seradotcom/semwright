import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {createRequire} from 'node:module';
import {fileURLToPath, pathToFileURL} from 'node:url';
import test from 'node:test';
import {verifyMotionCanvasPlugin, verifyExportedMotionCanvasPlugin} from '../plugin-integrity.mjs';

const runtime = fileURLToPath(new URL('..', import.meta.url));
const require = createRequire(import.meta.url);
const {settingsPlugin} = require('@semwright/motion-canvas-vite-plugin/lib/partials/settings.js');
const pluginName = '@semwright/motion-canvas-vite-plugin';
const directory = 'node_modules/' + pluginName;
const provenance = 'vendor/motion-canvas-vite-plugin.provenance.json';
const archive = 'vendor/semwright-motion-canvas-vite-plugin-3.17.2-semwright.1.tgz';
const entry = root => pathToFileURL(path.join(root, directory, 'lib/index.js')).href;

async function isolated(t) {
  const temporaryRoot = await fs.realpath(os.tmpdir());
  const root = await fs.mkdtemp(path.join(temporaryRoot, 'semwright-vite-contract-'));
  t.after(() => fs.rm(root, {recursive:true, force:true}));
  assert.equal(path.relative(root, await fs.realpath(root)), '', 'fixture root must have physical spelling');
  return root;
}
async function packageFixture(t) {
  const root = await isolated(t);
  await fs.mkdir(path.join(root, 'vendor'));
  await fs.mkdir(path.join(root, 'node_modules/@semwright'), {recursive:true});
  await fs.copyFile(path.join(runtime, provenance), path.join(root, provenance));
  await fs.copyFile(path.join(runtime, archive), path.join(root, archive));
  await fs.cp(path.join(runtime, directory), path.join(root, directory), {recursive:true, dereference:false});
  return {root, lock:await fs.readFile(path.join(runtime, 'package-lock.json'))};
}
test('fork archive, lock, installed roster and resolved entry agree', async () => {
  const lock = await fs.readFile(path.join(runtime, 'package-lock.json'));
  const report = await verifyMotionCanvasPlugin(runtime, lock, import.meta.resolve(pluginName));
  assert.equal(report.name, pluginName);
  assert.equal(report.version, '3.17.2-semwright.1');
  assert.equal(report.files, 51);
  assert.ok(report.bytes < 262144);
  const render = await fs.readFile(path.join(runtime, 'render.mjs'), 'utf8');
  const verifier = await fs.readFile(path.join(runtime, 'plugin-integrity.mjs'));
  assert.ok(render.includes(createHash('sha256').update(verifier).digest('hex')));
  assert.ok(render.indexOf('await verifyMotionCanvasPlugin(') < render.indexOf("await import('vite')"));
  assert.ok(render.includes("await import('data:text/javascript;base64,'+verifierBytes.toString('base64'))"));
});
for (const mutation of ['archive', 'provenance', 'member', 'extra', 'lock', 'entry', 'alias']) {
  test('fork identity rejects ' + mutation + ' substitution', async t => {
    const {root, lock} = await packageFixture(t);
    let candidateLock = lock;
    let candidateEntry = entry(root);
    if (mutation === 'archive') await fs.appendFile(path.join(root, archive), 'x');
    if (mutation === 'provenance') await fs.appendFile(path.join(root, provenance), ' ');
    if (mutation === 'member') await fs.appendFile(path.join(root, directory, 'lib/main.js'), '\n// substituted');
    if (mutation === 'extra') await fs.writeFile(path.join(root, directory, 'injected.js'), '');
    if (mutation === 'lock') {
      const value = JSON.parse(lock);
      value.packages['node_modules/' + pluginName].integrity = 'sha512-changed';
      candidateLock = Buffer.from(JSON.stringify(value));
    }
    if (mutation === 'entry') candidateEntry = pathToFileURL(path.join(root, directory, 'lib/main.js')).href;
    if (mutation === 'alias') {
      const original = path.join(root, directory, 'lib/main.js');
      const alias = path.join(root, 'alias.js');
      await fs.link(original, alias);
    }
    await assert.rejects(verifyMotionCanvasPlugin(root, candidateLock, candidateEntry), /motion plugin identity:/);
  });
}
test('settings require an explicit ordinary project filename', () => {
  for (const settingsFile of [undefined, '', '../settings.json', '/settings.json', 'C:\\settings.json',
    'sub/settings.json', 'sub\\settings.json', 'settings:stream', 'CON', 'nul.json', '..', 'settings.']) {
    assert.throws(() => settingsPlugin({settingsFile}), /explicit ordinary filename/);
  }
});
test('settings distinguish absence, invalid JSON and nonordinary files', async t => {
  const root = await isolated(t);
  const plugin = settingsPlugin({settingsFile:'.semwright-settings.json'});
  await plugin.configResolved({root});
  assert.equal(plugin.resolveId('virtual:settings.meta'), '\0virtual:settings.meta');
  assert.equal(await plugin.load('\0virtual:settings.meta'), '{}');
  const target = path.join(root, '.semwright-settings.json');
  await fs.writeFile(target, '{"theme":"dark"}');
  assert.equal(await plugin.load('\0virtual:settings.meta'), '{"theme":"dark"}');
  await fs.writeFile(target, '{invalid');
  await assert.rejects(plugin.load('\0virtual:settings.meta'), SyntaxError);
  await fs.unlink(target);
  await fs.mkdir(target);
  await assert.rejects(plugin.load('\0virtual:settings.meta'), /ordinary file/);
});
test('settings reject file aliases and linked project roots', async t => {
  const parent = await isolated(t);
  const root = path.join(parent, 'root');
  await fs.mkdir(root);
  const plugin = settingsPlugin({settingsFile:'.semwright-settings.json'});
  await plugin.configResolved({root});
  const target = path.join(root, '.semwright-settings.json');
  await fs.writeFile(target, '{}');
  await fs.link(target, path.join(parent, 'alias.json'));
  await assert.rejects(plugin.load('\0virtual:settings.meta'), /ordinary file/);
  const linked = path.join(parent, 'linked');
  await fs.symlink(root, linked, process.platform === 'win32' ? 'junction' : 'dir');
  const throughLink = settingsPlugin({settingsFile:'.semwright-settings.json'});
  await assert.rejects(throughLink.configResolved({root:linked}), /link or non-directory/);
});
test('settings acknowledge only retained writes and invalidate the original virtual module', async t => {
  const root = await isolated(t);
  const plugin = settingsPlugin({settingsFile:'.semwright-settings.json'});
  await plugin.configResolved({root});
  let receive;
  const events = [];
  const module = {};
  plugin.configureServer({
    ws:{on(event, callback) {assert.equal(event, 'motion-canvas:meta'); receive = callback;}},
    moduleGraph:{
      async getModuleByUrl(id) {assert.equal(id, '\0virtual:settings.meta'); return module;},
      invalidateModule(value) {assert.equal(value, module); events.push('invalidate');},
    },
  });
  const client = {send(event, data) {events.push([event, data]);}};
  await receive({source:'unrelated', data:{}}, client);
  assert.deepEqual(events, []);
  await receive({source:'\0virtual:settings.meta', data:{theme:'dark'}}, client);
  assert.equal(await plugin.load('\0virtual:settings.meta'), '{"theme":"dark"}');
  assert.deepEqual(events, ['invalidate', ['motion-canvas:meta-ack', {source:'\0virtual:settings.meta'}]]);
  events.length = 0;
  await receive({source:'\0virtual:settings.meta', data:{theme:'dark'}}, client);
  assert.deepEqual(events, [['motion-canvas:meta-ack', {source:'\0virtual:settings.meta'}]]);
  events.length = 0;
  const target = path.join(root, '.semwright-settings.json');
  await fs.unlink(target);
  await fs.mkdir(target);
  await assert.rejects(receive({source:'\0virtual:settings.meta', data:{}}, client), /ordinary file/);
  assert.deepEqual(events, []);
});

test('exported projects retain the linked dependency flow without granting Native authority', async t => {
  const {root:admitted, lock} = await packageFixture(t);
  const root = await isolated(t);
  await fs.mkdir(path.join(root, 'vendor'));
  await fs.copyFile(path.join(admitted, provenance), path.join(root, provenance));
  await fs.copyFile(path.join(admitted, archive), path.join(root, archive));
  await fs.symlink(path.join(admitted, 'node_modules'), path.join(root, 'node_modules'),
    process.platform === 'win32' ? 'junction' : 'dir');
  assert.equal((await verifyExportedMotionCanvasPlugin(root, lock, entry(admitted))).files, 51);
  await assert.rejects(verifyMotionCanvasPlugin(root, lock, entry(admitted)), /runtime dependency root is not ordinary/);
  await fs.appendFile(path.join(admitted, directory, 'lib/main.js'), '\n// substituted');
  await assert.rejects(verifyExportedMotionCanvasPlugin(root, lock, entry(admitted)), /installed member differs/);
});
