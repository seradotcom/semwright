import {readFileSync} from "node:fs";
import vm from "node:vm";
import {describe, expect, it} from "vitest";

type UiElement = {
  value: string;
  textContent: string;
  onclick: null | (() => void);
};

type Sent = Record<string, unknown>;

function uiHarness() {
  const html=readFileSync(new URL("../src/ui.html",import.meta.url),"utf8");
  const match=html.match(/<script>([\s\S]*?)<\/script>/);
  if(!match)throw new Error("ui script missing");
  const elements=new Map<string,UiElement>();
  for(const [id,value] of [["port","38471"],["secret",""],["connect",""],["disconnect",""],["state","Disconnected"]]){
    elements.set(id,{value,textContent:value,onclick:null});
  }
  const pluginMessages:any[]=[];
  const sockets:MockSocket[]=[];
  let timerId=0;
  const timers=new Map<number,()=>void>();

  class MockSocket {
    static CONNECTING=0;
    static OPEN=1;
    static CLOSING=2;
    static CLOSED=3;
    readyState=MockSocket.CONNECTING;
    sent:Sent[]=[];
    onopen:null|(()=>void)=null;
    onmessage:null|((event:{data:string})=>Promise<void>|void)=null;
    onclose:null|(()=>void)=null;
    onerror:null|(()=>void)=null;
    constructor(public url:string){sockets.push(this);}
    send(raw:string){this.sent.push(JSON.parse(raw));}
    close(){this.readyState=MockSocket.CLOSED;this.onclose?.();}
    open(){this.readyState=MockSocket.OPEN;this.onopen?.();}
    async server(value:Sent){await this.onmessage?.({data:JSON.stringify(value)});}
  }

  const context:any={
    console,
    document:{getElementById:(id:string)=>elements.get(id)??null},
    parent:{postMessage:(value:any)=>pluginMessages.push(value.pluginMessage)},
    WebSocket:MockSocket,
    crypto:globalThis.crypto,
    Blob:globalThis.Blob,
    ArrayBuffer:globalThis.ArrayBuffer,
    Uint8Array:globalThis.Uint8Array,
    DataView:globalThis.DataView,
    TextEncoder:globalThis.TextEncoder,
    TextDecoder:globalThis.TextDecoder,
    BigInt:globalThis.BigInt,
    setTimeout:(fn:()=>void)=>{const id=++timerId;timers.set(id,fn);return id;},
    clearTimeout:(id:number)=>{timers.delete(id);},
  };
  context.globalThis=context;
  vm.runInNewContext(match[1],context);

  const element=(id:string)=>{
    const value=elements.get(id);
    if(!value)throw new Error("missing element "+id);
    return value;
  };
  const deliver=(message:any)=>context.onmessage({data:{pluginMessage:message}});
  return {context,element,pluginMessages,sockets,deliver,MockSocket};
}

function latest<T=any>(values:T[],type:string):T {
  const found=[...values].reverse().find((value:any)=>value?.type===type);
  if(!found)throw new Error("missing message "+type);
  return found;
}

describe("Figma bridge trusted-session continuity UI",()=>{
  it("pairs once, resumes after plugin restart, and revokes on explicit disconnect",async()=>{
    const credential={resumeId:"a".repeat(32),resumeToken:"b".repeat(64)};
    const documentId="c".repeat(32);
    const initial=uiHarness();

    expect(latest(initial.pluginMessages,"bridge-status")).toEqual({type:"bridge-status"});
    initial.deliver({
      type:"document-context",editorType:"figma",documentId,pageId:"0:1",revision:7,
      resumeCredential:null,documentContinuity:true,
    });

    initial.element("secret").value="d".repeat(64);
    initial.element("connect").onclick?.();
    const pairingStatus=latest<any>(initial.pluginMessages,"bridge-status");
    expect(pairingStatus.ensureDocumentIdentity).toBe(true);
    initial.deliver({
      type:"document-context",editorType:"figma",documentId,pageId:"0:1",revision:7,
      resumeCredential:null,documentContinuity:true,
    });

    expect(initial.sockets).toHaveLength(1);
    const first=initial.sockets[0];
    first.open();
    const hello=first.sent.at(-1)!;
    expect(hello.type).toBe("hello");
    const session=String(hello.session_id);
    expect(session.length).toBeGreaterThan(10);
    await first.server({
      type:"challenge",session_id:session,generation:1,nonce:"e".repeat(32),
    });
    const authenticate=first.sent.at(-1)!;
    expect(authenticate.type).toBe("authenticate");
    expect(String(authenticate.proof)).toMatch(/^[0-9a-f]{64}$/);

    await first.server({
      type:"ready",session_id:session,generation:1,revision:7,
      resume_id:credential.resumeId,resume_token:credential.resumeToken,
    });
    expect(initial.element("state").textContent).toContain("Connected");
    expect(initial.element("secret").value).toBe("");
    expect(latest(initial.pluginMessages,"bridge-resume-store")).toEqual({
      type:"bridge-resume-store",documentId,credential,
    });

    // Simulate Figma/plugin process restart: only the opaque resume credential
    // comes back from clientStorage through the plugin main context.
    const restarted=uiHarness();
    restarted.deliver({
      type:"document-context",editorType:"figma",documentId,pageId:"0:1",revision:9,
      resumeCredential:credential,documentContinuity:true,
    });
    expect(restarted.sockets).toHaveLength(1);
    const resumed=restarted.sockets[0];
    resumed.open();
    expect(resumed.sent.at(-1)).toMatchObject({
      type:"resume_hello",document_id:documentId,resume_id:credential.resumeId,
    });

    await resumed.server({
      type:"resume_challenge",session_id:session,generation:2,
      nonce:"f".repeat(32),resume_id:credential.resumeId,
    });
    const resumeAuth=resumed.sent.at(-1)!;
    expect(resumeAuth).toMatchObject({
      type:"resume_authenticate",session_id:session,generation:2,
      resume_id:credential.resumeId,
    });
    expect(String(resumeAuth.proof)).toMatch(/^[0-9a-f]{64}$/);

    // Stable credential, fresh nonce/generation: no storage handoff window.
    await resumed.server({
      type:"ready",session_id:session,generation:2,revision:9,
      resume_id:credential.resumeId,resume_token:credential.resumeToken,
    });
    expect(restarted.element("state").textContent).toContain("Connected");
    expect(latest(restarted.pluginMessages,"bridge-resume-store")).toEqual({
      type:"bridge-resume-store",documentId,credential,
    });

    restarted.element("disconnect").onclick?.();
    expect(resumed.sent.some(message=>
      message.type==="revoke_resume"&&message.resume_id===credential.resumeId
    )).toBe(true);
    expect(latest(restarted.pluginMessages,"bridge-resume-clear")).toEqual({
      type:"bridge-resume-clear",documentId,
    });
    expect(restarted.element("state").textContent).toBe("Disconnected");
  });
});
