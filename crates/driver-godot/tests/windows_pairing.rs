//! Bounded physical negative pairing probe; never reads the real secret or sends operations.
#![cfg(target_os = "windows")]
use futures_util::{SinkExt,StreamExt};
use semwright_godot_driver::bridge::{proof,random_hex,transcript};
use serde_json::{json,Value};
use sha2::{Digest,Sha256};
use std::{env,fs,io::{ErrorKind,Write},path::PathBuf,time::{Duration,Instant}};
use tokio_tungstenite::{connect_async_with_config,tungstenite::{
    error::ProtocolError,protocol::WebSocketConfig,Error as WsError,Message}};
fn hex_value(s:&str,n:usize)->bool {
    s.len()==n && s.bytes().all(|b|b.is_ascii_digit()||(b'a'..=b'f').contains(&b))
}
fn field<'a>(v:&'a Value,k:&str,n:usize)->Result<&'a str,&'static str> {
    v.get(k).and_then(Value::as_str).filter(|s|hex_value(s,n)).ok_or("challenge_field_invalid")
}
async fn attempt(port:u16,project:&str,r:&mut Value)->Result<(&'static str,Option<u16>),&'static str> {
    let mut config=WebSocketConfig::default();
    config.max_message_size=Some(4096);config.max_frame_size=Some(4096);
    let (mut ws,response)=connect_async_with_config(format!("ws://127.0.0.1:{port}"),Some(config),false)
        .await.map_err(|_|"websocket_connect_failed")?;
    r["websocket_status"]=json!(response.status().as_u16());
    let client=random_hex(16).map_err(|_|"nonce_generation_failed")?;
    r["client_nonce_sha256"]=json!(hex::encode(Sha256::digest(client.as_bytes())));
    ws.send(Message::Text(json!({"type":"hello","project":project,"nonce":client,
        "plugin_version":"0.2.0","engine_version":"4.7.2-stable (official)"}).to_string().into()))
        .await.map_err(|_|"hello_send_failed")?;
    r["phase"]=json!("hello_sent");
    let mut challenge=None;
    for _ in 0..16 {
        match ws.next().await {
            Some(Ok(Message::Text(t)))=>{challenge=Some(serde_json::from_str::<Value>(t.as_ref()).map_err(|_|"challenge_json_invalid")?);break;}
            Some(Ok(Message::Ping(p)))=>ws.send(Message::Pong(p)).await.map_err(|_|"pre_auth_pong_failed")?,
            Some(Ok(Message::Pong(_)))=>{},
            Some(Ok(Message::Close(_)))|None=>return Err("closed_before_challenge"),
            Some(Err(_))=>return Err("transport_failure_before_challenge"),
            _=>return Err("unexpected_pre_auth_frame"),
        }
    }
    let c=challenge.ok_or("pre_auth_frame_limit")?;
    if c["type"]=="ready" {r["ready_observed"]=json!(true);return Err("ready_before_auth");}
    let o=c.as_object().ok_or("challenge_not_object")?;
    let fields=["type","bridge","nonce","generation","session","proof"];
    if o.len()!=fields.len() || fields.iter().any(|f|!o.contains_key(*f)) || c["type"]!="challenge" || c["bridge"]!=1 {
        return Err("challenge_schema_mismatch");
    }
    let server=field(&c,"nonce",32)?;let generation=field(&c,"generation",32)?;let session=field(&c,"session",32)?;
    field(&c,"proof",64)?;
    r["challenge"]=json!({"bridge":1,"server_nonce":server,"generation":generation,"session":session,"proof_well_formed":true});
    let invalid=proof(&[0xA7;32],&transcript("client",project,&client,server,generation,session)).map_err(|_|"proof_construction_failed")?;
    ws.send(Message::Text(json!({"type":"authenticate","proof":invalid}).to_string().into()))
        .await.map_err(|_|"invalid_auth_send_failed")?;
    r["auth_frame_sent"]=json!(true);r["phase"]=json!("awaiting_post_auth_termination");
    for _ in 0..16 {
        match ws.next().await {
            Some(Ok(Message::Close(c)))=>return Ok(("close_frame_after_auth",c.map(|f|u16::from(f.code)))),
            None=>return Ok(("stream_ended_after_auth",None)),
            Some(Err(WsError::ConnectionClosed))=>return Ok(("connection_closed_after_auth",None)),
            Some(Err(WsError::Protocol(ProtocolError::ResetWithoutClosingHandshake)))=>return Ok(("closed_without_close_frame_after_auth",None)),
            Some(Err(WsError::Io(e))) if matches!(e.kind(),ErrorKind::ConnectionReset|ErrorKind::UnexpectedEof)=>return Ok(("connection_reset_or_eof_after_auth",None)),
            Some(Ok(Message::Text(t)))=>{
                let v:Value=serde_json::from_str(t.as_ref()).map_err(|_|"invalid_application_frame_after_auth")?;
                if v["type"]=="ready" {r["ready_observed"]=json!(true);return Err("ready_with_wrong_key");}
                return Err("unexpected_application_frame_after_auth");
            }
            Some(Ok(Message::Ping(p)))=>ws.send(Message::Pong(p)).await.map_err(|_|"post_auth_pong_failed")?,
            Some(Ok(Message::Pong(_)))=>{},
            Some(Err(_))=>return Err("unclassified_transport_failure_after_auth"),
            _=>return Err("unexpected_post_auth_frame"),
        }
    }
    Err("post_auth_frame_limit")
}
#[tokio::test(flavor="current_thread")]
#[ignore="Physical Windows Host-proxy pairing test; explicit owned endpoint required"]
async fn wrong_key_pairing_closes_before_ready() {
    let port:u16=env::var("SEMWRIGHT_TEST_GODOT_BRIDGE_PORT").expect("Explicit port").parse().expect("u16 port");
    assert_ne!(port,0);
    let project=env::var("SEMWRIGHT_TEST_GODOT_PROJECT_ID").expect("Explicit project identity");
    assert!(hex_value(&project,64));
    let dir=PathBuf::from(env::var_os("SEMWRIGHT_TEST_GODOT_EVIDENCE_DIR").expect("New evidence directory"));
    assert!(dir.is_absolute());fs::create_dir(&dir).expect("Preserve prior evidence");
    let start=Instant::now();
    let mut r=json!({"schema":1,"test":"windows_godot_wrong_key_pairing","project":project,
        "endpoint":{"host":"127.0.0.1","port":port},"phase":"not_connected","auth_frame_sent":false,
        "ready_observed":false,"server_proof_verified":false,"server_rejection_reason":"UNOBSERVED",
        "asserts_permission_denied_response":false,"proofs_recorded":false,"key_material_recorded":false,
        "complete_transcript_recorded":false,"raw_frames_recorded":false,"operations_sent":0,"resume_session_sent":false});
    let passed=match tokio::time::timeout(Duration::from_secs(10),attempt(port,&project,&mut r)).await {
        Ok(Ok((reason,code)))=>{r["result"]=json!("PASS_POST_AUTH_CLOSURE_WITHOUT_READY");r["termination"]=json!(reason);r["close_code"]=json!(code);true}
        Ok(Err(reason))=>{r["result"]=json!("FAIL_OR_INCONCLUSIVE");r["failure_kind"]=json!(reason);false}
        Err(_)=>{r["result"]=json!("FAIL_OR_INCONCLUSIVE");r["failure_kind"]=json!("handshake_deadline");false}
    };
    r["elapsed_ms"]=json!(start.elapsed().as_millis());
    r["pass_scope"]=json!("Schema-valid challenge received, deliberately wrong-key authentication sent, same connection terminated without Ready; server error cause unobserved");
    let mut raw=serde_json::to_vec_pretty(&r).unwrap();raw.push(b'\n');assert!(raw.len()<=8192);
    let mut f=fs::OpenOptions::new().write(true).create_new(true).open(dir.join("receipt.json")).unwrap();
    f.write_all(&raw).unwrap();f.sync_all().unwrap();
    assert!(passed,"Inspect preserved pairing receipt; no operation was sent");
}
