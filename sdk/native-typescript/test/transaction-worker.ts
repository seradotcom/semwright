import { readFileSync } from 'node:fs';
import { Inventory } from '../../../examples/native-inventory/application.js';
import {
  NativeError,
  applicationContext,
  type Json,
  type ResourceVersion,
} from '../src/index.js';

interface WorkerRequest {
  root: string;
  method: string;
  expected: ResourceVersion;
  args: Json;
  discardReply?: boolean;
}

const raw = readFileSync(0, 'utf8');
const request = JSON.parse(raw) as WorkerRequest;
const app = new Inventory(request.root);
try {
  const handler = app.operations.get('driver.native-inventory.' + request.method);
  if (!handler) throw new Error('worker operation is not exposed');
  const result = handler(
    request.args,
    applicationContext('external-worker', request.expected),
  );
  if (request.discardReply) {
    // Model a transport loss after the durable application transaction returned.
    // Exit 42 is harness-only and carries no SDK meaning.
    process.exitCode = 42;
  } else {
    process.stdout.write(JSON.stringify({ ok: true, result }));
  }
} catch (error) {
  const code = error instanceof NativeError ? error.code : 'Internal';
  process.stdout.write(JSON.stringify({ ok: false, error: code }));
} finally {
  app.close();
}
