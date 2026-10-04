import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawn } from 'node:child_process';
import { DatabaseSync } from 'node:sqlite';
import {
  NativeError, MAX_SAFE_INTEGER, MAX_VALUE_BYTES, validateValue, version, query,
  observation, requestIdentity, recoveryRecord, exactRequestDigest, applicationContext,
  dispatchApplication, type Json, type ResourceVersion, type ObservationPage, type RequestIdentity,
} from '../src/index.js';
import { Inventory, prepareRequest, RECEIPTS_PER_EPOCH, EVENT_WINDOW } from '../../../examples/native-inventory/application.js';

const PREFIX = 'driver.native-inventory.';
const GOLDEN = JSON.parse(readFileSync(join(process.cwd(), '..', '..', 'contracts', 'native', 'cooperation-vectors.json'), 'utf8')) as {
  schema_version: number;
  resource_version: ResourceVersion;
  request_digest: { domain: string; value: Json; sha256: string };
  safe_integer: number;
  large_integer_string: string;
};

const context = (expected: ResourceVersion | null = null) => applicationContext('owned-local-consumer', expected);
const request = (resource = 'inventory', limit = 256) => ({ resource, scope: 'stock', limit, cursor: null });
function fixture<T>(action: (app: Inventory, root: string) => T): T {
  const root = mkdtempSync(join(tmpdir(), 'native-owned-inventory-'));
  Inventory.provision(root); const app = new Inventory(root);
  try { return action(app, root); } finally { app.close(); rmSync(root, { recursive: true, force: true }); }
}
function view(app: Inventory, resource = 'inventory'): ObservationPage { return app.observe(request(resource), context()); }
function args(app: Inventory, name: string, key: string, parameters: Json, resource = 'inventory') {
  const before = view(app, resource); const epoch = Number((before.items[0] as Record<string, Json>).request_epoch);
  const identity = prepareRequest(PREFIX + name, before.version, epoch, key, parameters);
  return { before, identity, input: { request: identity, parameters } as unknown as Json };
}
function invoke(app: Inventory, name: string, key: string, parameters: Json, resource = 'inventory'): Json {
  const call = args(app, name, key, parameters, resource);
  return app.operations.get(PREFIX + name)!(call.input, context(call.before.version));
}
function row(page: ObservationPage, sku: string): Record<string, Json> {
  return page.items.find(item => (item as Record<string, Json>).sku === sku) as Record<string, Json>;
}
function code(error: unknown, expected: string): boolean { return error instanceof NativeError && error.code === expected; }
function effect(result: Json): Record<string, Json> {
  return ((result as Record<string, Json>).receipt as Record<string, Json>).effect as Record<string, Json>;
}

async function worker(message: unknown): Promise<{ code: number; reply: unknown }> {
  const path = fileURLToPath(new URL('./transaction-worker.js', import.meta.url));
  const child = spawn(process.execPath, [path], { stdio: ['pipe', 'pipe', 'pipe'] });
  let output = ''; let diagnostic = '';
  child.stdout.setEncoding('utf8').on('data', chunk => { output += chunk; });
  child.stderr.setEncoding('utf8').on('data', chunk => { diagnostic += chunk; });
  child.stdin.end(JSON.stringify(message));
  const timeout = setTimeout(() => child.kill('SIGKILL'), 15_000);
  try {
    const exit = await new Promise<number>((resolve, reject) => { child.on('error', reject); child.on('exit', code => resolve(code ?? -1)); });
    if (exit !== 0 && exit !== 42) throw new Error(`worker failed: ${diagnostic}`);
    return { code: exit, reply: output ? JSON.parse(output) : null };
  } finally { clearTimeout(timeout); }
}


