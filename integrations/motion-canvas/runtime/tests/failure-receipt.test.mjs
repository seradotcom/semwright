import assert from 'node:assert/strict';
import fs from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';

const source = await fs.readFile(new URL('../render.mjs', import.meta.url), 'utf8');
const paths = source.slice(source.indexOf('function safeRelative('), source.indexOf('async function containedFile('));
const receipts = source.slice(source.indexOf('const DIAGNOSTIC_PHASES'), source.indexOf('async function main()'));
const {failureReceipt, writeFailureReceipt} = new Function('fs', 'path', 'fail',
  `${paths}\n${receipts}\nreturn {failureReceipt, writeFailureReceipt};`,
)(fs, path, message => { throw new Error(message); });
const binding = 'a'.repeat(64);

test('local error details are UTF-8 bounded and classified by finite values', () => {
  const bytes = failureReceipt({name: 'TypeError', code: 'EACCES', stack: '🛩'.repeat(100000)}, 'runtime_module_load', binding, 'vite');
  const value = JSON.parse(bytes);
  assert.ok(bytes.length <= 64 * 1024);
  assert.ok(Buffer.byteLength(value.local_stack_only) <= 16 * 1024);
  assert.equal(value.render_input_digest, binding);
  assert.equal(value.runtime_module, 'vite');
  assert.equal(value.exception_code, 'EACCES');
  const unknown = JSON.parse(failureReceipt({name: '/private/path', code: 'secret', stack: 'local'}, 'arguments', binding, 'secret-module'));
  assert.equal(unknown.exception_name, 'OtherError');
  assert.equal(unknown.exception_code, null);
  assert.equal(unknown.runtime_module, null);
  assert.throws(() => failureReceipt(new Error(), '/private/class', binding));
  assert.throws(() => failureReceipt(new Error(), 'arguments', 'bad-binding'));
});

test('receipt publication is exclusive, private and bound to the selected output child', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'semwright-receipt-'));
  try {
    const relative = 'render-' + 'b'.repeat(32), target = path.join(root, relative);
    await fs.mkdir(target);
    const selected = {root: await fs.realpath(root), target, relative, binding};
    assert.equal(await writeFailureReceipt(new Error('local detail'), 'arguments', selected, 'vite'), true);
    const output = path.join(target, 'native-failure-receipt.json');
    const before = await fs.readFile(output);
    assert.equal((await fs.stat(output)).mode & 0o777, 0o600);
    await assert.rejects(writeFailureReceipt(new Error('replacement'), 'arguments', selected));
    assert.deepEqual(await fs.readFile(output), before);
    await assert.rejects(writeFailureReceipt(new Error(), 'arguments', {...selected, binding: 'invalid'}));
    await assert.rejects(writeFailureReceipt(new Error(), 'arguments', {...selected, relative: '../outside'}));
    await assert.rejects(writeFailureReceipt(new Error(), 'arguments', {...selected, target: root}));
  } finally { await fs.rm(root, {recursive: true, force: true}); }
});

test('output aliases cannot redirect private diagnostics to another directory', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'semwright-receipt-'));
  try {
    const relative = 'render-' + 'c'.repeat(32), target = path.join(root, relative);
    const other = path.join(root, 'other');
    await fs.mkdir(other);
    await fs.symlink(other, target);
    await assert.rejects(writeFailureReceipt(new Error(), 'arguments', {root, target, relative, binding}));
    assert.deepEqual(await fs.readdir(other), []);
  } finally { await fs.rm(root, {recursive: true, force: true}); }
});

test('render wait rejection keeps the first error without another browser query', async () => {
  const start = source.indexOf("    failurePhase = 'render_wait';");
  const end = source.indexOf('\n    const state = await page.evaluate', start);
  assert.ok(start >= 0 && end > start);
  const wait = new Function('page', 'config', 'fail',
    'return (async () => { let failurePhase; const diagnostics = [];\n' +
    source.slice(start, end) + '\n})();');
  const first = new Error('original render wait failure');
  first.name = 'TimeoutError';
  let queries = 0;
  const result = await Promise.race([
    wait({
      async waitForFunction() { throw first; },
      evaluate() { queries++; return new Promise(() => {}); },
    }, {timeoutMs: 25}, message => { throw new Error(message); })
      .then(() => ({resolved: true}), error => ({error})),
    new Promise(resolve => setImmediate(() => resolve({pending: true}))),
  ]);
  assert.deepEqual(result, {error: first});
  assert.equal(queries, 0);
});

