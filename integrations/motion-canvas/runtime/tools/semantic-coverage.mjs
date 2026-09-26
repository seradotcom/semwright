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
    if (!['managed','represented_by','compiler_managed','unsupported_by_design'].includes(p.status)) throw new Error(`bad ${name}.${prop} status`);
    if ((p.status === 'managed' || p.status === 'represented_by') && !p.semantic) throw new Error(`missing semantic mapping for ${name}.${prop}`);
    if (p.status === 'unsupported_by_design' && !p.reason) throw new Error(`missing exclusion reason for ${name}.${prop}`);
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

