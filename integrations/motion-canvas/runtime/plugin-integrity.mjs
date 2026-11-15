import fs from 'node:fs/promises';
import {constants} from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
import {fileURLToPath} from 'node:url';

// Pinned by the driver-embedded render helper; copied with exported projects.
export const motionPluginProvenanceSha256 = '68e9d93fc614f19f941dfc8abebc00c5a00461496252c718a8bc7935feec15f6';
const name = '@semwright/motion-canvas-vite-plugin';
const version = '3.17.2-semwright.1';
const archive = 'vendor/semwright-motion-canvas-vite-plugin-3.17.2-semwright.1.tgz';
const provenance = 'vendor/motion-canvas-vite-plugin.provenance.json';
const digest = (bytes, algorithm = 'sha256', encoding = 'hex') =>
  createHash(algorithm).update(bytes).digest(encoding);
const reject = reason => {throw new Error('motion plugin identity: ' + reason);};
const same = (a, b) => a.dev === b.dev && a.ino === b.ino && a.mode === b.mode &&
  a.size === b.size && a.mtimeMs === b.mtimeMs && a.nlink === b.nlink;

async function regular(root, relative, maximum) {
  if (typeof relative !== 'string' || !/^[A-Za-z0-9_@./-]+$/.test(relative) ||
      path.isAbsolute(relative) || relative.split('/').some(x => !x || x === '.' || x === '..')) reject('unsafe member path');
  let current = root;
  const parts = relative.split('/');
  for (let i = 0; i < parts.length; i++) {
    current = path.join(current, parts[i]);
    const stat = await fs.lstat(current);
    if (stat.isSymbolicLink() || (i < parts.length - 1 ? !stat.isDirectory() : !stat.isFile())) reject('linked or special member');
  }
  const before = await fs.lstat(current);
  if (before.nlink !== 1 || before.size > maximum) reject('member exceeds bounds or is aliased');
  const file = await fs.open(current, constants.O_RDONLY | (constants.O_NOFOLLOW || 0));
  try {
    const opened = await file.stat();
    if (!same(before, opened)) reject('member changed before open');
    const buffer = Buffer.alloc(maximum + 1);
    let length = 0;
    while (length < buffer.length) {
      const result = await file.read(buffer, length, buffer.length - length, length);
      if (!result.bytesRead) break;
      length += result.bytesRead;
    }
    if (length > maximum || !same(opened, await file.stat()) || !same(before, await fs.lstat(current))) reject('member changed while reading');
    return buffer.subarray(0, length);
  } finally {
    await file.close();
  }
}
async function roster(root, directory, result = [], state = {visits:0}) {
  if (++state.visits > 128 || directory.split('/').length > 6 || result.length > 64) reject('installed roster exceeds bounds');
  const stat = await fs.lstat(path.join(root, directory));
  if (!stat.isDirectory() || stat.isSymbolicLink()) reject('linked package directory');
  const entries = await fs.readdir(path.join(root, directory), {withFileTypes:true});
  if (entries.length > 64) reject('directory exceeds bounds');
  for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name, 'en'))) {
    const relative = directory + '/' + entry.name;
    if (entry.isDirectory()) await roster(root, relative, result, state);
    else if (entry.isFile()) result.push(relative);
    else reject('linked or special installed member');
  }
  return result;
}
async function verify(runtimeRoot, lockBytes, resolvedEntry, dependenciesRoot) {
  const root = await fs.realpath(runtimeRoot);
  const manifestBytes = await regular(root, provenance, 32768);
  if (digest(manifestBytes) !== motionPluginProvenanceSha256) reject('provenance digest changed');
  const manifest = JSON.parse(manifestBytes);
  if (manifest.schema !== 'semwright.motion-canvas-vite-plugin-provenance/1' ||
      manifest.fork.name !== name || manifest.fork.version !== version) reject('fork identity differs');
  const bytes = await regular(root, archive, 65536);
  if (bytes.length !== manifest.archive.bytes || digest(bytes) !== manifest.archive.sha256 ||
      'sha512-' + digest(bytes, 'sha512', 'base64') !== manifest.archive.integrity) reject('archive digest changed');
  const lock = JSON.parse(lockBytes);
  const dependency = lock.packages?.['node_modules/' + name];
  if (lock.packages?.['']?.dependencies?.[name] !== 'file:' + archive ||
      dependency?.version !== version || dependency?.resolved !== 'file:' + archive ||
      dependency?.integrity !== manifest.archive.integrity || dependency?.link === true) reject('lock does not bind the exact archive');
  const directory = name;
  const expected = manifest.members.map(member => directory + '/' + member.path).sort();
  const observed = (await roster(dependenciesRoot, directory)).sort();
  if (JSON.stringify(expected) !== JSON.stringify(observed)) reject('installed roster differs');
  let total = 0;
  for (const member of manifest.members) {
    const actual = await regular(dependenciesRoot, directory + '/' + member.path, 65536);
    total += actual.length;
    if (total > 262144 || actual.length !== member.bytes || digest(actual) !== member.sha256) reject('installed member differs: ' + member.path);
  }
  const entry = path.join(dependenciesRoot, directory, 'lib/index.js');
  if (typeof resolvedEntry !== 'string' || !resolvedEntry.startsWith('file:') ||
      await fs.realpath(fileURLToPath(resolvedEntry)) !== await fs.realpath(entry)) reject('resolved entry differs');
  return {name, version, files:observed.length, bytes:total, integrity:manifest.archive.integrity};
}

// Native runtime: the owner-granted bundle has an ordinary node_modules root.
export async function verifyMotionCanvasPlugin(runtimeRoot, lockBytes, resolvedEntry) {
  const root = await fs.realpath(runtimeRoot);
  const dependenciesRoot = path.join(root, 'node_modules');
  const stat = await fs.lstat(dependenciesRoot);
  if (!stat.isDirectory() || stat.isSymbolicLink()) reject('runtime dependency root is not ordinary');
  return verify(root, lockBytes, resolvedEntry, dependenciesRoot);
}
// Exported source projects may use the existing linked runtime dependency tree.
// Resolve that explicit node_modules reference once, verify the same complete
// package and exact resolved entry, then ensure the reference did not change.
// This read-only source-project check grants no Native runtime authority.
export async function verifyExportedMotionCanvasPlugin(projectRoot, lockBytes, resolvedEntry) {
  const root = await fs.realpath(projectRoot);
  const reference = path.join(root, 'node_modules');
  const dependenciesRoot = await fs.realpath(reference);
  const result = await verify(root, lockBytes, resolvedEntry, dependenciesRoot);
  if (await fs.realpath(reference) !== dependenciesRoot) reject('project dependency reference changed');
  return result;
}