function mainFailureBoundary(first, cleanup, publishFailureOnce) {
  const finalize = source.indexOf("    failurePhase = 'finalize';");
  const begin = source.indexOf('\n  }', finalize);
  const end = source.indexOf('\n}\nmain().catch', begin);
  assert.ok(finalize >= 0 && begin > finalize && end > begin);
  return new Function('first', 'cleanup', 'publishFailureOnce',
    'return (async () => { try { throw first;' + source.slice(begin, end) + '\n})();',
  )(first, cleanup, publishFailureOnce);
}

test('first-error custody publishes before a pending browser cleanup', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'semwright-first-error-'));
  let finishCleanup;
  let cleanupStarted;
  const cleanup = new Promise(resolve => { finishCleanup = resolve; });
  const started = new Promise(resolve => { cleanupStarted = resolve; });
  const first = new Error('original failure before cleanup');
  const relative = 'render-' + 'd'.repeat(32), target = path.join(root, relative);
  await fs.mkdir(target);
  const selected = {root: await fs.realpath(root), target, relative, binding};
  const outcome = mainFailureBoundary(first, async () => {
    cleanupStarted();
    await cleanup;
  }, error => writeFailureReceipt(error, 'render_wait_timeout', selected))
    .then(() => ({resolved: true}), error => ({error}));
  try {
    await started;
    const bytes = await fs.readFile(path.join(target, 'native-failure-receipt.json'))
      .catch(error => { if (error.code === 'ENOENT') return null; throw error; });
    assert.ok(bytes, 'the first failure must already be durable when cleanup begins');
    const receipt = JSON.parse(bytes);
    assert.equal(receipt.error_class, 'render_wait_timeout');
    assert.equal(receipt.render_input_digest, binding);
    assert.equal(receipt.local_stack_only, first.stack);
  } finally {
    finishCleanup();
    assert.deepEqual(await outcome, {error: first});
    await fs.rm(root, {recursive: true, force: true});
  }
});

function publicationHarness(fsImplementation = fs) {
  const start = source.indexOf("let failurePhase = 'startup';");
  const end = source.indexOf('async function main()', start);
  assert.ok(start >= 0 && end > start);
  return new Function('fs', 'path', 'fail',
    paths + '\n' + source.slice(start, end) +
    '\nreturn {publishFailureOnce, select(output, phase, module) {' +
    'failureOutput = output; failurePhase = phase; failureModule = module;}};',
  )(fsImplementation, path, message => { throw new Error(message); });
}

test('first-error custody does not retry failed publication in the outer handler', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'semwright-first-error-'));
  try {
    const relative = 'render-' + 'e'.repeat(32), target = path.join(root, relative);
    await fs.mkdir(target);
    let attempts = 0;
    const harness = publicationHarness({...fs, async open() {
      attempts++;
      const error = new Error('controlled publication failure');
      error.code = 'EACCES';
      throw error;
    }});
    harness.select({root: await fs.realpath(root), target, relative, binding}, 'render_wait_timeout');
    const first = new Error('original render wait failure');
    await assert.rejects(mainFailureBoundary(first, async () => {}, harness.publishFailureOnce),
      error => error === first);
    const publication = await harness.publishFailureOnce(new Error('later failure'));
    assert.deepEqual(publication, {errorClass: 'render_wait_timeout', diagnosticPersisted: false});
    assert.equal(attempts, 1);
    assert.deepEqual(await fs.readdir(target), []);
  } finally { await fs.rm(root, {recursive: true, force: true}); }
});

test('first-error custody publishes the original setup failure once', async () => {
  const root = await fs.mkdtemp(path.join(os.tmpdir(), 'semwright-first-error-'));
  try {
    const relative = 'render-' + 'f'.repeat(32), target = path.join(root, relative);
    await fs.mkdir(target);
    const harness = publicationHarness();
    harness.select({root: await fs.realpath(root), target, relative, binding}, 'runtime_module_load', 'vite');
    const first = new Error('original module load failure');
    const publication = harness.publishFailureOnce(first);
    assert.equal(harness.publishFailureOnce(new Error('later failure')), publication);
    assert.deepEqual(await publication, {errorClass: 'runtime_module_load', diagnosticPersisted: true});
    const receipt = JSON.parse(await fs.readFile(path.join(target, 'native-failure-receipt.json')));
    assert.equal(receipt.error_class, 'runtime_module_load');
    assert.equal(receipt.runtime_module, 'vite');
    assert.equal(receipt.render_input_digest, binding);
    assert.equal(receipt.local_stack_only, first.stack);
    assert.deepEqual(await fs.readdir(target), ['native-failure-receipt.json']);
  } finally { await fs.rm(root, {recursive: true, force: true}); }
});
