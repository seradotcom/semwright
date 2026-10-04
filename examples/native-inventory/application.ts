/* An application-owned transactional inventory. The SDK does not own this schema. */
import { DatabaseSync, type StatementSync } from 'node:sqlite';
import { randomUUID } from 'node:crypto';
import { existsSync, lstatSync } from 'node:fs';
import { join, isAbsolute } from 'node:path';
import {
  NativeError, requireCondition, object, text, integer, query, version, sameVersion,
  requestIdentity, exactRequestDigest, checkCancelled, observation,
  type Json, type CallContext, type Query, type ResourceVersion, type ObservationPage,
  type RequestIdentity, type RecoveryRecord, type NativeApplication, type PrivatePublication,
} from '../../sdk/native-typescript/src/index.js';

export const APP_VERSION = '1.0.0';
export const RECEIPTS_PER_EPOCH = 128;
export const EVENT_WINDOW = 128;
const PREFIX = 'driver.native-inventory.';
type Row = Record<string, string | number | null>;
type Parameter = string | number | null;
interface Stock { sku: string; available: number; reserved: number }
interface ResourceRow { id: string; incarnation: string; revision: string; epoch: number; floor_epoch: number; used: number; sequence: number; enabled: number; is_private: number }
interface SnapshotData { source: ResourceVersion; items: Stock[] }

export function prepareRequest(command: string, expected: ResourceVersion, epoch: number, key: string, parameters: Json): RequestIdentity {
  const base = { command, app_version: APP_VERSION, expected: version(expected), epoch: integer(epoch, 0, Number.MAX_SAFE_INTEGER), key: text(key, 128), parameters };
  return { resource: expected.resource, epoch, key, request_sha256: exactRequestDigest('inventory-request/1', base) };
}

/** Both the local UI and the Native SDK use these native transactions. */
export class Inventory implements NativeApplication {
  private readonly db: DatabaseSync;
  private readonly statements = new Map<string, StatementSync>();
  private readonly readOnly: boolean;
  readonly operations: ReadonlyMap<string, (args: Json, context: CallContext) => Json>;

  static provision(root: string): void {
    requireCondition(isAbsolute(root) && lstatSync(root).isDirectory() && !lstatSync(root).isSymbolicLink(), 'Inventory root must be an owner-provisioned directory');
    const path = join(root, 'supply.sqlite3');
    requireCondition(!existsSync(path), 'Inventory already exists', 'Conflict');
    const db = new DatabaseSync(path, { allowExtension: false, timeout: 2000 });
    try {
      db.exec(`
        PRAGMA journal_mode=DELETE;
        PRAGMA synchronous=FULL;
        PRAGMA foreign_keys=ON;
        BEGIN IMMEDIATE;
        CREATE TABLE resources (
          id TEXT PRIMARY KEY, incarnation TEXT NOT NULL, revision TEXT NOT NULL,
          epoch INTEGER NOT NULL CHECK(epoch>=0), floor_epoch INTEGER NOT NULL CHECK(floor_epoch>=0),
          used INTEGER NOT NULL CHECK(used>=0), sequence INTEGER NOT NULL CHECK(sequence>=0),
          enabled INTEGER NOT NULL CHECK(enabled IN (0,1)), is_private INTEGER NOT NULL CHECK(is_private IN (0,1))
        ) STRICT;
        CREATE TABLE stock (
          resource TEXT NOT NULL REFERENCES resources(id) ON DELETE CASCADE,
          sku TEXT NOT NULL, available INTEGER NOT NULL CHECK(available>=0),
          reserved INTEGER NOT NULL CHECK(reserved>=0), PRIMARY KEY(resource,sku)
        ) STRICT;
        CREATE TABLE receipts (
          resource TEXT NOT NULL REFERENCES resources(id) ON DELETE CASCADE,
          epoch INTEGER NOT NULL, key TEXT NOT NULL, digest TEXT NOT NULL,
          result TEXT NOT NULL, PRIMARY KEY(resource,epoch,key)
        ) STRICT;
        CREATE TABLE events (
          resource TEXT NOT NULL REFERENCES resources(id) ON DELETE CASCADE,
          sequence INTEGER NOT NULL, generation TEXT NOT NULL, payload TEXT NOT NULL,
          PRIMARY KEY(resource,sequence)
        ) STRICT;
        CREATE TABLE snapshots (
          id TEXT PRIMARY KEY, resource TEXT NOT NULL, source_revision TEXT NOT NULL,
          payload TEXT NOT NULL, serial INTEGER NOT NULL
        ) STRICT;
        CREATE TABLE derivations (
          child TEXT PRIMARY KEY REFERENCES resources(id) ON DELETE CASCADE,
          source TEXT NOT NULL, source_generation TEXT NOT NULL, source_revision TEXT NOT NULL,
          snapshot_id TEXT NOT NULL REFERENCES snapshots(id)
        ) STRICT;
        PRAGMA user_version=1;
      `);
      const insert = db.prepare('INSERT INTO resources VALUES (?,?,?,0,0,0,0,1,1)');
      for (const id of ['inventory', 'spare']) insert.run(id, randomUUID(), '9007199254740993');
      const stock = db.prepare('INSERT INTO stock VALUES (?,?,?,0)');
      stock.run('inventory', 'alpha', 100_000); stock.run('inventory', 'beta', 200);
      stock.run('spare', 'omega', 7);
      db.exec('COMMIT');
    } catch (error) {
      if (db.isTransaction) db.exec('ROLLBACK');
      throw error;
    } finally { db.close(); }
  }

