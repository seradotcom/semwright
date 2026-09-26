import fs from 'node:fs';
import path from 'node:path';

const root = path.resolve(import.meta.dirname, '..');
const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
const expectedVersion = pkg.dependencies['@motion-canvas/2d'];
const coveragePath = path.resolve(root, '../../../docs/motion-canvas/API_COVERAGE.json');
const coverage = JSON.parse(fs.readFileSync(coveragePath, 'utf8'));
if (coverage.motion_canvas_version !== expectedVersion) throw new Error('coverage version drift');
const componentsDir = process.env.SEMWRIGHT_MOTION_COMPONENTS_DIR ?? path.join(root, 'node_modules/@motion-canvas/2d/lib/components');
const index = fs.readFileSync(path.join(componentsDir, 'index.d.ts'), 'utf8');
const exports = [...index.matchAll(/export \* from '\.\/([^']+)'/g)].map(m => m[1]).filter(x => x !== 'types').sort();
const classified = Object.keys(coverage.components).sort();
if (JSON.stringify(exports) !== JSON.stringify(classified)) throw new Error(`component coverage drift\nupstream=${exports}\nclassified=${classified}`);
for (const name of exports) {
  const entry = coverage.components[name];
  if (!['managed','abstract_substrate','compiler_managed','unsupported_by_design'].includes(entry.status)) throw new Error(`bad status for ${name}`);
  const file = fs.readFileSync(path.join(componentsDir, `${name}.d.ts`), 'utf8');
  if (!entry.props_interface) continue;
  const escaped = entry.props_interface.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const match = file.match(new RegExp(`export interface ${escaped}(?: extends [^{]+)? \\{([\\s\\S]*?)\\n\\}`));
  if (!match) throw new Error(`missing ${entry.props_interface} in ${name}.d.ts`);
  const props = [...match[1].matchAll(/^\s{4}([A-Za-z_][A-Za-z0-9_]*)\??:/gm)].map(m => m[1]).sort();
  const covered = Object.keys(entry.properties ?? {}).sort();
  if (JSON.stringify(props) !== JSON.stringify(covered)) throw new Error(`${name} property coverage drift\nupstream=${props}\nclassified=${covered}`);
  for (const prop of props) {
    const p = entry.properties[prop];
    if (!['managed','represented_by','compiler_managed','unsupported_by_design','mixed'].includes(p.status)) throw new Error(`bad ${name}.${prop} status`);
    if ((p.status === 'managed' || p.status === 'represented_by') && !p.semantic) throw new Error(`missing semantic mapping for ${name}.${prop}`);
    if (p.status === 'unsupported_by_design' && !p.reason) throw new Error(`missing exclusion reason for ${name}.${prop}`);
    if (p.status === 'mixed') {
      if (!p.reason || !p.variants || Object.keys(p.variants).length < 2) throw new Error(`incomplete mixed classification for ${name}.${prop}`);
      for (const [variant, v] of Object.entries(p.variants)) {
        if (!['managed','represented_by','unsupported_by_design'].includes(v.status)) throw new Error(`bad mixed status ${name}.${prop}.${variant}`);
        if ((v.status === 'managed' || v.status === 'represented_by') && !v.semantic) throw new Error(`missing semantic mapping ${name}.${prop}.${variant}`);
        if (v.status === 'unsupported_by_design' && !v.reason) throw new Error(`missing exclusion reason ${name}.${prop}.${variant}`);
      }
    }
  }
}
console.log(JSON.stringify({motion_canvas_version: expectedVersion, exports: exports.length, classified: classified.length, status: 'complete'}));

const coreCoveragePath = path.resolve(root, '../../../docs/motion-canvas/CORE_API_COVERAGE.json');
const coreCoverage = JSON.parse(fs.readFileSync(coreCoveragePath, 'utf8'));
const expectedCoreVersion = pkg.dependencies['@motion-canvas/core'];
if (coreCoverage.motion_canvas_version !== expectedCoreVersion) throw new Error('core coverage version drift');
const coreDir = process.env.SEMWRIGHT_MOTION_CORE_DIR ?? path.join(root, 'node_modules/@motion-canvas/core/lib');
const coreIndex = fs.readFileSync(path.join(coreDir, 'index.d.ts'), 'utf8');
const coreRoot = [
  ...[...coreIndex.matchAll(/export \* from '\.\/([^']+)'/g)].map(m => m[1]),
  ...[...coreIndex.matchAll(/export \{ default as ([A-Za-z0-9_]+) \}/g)].map(m => m[1]),
].sort();
const classifiedRoot = Object.keys(coreCoverage.root_exports).sort();
if (JSON.stringify(coreRoot) !== JSON.stringify(classifiedRoot)) throw new Error(`core root coverage drift\nupstream=${coreRoot}\nclassified=${classifiedRoot}`);

const projectText = fs.readFileSync(path.join(coreDir, 'app/Project.d.ts'), 'utf8');
const projectMatch = projectText.match(/export interface ProjectSettings \{([\s\S]*?)\n\}/);
if (!projectMatch) throw new Error('ProjectSettings interface missing');
const projectFields = [...projectMatch[1].matchAll(/^\s{4}([A-Za-z_][A-Za-z0-9_]*)\??:/gm)].map(m => m[1]).sort();
const classifiedProject = Object.keys(coreCoverage.project_settings).sort();
if (JSON.stringify(projectFields) !== JSON.stringify(classifiedProject)) throw new Error(`ProjectSettings coverage drift\nupstream=${projectFields}\nclassified=${classifiedProject}`);

