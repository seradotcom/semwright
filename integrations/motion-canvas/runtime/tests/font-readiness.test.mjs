import assert from 'node:assert/strict';
import fs from 'node:fs';
const source=fs.readFileSync(new URL('../render.mjs',import.meta.url),'utf8');
const contract=source.split('// BEGIN PINNED FONT READINESS V04 (browser API mock tests extract exactly these functions).')[1].split('// END PINNED FONT READINESS V04.')[0];
const loadPinnedFontFaces=Function(contract+';return loadPinnedFontFaces')();
const gateSource=source.split('(async()=>{\n  try {')[1].split("    state.phase='rendering';")[0];

let checks=0;const cases=[];
const check=(value,label)=>{assert.ok(value,label);checks++;};
const deferred=()=>{let resolve,reject;const promise=new Promise((a,b)=>{resolve=a;reject=b;});return{promise,resolve,reject};};
const tick=()=>new Promise(resolve=>setTimeout(resolve,2));
function fixture(){
 const calls=[],checksSeen=[];
 const fonts={status:'loaded',ready:Promise.resolve(),
  async load(descriptor,sample){calls.push([descriptor,sample]);
   const family=descriptor.includes('Instrument')?'Instrument Sans Variable':'IBM Plex Mono';
   return[{family:'"'+family+'"',status:'loaded',weight:family.startsWith('Instrument')?'400 700':'400',style:'normal'}];},
  check(descriptor,sample){checksSeen.push([descriptor,sample]);return true;}};
 return{fonts,calls,checksSeen};
}
async function reject(label,change,pattern=/pinned font/){
 const f=fixture();change(f);
 await assert.rejects(loadPinnedFontFaces(f.fonts,100),pattern);checks++;cases.push(label);
}
const happy=fixture(),proof=await loadPinnedFontFaces(happy.fonts,100);
check(happy.calls.length===5,'exactly five bounded requests');
check(JSON.stringify(proof.requests.map(x=>[x.family,x.requested_weight]))===JSON.stringify([
 ['Instrument Sans Variable',400],['Instrument Sans Variable',500],['Instrument Sans Variable',600],['Instrument Sans Variable',700],['IBM Plex Mono',400]]),'both exact pinned families and all declared weights');
