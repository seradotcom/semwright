//! Optional clipboard providers. Fixed executable/argv, scrubbed environment, no shell.
//! Clipboard data never enters stderr, audit, tracing or command-line arguments.
use async_trait::async_trait;
use semwright_backend_api::{Backend,Context};
use semwright_types::*;
use serde_json::{Value,json};
use std::{path::Path,process::Stdio};
use tokio::{io::{AsyncReadExt,AsyncWriteExt},process::{Child,Command},sync::Mutex};
pub struct Clipboard{provider:Option<&'static str>,writer:Mutex<Option<Child>>}
impl Default for Clipboard{fn default()->Self{
    let provider=if std::env::var_os("WAYLAND_DISPLAY").is_some()&&Path::new("/usr/bin/wl-paste").is_file()&&Path::new("/usr/bin/wl-copy").is_file(){Some("wayland")}
        else if std::env::var_os("DISPLAY").is_some()&&Path::new("/usr/bin/xclip").is_file(){Some("x11")}else{None};
    Self{provider,writer:Mutex::new(None)}
}}
impl Clipboard{
    fn command(&self,write:bool)->Result<Command>{
        let provider=self.provider.ok_or_else(||Error::unavailable("Install wl-clipboard for Wayland or xclip for X11; these helpers are optional"))?;
        let mut c=match (provider,write){
            ("wayland",false)=>{let mut c=Command::new("/usr/bin/wl-paste");c.args(["--no-newline","--type","text/plain"]);c},
            ("wayland",true)=>{let mut c=Command::new("/usr/bin/wl-copy");c.args(["--foreground","--type","text/plain;charset=utf-8"]);c},
            (_,false)=>{let mut c=Command::new("/usr/bin/xclip");c.args(["-selection","clipboard","-out"]);c},
            (_,true)=>{let mut c=Command::new("/usr/bin/xclip");c.args(["-selection","clipboard","-in","-quiet"]);c},
        };
        c.env_clear().env("PATH","/usr/bin:/bin");for k in ["DISPLAY","WAYLAND_DISPLAY","XDG_RUNTIME_DIR","XAUTHORITY","LANG"]{if let Some(v)=std::env::var_os(k){c.env(k,v);}}
        c.stdin(if write{Stdio::piped()}else{Stdio::null()}).stdout(if write{Stdio::null()}else{Stdio::piped()}).stderr(Stdio::null()).kill_on_drop(true);Ok(c)
    }
}
#[async_trait]impl Backend for Clipboard{
    fn name(&self)->&'static str{"clipboard"}fn supports(&self,c:&str)->bool{matches!(c,"clipboard.read"|"clipboard.write")}
    async fn probe(&self)->Vec<Feature>{vec![Feature{backend:self.name().into(),capability:"clipboard.explicit".into(),status:if self.provider.is_some(){CapabilityStatus::SupportedWithHelper}else{CapabilityStatus::Unavailable},reason:"wl-clipboard/xclip are optional fixed-protocol helpers, not a generic shell".into(),remediation:"Install the correct helper and grant read/write capabilities independently".into()}]}
    async fn execute(&self,ctx:&Context,c:&str,args:&Value)->Result<Value>{ctx.check_cancelled()?;match c{
        "clipboard.read"=>{
            let mut child=self.command(false)?.spawn()?;let stdout=child.stdout.take().ok_or_else(||Error::new(ErrorCode::BackendFailed,"Clipboard pipe missing"))?;
            let limit=args["max_bytes"].as_u64().unwrap_or(1048576).min(1048576);let mut bytes=vec![];
            stdout.take(limit+1).read_to_end(&mut bytes).await?;
            if bytes.len()>limit as usize{let _=child.kill().await;return Err(Error::new(ErrorCode::ResourceExhausted,"Clipboard exceeds read budget"));}
            if !child.wait().await?.success(){return Err(Error::unavailable("No clipboard text or compositor rejected the helper"));}
            let text=String::from_utf8(bytes).map_err(|_|Error::invalid("Clipboard is not UTF-8 text"))?;Ok(json!({"text":text}))
        }
        "clipboard.write"=>{
            let mut writer=self.writer.lock().await;if let Some(mut previous)=writer.take(){let _=previous.kill().await;}
            let mut child=self.command(true)?.spawn()?;let mut stdin=child.stdin.take().ok_or_else(||Error::new(ErrorCode::BackendFailed,"Clipboard input pipe missing"))?;
            let text=arg_str(args,"text")?;stdin.write_all(text.as_bytes()).await?;stdin.shutdown().await?;drop(stdin);
            *writer=Some(child);Ok(json!({"written":true,"bytes":text.len(),"lifetime":"until_next_write_or_broker_shutdown"}))
        }
        _=>Err(Error::new(ErrorCode::Unsupported,"Unknown clipboard command")),
    }}
    async fn shutdown(&self)->Result<()>{if let Some(mut child)=self.writer.lock().await.take(){let _=child.kill().await;}Ok(())}
}
