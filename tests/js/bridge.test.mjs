import assert from 'node:assert/strict';
import {test} from 'node:test';
import fs from 'node:fs';
import vm from 'node:vm';
const root = new URL('../../', import.meta.url);
const source = fs.readFileSync(new URL('bridges/shared/contract.js', root), 'utf8');
const context = vm.createContext({});
vm.runInContext(source, context);
const valid = {id: 'request', operation: 'focus', target: '1', fingerprint: 'epoch:42:app'};
const call = value => context.validateOperation(JSON.stringify(value));
const describe = value => value;

test('all bridge copies share the exact contract', () => {
    const gnome = fs.readFileSync(new URL('bridges/gnome/contract.js', root), 'utf8');
    const kwin = fs.readFileSync(new URL('bridges/kwin/contents/code/main.js', root), 'utf8');
    assert.equal(gnome.replace(/\nexport \{validateOperation, exactWindow\};\s*$/, '\n'), source);
    assert.ok(kwin.startsWith(source));
});
test('focus allowed', () => assert.equal(call(valid).operation, 'focus'));
test('close allowed', () => assert.equal(call({...valid, operation: 'close'}).operation, 'close'));
test('move bounded', () => assert.equal(call({...valid, operation: 'move', x: -100, y: 40}).x, -100));
test('resize bounded', () => assert.equal(call({...valid, operation: 'resize', width: 800, height: 600}).width, 800));
test('arbitrary evaluation is not an operation', () => assert.throws(() => call({...valid, operation: 'eval'})));
test('extra payload rejected', () => assert.throws(() => call({...valid, code: 'untrusted'})));
test('prototype payload rejected', () => assert.throws(() => context.validateOperation('{"__proto__":{"operation":"close"}}')));
test('null rejected', () => assert.throws(() => call(null)));
test('array rejected', () => assert.throws(() => call([])));
test('missing identity rejected', () => assert.throws(() => call({...valid, target: undefined})));
test('oversized payload rejected', () => assert.throws(() => context.validateOperation(' '.repeat(8193))));
test('non-integer coordinates rejected', () => assert.throws(() => call({...valid, operation: 'move', x: 1.2, y: 2})));
test('zero dimensions rejected', () => assert.throws(() => call({...valid, operation: 'resize', width: 0, height: 1})));
test('out-of-range dimensions rejected', () => assert.throws(() => call({...valid, operation: 'resize', width: 16385, height: 1})));
test('exact unique window resolved', () => {
    const window = {id: '1', fingerprint: valid.fingerprint};
    assert.equal(context.exactWindow([window], valid, describe), window);
});
test('missing window fails stale', () => assert.throws(() => context.exactWindow([], valid, describe), /StaleReference/));
test('duplicate identity fails ambiguity', () => {
    const window = {id: '1', fingerprint: valid.fingerprint};
    assert.throws(() => context.exactWindow([window, window], valid, describe), /AmbiguousTarget/);
});
test('reused id with different process fingerprint fails', () => assert.throws(() => context.exactWindow([{id: '1', fingerprint: 'other'}], valid, describe), /StaleReference/));
test('seeded coordinate range regression', () => {
    let seed = 20260921;
    for (let i = 0; i < 1000; i++) {
        seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
        const x = (seed % 200000) - 100000;
        const input = {...valid, operation: 'move', x, y: 0};
        if (x >= -32768 && x <= 32767) assert.equal(call(input).x, x);
        else assert.throws(() => call(input));
    }
});
