import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {createHash} from 'node:crypto';
// This suite executes the exact metadata/readiness/gate bodies against explicit
// mocks. It starts no browser, parses no real font and claims no native loading.
const source=fs.readFileSync(new URL('../render.mjs',import.meta.url),'utf8');
const metadataCode=source.slice(source.indexOf('function unicodeRanges('),source.indexOf('// BEGIN MANAGED FONT CONFIG V1'));
const mockFs={async readFile(file,encoding){
  if(encoding==='utf8')return '@font-face{src:url(./fixture.woff2);unicode-range:U+0000-00FF;}';
  return Buffer.from('explicit G0 mock WOFF2 bytes');
}};
const pinnedFontEvidence=Function('fs','path','createHash','fail',metadataCode+';return pinnedFontEvidence')(mockFs,path,createHash,message=>{throw new Error(message);});
const readinessCode=source.split('// BEGIN PINNED FONT READINESS V04 (browser API mock tests extract exactly these functions).')[1].split('// END PINNED FONT READINESS V04.')[0];
const loadPinnedFontFaces=Function(readinessCode+';return loadPinnedFontFaces')();
const gateSource=source.split('(async()=>{\n  try {')[1].split("    state.phase='rendering';")[0];
const AsyncFunction=Object.getPrototypeOf(async function(){}).constructor;
const gate=new AsyncFunction('loadPinnedFontFaces','document','config','state','globalThis',gateSource+';return state;');
const cases=[];
function check(name,fn){fn();cases.push(name);}
const fonts={status:'loaded',ready:Promise.resolve(),
  async load(descriptor){const family=JSON.parse(descriptor.slice(descriptor.indexOf('"')));return [{family,status:'loaded',style:'normal',weight:family==='IBM Plex Mono'?'400':'400 700'}];},
  check(){return true;},
};
const evidence=await pinnedFontEvidence('/g0-pinned-fonts');
const observed=await loadPinnedFontFaces(fonts,100);
check('metadata-covers-exact-bundled-request-roster',()=>assert.deepEqual(evidence.map(f=>[f.family,f.weights]),[
 ['Instrument Sans Variable',[400,500,600,700]],['IBM Plex Mono',[400]],
]));
check('all-advertised-weights-were-requested',()=>assert.deepEqual(
 evidence.flatMap(f=>f.weights.map(w=>[f.family,w])),observed.requests.map(r=>[r.family,r.requested_weight]),
));
check('metadata-is-unloaded-before-observation',()=>assert.ok(evidence.every(f=>f.face_loaded===false)));
async function bind(faces,readiness,managedFonts=[]){
 const binding={__SEMWRIGHT_NATIVE_CONFIG__:{font_evidence:structuredClone(faces)}};
 await gate(async()=>structuredClone(readiness),{}, {authoring:true,timeoutMs:100,managedFonts},{},binding);
 return binding.__SEMWRIGHT_NATIVE_CONFIG__.font_evidence;
}
const exactBinding={__SEMWRIGHT_NATIVE_CONFIG__:{font_evidence:structuredClone(evidence)}};
await gate(loadPinnedFontFaces,{fonts},{authoring:true,timeoutMs:100},{},exactBinding);
check('actual-mock-readiness-unlocks-both-bundled-faces',()=>assert.ok(exactBinding.__SEMWRIGHT_NATIVE_CONFIG__.font_evidence.every(f=>f.face_loaded===true)));
for(const weight of [100,300,800,900]){
 const changed=structuredClone(evidence);changed[0].weights.push(weight);
 const result=await bind(changed,observed);
 check('unsupported-real-axis-weight-'+weight+'-is-not-loaded',()=>assert.equal(result[0].face_loaded,false));
 check('unsupported-real-axis-weight-'+weight+'-does-not-taint-other-family',()=>assert.equal(result[1].face_loaded,true));
}
for(const [name,mutate] of [
 ['missing-request',r=>r.requests.splice(1,1)],
 ['failed-check-after-ready',r=>r.requests[1].check_after_ready=false],
 ['wrong-family',r=>r.requests[1].family='Other Family'],
]){
 const changed=structuredClone(observed);mutate(changed);const result=await bind(evidence,changed);
 check(name+'-cannot-claim-loaded-face',()=>assert.equal(result[0].face_loaded,false));
}
const managed={family:'Managed Fixture',sha256:'a'.repeat(64),weights:[400,700],face_loaded:false};
const managedReadiness={requests:[400,700].map(weight=>({family:managed.family,requested_weight:weight,check_after_ready:true,asset_sha256:managed.sha256}))};
const admitted=await bind([managed],managedReadiness,[managed]);
check('all-managed-weights-require-exact-asset-digest',()=>assert.equal(admitted[0].face_loaded,true));
for(const [name,mutate] of [
 ['wrong-digest',r=>r.requests[1].asset_sha256='b'.repeat(64)],
 ['missing-digest',r=>delete r.requests[1].asset_sha256],
 ['partial-weights',r=>r.requests.pop()],
]){
 const changed=structuredClone(managedReadiness);mutate(changed);const result=await bind([managed],changed,[managed]);
 check('managed-'+name+'-cannot-claim-loaded-face',()=>assert.equal(result[0].face_loaded,false));
}
console.log(JSON.stringify({status:'PURE_FONT_EVIDENCE_JOIN_MOCK_PASS',checks:cases.length,cases,
 browser_api_is_mock:true,font_bytes_are_mock:true,actual_native_font_proof:false,
 native_apps_started:0,third_party_imports_or_main_invoked:false}));