  constructor(root: string, readOnly = false) {
    requireCondition(isAbsolute(root), 'Inventory root must be absolute');
    const path = join(root, 'supply.sqlite3');
    const metadata = lstatSync(path);
    requireCondition(metadata.isFile() && !metadata.isSymbolicLink(), 'Inventory database must be an existing regular application file');
    this.readOnly = readOnly;
    this.db = new DatabaseSync(path, { readOnly, allowExtension: false, timeout: 2000 });
    this.db.exec('PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;');
    if (readOnly) this.db.exec('PRAGMA query_only=ON;');
    requireCondition(this.one('PRAGMA user_version').user_version === 1, 'Unsupported application database schema', 'ProtocolMismatch');
    this.operations = new Map([
      ['reserve', (args: Json, ctx: CallContext) => this.mutate('reserve', args, ctx)],
      ['release', (args: Json, ctx: CallContext) => this.mutate('release', args, ctx)],
      ['snapshot', (args: Json, ctx: CallContext) => this.mutate('snapshot', args, ctx)],
      ['fork', (args: Json, ctx: CallContext) => this.mutate('fork', args, ctx)],
      ['restore', (args: Json, ctx: CallContext) => this.mutate('restore', args, ctx)],
      ['publish-private', (args: Json, ctx: CallContext) => this.mutate('publish-private', args, ctx)],
      ['events', (args: Json, ctx: CallContext) => this.events(args, ctx)],
    ].map(([name, handler]) => [PREFIX + name, handler] as [string, (args: Json, context: CallContext) => Json]));
  }

  publish(candidate: PrivatePublication, context: CallContext): Json {
    checkCancelled(context);
    requireCondition(
      context.expected !== null
        && context.expected.resource === candidate.destination.resource
        && context.expected.generation === candidate.destination.generation
        && context.expected.revision === candidate.destination.revision,
      'Publication destination is stale',
      'StaleReference',
    );
    return this.mutate('publish-private', {
      request: candidate.request as unknown as Json,
      parameters: {
        snapshot_id: candidate.candidate_sha256,
        candidate_sha256: candidate.candidate_sha256,
      },
    }, context);
  }

