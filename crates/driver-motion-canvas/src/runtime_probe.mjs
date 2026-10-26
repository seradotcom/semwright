// Fixed Host program: read package versions in the delegated runtime only.
// No renderer, browser, package install, project mutation or evidence-file write.
import {closeSync, fstatSync, openSync, readSync, realpathSync, statSync} from 'node:fs';
import {createRequire} from 'node:module';
import {join, relative, isAbsolute, sep} from 'node:path';
import process from 'node:process';

const output = {
  state: 'observed',
  node_version: process.versions.node,
  core_version: null,
  two_d_version: null,
  vite_plugin_version: null,
};
const version = value =>
  typeof value === 'string' &&
  /^[0-9A-Za-z][0-9A-Za-z.+_-]{0,79}$/.test(value);
const fault = state => {
  const error = new Error('bounded package probe');
  error.probeState = state;
  return error;
};
const MAX_PACKAGE_BYTES = 65536;
const sameFile = (left, right) =>
  left.dev === right.dev && left.ino === right.ino && left.size === right.size &&
  left.mtimeMs === right.mtimeMs && left.ctimeMs === right.ctimeMs;
const readPackageBytes = path => {
  const fd = openSync(path, 'r');
  let failed = false;
  try {
    const before = fstatSync(fd);
    if (!before.isFile() || before.size < 2 || before.size > MAX_PACKAGE_BYTES) {
      throw fault('invalid_package');
    }
    // The extra byte detects growth without allocating from a mutable file size.
    const bytes = Buffer.alloc(MAX_PACKAGE_BYTES + 1);
    let length = 0;
    while (length < bytes.length) {
      const count = readSync(fd, bytes, length, bytes.length - length, null);
      if (count === 0) break;
      length += count;
    }
    const after = fstatSync(fd);
    const linked = statSync(path);
    if (length > MAX_PACKAGE_BYTES || length !== before.size ||
        !sameFile(before, after) || !sameFile(after, linked) ||
        realpathSync(path) !== path) {
      throw fault('invalid_package');
    }
    return bytes.subarray(0, length);
  } catch (error) {
    failed = true;
    throw error;
  } finally {
    try {
      closeSync(fd);
    } catch (error) {
      if (!failed) throw error;
    }
  }
};
try {
  if (process.argv.length !== 2 || process.argv[1] !== '-') {
    throw fault('invalid_package');
  }
  const root = realpathSync(process.cwd());
  const require = createRequire(join(root, 'package.json'));
  const observed = {};
  for (const [name, field] of [
    ['@motion-canvas/core', 'core_version'],
    ['@motion-canvas/2d', 'two_d_version'],
    ['@motion-canvas/vite-plugin', 'vite_plugin_version'],
  ]) {
    let path;
    try {
      path = realpathSync(require.resolve(name + '/package.json'));
    } catch (error) {
      if (error.code === 'MODULE_NOT_FOUND' || error.code === 'ENOENT') {
        throw fault('package_not_found');
      }
      throw fault('invalid_package');
    }
    const child = relative(root, path);
    if (!child || child === '..' || child.startsWith('..' + sep) || isAbsolute(child)) {
      throw fault('scope_violation');
    }
    const bytes = readPackageBytes(path);
    const pkg = JSON.parse(bytes.toString('utf8'));
    if (!pkg || Array.isArray(pkg) || pkg.name !== name || !version(pkg.version)) {
      throw fault('invalid_package');
    }
    observed[field] = pkg.version;
  }
  Object.assign(output, observed);
} catch (error) {
  output.state = ['package_not_found', 'invalid_package', 'scope_violation']
    .includes(error.probeState) ? error.probeState : 'invalid_package';
}
process.stdout.write(JSON.stringify(output) + '\n');
