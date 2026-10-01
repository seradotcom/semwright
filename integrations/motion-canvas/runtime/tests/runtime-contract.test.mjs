import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';

const pkg = JSON.parse(fs.readFileSync(new URL('../package.json', import.meta.url)));
const render = fs.readFileSync(new URL('../render.mjs', import.meta.url), 'utf8');
const nativeAuthoring = fs.readFileSync(new URL('../../../composition/motion/native.ts', import.meta.url), 'utf8');

test('runtime dependencies use exact versions', () => {
  for (const [name, version] of Object.entries({...pkg.dependencies, ...pkg.devDependencies})) {
    assert.match(version, /^\d+\.\d+\.\d+$/, `${name} must use an exact version`);
  }
  assert.equal(pkg.engines.node, '22.22.0');
  assert.equal(pkg.dependencies['@motion-canvas/core'], '3.17.2');
  assert.equal(pkg.dependencies.playwright, '1.61.1');
});

test('render harness is bound to Driver Host, pinned Firefox, and local origin', () => {
  assert.ok(render.includes('SEMWRIGHT_DRIVER_SANDBOX'));
  assert.ok(render.includes('landlock-bwrap-v1'));
  assert.ok(render.includes('buildForEditor:true'));
  assert.ok(render.includes('firefox.launchPersistentContext'));
  assert.ok(render.includes('context.pages()[0]'));
  assert.ok(!render.includes('context.newPage()'));
  assert.ok(render.includes('executablePath:a.browser'));
  assert.ok(render.includes("MOZ_DISABLE_CONTENT_SANDBOX:'1'"));
  assert.ok(render.includes("'dom.ipc.forkserver.enable':false"));
  assert.ok(!render.includes('MOZ_FORCE_DISABLE_E10S'));
  assert.ok(!render.includes('MOZ_WEBRENDER'));
  assert.ok(!render.includes('connectOverCDP'));
  assert.ok(render.includes('semwright.invalid'));
  assert.ok(render.includes('undefined, {timeout:config.timeoutMs}'));
  assert.ok(render.includes("route.abort('blockedbyclient')"));
});

test('render harness reports bounded state when browser rendering stalls', () => {
  assert.ok(render.includes("phase:'created'"));
  assert.ok(render.includes('render wait failed:'));
  assert.ok(render.includes('diagnostics.length < 32'));
});

test('renderer TypeErrors expose only allowlisted stack origins', () => {
  assert.ok(render.includes('function classifyLogStack(payload)'));
  assert.ok(render.includes("stack.includes('semwright-authoring-native')"));
  assert.ok(render.includes("stack.includes('semwright-exporter')"));
  assert.ok(render.includes("stack.includes('@motion-canvas/core')"));
  assert.ok(render.includes("stack.includes('@motion-canvas/2d')"));
  for (const classification of [
    'renderer_state_semwright_native',
    'renderer_state_semwright_exporter',
    'renderer_state_motion_core',
    'renderer_state_motion_2d',
  ]) {
    assert.ok(render.includes(classification));
  }
  assert.ok(!render.includes('renderer_log_type_error_semwright_native'));
  assert.ok(!render.includes('renderer_log_type_error_motion_core'));
});

test('authoring font evidence is derived from pinned Fontsource resources and browser readiness', () => {
  assert.equal(pkg.dependencies['@fontsource-variable/instrument-sans'], '5.3.0');
  assert.equal(pkg.dependencies['@fontsource/ibm-plex-mono'], '5.3.0');
  assert.ok(render.includes('pinnedFontEvidence'));
  assert.ok(render.includes('unicode-range'));
  assert.ok(render.includes('document.fonts.ready'));
  assert.ok(render.includes('document.fonts.check'));
  assert.ok(render.includes('font_resources_sha256'));
  assert.ok(!render.includes('font_ready:known(true)'));
});

test('native styled text runs use TxtLeaf rather than nested layout Txt nodes', () => {
  assert.ok(nativeAuthoring.includes('Txt,TxtLeaf,Code'));
  assert.ok(nativeAuthoring.includes('new TxtLeaf({text:r.text'));
  assert.ok(!nativeAuthoring.includes('new Txt({text:r.text'));
});
