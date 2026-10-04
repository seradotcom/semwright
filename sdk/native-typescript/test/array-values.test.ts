/** Regression tests for the current @semwright/native-sdk public API. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { validateValue } from '../src/index.js';

test('array value validation does not execute an accessor', () => {
  let executed = false;
  const value: unknown[] = [];
  Object.defineProperty(value, '0', {
    enumerable: true,
    configurable: true,
    get() {
      executed = true;
      return 1;
    },
  });
  assert.throws(() => validateValue(value));
  assert.equal(executed, false);
});

test('nested array accessors are rejected without executing application code', () => {
  let calls = 0;
  const value: unknown[] = [];
  Object.defineProperty(value, '0', {
    enumerable: true,
    get() {
      calls += 1;
      return 'synthetic';
    },
  });
  assert.throws(() => validateValue({ nested: [value] }));
  assert.equal(calls, 0);
});

test('custom array prototypes are not silently serialized as JSON data', () => {
  let called = false;
  const value = [1];
  Object.setPrototypeOf(value, {
    toJSON() {
      called = true;
      return 'different';
    },
  });
  assert.throws(() => validateValue(value));
  assert.equal(called, false);
});

test('plain dense arrays and repeated independent references remain valid', () => {
  const item = { value: '90071992547409930000001' };
  const value = [null, true, 1.5, item, item, ['á😀']];
  assert.doesNotThrow(() => validateValue(value));
  assert.deepEqual(value[3], item);
});

test('sparse, hidden and decorated arrays are still rejected', () => {
  const hidden: unknown[] = [];
  Object.defineProperty(hidden, '0', { value: 1, enumerable: false });
  const extra = Object.assign([1], { extra: 2 });
  for (const value of [new Array(2), hidden, extra]) {
    assert.throws(() => validateValue(value));
  }
});

test('cycles still fail and a frozen dense array remains supported', () => {
  const cyclic: unknown[] = [];
  cyclic.push(cyclic);
  assert.throws(() => validateValue(cyclic));
  assert.doesNotThrow(() => validateValue(Object.freeze([1, 'two', null])));
});