test('Rust and TypeScript consume the same versioned cooperation golden vectors', () => {
  assert.equal(GOLDEN.schema_version, 1);
  assert.deepEqual(version(GOLDEN.resource_version), GOLDEN.resource_version);
  assert.equal(exactRequestDigest(GOLDEN.request_digest.domain, GOLDEN.request_digest.value), GOLDEN.request_digest.sha256);
  validateValue(GOLDEN.safe_integer);
  assert.throws(() => validateValue(Number(GOLDEN.large_integer_string)));
  assert.throws(() => exactRequestDigest('app/1', { value: 0.25 }));
});
test('TypeScript preserves large opaque revisions and rejects lossy numeric tokens', () => {
  assert.equal(version({ resource: 'stock', generation: 'g', revision: '18446744073709551616000001' }).revision, '18446744073709551616000001');
  assert.throws(() => version({ resource: 'stock', generation: 'g', revision: 1 }));
  assert.throws(() => version({ resource: 'stock', generation: 'g', revision: '' }));
});
test('Rust-compatible JSON bounds cover numbers, Unicode, cycles and bytes', () => {
  validateValue({ exact: MAX_SAFE_INTEGER, fraction: 0.25, negative: -MAX_SAFE_INTEGER });
  for (const value of [MAX_SAFE_INTEGER + 1, Infinity, NaN, 1n, '\ud800', 'x'.repeat(MAX_VALUE_BYTES)]) assert.throws(() => validateValue(value));
  const cyclic: Record<string, unknown> = {}; cyclic.self = cyclic; assert.throws(() => validateValue(cyclic));
  const shared = { value: 1 }; validateValue({ a: shared, b: shared });
  let accessorCalled = false;
  assert.throws(() => validateValue({ get value() { accessorCalled = true; return 1; } }));
  assert.equal(accessorCalled, false);
});
test('cursor revision, scope and progress are binding, not pagination suggestions', () => {
  const current = { resource: 'stock', generation: 'g', revision: '1' };
  const q = { resource: 'stock', scope: 'all', limit: 1, cursor: null };
  observation({ version: current, scope: 'all', items: [], next: null, complete: false }, q);
  assert.throws(() => query({ ...q, cursor: { version: current, scope: 'another', token: 'x' } }), error => code(error, 'StaleReference'));
  const cursor = { version: current, scope: 'all', token: 'x' };
  assert.throws(() => observation({ version: current, scope: 'all', items: [], next: cursor, complete: false }, { ...q, cursor }));
  assert.throws(() => observation({ version: { ...current, revision: '2' }, scope: 'all', items: [], next: null, complete: true }, { ...q, cursor }), error => code(error, 'StaleReference'));
});
test('recovery cannot upgrade UNKNOWN, mismatch or expired records into authority', () => {
  const identity = requestIdentity({ resource: 'stock', epoch: 2, key: 'k', request_sha256: 'a'.repeat(64) });
  recoveryRecord({ state: 'outcome_unknown', identity }, identity);
  recoveryRecord({ state: 'retention_expired', identity, current_epoch: 3 }, identity);
  assert.throws(() => recoveryRecord({ state: 'retention_expired', identity, current_epoch: 2 }, identity));
  assert.throws(() => recoveryRecord({ state: 'recorded', identity: { ...identity, request_sha256: 'b'.repeat(64) }, result: {} }, identity), error => code(error, 'Conflict'));
  assert.throws(() => recoveryRecord({ state: 'outcome_unknown', identity, approved: true }, identity));
});
test('exact app digest is independent of key insertion order and rejects undocumented floats', () => {
  assert.equal(exactRequestDigest('app/1', { b: 2, a: 1 }), exactRequestDigest('app/1', { a: 1, b: 2 }));
  assert.notEqual(exactRequestDigest('app/1', { a: 1 }), exactRequestDigest('app/2', { a: 1 }));
  assert.throws(() => exactRequestDigest('app/1', { value: 0.25 }));
});
test('read-only application can implement only one optional provider', async () => {
  const page = { version: { resource: 'books', generation: 'g', revision: 'catalog:one' }, scope: 'titles', items: ['First'], next: null, complete: true };
  const app = { observe: () => page };
  const result = await dispatchApplication(app, 'observe', null, { resource: 'books', scope: 'titles', limit: 1 }, context());
  assert.deepEqual(result, page);
  await assert.rejects(dispatchApplication(app, 'invoke', 'driver.books.edit', {}, context()), error => code(error, 'Unsupported'));
});
test('application owns a real SQLite schema, not the optional SDK document', () => fixture((app, root) => {
  assert.equal(readFileSync(join(root, 'supply.sqlite3')).subarray(0, 16).toString(), 'SQLite format 3\0');
  const before = view(app); assert.equal(before.version.revision, '9007199254740993');
  invoke(app, 'reserve', 'reserve-1', { sku: 'alpha', quantity: 3 });
  assert.equal(row(view(app), 'alpha').reserved, 3);
  assert.equal(view(app).version.revision, '9007199254740994');
}));
test('manual UI transaction invalidates stale SDK CAS without invalidating an independent resource', () => fixture(app => {
  const prepared = args(app, 'reserve', 'ui-race', { sku: 'alpha', quantity: 1 }); const spare = view(app, 'spare');
  app.manualAdjust('inventory', 'alpha', 321, prepared.before.version);
  assert.throws(() => app.operations.get(PREFIX + 'reserve')!(prepared.input, context(prepared.before.version)), error => code(error, 'StaleReference'));
  assert.equal(row(view(app), 'alpha').available, 321);
  assert.deepEqual(view(app, 'spare'), spare);
}));
test('two actual application processes sharing SQLite have exactly one CAS winner', async () => {
  const root = mkdtempSync(join(tmpdir(), 'native-cas-processes-')); Inventory.provision(root);
  const app = new Inventory(root); const prepared = args(app, 'reserve', 'race-a', { sku: 'alpha', quantity: 1 });
  const second = { ...prepared.input as Record<string, Json>, request: prepareRequest(PREFIX + 'reserve', prepared.before.version, prepared.identity.epoch, 'race-b', { sku: 'alpha', quantity: 1 }) };
  app.close();
  try {
    const results = await Promise.all([
      worker({ root, method: 'reserve', expected: prepared.before.version, args: prepared.input }),
      worker({ root, method: 'reserve', expected: prepared.before.version, args: second }),
    ]);
    const replies = results.map(result => result.reply as { ok: boolean; error?: string });
    assert.equal(replies.filter(result => result.ok).length, 1);
    assert.equal(replies.filter(result => result.error === 'StaleReference').length, 1);
    const reopened = new Inventory(root); try { assert.equal(row(view(reopened), 'alpha').reserved, 1); } finally { reopened.close(); }
  } finally { rmSync(root, { recursive: true, force: true }); }
});
test('key and digest bind parameters, operation, epoch and the complete expected revision', () => fixture(app => {
  const prepared = args(app, 'reserve', 'content-binding', { sku: 'alpha', quantity: 1 });
  const changed = { ...prepared.input as Record<string, Json>, parameters: { sku: 'alpha', quantity: 5 } };
  assert.throws(() => app.operations.get(PREFIX + 'reserve')!(changed, context(prepared.before.version)), error => code(error, 'Conflict'));
  assert.equal(row(view(app), 'alpha').reserved, 0);
}));
test('lost completion is recovered after reopening, without applying the operation twice', async () => {
  const root = mkdtempSync(join(tmpdir(), 'native-lost-result-')); Inventory.provision(root);
  const initial = new Inventory(root); const prepared = args(initial, 'reserve', 'lost', { sku: 'alpha', quantity: 4 }); initial.close();
  try {
    const result = await worker({ root, method: 'reserve', expected: prepared.before.version, args: prepared.input, discardReply: true });
    assert.equal(result.code, 42); assert.equal(result.reply, null);
    const reopened = new Inventory(root);
    try {
      const receipt = reopened.lookup(prepared.identity, context(view(reopened).version));
      assert.equal(receipt.state, 'recorded'); assert.equal(row(view(reopened), 'alpha').reserved, 4);
      assert.equal(reopened.lookup({ ...prepared.identity, key: 'never-seen' }, context()).state, 'outcome_unknown');
    } finally { reopened.close(); }
  } finally { rmSync(root, { recursive: true, force: true }); }
});
test('current application authorization precedes historical result lookup', () => fixture(app => {
  const prepared = args(app, 'reserve', 'access', { sku: 'alpha', quantity: 1 });
  app.operations.get(PREFIX + 'reserve')!(prepared.input, context(prepared.before.version));
  app.ownerSetAccess('inventory', false);
  assert.throws(() => app.lookup(prepared.identity, context()), error => code(error, 'PermissionDenied'));
  app.ownerSetAccess('inventory', true); assert.equal(app.lookup(prepared.identity, context()).state, 'recorded');
}));
test('cancelled local request performs no application effects', () => fixture(app => {
  const prepared = args(app, 'reserve', 'cancelled', { sku: 'alpha', quantity: 2 });
  const controller = new AbortController(); controller.abort();
  assert.throws(() => app.operations.get(PREFIX + 'reserve')!(prepared.input, applicationContext('cancel', prepared.before.version, controller.signal)), error => code(error, 'Cancelled'));
  assert.equal(row(view(app), 'alpha').reserved, 0);
}));
test('1300 durable operations retain bounded receipts and never replay forgotten epochs', { timeout: 120_000 }, () => fixture(app => {
  const first = args(app, 'reserve', 'retained-0', { sku: 'alpha', quantity: 1 });
  app.operations.get(PREFIX + 'reserve')!(first.input, context(first.before.version));
  for (let i = 1; i < 1300; ++i) invoke(app, 'reserve', `retained-${i}`, { sku: 'alpha', quantity: 1 });
  const state = app.diagnostics('inventory'); assert.ok(state.receipts <= RECEIPTS_PER_EPOCH * 2); assert.ok(state.events <= EVENT_WINDOW); assert.ok(state.epoch >= 10);
  assert.equal(app.lookup(first.identity, context()).state, 'retention_expired');
  const now = view(app); const expired = prepareRequest(PREFIX + 'reserve', now.version, first.identity.epoch, first.identity.key, { sku: 'alpha', quantity: 1 });
  assert.throws(() => app.operations.get(PREFIX + 'reserve')!({ request: expired as unknown as Json, parameters: { sku: 'alpha', quantity: 1 } }, context(now.version)), error => code(error, 'Conflict'));
  assert.equal(row(view(app), 'alpha').reserved, 1300);
}));
test('inventory pagination does not splice data across native revisions', () => fixture(app => {
  const first = app.observe(request('inventory', 1), context()); assert.equal(first.complete, false); assert.ok(first.next);
  const second = app.observe({ ...request('inventory', 1), cursor: first.next }, context()); assert.equal(second.complete, true);
  invoke(app, 'reserve', 'page-edit', { sku: 'alpha', quantity: 1 });
  assert.throws(() => app.observe({ ...request('inventory', 1), cursor: first.next }, context()), error => code(error, 'StaleReference'));
}));
test('event window requires resynchronization, not a fictional complete replay', () => fixture(app => {
  const start = view(app).version;
  for (let i = 0; i < EVENT_WINDOW + 2; ++i) invoke(app, 'reserve', `event-${i}`, { sku: 'alpha', quantity: 1 });
  const result = app.operations.get(PREFIX + 'events')!({ resource: 'inventory', limit: 10, cursor: JSON.stringify({ resource: 'inventory', generation: start.generation, sequence: 0 }) }, context()) as Record<string, Json>;
  assert.equal(result.resync_required, true); assert.deepEqual(result.events, []); assert.equal(result.historical_hints_only, true);
}));
test('snapshots, fresh-identity forks, restore and private destination CAS remain app transactions', () => fixture(app => {
  const captured = effect(invoke(app, 'snapshot', 'snapshot-a', {})); const snapshot = String(captured.snapshot_id);
  const forked = effect(invoke(app, 'fork', 'fork-a', { snapshot_id: snapshot })); const child = forked.workspace as Record<string, Json>; const resource = String(child.resource);
  assert.notEqual(resource, 'inventory'); assert.notEqual(child.generation, view(app).version.generation);
  invoke(app, 'reserve', 'child-edit', { sku: 'alpha', quantity: 5 }, resource);
  assert.equal(row(view(app), 'alpha').reserved, 0);
  const candidate = effect(invoke(app, 'snapshot', 'child-snapshot', {}, resource)); const candidateId = String(candidate.snapshot_id);
  const prepared = args(app, 'publish-private', 'stale-publication', { snapshot_id: candidateId, candidate_sha256: candidateId });
  app.manualAdjust('inventory', 'beta', 222, prepared.before.version);
  assert.throws(() => app.operations.get(PREFIX + 'publish-private')!(prepared.input, context(prepared.before.version)), error => code(error, 'StaleReference'));
  assert.throws(() => invoke(app, 'publish-private', 'wrong-candidate', { snapshot_id: candidateId, candidate_sha256: '0'.repeat(64) }), error => code(error, 'Conflict'));
  invoke(app, 'publish-private', 'private-publish', { snapshot_id: candidateId, candidate_sha256: candidateId });
  assert.equal(row(view(app), 'alpha').reserved, 5);
  const beforeRestore = view(app); invoke(app, 'restore', 'restore-a', { snapshot_id: snapshot });
  assert.notEqual(view(app).version.generation, beforeRestore.version.generation);
  assert.equal(row(view(app), 'alpha').reserved, 0);
  app.ownerCloseWorkspace(resource); assert.throws(() => view(app, resource), error => code(error, 'NotFound'));
}));
test('read-only database connection cannot borrow a write grant from operation JSON', () => fixture((app, root) => {
  const prepared = args(app, 'reserve', 'readonly', { sku: 'alpha', quantity: 1 }); const readonly = new Inventory(root, true);
  try { assert.throws(() => readonly.operations.get(PREFIX + 'reserve')!(prepared.input, context(prepared.before.version)), error => code(error, 'PermissionDenied')); }
  finally { readonly.close(); }
  assert.equal(row(view(app), 'alpha').reserved, 0);
}));

