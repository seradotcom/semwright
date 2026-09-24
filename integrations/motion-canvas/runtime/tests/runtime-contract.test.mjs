import assert from 'node:assert/strict';
import fs from 'node:fs';
import test from 'node:test';

const pkg = JSON.parse(fs.readFileSync(new URL('../package.json', import.meta.url)));
const render = fs.readFileSync(new URL('../render.mjs', import.meta.url), 'utf8');

test('runtime dependencies use exact versions', () => {
  for (const [name, version] of Object.entries({...pkg.dependencies, ...pkg.devDependencies})) {
    assert.match(version, /^\d+\.\d+\.\d+$/, `${name} must use an exact version`);
  }
  assert.equal(pkg.engines.node, '22.22.0');
  assert.equal(pkg.dependencies['@motion-canvas/core'], '3.17.2');
  assert.equal(pkg.dependencies.playwright, '1.63.0');
});

test('render harness is bound to the Driver Host marker and local origin', () => {
  assert.ok(render.includes('SEMWRIGHT_DRIVER_SANDBOX'));
  assert.ok(render.includes('landlock-bwrap-v1'));
  assert.ok(render.includes('chromiumSandbox:false'));
  assert.ok(render.includes("'--single-process'"));
  assert.ok(render.includes("'--no-zygote'"));
  assert.ok(render.includes('semwright.invalid'));
  assert.ok(render.includes("route.abort('blockedbyclient')"));
});