check(happy.checksSeen.length===10&&proof.requests.every(x=>x.check_after_load&&x.check_after_ready),'checks on both sides of ready');
check(proof.observed_before_renderer_render&&proof.status_after_ready==='loaded','private scalar gate result');
cases.push('all-families-and-weights');
const AsyncFunction=Object.getPrototypeOf(async function(){}).constructor;
const exactGate=new AsyncFunction('loadPinnedFontFaces','document','config','state','globalThis',gateSource+'\n state.renderer_started=true; return state;');
for(const authoring of [false,true]){
 const f=fixture(),state={renderer_started:false},binding={__SEMWRIGHT_NATIVE_CONFIG__:{font_evidence:[{family:'Instrument Sans Variable',weights:[700]},{family:'IBM Plex Mono',weights:[400]}]}};
 await exactGate(loadPinnedFontFaces,{fonts:f.fonts},{authoring,generation:0,timeoutMs:100},state,binding);
 check(state.renderer_started&&f.calls.length===5,'exact gate with generation zero and authoring '+authoring);
 if(authoring)check(binding.__SEMWRIGHT_NATIVE_CONFIG__.font_evidence.every(x=>x.face_loaded),'existing authoring readback is actual mock check');
 cases.push('exact-unconditional-gate-authoring-'+authoring);
}
const loadPending=fixture(),loadReady=deferred();
const loadOriginal=loadPending.fonts.load;
loadPending.fonts.load=async function(...args){await loadReady.promise;return loadOriginal(...args);};
const loadState={renderer_started:false};
const loading=exactGate(loadPinnedFontFaces,{fonts:loadPending.fonts},{authoring:false,timeoutMs:100},loadState,{});
await tick();check(!loadState.renderer_started,'pending load blocks first render');
loadReady.resolve();await loading;check(loadState.renderer_started,'render only after actual load returns');cases.push('pending-load');
const readyPending=fixture(),allReady=deferred();readyPending.fonts.ready=allReady.promise;
const readyState={renderer_started:false};
const waiting=exactGate(loadPinnedFontFaces,{fonts:readyPending.fonts},{authoring:false,timeoutMs:100},readyState,{});
await tick();check(readyPending.calls.length===5&&!readyState.renderer_started,'ready pending blocks first render after loads');
allReady.resolve();await waiting;check(readyState.renderer_started,'ready completion releases first render');cases.push('pending-ready');
await reject('load-rejected',f=>{f.fonts.load=async()=>{throw new Error('fixture load failure');};},/fixture load failure/);
await reject('empty-loaded-set',f=>{f.fonts.load=async()=>[];});
await reject('non-array-loaded-set',f=>{f.fonts.load=async()=>({});});
await reject('too-many-loaded-faces',f=>{f.fonts.load=async()=>Array(17).fill({});});
await reject('wrong-family',f=>{f.fonts.load=async()=>[{family:'sans-serif',status:'loaded',weight:'400 700',style:'normal'}];});
await reject('wrong-status',f=>{f.fonts.load=async()=>[{family:'Instrument Sans Variable',status:'loading',weight:'400 700',style:'normal'}];});
await reject('wrong-style',f=>{f.fonts.load=async()=>[{family:'Instrument Sans Variable',status:'loaded',weight:'400 700',style:'italic'}];});
await reject('wrong-weight-range',f=>{f.fonts.load=async()=>[{family:'Instrument Sans Variable',status:'loaded',weight:'400',style:'normal'}];});
await reject('bad-weight-value',f=>{f.fonts.load=async()=>[{family:'Instrument Sans Variable',status:'loaded',weight:'bold',style:'normal'}];});
await reject('oversize-weight',f=>{f.fonts.load=async()=>[{family:'Instrument Sans Variable',status:'loaded',weight:'4'.repeat(65),style:'normal'}];});
await reject('check-false-after-load',f=>{f.fonts.check=()=>false;});
await reject('check-false-after-ready',f=>{let count=0;f.fonts.check=()=>++count<=5;});
await reject('set-still-pending',f=>{f.fonts.status='loading';});
await reject('ready-is-not-promise',f=>{f.fonts.ready={};});
await reject('load-api-missing',f=>{delete f.fonts.load;});
await reject('check-api-missing',f=>{delete f.fonts.check;});
for(const timeout of [-1,0,1.5,10001,NaN,Infinity]){
 await assert.rejects(loadPinnedFontFaces(fixture().fonts,timeout),/API or deadline/);checks++;
}cases.push('invalid-deadlines');
for(const phase of ['load','ready']){
 const f=fixture();
 if(phase==='load')f.fonts.load=()=>new Promise(()=>{});else f.fonts.ready=new Promise(()=>{});
 const started=performance.now();await assert.rejects(loadPinnedFontFaces(f.fonts,10),/deadline exceeded/);checks++;
 check(performance.now()-started<1000,'bounded '+phase+' timeout');cases.push('deadline-'+phase);
}
const rejected=fixture();rejected.fonts.load=async()=>[];
const noRenderer={renderer_started:false};
await assert.rejects(exactGate(loadPinnedFontFaces,{fonts:rejected.fonts},{authoring:false,timeoutMs:100},noRenderer,{}));checks++;
check(!noRenderer.renderer_started,'failed gate never starts a render');cases.push('no-render-after-failed-gate');
console.log(JSON.stringify({status:'PURE_BROWSER_API_MOCK_PASS',fixture_checks:checks,cases,
 browser_api_is_mock:true,actual_native_font_proof:false,native_apps_started:0,third_party_imports_or_main_invoked:false}));