  close(): void { this.statements.clear(); this.db.close(); }
  private statement(sql: string): StatementSync {
    let stmt = this.statements.get(sql);
    if (!stmt) { stmt = this.db.prepare(sql); this.statements.set(sql, stmt); }
    return stmt;
  }
  private one(sql: string, ...parameters: Parameter[]): Row {
    const row = this.statement(sql).get(...parameters);
    requireCondition(row !== undefined, 'Application resource is absent', 'NotFound');
    return row as Row;
  }
  private all(sql: string, ...parameters: Parameter[]): Row[] { return this.statement(sql).all(...parameters) as Row[]; }
  private run(sql: string, ...parameters: Parameter[]): void { this.statement(sql).run(...parameters); }
  private transaction<T>(write: boolean, action: () => T): T {
    requireCondition(!write || !this.readOnly, 'Application connection is read-only', 'PermissionDenied');
    let commitAttempted = false;
    try {
      this.db.exec(write ? 'BEGIN IMMEDIATE' : 'BEGIN');
      const result = action(); commitAttempted = true; this.db.exec('COMMIT'); return result;
    } catch (error) {
      let rollbackConfirmed = true;
      if (this.db.isTransaction) { try { this.db.exec('ROLLBACK'); } catch { rollbackConfirmed = false; } }
      if (commitAttempted || !rollbackConfirmed) throw new NativeError('BackendFailed', 'Database commit result is uncertain', false);
      if (error instanceof NativeError) throw error;
      throw new NativeError('BackendFailed', 'Application transaction rejected before commit');
    }
  }
  private resource(id: string, requireAccess = true): ResourceRow {
    text(id, 512);
    const row = this.one('SELECT * FROM resources WHERE id=?', id) as unknown as ResourceRow;
    requireCondition(!requireAccess || row.enabled === 1, 'Application currently denies access to this resource', 'PermissionDenied');
    return row;
  }
  private resourceVersion(row: ResourceRow): ResourceVersion {
    return { resource: row.id, generation: row.incarnation, revision: row.revision };
  }
  private stocks(id: string): Stock[] { return this.all('SELECT sku,available,reserved FROM stock WHERE resource=? ORDER BY sku', id) as unknown as Stock[]; }
  private compare(current: ResourceRow, expected: ResourceVersion | null): void {
    requireCondition(expected !== null && sameVersion(this.resourceVersion(current), expected), 'Application transaction CAS rejected', 'StaleReference');
  }

  observe(raw: Query, context: CallContext): ObservationPage {
    checkCancelled(context);
    const request = query(raw);
    requireCondition(request.scope === 'stock', 'Unknown inventory observation scope', 'NotFound');
    return this.transaction(false, () => {
      const row = this.resource(request.resource); const current = this.resourceVersion(row);
      if (request.cursor) requireCondition(sameVersion(request.cursor.version, current), 'Inventory changed while enumerating', 'StaleReference');
      let after = '';
      if (request.cursor) { after = text(request.cursor.token, 128); requireCondition(/^[a-z0-9-]+$/u.test(after), 'Invalid stock continuation'); }
      const values = this.all('SELECT sku,available,reserved FROM stock WHERE resource=? AND sku>? ORDER BY sku LIMIT ?', row.id, after, request.limit + 1);
      const more = values.length > request.limit; const selected = values.slice(0, request.limit);
      const result: ObservationPage = {
        version: current, scope: 'stock', complete: !more,
        items: selected.map(item => ({ sku: String(item.sku), available: Number(item.available), reserved: Number(item.reserved), request_epoch: row.epoch, oldest_recoverable_epoch: row.floor_epoch })),
        next: more ? { version: { ...current }, scope: 'stock', token: String(selected.at(-1)!.sku) } : null,
      };
      return observation(result, request);
    });
  }

  lookup(raw: RequestIdentity, context: CallContext): RecoveryRecord {
    checkCancelled(context); const identity = requestIdentity(raw);
    return this.transaction(false, () => {
      // Current authorization is checked before any historical record is read.
      const resource = this.resource(identity.resource);
      requireCondition(context.expected === null || context.expected.resource === resource.id, 'Recovery resource differs from current observation', 'StaleReference');
      if (identity.epoch < resource.floor_epoch) return { state: 'retention_expired', identity, current_epoch: resource.epoch };
      const stored = this.statement('SELECT digest,result FROM receipts WHERE resource=? AND epoch=? AND key=?').get(identity.resource, identity.epoch, identity.key);
      if (!stored) return { state: 'outcome_unknown', identity };
      requireCondition(stored.digest === identity.request_sha256, 'Operation key belongs to different request content', 'Conflict');
      return { state: 'recorded', identity, result: JSON.parse(String(stored.result)) as Json };
    });
  }

