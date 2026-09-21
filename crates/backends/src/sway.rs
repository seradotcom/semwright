//! Native i3/Sway IPC, not shell/swaymsg interpolation.
use async_trait::async_trait;
use semwright_backend_api::{Backend,Context,feature};
use semwright_types::*;
use serde_json::{Value,json};
use std::path::PathBuf;
use tokio::io::{AsyncReadExt,AsyncWriteExt};
use tokio::net::UnixStream;

pub struct Sway{socket:Option<PathBuf>}
impl Default for Sway{fn default()->Self{Self{socket:std::env::var_os("SWAYSOCK").or_else(||std::env::var_os("I3SOCK")).map(PathBuf::from)}}}
impl Sway{
    pub fn at(socket:PathBuf)->Self{Self{socket:Some(socket)}}
    async fn request(&self,kind:u32,payload:&str)->Result<Value>{
        let path=self.socket.as_ref().ok_or_else(||Error::unavailable("SWAYSOCK/I3SOCK is not configured"))?;
        let mut stream=UnixStream::connect(path).await?;
        // SAFETY: getuid has no pointer/lifetime preconditions.
        if stream.peer_cred()?.uid()!=unsafe{libc::getuid()}{return Err(Error::new(ErrorCode::PermissionDenied,"Compositor socket UID mismatch"));}
        if payload.len()>4096{return Err(Error::invalid("Compositor request is too large"));}
        let mut header=b"i3-ipc".to_vec();header.extend_from_slice(&(payload.len()as u32).to_le_bytes());header.extend_from_slice(&kind.to_le_bytes());
        stream.write_all(&header).await?;stream.write_all(payload.as_bytes()).await?;
        let mut header=[0u8;14];stream.read_exact(&mut header).await?;
        if &header[..6]!=b"i3-ipc"{return Err(Error::new(ErrorCode::BackendFailed,"Invalid Sway IPC magic"));}
        let length=u32::from_le_bytes(header[6..10].try_into().map_err(|_|Error::invalid("Bad IPC header"))?)as usize;
        let response_kind=u32::from_le_bytes(header[10..14].try_into().map_err(|_|Error::invalid("Bad IPC type"))?);
        if length>4_194_304||response_kind!=kind{return Err(Error::new(ErrorCode::BackendFailed,"Sway response exceeds contract limits"));}
        let mut payload=vec![0;length];stream.read_exact(&mut payload).await?;serde_json::from_slice(&payload).map_err(Into::into)
    }
    pub async fn windows(&self)->Result<Vec<Value>>{let tree=self.request(4,"").await?;Ok(flatten_windows(&tree))}
    fn target(window:&Value)->Result<NativeTarget>{
        let id=window["id"].as_u64().ok_or_else(||Error::new(ErrorCode::BackendFailed,"Sway returned an invalid window id"))?;
        let app=window["app_id"].as_str().or_else(||window["window_properties"]["class"].as_str()).unwrap_or("");
        Ok(NativeTarget{kind:"win".into(),identity:id.to_string(),revision:0,fingerprint:format!("{}:{app}",window["pid"]),app:app.into()})
    }
}
/// Traversal includes floating_nodes and bounds broken/cyclic-size trees by count.
pub fn flatten_windows(tree:&Value)->Vec<Value>{
    let mut stack=vec![tree];let mut output=vec![];let mut visited=0;
    while let Some(node)=stack.pop(){visited+=1;if visited>20_000{break;}
        if node["type"]=="con" && (node["app_id"].is_string()||node["window"].is_number()) {output.push(node.clone());}
        for key in ["floating_nodes","nodes"]{if let Some(children)=node[key].as_array(){stack.extend(children.iter().rev());}}
    }output
}
#[async_trait]impl Backend for Sway{
    fn name(&self)->&'static str{"sway"}
    fn supports(&self,c:&str)->bool{matches!(c,"window.list"|"window.focus"|"window.move"|"window.resize"|"window.close"|"app.close")}
    async fn probe(&self)->Vec<Feature>{let ready=tokio::time::timeout(std::time::Duration::from_secs(2),self.request(7,"")).await.is_ok_and(|r|r.is_ok());vec![feature(self.name(),"window.manage",ready,"Native i3/Sway socket handshake","Run inside Sway/i3 and preserve the owner-provided SWAYSOCK/I3SOCK")]}
    async fn execute(&self,ctx:&Context,c:&str,args:&Value)->Result<Value>{
        ctx.check_cancelled()?;
        if c=="window.list"{let mut windows=vec![];for v in self.windows().await?{windows.push(json!({"ref":target_marker(Self::target(&v)?),"title":v["name"],"app":v["app_id"],"focused":v["focused"],"bounds":v["rect"],"coordinate_space":"compositor_logical"}));}return Ok(json!({"windows":windows}));}
        let target=native_target(args)?;self.validate(&target).await?;
        let id=target.identity.parse::<u64>().map_err(|_|Error::invalid("Invalid Sway target"))?;
        let instruction=match c{
            "window.focus"=>"focus".into(),
            "window.close"|"app.close"=>"kill".into(),
            "window.move"=>format!("move position {} {}",args["x"].as_i64().ok_or_else(||Error::invalid("x required"))?,args["y"].as_i64().ok_or_else(||Error::invalid("y required"))?),
            "window.resize"=>format!("resize set width {} px height {} px",args["width"].as_u64().ok_or_else(||Error::invalid("width required"))?,args["height"].as_u64().ok_or_else(||Error::invalid("height required"))?),
            _=>return Err(Error::new(ErrorCode::Unsupported,"Unsupported Sway command")),
        };
        ctx.check_cancelled()?;
        let result=self.request(0,&format!("[con_id={id}] {instruction}")).await.map_err(Error::uncertain)?;
        let replies=result.as_array().ok_or_else(||Error::new(ErrorCode::BackendFailed,"Malformed Sway command response").uncertain())?;
        if replies.is_empty()||!replies.iter().all(|v|v["success"].as_bool()==Some(true)){return Err(Error::new(ErrorCode::BackendFailed,"Compositor rejected operation; tiled geometry constraints may apply").uncertain());}
        Ok(json!({"accepted":true,"coordinate_space":"compositor_logical"}))
    }
    async fn validate(&self,t:&NativeTarget)->Result<()>{for w in self.windows().await?{let current=Self::target(&w)?;if current.identity==t.identity&&current.fingerprint==t.fingerprint{return Ok(());}}Err(Error::new(ErrorCode::StaleReference,"Sway window identity changed"))}
    async fn is_focused(&self,t:&NativeTarget)->Result<bool>{for w in self.windows().await?{let current=Self::target(&w)?;if current.identity==t.identity&&current.fingerprint==t.fingerprint{return Ok(w["focused"].as_bool().unwrap_or(false));}}Ok(false)}
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn floating_and_tiled_are_both_visible(){let tree=json!({"type":"root","nodes":[{"type":"con","id":1,"app_id":"a"}],"floating_nodes":[{"type":"con","id":2,"app_id":"b"}]});assert_eq!(flatten_windows(&tree).len(),2);}
    #[test]fn container_is_not_window(){assert!(flatten_windows(&json!({"type":"con","nodes":[]})).is_empty());}
    #[test]fn target_contains_no_command_source(){let t=Sway::target(&json!({"id":42,"pid":7,"app_id":"a"})).unwrap();assert_eq!(t.identity,"42");}
}
