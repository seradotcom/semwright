import {describe,it,expect} from "vitest";
import fs from "node:fs";
import path from "node:path";
const sourceDir=path.join(process.cwd(),"src");
const sourceFiles=fs.readdirSync(sourceDir).filter(name=>name.endsWith(".ts")).sort();
const allCode=sourceFiles.map(name=>fs.readFileSync(path.join(sourceDir,name),"utf8")).join("\n");
const code=fs.readFileSync(path.join(sourceDir,"code.ts"),"utf8");
const ui=fs.readFileSync(path.join(sourceDir,"ui.html"),"utf8");
const manifest=JSON.parse(fs.readFileSync(path.join(process.cwd(),"manifest.json"),"utf8"));
const devInspect=JSON.parse(fs.readFileSync(path.join(process.cwd(),"manifest.dev-inspect.json"),"utf8"));
const devCodegen=JSON.parse(fs.readFileSync(path.join(process.cwd(),"manifest.dev-codegen.json"),"utf8"));
const textReview=JSON.parse(fs.readFileSync(path.join(process.cwd(),"manifest.textreview.json"),"utf8"));
const collaboration=JSON.parse(fs.readFileSync(path.join(process.cwd(),"manifest.collaboration.json"),"utf8"));
const coverage=JSON.parse(fs.readFileSync(path.join(process.cwd(),"../docs/API_COVERAGE.json"),"utf8"));
const generatedSurface=fs.readFileSync(path.join(sourceDir,"generated_api_surface.ts"),"utf8");
describe("security surface",()=>{
 it("has no eval or Function constructor",()=>{expect(allCode).not.toMatch(/\beval\s*\(/);expect(allCode).not.toMatch(/new\s+Function/);});
 it("uses dynamic page access",()=>expect(manifest.documentAccess).toBe("dynamic-page"));
 it("does not allow wildcard network",()=>expect(manifest.networkAccess.allowedDomains).toEqual(["none"]));
 it("limits dev websocket to loopback",()=>expect(manifest.networkAccess.devAllowedDomains).toEqual(["ws://localhost:38471"]));
 it("uses async node lookup",()=>expect(code).toContain("getNodeByIdAsync"));
 it("uses async page switching",()=>expect(code).toContain("setCurrentPageAsync"));
 it("loads fonts before text mutation",()=>expect(code).toContain("loadFontAsync"));
 it("uses setReactionsAsync",()=>expect(code).toContain("setReactionsAsync"));
 it("sanitizes SVG",()=>expect(code).toContain("rejectUnsafeSvg"));
 it("bounds tree traversal",()=>expect(code).toContain("MAX_TREE"));
 it("keeps normal editors separate from Dev Mode",()=>{expect(manifest.editorType).toEqual(["figma","figjam","slides","buzz"]);expect(manifest.editorType).not.toContain("dev");});
 it("ships an inspect-only Dev Mode manifest",()=>{expect(devInspect.editorType).toEqual(["dev"]);expect(devInspect.capabilities).toEqual(["inspect","vscode"]);});
 it("ships a dedicated codegen Dev Mode manifest",()=>{expect(devCodegen.editorType).toEqual(["dev"]);expect(devCodegen.capabilities).toEqual(["codegen","vscode"]);expect(devCodegen.codegenLanguages.length).toBeGreaterThan(0);});
 it("ships a dedicated text-review manifest",()=>{expect(textReview.editorType).toEqual(["figma","figjam"]);expect(textReview.capabilities).toEqual(["textreview"]);expect(textReview.permissions).toBeUndefined();});
 it("keeps collaboration permissions out of the default manifest",()=>{expect(manifest.permissions).toEqual(["teamlibrary"]);expect(collaboration.permissions).toEqual(["teamlibrary","currentuser","activeusers","fileusers"]);});
 it("keeps every manifest offline except the loopback development bridge",()=>{for(const m of [manifest,collaboration,devInspect,devCodegen,textReview]){expect(m.networkAccess.allowedDomains).toEqual(["none"]);expect(m.networkAccess.devAllowedDomains).toEqual(["ws://localhost:38471"]);}});
});
describe("advanced API",()=>{
 it("implements Motion style operations",()=>expect(code).toContain("applyAnimationStyle"));
 it("implements manual keyframes",()=>expect(allCode).toContain("applyManualKeyframeTrack"));
 it("implements timeline duration",()=>expect(code).toContain("setTimelineDuration"));
 it("implements spring normalization",()=>expect(code).toContain("physicalSpringToNormalized"));
 it("implements FigJam connectors",()=>expect(code).toContain("createConnector"));
 it("implements variables",()=>expect(code).toContain("getLocalVariablesAsync"));
 it("keeps the pinned SceneNode surface exhaustively classified",()=>{expect(coverage.summary.scene_node_members).toBeGreaterThan(3000);expect(coverage.summary.supported_scene_node_members).toBe(coverage.summary.scene_node_members);expect(coverage.summary.unmapped_method_names).toEqual([]);});
 it("routes dynamic-page special writes through semantic operations",()=>{
   const writable=new Set(coverage.generic_property_surface.writable);
   for(const property of ["reactions","vectorNetwork","explicitVariableModes","resolvedVariableModes","backgroundStyleId","fillStyleId","strokeStyleId","effectStyleId","gridStyleId","textStyleId"]){
     expect(writable.has(property),property).toBe(false);
   }
   expect(coverage.scene_node_types.FRAME.members.reactions.write_capability).toBe("prototype.reaction.set");
   expect(coverage.scene_node_types.VECTOR.members.vectorNetwork.write_capability).toBe("vector.network.set");
   expect(coverage.scene_node_types.FRAME.members.explicitVariableModes.write_capability).toBe("variable.mode.set_explicit");
   expect(coverage.scene_node_types.FRAME.members.resolvedVariableModes.status).toBe("SUPPORTED_COMPUTED_READ_ONLY_PROPERTY");
 });
 it("pins an explicit reviewed type for every generic writable property",()=>{
   const writable=[...coverage.generic_property_surface.writable].sort();
   const reviewed=coverage.generic_property_surface.write_types;
   expect(Object.keys(reviewed).sort()).toEqual(writable);
   for(const property of writable){
     expect(["number","boolean","string","array","object"]).toContain(reviewed[property].kind);
     expect(typeof reviewed[property].nullable).toBe("boolean");
     expect(typeof reviewed[property].type).toBe("string");
   }
   expect(generatedSurface).toContain("SEMWRIGHT_FIGMA_NODE_WRITE_TYPES");
 });
 it("keeps auxiliary public methods exhaustively classified",()=>{expect(coverage.summary.auxiliary_method_entries).toBeGreaterThan(40);expect(coverage.summary.unclassified_auxiliary_methods).toBe(0);});
 it("generates auxiliary property allowlists from typings",()=>{for(const kind of ["STYLE","VARIABLE","COLLECTION"]){expect(coverage.auxiliary_property_surface[kind].readable.length).toBeGreaterThan(0);}expect(generatedSurface).toContain("SEMWRIGHT_FIGMA_AUX_READ_PROPERTIES");expect(generatedSurface).toContain("SEMWRIGHT_FIGMA_AUX_WRITE_PROPERTIES");});
 it("covers privileged auxiliary APIs without eval",()=>{expect(allCode).toContain("addMeasurement");expect(allCode).toContain("valuesByModeForCollectionAsync");expect(allCode).toContain("getStyleConsumersAsync");});
});
describe("authenticated loopback bridge",()=>{
 it("never sends the pairing secret as protocol data",()=>expect(ui).not.toContain("pairing_secret"));
 it("uses WebCrypto HMAC SHA-256",()=>{expect(ui).toContain("crypto.subtle.importKey");expect(ui).toContain('name:"HMAC"');});
 it("does not require crypto.randomUUID in the Figma UI sandbox",()=>{expect(ui).toContain("function randomSessionId()");expect(ui).toContain("crypto.getRandomValues(bytes)");expect(ui).toContain("session=randomSessionId()");});
 it("surfaces a safe synchronous pairing failure reason",()=>{expect(ui).toContain('state("Pairing failed: "+(err instanceof Error?err.message:"unknown error"))');});
 it("waits for a server-generated challenge",()=>{expect(ui).toContain('type:"hello",protocol:2');expect(ui).toContain('m.type==="challenge"');});
 it("accepts Figma WebSocket payloads delivered as string, Blob, or ArrayBuffer",()=>{expect(ui).toContain("async function websocketText(data)");expect(ui).toContain('data instanceof Blob');expect(ui).toContain("data instanceof ArrayBuffer");expect(ui).toContain("await websocketText(e.data)");});
 it("authenticates the server challenge with a separate HMAC proof",()=>{expect(ui).toContain('type:"authenticate"');expect(ui).toContain("proof:authProof");expect(ui).toContain("m.nonce");});
 it("only opens a loopback websocket",()=>{expect(ui).toContain('ws://localhost:');expect(ui).not.toContain('ws://127.0.0.1:');});
});