  private mutate(suffix: string, args: Json, context: CallContext): Json {
    checkCancelled(context);
    const input = object(args, ['ref', 'request', 'parameters'], ['request', 'parameters']);
    const identity = requestIdentity(input.request); const parameters = object(input.parameters);
    requireCondition(context.expected !== null && identity.resource === context.expected.resource, 'Mutation requires the current full application revision', 'StaleReference');
    const expected = context.expected;
    const claimed = prepareRequest(PREFIX + suffix, expected, identity.epoch, identity.key, parameters as Json);
    requireCondition(claimed.request_sha256 === identity.request_sha256, 'Request digest does not bind operation, parameters and revision', 'Conflict');
    return this.transaction(true, () => {
      checkCancelled(context);
      const before = this.resource(identity.resource);
      const stored = this.statement('SELECT digest,result FROM receipts WHERE resource=? AND epoch=? AND key=?').get(identity.resource, identity.epoch, identity.key);
      if (stored) {
        requireCondition(stored.digest === identity.request_sha256, 'Operation key belongs to another request', 'Conflict');
        return { historical_only: true, current_authority: false, deduplicated: true, receipt: JSON.parse(String(stored.result)) as Json };
      }
      // An expired or closed epoch never becomes permission to execute a forgotten key.
      requireCondition(identity.epoch === before.epoch, 'Request epoch is closed; inspect and create a new request, never replay the old one', 'Conflict');
      this.compare(before, expected);
      const effect = this.apply(suffix, parameters, before);
      const incarnation = this.resource(before.id).incarnation;
      const revision = (BigInt(before.revision) + 1n).toString();
      requireCondition(revision.length <= 512, 'Application revision exhausted', 'ResourceExhausted');
      this.run('UPDATE resources SET revision=?,incarnation=? WHERE id=?', revision, incarnation, before.id);
      const committed = this.resourceVersion(this.resource(before.id));
      const receipt: Json = { command: PREFIX + suffix, request: { ...identity }, committed_version: { ...committed }, effect };
      this.run('INSERT INTO receipts(resource,epoch,key,digest,result) VALUES (?,?,?,?,?)', before.id, identity.epoch, identity.key, identity.request_sha256, JSON.stringify(receipt));
      const used = before.used + 1;
      if (used >= RECEIPTS_PER_EPOCH) {
        const nextEpoch = integer(before.epoch + 1, 1, Number.MAX_SAFE_INTEGER);
        const floor = Math.max(0, nextEpoch - 1);
        this.run('UPDATE resources SET epoch=?,floor_epoch=?,used=0 WHERE id=?', nextEpoch, floor, before.id);
        this.run('DELETE FROM receipts WHERE resource=? AND epoch<?', before.id, floor);
      } else this.run('UPDATE resources SET used=? WHERE id=?', used, before.id);
      this.recordEvent(before.id, PREFIX + suffix, { request_key: identity.key, committed_version: { ...committed } });
      return { historical_only: true, current_authority: false, deduplicated: false, receipt };
    });
  }