test('private publication is optional, destination-bound and not implicit authority', async () => {
  const destination = { resource: 'inventory', generation: 'generation-a', revision: 'publication-base' };
  const identity = requestIdentity({ resource: 'inventory', epoch: 2, key: 'publish-a', request_sha256: 'd'.repeat(64) });
  const candidate = { candidate_sha256: 'c'.repeat(64), destination, request: identity };
  const published = await dispatchApplication(
    { publish: value => ({ candidate_sha256: value.candidate_sha256, destination: value.destination.resource }) },
    'publish', null, candidate, context(destination),
  ) as Record<string, Json>;
  assert.equal(published.candidate_sha256, candidate.candidate_sha256);
  assert.equal(published.destination, 'inventory');
  await assert.rejects(
    dispatchApplication({}, 'publish', null, candidate, context(destination)),
    error => code(error, 'Unsupported'),
  );
  await assert.rejects(
    dispatchApplication({ publish: () => ({}) }, 'publish', null, candidate, context({ ...destination, revision: 'changed' })),
    error => code(error, 'StaleReference'),
  );
});

test('post-effect cancellation and lost recovery record preserve committed state as UNKNOWN', () => {
  const root = mkdtempSync(join(tmpdir(), 'native-recovery-record-loss-'));
  Inventory.provision(root);
  let app = new Inventory(root);
  const prepared = args(app, 'reserve', 'record-loss', { sku: 'alpha', quantity: 2 });
  app.operations.get(PREFIX + 'reserve')!(prepared.input, context(prepared.before.version));
  const committed = view(app);
  const controller = new AbortController(); controller.abort();
  assert.throws(
    () => app.lookup(prepared.identity, applicationContext('cancel-after-effect', committed.version, controller.signal)),
    error => code(error, 'Cancelled'),
  );
  assert.equal(row(committed, 'alpha').reserved, 2);
  app.close();

  const database = new DatabaseSync(join(root, 'supply.sqlite3'), { allowExtension: false });
  database.prepare('DELETE FROM receipts WHERE resource=? AND epoch=? AND key=?').run(
    prepared.identity.resource, prepared.identity.epoch, prepared.identity.key,
  );
  database.close();

  app = new Inventory(root);
  try {
    const current = view(app);
    assert.equal(app.lookup(prepared.identity, context(current.version)).state, 'outcome_unknown');
    assert.equal(row(current, 'alpha').reserved, 2);
    assert.throws(
      () => app.operations.get(PREFIX + 'reserve')!(prepared.input, context(current.version)),
      error => code(error, 'Conflict'),
    );
    assert.equal(row(view(app), 'alpha').reserved, 2);
  } finally {
    app.close();
    rmSync(root, { recursive: true, force: true });
  }
});
