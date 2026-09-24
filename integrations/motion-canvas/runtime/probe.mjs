import {chromium} from 'playwright';
import {readFile, writeFile, mkdir} from 'node:fs/promises';
import {createRequire} from 'node:module';
import process from 'node:process';
const require = createRequire(import.meta.url);
await mkdir('evidence', {recursive: true});
const versions = {node: process.version};
for (const name of ['@motion-canvas/core', '@motion-canvas/2d', '@motion-canvas/vite-plugin', 'vite', 'playwright']) {
  versions[name] = JSON.parse(await readFile(require.resolve(`${name}/package.json`), 'utf8')).version;
}
let browser;
try {
  browser = await chromium.launch({headless: true, timeout: 30000, chromiumSandbox: false, args: ['--no-zygote', '--disable-gpu']});
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.setContent('<canvas width="32" height="32"></canvas>');
  versions.browser = browser.version();
  versions.browserFamily = "chromium";
  versions.canvas = await page.evaluate(() => document.querySelector('canvas').getContext('2d') !== null);
} catch (error) {
  versions.browser_probe_error = String(error).slice(0, 8000);
} finally { await browser?.close(); }
await writeFile('evidence/versions.json', JSON.stringify(versions, null, 2));
console.log(JSON.stringify(versions, null, 2));