  private apply(suffix: string, parameters: Record<string, unknown>, before: ResourceRow): Json {
    if (suffix === 'reserve' || suffix === 'release') {
      object(parameters, ['sku', 'quantity'], ['sku', 'quantity']);
      const sku = text(parameters.sku, 128); const quantity = integer(parameters.quantity, 1, 10_000);
      const item = this.one('SELECT available,reserved FROM stock WHERE resource=? AND sku=?', before.id, sku);
      const available = Number(item.available); const reserved = Number(item.reserved);
      requireCondition((suffix === 'reserve' ? available : reserved) >= quantity, 'Insufficient stock or reservation', 'Conflict');
      const delta = suffix === 'reserve' ? quantity : -quantity;
      integer(available - delta, 0, Number.MAX_SAFE_INTEGER); integer(reserved + delta, 0, Number.MAX_SAFE_INTEGER);
      this.run('UPDATE stock SET available=?,reserved=? WHERE resource=? AND sku=?', available - delta, reserved + delta, before.id, sku);
      return { sku, available: available - delta, reserved: reserved + delta };
    }
    if (suffix === 'snapshot') {
      object(parameters, []);
      const data: SnapshotData = { source: this.resourceVersion(before), items: this.stocks(before.id) };
      const id = exactRequestDigest('inventory-snapshot/1', data);
      this.run('INSERT OR IGNORE INTO snapshots(id,resource,source_revision,payload,serial) VALUES (?,?,?,?,?)', id, before.id, before.revision, JSON.stringify(data), before.sequence + 1);
      // Retain referenced snapshots; discard old unreferenced snapshots, not live workspaces.
      this.run('DELETE FROM snapshots WHERE id IN (SELECT id FROM snapshots WHERE resource=? AND id NOT IN (SELECT snapshot_id FROM derivations) ORDER BY serial DESC LIMIT -1 OFFSET 128)', before.id);
      return { snapshot_id: id, content_sha256: id, source: { ...data.source } };
    }
    if (suffix === 'fork') {
      object(parameters, ['snapshot_id'], ['snapshot_id']);
      const { id, data } = this.snapshot(parameters.snapshot_id);
      requireCondition(Number(this.one('SELECT COUNT(*) AS count FROM resources').count) < 64, 'Active workspace budget; owner must close an unused workspace', 'ResourceExhausted');
      const child = 'workspace-' + randomUUID();
      this.run('INSERT INTO resources VALUES (?,?,?,0,0,0,0,1,1)', child, randomUUID(), '1');
      this.replaceStock(child, data.items);
      this.run('INSERT INTO derivations VALUES (?,?,?,?,?)', child, data.source.resource, data.source.generation, data.source.revision, id);
      return { workspace: this.resourceVersion(this.resource(child)) as unknown as Json, source: { ...data.source }, snapshot_id: id };
    }
    if (suffix === 'restore' || suffix === 'publish-private') {
      object(parameters, suffix === 'restore' ? ['snapshot_id'] : ['snapshot_id', 'candidate_sha256'], suffix === 'restore' ? ['snapshot_id'] : ['snapshot_id', 'candidate_sha256']);
      const { id, data } = this.snapshot(parameters.snapshot_id);
      if (suffix === 'publish-private') {
        requireCondition(before.is_private === 1, 'Destination is not configured for private publication', 'PermissionDenied');
        requireCondition(parameters.candidate_sha256 === id, 'Publication candidate differs from reviewed immutable content', 'Conflict');
      }
      this.replaceStock(before.id, data.items);
      if (suffix === 'restore') this.run('UPDATE resources SET incarnation=? WHERE id=?', randomUUID(), before.id);
      return { snapshot_id: id, destination: before.id, publication: suffix === 'publish-private' ? 'private-application-transaction' : 'not-publication' };
    }
    throw new NativeError('Unsupported', 'Mutation is unavailable');
  }
  private snapshot(raw: unknown): { id: string; data: SnapshotData } {
    const id = text(raw, 64); requireCondition(/^[0-9a-f]{64}$/u.test(id), 'Invalid snapshot identity');
    const stored = this.one('SELECT payload FROM snapshots WHERE id=?', id);
    const data = JSON.parse(String(stored.payload)) as SnapshotData;
    requireCondition(exactRequestDigest('inventory-snapshot/1', data) === id, 'Stored snapshot no longer matches its identity', 'Conflict');
    version(data.source);
    return { id, data };
  }
  private replaceStock(resource: string, items: Stock[]): void {
    requireCondition(items.length <= 256, 'Snapshot stock budget', 'ResourceExhausted');
    this.run('DELETE FROM stock WHERE resource=?', resource);
    for (const item of items) {
      text(item.sku, 128); integer(item.available, 0, Number.MAX_SAFE_INTEGER); integer(item.reserved, 0, Number.MAX_SAFE_INTEGER);
      this.run('INSERT INTO stock VALUES (?,?,?,?)', resource, item.sku, item.available, item.reserved);
    }
  }
  private recordEvent(resource: string, command: string, payload: Json): void {
    const row = this.resource(resource, false); const sequence = integer(row.sequence + 1, 1, Number.MAX_SAFE_INTEGER);
    this.run('UPDATE resources SET sequence=? WHERE id=?', sequence, resource);
    this.run('INSERT INTO events VALUES (?,?,?,?)', resource, sequence, row.incarnation, JSON.stringify({ command, payload }));
    this.run('DELETE FROM events WHERE resource=? AND sequence<=?', resource, Math.max(0, sequence - EVENT_WINDOW));
  }
  private events(args: Json, context: CallContext): Json {
    checkCancelled(context);
    const input = object(args, ['ref', 'resource', 'cursor', 'limit'], ['resource', 'limit']);
    const resource = text(input.resource, 512); const limit = integer(input.limit, 1, 256);
    return this.transaction(false, () => {
      const row = this.resource(resource); let after = 0; let resync = false;
      if (input.cursor !== undefined && input.cursor !== null) {
        const cursor = object(JSON.parse(text(input.cursor, 1024)), ['resource', 'generation', 'sequence'], ['resource', 'generation', 'sequence']);
        requireCondition(cursor.resource === resource, 'Event cursor resource differs', 'StaleReference');
        after = integer(cursor.sequence, 0, Number.MAX_SAFE_INTEGER);
        resync = cursor.generation !== row.incarnation || after < Math.max(0, row.sequence - EVENT_WINDOW) || after > row.sequence;
      }
      const events = resync ? [] : this.all('SELECT sequence,generation,payload FROM events WHERE resource=? AND sequence>? ORDER BY sequence LIMIT ?', resource, after, limit);
      const sequence = resync ? row.sequence : events.length ? Number(events.at(-1)!.sequence) : Math.min(after, row.sequence);
      return { generation: row.incarnation, next_cursor: JSON.stringify({ resource, generation: row.incarnation, sequence }), resync_required: resync, historical_hints_only: true, events: events.map(event => ({ sequence: Number(event.sequence), generation: String(event.generation), event: JSON.parse(String(event.payload)) as Json })) };
    });
  }

