import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';

// Exercise the same functions serialized into the browser harness, without
// launching Vite, a browser or a renderer during these boundary tests.
const source = fs.readFileSync(new URL('../render.mjs', import.meta.url), 'utf8');
const functions = source.slice(source.indexOf('function stableFrameSeconds('), source.indexOf('function harnessPlugin('));
const {stableFrameSeconds, installSingletonTailGuard, boundedRendererLog} = new Function(
  `${functions}; return {stableFrameSeconds, installSingletonTailGuard, boundedRendererLog};`,
)();

test('integer frame ranges round trip through the native ceil clock', () => {
  for (const fps of [1, 12, 24, 25, 30, 48, 50, 60, 120, 240, 24000 / 1001, 30000 / 1001]) {
    for (let frame = 0; frame <= 1437; frame++) {
      assert.equal(Math.ceil(stableFrameSeconds(frame, fps) * fps), frame);
    }
  }
  for (const [frame, fps] of [[-1, 25], [55.5, 25], [NaN, 25], [Infinity, 25], [2 ** 53, 25], [55, 0], [55, -1], [55, NaN], [55, Infinity]]) {
    assert.throws(() => stableFrameSeconds(frame, fps));
  }
});

function fixture(firstFrame = 55, endFrameExclusive = 56) {
  const accepted = [], seen = new Set(), observed = {}, signal = {aborted: false};
  const exporter = {async handleFrame(_canvas, frame, _sceneFrame, _sceneName, signal) {
    if (signal.aborted) return;
    if (!Number.isSafeInteger(frame) || frame < firstFrame || frame >= endFrameExclusive || seen.has(frame)) {
      throw new Error('invalid frame payload');
    }
    seen.add(frame);
    accepted.push(frame);
  }};
  const factory = {async create() { return exporter; }};
  installSingletonTailGuard(factory, {firstFrame, endFrameExclusive}, observed);
  return {factory, accepted, observed, signal};
}

test('only one native singleton tail is dropped after the awaited requested frame', async () => {
  for (const first of [0, 55, 117, 1436]) {
    const value = fixture(first, first + 1);
    const exporter = await value.factory.create();
    await exporter.handleFrame({}, first, 0, 'test', value.signal);
    await exporter.handleFrame({}, first + 1, 0, 'test', value.signal);
    assert.deepEqual(value.accepted, [first]);
    assert.equal(value.observed.singletonTailFiltered, true);
    await assert.rejects(exporter.handleFrame({}, first + 1, 0, 'test', value.signal));
  }
});

test('out of order, duplicate, noninteger and unrelated frames reach the strict binding', async () => {
  for (const sequence of [[56], [54], [57], [55.5], ['55'], [55, 55], [55, 57]]) {
    const value = fixture();
    const exporter = await value.factory.create();
    await assert.rejects(async () => {
      for (const frame of sequence) await exporter.handleFrame({}, frame, 0, 'test', value.signal);
    });
  }
  const value = fixture(55, 58), exporter = await value.factory.create();
  for (const frame of [55, 56, 57]) await exporter.handleFrame({}, frame, 0, 'test', value.signal);
  assert.deepEqual(value.accepted, [55, 56, 57]);
  assert.equal(value.observed.singletonTailFiltered, undefined);
  await assert.rejects(exporter.handleFrame({}, 58, 0, 'test', value.signal));
});

test('an aborted or failed export cannot authorize dropping the following frame', async () => {
  const observed = {}, signal = {aborted: false};
  const factory = {async create() { return {async handleFrame() { throw new Error('native export failed'); }}; }};
  installSingletonTailGuard(factory, {firstFrame: 55, endFrameExclusive: 56}, observed);
  const exporter = await factory.create();
  await assert.rejects(exporter.handleFrame({}, 55, 0, 'test', signal));
  await assert.rejects(exporter.handleFrame({}, 56, 0, 'test', signal));
  assert.equal(observed.singletonTailFiltered, undefined);
  const value = fixture(), abortedExporter = await value.factory.create();
  await abortedExporter.handleFrame({}, 55, 0, 'test', value.signal);
  value.signal.aborted = true;
  await abortedExporter.handleFrame({}, 56, 0, 'test', value.signal);
  assert.equal(value.observed.singletonTailFiltered, undefined);
});

test('renderer diagnostics handle nonenumerable fields, hostile getters and UTF-8 budgets', () => {
  const diagnostic = boundedRendererLog(new Error('native error'));
  assert.equal(diagnostic.name, 'Error');
  assert.equal(diagnostic.message, 'native error');
  assert.match(diagnostic.stack, /native error/);
  const large = boundedRendererLog({name: 'Error', message: '🛩'.repeat(100000), stack: '\u0001'.repeat(100000)});
  assert.ok(Buffer.byteLength(JSON.stringify(large)) <= 12 * 1024);
  assert.ok(Buffer.byteLength(large.message) <= 2048);
  assert.ok(Buffer.byteLength(large.stack) <= 8192);
  assert.deepEqual(boundedRendererLog({get name() { throw new Error(); }, get message() { throw new Error(); }, get stack() { throw new Error(); }}), {name: '', message: '', stack: ''});
});
