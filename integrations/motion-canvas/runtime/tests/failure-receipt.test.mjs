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
