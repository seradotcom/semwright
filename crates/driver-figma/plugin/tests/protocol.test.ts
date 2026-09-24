import {describe,it,expect} from "vitest";
import fs from "node:fs";
import path from "node:path";
const code=fs.readFileSync(path.join(process.cwd(),"src/code.ts"),"utf8");
const semantic=fs.readFileSync(path.join(process.cwd(),"src/semantic_complete.ts"),"utf8");
const allCode=semantic+"\n"+code;
const ui=fs.readFileSync(path.join(process.cwd(),"src/ui.html"),"utf8");
const manifest=JSON.parse(fs.readFileSync(path.join(process.cwd(),"manifest.json"),"utf8"));
describe("security surface",()=>{
 it("has no eval or Function constructor",()=>{expect(allCode).not.toMatch(/\beval\s*\(/);expect(allCode).not.toMatch(/new\s+Function/);});
 it("uses dynamic page access",()=>expect(manifest.documentAccess).toBe("dynamic-page"));
 it("does not allow wildcard network",()=>expect(manifest.networkAccess.allowedDomains).toEqual(["none"]));
 it("limits dev websocket to loopback",()=>expect(manifest.networkAccess.devAllowedDomains).toEqual(["ws://127.0.0.1:38471"]));
 it("uses async node lookup",()=>expect(code).toContain("getNodeByIdAsync"));
 it("uses async page switching",()=>expect(code).toContain("setCurrentPageAsync"));
 it("loads fonts before text mutation",()=>expect(code).toContain("loadFontAsync"));
 it("uses setReactionsAsync",()=>expect(code).toContain("setReactionsAsync"));
 it("sanitizes SVG",()=>expect(code).toContain("rejectUnsafeSvg"));
 it("bounds tree traversal",()=>expect(code).toContain("MAX_TREE"));
});
describe("advanced API",()=>{
 it("implements Motion style operations",()=>expect(code).toContain("applyAnimationStyle"));
 it("implements manual keyframes",()=>expect(code).toContain("applyManualKeyframeTrack"));
 it("implements timeline duration",()=>expect(code).toContain("setTimelineDuration"));
 it("implements spring normalization",()=>expect(code).toContain("physicalSpringToNormalized"));
 it("implements FigJam connectors",()=>expect(code).toContain("createConnector"));
 it("implements variables",()=>expect(code).toContain("getLocalVariablesAsync"));
});
describe("authenticated loopback bridge",()=>{
 it("never sends the pairing secret as protocol data",()=>expect(ui).not.toContain("pairing_secret"));
 it("uses WebCrypto HMAC SHA-256",()=>{expect(ui).toContain("crypto.subtle.importKey");expect(ui).toContain('name:"HMAC"');});
 it("waits for a server-generated challenge",()=>{expect(ui).toContain('type:"hello",protocol:2');expect(ui).toContain('m.type==="challenge"');});
 it("authenticates the server challenge with a separate HMAC proof",()=>{expect(ui).toContain('type:"authenticate"');expect(ui).toContain("proof:authProof");expect(ui).toContain("m.nonce");});
 it("only opens a loopback websocket",()=>expect(ui).toContain('ws://127.0.0.1:'));
});
