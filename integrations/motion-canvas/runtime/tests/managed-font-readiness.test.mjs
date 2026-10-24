import assert from 'node:assert/strict';
import fs from 'node:fs';
const source=fs.readFileSync(new URL('../render.mjs',import.meta.url),'utf8');
const configCode=source.split('// BEGIN MANAGED FONT CONFIG V1 (pure configuration tests extract these functions).')[1].split('// END MANAGED FONT CONFIG V1.')[0];
const validateManagedFontConfig=Function(configCode+';return validateManagedFontConfig')();
const readinessCode=source.split('// BEGIN PINNED FONT READINESS V04 (browser API mock tests extract exactly these functions).')[1].split('// END PINNED FONT READINESS V04.')[0];
const loadPinnedFontFaces=Function(readinessCode+';return loadPinnedFontFaces')();
let checks=0;
const fixture=()=>({
  family:'Fixture Brand',path:'assets/brand.ttf',sha256:'a'.repeat(64),bytes:1234,
  weights:[400,700],codepoints:[[65,66],[241,241]],sample:'ñ',face_loaded:false,
  metadata:{family_names:['Fixture Brand'],normal:true,weight_min:100,weight_max:900,
    variable_weight:true,glyph_count:256,embedding_rights:0},
});
const fontSet=()=>({
  status:'loaded',ready:Promise.resolve(),calls:[],
  async load(descriptor,sample){
    this.calls.push({descriptor,sample});
    const family=JSON.parse(descriptor.slice(descriptor.indexOf('"')));
    return [{family,status:'loaded',weight:family==='Fixture Brand'?'100 900':family==='IBM Plex Mono'?'400':'400 700',style:'normal'}];
  },
  check(){return true;},
});
const original=fixture();
assert.deepEqual(validateManagedFontConfig([original]),[original]);checks++;
const fonts=fontSet();
const receipt=await loadPinnedFontFaces(fonts,100,[original]);
assert.equal(receipt.requests.length,7);checks++;
assert.deepEqual(receipt.requests.slice(5).map(r=>[r.family,r.requested_weight,r.asset_sha256,r.sample]),[
 ['Fixture Brand',400,'a'.repeat(64),'ñ'],['Fixture Brand',700,'a'.repeat(64),'ñ'],
]);checks++;
assert.ok(receipt.requests.every(r=>r.check_after_load&&r.check_after_ready));checks++;
for(const mutate of [
 f=>f.family='Instrument Sans Variable',
 f=>f.path='../brand.ttf',
 f=>f.sha256='b'.repeat(63),
 f=>f.face_loaded=true,
 f=>f.metadata.family_names=['Other'],
 f=>f.metadata.normal=false,
 f=>f.metadata.embedding_rights=2,
 f=>f.metadata.embedding_rights=4,
 f=>f.metadata.embedding_rights=0x200,
 f=>f.metadata.weight_min=800,
 f=>f.weights=[400,400],
 f=>f.weights=[700,400],
 f=>f.codepoints=[[65,70],[69,80]],
 f=>f.codepoints=[[0xd800,0xdfff]],
 f=>f.sample='Z',
 f=>f.sample='ñA',
]){
 const changed=structuredClone(original);mutate(changed);
 assert.throws(()=>validateManagedFontConfig([changed]),/managed font/);checks++;
}
assert.throws(()=>validateManagedFontConfig([original,original]),/managed font/);checks++;
for(const change of [
 f=>f.family='IBM Plex Mono',
 f=>f.weights=[1001],
 f=>f.sha256='',
 f=>f.sample='',
]){
 const changed=structuredClone(original);change(changed);
 await assert.rejects(loadPinnedFontFaces(fontSet(),100,[changed]),/managed font/);checks++;
}
for(const readback of [
 [{family:'Other',status:'loaded',weight:'100 900',style:'normal'}],
 [{family:'Fixture Brand',status:'loading',weight:'100 900',style:'normal'}],
 [{family:'Fixture Brand',status:'loaded',weight:'400',style:'normal'}],
 [{family:'Fixture Brand',status:'loaded',weight:'100 900',style:'italic'}],
 [],
 Array(2).fill({family:'Fixture Brand',status:'loaded',weight:'100 900',style:'normal'}),
]){
 const fake=fontSet(),load=fake.load.bind(fake);
 fake.load=async(desc,sample)=>desc.includes('Fixture Brand')?readback:load(desc,sample);
 await assert.rejects(loadPinnedFontFaces(fake,100,[original]),/pinned font/);checks++;
}
const waiting=fontSet(),load=waiting.load.bind(waiting);
waiting.load=async(desc,sample)=>desc.includes('Fixture Brand')?new Promise(()=>{}):load(desc,sample);
await assert.rejects(loadPinnedFontFaces(waiting,10,[original]),/deadline/);checks++;
console.log(JSON.stringify({status:'PURE_MANAGED_FONT_CONTRACT_MOCK_PASS',checks,
 browser_api_is_mock:true,metadata_is_explicit_test_fixture:true,
 actual_binary_parser_exercised:false,actual_native_font_proof:false,native_apps_started:0}));
