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

test('renderer TypeErrors expose only allowlisted stack origins and normalized hints', () => {
  assert.ok(render.includes('function classifyLogStack(payload)'));
  assert.ok(render.includes('function classifyTypeErrorDetail(message)'));
  assert.ok(render.includes("'read:'+match[1]"));
  assert.ok(render.includes("'set:'+match[1]"));
  assert.ok(render.includes("'not_function:'+match[1]"));
  assert.ok(render.includes("(?:can't|Can't) access property"));
  assert.ok(render.includes('message.length>512'));
  assert.ok(render.includes('detail:failureDetail'));
  assert.equal((render.match(/failureDetail=typeof state\.typeErrorDetail/g) ?? []).length, 2);
  assert.ok(render.includes("stack.includes('semwright-authoring-native')"));
  assert.ok(render.includes("stack.includes('semwright-exporter')"));
  assert.ok(render.includes("stack.includes('@motion-canvas/core')"));
  assert.ok(render.includes("stack.includes('@motion-canvas_core')"));
  assert.ok(render.includes("stack.includes('@motion-canvas/2d')"));
  assert.ok(render.includes("stack.includes('@motion-canvas_2d')"));
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

test('native styled text preserves Motion Canvas public Txt layout contract', () => {
  assert.ok(nativeAuthoring.includes('new Txt({text:r.text'));
  assert.ok(!nativeAuthoring.includes('TxtLeaf'));
  assert.ok(nativeAuthoring.includes("case 'fixed':if(!(n instanceof Txt))n.layout(false)"));
});

test('native observation never forces layout geometry before the native draw', () => {
  const renderIndex=nativeAuthoring.indexOf('const result=render(context);');
  const matrixIndex=nativeAuthoring.indexOf('const matrix=context.getTransform().multiply(n.localToParent());');
  const bboxIndex=nativeAuthoring.indexOf('const box=n.cacheBBox();');
  assert.ok(renderIndex>=0);
  assert.ok(matrixIndex>renderIndex);
  assert.ok(bboxIndex>matrixIndex);
  assert.ok(nativeAuthoring.includes('reg.rendered.add(id)'));
  assert.ok(nativeAuthoring.includes("drawn:rendered"));
  assert.ok(!nativeAuthoring.includes('local_size:n instanceof Layout&&n.width()>0'));
});

test('native text probe normalizes DOMRectList before iteration', () => {
  assert.ok(nativeAuthoring.includes('Array.from(range.getClientRects())'));
  assert.ok(!nativeAuthoring.includes('[...range.getClientRects()]'));
});

test('native text digests use the bounded Node harness binding, not browser WebCrypto', () => {
  assert.ok(render.includes("page.exposeBinding('__SEMWRIGHT_TEXT_DIGEST__'"));
  assert.ok(render.includes("Buffer.byteLength(text, 'utf8') > 65_536"));
  assert.ok(render.includes("createHash('sha256').update(Buffer.from(text, 'utf8')).digest('hex')"));
  assert.ok(nativeAuthoring.includes('globalThis.__SEMWRIGHT_TEXT_DIGEST__'));
  assert.ok(nativeAuthoring.includes('native text digest binding unavailable'));
  assert.ok(!nativeAuthoring.includes('crypto.subtle.digest'));
});