function publicFunctions(dir) {
  const names = new Set();
  for (const file of fs.readdirSync(path.join(coreDir, dir)).filter(x => x.endsWith('.d.ts'))) {
    const text = fs.readFileSync(path.join(coreDir, dir, file), 'utf8');
    for (const match of text.matchAll(/^export declare (?:function|const) ([A-Za-z_][A-Za-z0-9_]*)/gm)) names.add(match[1]);
  }
  return [...names].sort();
}
for (const group of ['flow','transitions','tweening']) {
  const upstream = publicFunctions(group);
  const classified = Object.keys(coreCoverage.authoring_exports[group]).sort();
  if (JSON.stringify(upstream) !== JSON.stringify(classified)) throw new Error(`${group} coverage drift\nupstream=${upstream}\nclassified=${classified}`);
}
const allowedCoreStatuses = new Set(['managed','represented_by','compiler_managed','runtime_internal','runtime_utility','unsupported_by_design','mixed']);
for (const [sectionName, section] of Object.entries({root_exports:coreCoverage.root_exports, project_settings:coreCoverage.project_settings})) {
  for (const [name, entry] of Object.entries(section)) {
    if (!allowedCoreStatuses.has(entry.status)) throw new Error(`bad core status ${sectionName}.${name}`);
    if (entry.status === 'unsupported_by_design' && !entry.reason) throw new Error(`missing core exclusion reason ${sectionName}.${name}`);
  }
}
for (const [group, section] of Object.entries(coreCoverage.authoring_exports)) {
  for (const [name, entry] of Object.entries(section)) {
    if (!allowedCoreStatuses.has(entry.status)) throw new Error(`bad core authoring status ${group}.${name}`);
    if (entry.status === 'unsupported_by_design' && !entry.reason) throw new Error(`missing core exclusion reason ${group}.${name}`);
    if (['managed','represented_by','compiler_managed'].includes(entry.status) && !entry.semantic) throw new Error(`missing core semantic mapping ${group}.${name}`);
  }
}
console.log(JSON.stringify({motion_canvas_core_version: expectedCoreVersion, root_exports: coreRoot.length, project_settings: projectFields.length, authoring_exports: Object.fromEntries(['flow','transitions','tweening'].map(g => [g, publicFunctions(g).length])), status:'complete'}));



const auxCoveragePath = path.resolve(root, '../../../docs/motion-canvas/AUX_API_COVERAGE.json');
const auxCoverage = JSON.parse(fs.readFileSync(auxCoveragePath, 'utf8'));
if (auxCoverage.motion_canvas_version !== expectedVersion) throw new Error('aux coverage version drift');
const twoDDir = process.env.SEMWRIGHT_MOTION_2D_DIR ?? path.dirname(componentsDir);
const twoDIndex = fs.readFileSync(path.join(twoDDir, 'index.d.ts'), 'utf8');
const rootModules = [...twoDIndex.matchAll(/export \* from '\.\/([^']+)'/g)].map(m => m[1]).sort();
const classifiedModules = Object.keys(auxCoverage.root_modules).sort();
if (JSON.stringify(rootModules) !== JSON.stringify(classifiedModules)) throw new Error(`2d root module coverage drift\nupstream=${rootModules}\nclassified=${classifiedModules}`);

function declarations(text) {
  const names = new Set();
  for (const m of text.matchAll(/^export (?:declare )?(?:abstract )?(?:class|function|const|interface|type|enum|namespace) ([A-Za-z_][A-Za-z0-9_]*)/gm)) names.add(m[1]);
  for (const m of text.matchAll(/^export \{([^}]+)\};?/gm)) {
    for (const raw of m[1].split(',')) {
      const part = raw.trim();
      const alias = part.match(/^([A-Za-z_][A-Za-z0-9_]*)\s+as\s+([A-Za-z_][A-Za-z0-9_]*)$/);
      names.add(alias ? alias[2] : part.split(/\s+/)[0]);
    }
  }
  return names;
}
function moduleExports(name) {
  if (name === 'jsx-runtime') return [...declarations(fs.readFileSync(path.join(twoDDir, 'jsx-runtime.d.ts'), 'utf8'))].sort();
  const moduleDir = path.join(twoDDir, name);
  const moduleIndex = fs.readFileSync(path.join(moduleDir, 'index.d.ts'), 'utf8');
  const names = new Set();
  for (const m of moduleIndex.matchAll(/export \* from '\.\/([^']+)'/g)) {
    const file = path.join(moduleDir, `${m[1]}.d.ts`);
    for (const item of declarations(fs.readFileSync(file, 'utf8'))) names.add(item);
  }
  return [...names].sort();
}
const auxStatuses = new Set(['represented_by','compiler_managed','runtime_utility','unsupported_by_design','mixed']);
for (const [moduleName, moduleEntry] of Object.entries(auxCoverage.modules)) {
  const upstream = moduleExports(moduleName);
  const classified = Object.keys(moduleEntry.exports).sort();
  if (JSON.stringify(upstream) !== JSON.stringify(classified)) throw new Error(`${moduleName} aux coverage drift\nupstream=${upstream}\nclassified=${classified}`);
  for (const [name, entry] of Object.entries(moduleEntry.exports)) {
    if (!auxStatuses.has(entry.status)) throw new Error(`bad aux status ${moduleName}.${name}`);
    if (entry.status === 'represented_by' && !entry.semantic) throw new Error(`missing aux semantic mapping ${moduleName}.${name}`);
    if ((entry.status === 'unsupported_by_design' || entry.status === 'mixed') && !entry.reason) throw new Error(`missing aux boundary reason ${moduleName}.${name}`);
  }
}
console.log(JSON.stringify({motion_canvas_2d_aux_version: expectedVersion, modules:Object.fromEntries(Object.keys(auxCoverage.modules).map(name => [name,moduleExports(name).length])), status:'complete'}));
