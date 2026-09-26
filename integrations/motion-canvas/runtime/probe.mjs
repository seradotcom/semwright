import {firefox} from 'playwright';
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import {createRequire} from 'node:module';
import process from 'node:process';
const require = createRequire(import.meta.url);
await mkdir('evidence', {recursive: true});
const versions = {node: process.version};
for (const name of ['@motion-canvas/core', '@motion-canvas/2d', '@motion-canvas/vite-plugin', 'vite', 'playwright']) {
  versions[name] = JSON.parse(await readFile(require.resolve(`${name}/package.json`), 'utf8')).version;
}
let context;
try {
  context = await firefox.launchPersistentContext('', {headless:true,timeout:30000,firefoxUserPrefs:{'dom.ipc.forkserver.enable':false},env:{...process.env,MOZ_ASSUME_USER_NS:'0'}});
  const page = context.pages()[0];
  if (!page) throw new Error('Firefox persistent context exposed no startup page');
  await page.setContent('<canvas width="32" height="32"></canvas>');
  versions.browser = context.browser()?.version() ?? 'unknown';
  versions.browserFamily = 'firefox';
  versions.canvas = await page.evaluate(() => document.querySelector('canvas').getContext('2d') !== null);
} catch (error) {
  versions.browser_probe_error = String(error).slice(0, 8000);
} finally { await context?.close(); }
await writeFile('evidence/versions.json', JSON.stringify(versions, null, 2));
console.log(JSON.stringify(versions, null, 2));