  /** Existing UI workflow: not implemented as an SDK JSON document mutation. */
  manualAdjust(resource: string, sku: string, available: number, expected: ResourceVersion): void {
    integer(available, 0, Number.MAX_SAFE_INTEGER); text(sku, 128);
    this.transaction(true, () => {
      const row = this.resource(resource); this.compare(row, expected);
      this.one('SELECT sku FROM stock WHERE resource=? AND sku=?', resource, sku);
      this.run('UPDATE stock SET available=? WHERE resource=? AND sku=?', available, resource, sku);
      this.run('UPDATE resources SET revision=? WHERE id=?', (BigInt(row.revision) + 1n).toString(), resource);
      this.recordEvent(resource, 'application-ui.adjust', { sku, available });
    });
  }
  /** Owner-only UI permission change; intentionally absent from SDK-exposed operations. */
  ownerSetAccess(resource: string, enabled: boolean): void {
    this.transaction(true, () => {
      const row = this.resource(resource, false);
      this.run('UPDATE resources SET enabled=?,revision=? WHERE id=?', enabled ? 1 : 0, (BigInt(row.revision) + 1n).toString(), resource);
    });
  }
  /** Owner closes a live workspace to release active capacity. IDs are never reused. */
  ownerCloseWorkspace(resource: string): void {
    requireCondition(resource.startsWith('workspace-'), 'Only private child workspaces may be closed');
    this.transaction(true, () => { this.resource(resource); this.run('DELETE FROM resources WHERE id=?', resource); });
  }
  diagnostics(resource: string): { receipts: number; events: number; epoch: number; floor: number } {
    const row = this.resource(resource);
    return { receipts: Number(this.one('SELECT COUNT(*) AS count FROM receipts WHERE resource=?', resource).count), events: Number(this.one('SELECT COUNT(*) AS count FROM events WHERE resource=?', resource).count), epoch: row.epoch, floor: row.floor_epoch };
  }
}
