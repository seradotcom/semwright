//! Peer-authenticated Blender bridge. The add-on dispatches only on Blender's main thread.
use async_trait::async_trait;
use semwright_backend_api::{Backend,Context};
use semwright_protocol::{read_frame,write_frame,validate_peer,current_uid};
use semwright_types::*;
use serde_json::{Value,json};
use std::{path::PathBuf,os::unix::fs::MetadataExt};
use tokio::net::UnixStream;

pub struct Blender { socket: PathBuf }
impl Blender {
    pub fn new(socket:PathBuf)->Self { Self{socket} }
    async fn connect(&self)->Result<UnixStream> {
        let meta=std::fs::symlink_metadata(&self.socket).map_err(|_|Error::unavailable("Blender add-on socket is absent; enable the add-on explicitly"))?;
        if !std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type()) || meta.uid()!=current_uid() || meta.mode()&0o777!=0o600 {
            return Err(Error::new(ErrorCode::PermissionDenied,"Blender socket must be owned by this user and mode 0600"));
        }
        let mut stream=UnixStream::connect(&self.socket).await?;
        validate_peer(&stream)?;
        write_frame(&mut stream,&json!({"type":"hello","protocol":1})).await?;
        let hello:Value=read_frame(&mut stream).await?;
        if hello.get("type").and_then(Value::as_str)!=Some("ready") || hello.get("protocol").and_then(Value::as_u64)!=Some(1) {
            return Err(Error::new(ErrorCode::PluginProtocolError,"Blender protocol handshake failed"));
        }
        Ok(stream)
    }
}
#[async_trait]
impl Backend for Blender {
    fn name(&self)->&'static str { "blender" }
    fn supports(&self,command:&str)->bool { command.starts_with("blender.") }
    async fn probe(&self)->Vec<Feature> {
        let available=matches!(tokio::time::timeout(std::time::Duration::from_secs(2),self.connect()).await,Ok(Ok(_)));
        vec![Feature{backend:self.name().into(),capability:"blender.observe".into(),status:if available{CapabilityStatus::Experimental}else{CapabilityStatus::Unavailable},reason:if available{"Add-on handshake succeeded; live application acceptance remains separate"}else{"Blender add-on not reachable"}.into(),remediation:"Install adapters/blender/semwright_blender, configure a workspace, and enable its local bridge".into()}]
    }
    async fn execute(&self,ctx:&Context,command:&str,args:&Value)->Result<Value> {
        ctx.check_cancelled()?;
        let mut stream=self.connect().await?;
        let id=unique_id();
        write_frame(&mut stream,&json!({"type":"execute","id":id,"command":command,"args":args})).await?;
        // Once dispatched, cancellation cannot rewind Blender. Never retry a lost mutation.
        let response:Value=tokio::select! {
            _=ctx.cancellation.cancelled()=>return Err(Error::new(ErrorCode::Cancelled,"Blender action cancelled; application may already have applied it").uncertain()),
            r=read_frame(&mut stream)=>r.map_err(Error::uncertain)?,
        };
        if response.get("id").and_then(Value::as_str)!=Some(id.as_str()) { return Err(Error::new(ErrorCode::PluginProtocolError,"Blender response id mismatch").uncertain()); }
        if response.get("ok").and_then(Value::as_bool)==Some(true) {
            response.get("data").cloned().filter(Value::is_object).ok_or_else(||Error::new(ErrorCode::PluginProtocolError,"Blender response must contain an object").uncertain())
        } else {
            let code=response.pointer("/error/code").and_then(Value::as_str).unwrap_or("BackendFailed");
            let error=match code { "NotFound"=>ErrorCode::NotFound,"Conflict"=>ErrorCode::Conflict,"InvalidArgument"=>ErrorCode::InvalidArgument,"PolicyDenied"=>ErrorCode::PolicyDenied,"Unsupported"=>ErrorCode::Unsupported,_=>ErrorCode::BackendFailed };
            Err(Error::new(error,"Blender rejected or failed the typed action; host details are redacted").uncertain())
        }
    }
}
