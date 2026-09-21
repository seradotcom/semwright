//! XDG portal RemoteDesktop and Screenshot clients with native consent and RAII revocation.
//! Uses the documented Notify* route; EIS and PipeWire pixel decoding are NOT advertised.
use async_trait::async_trait;
use futures_util::StreamExt;
use semwright_backend_api::{Backend,Context};
use semwright_protocol::private_directory;
use semwright_types::*;
use serde_json::{Value,json};
use std::{collections::HashMap,path::PathBuf,sync::Arc,time::Duration};
use tokio::sync::{Mutex,OnceCell};
use zbus::{Connection,Proxy,zvariant::{OwnedObjectPath,OwnedValue,Value as DValue}};
const DEST:&str="org.freedesktop.portal.Desktop";
const PATH:&str="/org/freedesktop/portal/desktop";
const REMOTE:&str="org.freedesktop.portal.RemoteDesktop";
type Options=HashMap<String,OwnedValue>;
fn error<T>(r:zbus::Result<T>)->Result<T>{r.map_err(|_|Error::new(ErrorCode::BackendFailed,"Portal D-Bus operation failed"))}
fn option<T>(value:T)->OwnedValue where T:Into<DValue<'static>>{OwnedValue::try_from(value.into()).expect("owned static portal option")}
#[derive(Debug,Clone,Copy,serde::Serialize,PartialEq,Eq)]
#[serde(rename_all="snake_case")]
pub enum ConsentState{NotRequested,Pending,Granted,Denied,Expired}
/// Unit-tested transition rules used by the live lifecycle.
pub fn transition(state:ConsentState,event:&str)->Result<ConsentState>{
    match (state,event){
        (ConsentState::NotRequested|ConsentState::Denied|ConsentState::Expired,"request")=>Ok(ConsentState::Pending),
        (ConsentState::Pending,"grant")=>Ok(ConsentState::Granted),
        (ConsentState::Pending,"deny")=>Ok(ConsentState::Denied),
        (_,"close")=>Ok(ConsentState::Expired),
        _=>Err(Error::new(ErrorCode::Conflict,"Invalid portal consent transition")),
    }
}
struct Session{connection:Connection,path:OwnedObjectPath,owner:String,devices:u32,armed:bool,closed:Arc<std::sync::atomic::AtomicBool>,watch:Option<tokio::task::JoinHandle<()>>}
impl Drop for Session{
    fn drop(&mut self){
        if let Some(watch)=self.watch.take(){watch.abort();}
        if self.armed{let connection=self.connection.clone();let path=self.path.clone();
            // Cancellation drops this guard too. Cleanup does not retain the broker.
            if let Ok(handle)=tokio::runtime::Handle::try_current(){handle.spawn(async move{
                if let Ok(proxy)=Proxy::new(&connection,DEST,path.as_str(),"org.freedesktop.portal.Session").await{
                    let _=tokio::time::timeout(Duration::from_secs(2),proxy.call::<_,_,()>("Close",&())).await;
                }
            });}
        }
    }
}
struct PendingRequest{connection:Connection,path:String,armed:bool}
impl Drop for PendingRequest{fn drop(&mut self){if self.armed{let c=self.connection.clone();let p=self.path.clone();if let Ok(handle)=tokio::runtime::Handle::try_current(){handle.spawn(async move{
    if let Ok(proxy)=Proxy::new(&c,DEST,p.as_str(),"org.freedesktop.portal.Request").await{let _=tokio::time::timeout(Duration::from_secs(2),proxy.call::<_,_,()>("Close",&())).await;}
});}}}}
struct PendingConsent{consent:Arc<Mutex<ConsentState>>,armed:bool}
impl Drop for PendingConsent{fn drop(&mut self){if self.armed{let c=self.consent.clone();if let Ok(handle)=tokio::runtime::Handle::try_current(){handle.spawn(async move{let mut current=c.lock().await;if *current==ConsentState::Pending{*current=ConsentState::Expired;}});}}}}
pub struct Portal{connection:OnceCell<Connection>,session:Mutex<Option<Session>>,consent:Arc<Mutex<ConsentState>>,artifacts:PathBuf}
impl Portal{
    pub fn new(artifacts:PathBuf)->Result<Self>{private_directory(&artifacts)?;Ok(Self{connection:OnceCell::new(),session:Mutex::new(None),consent:Arc::new(Mutex::new(ConsentState::NotRequested)),artifacts})}
    async fn connection(&self)->Result<&Connection>{self.connection.get_or_try_init(||async{error(Connection::session().await)}).await}
    async fn proxy(&self,interface:&'static str)->Result<Proxy<'_>>{error(Proxy::new(self.connection().await?,DEST,PATH,interface).await)}
    async fn version(&self,interface:&'static str)->u32{match self.proxy(interface).await{Ok(p)=>p.get_property("version").await.unwrap_or(0),Err(_)=>0}}
    async fn request<B>(&self,ctx:&Context,interface:&'static str,method:&str,build:B)->Result<Options>
    where B:FnOnce(Options)->RequestBody{
        let c=self.connection().await?;
        let sender=c.unique_name().ok_or_else(||Error::new(ErrorCode::BackendFailed,"Portal connection has no unique bus name"))?.as_str().trim_start_matches(':').replace('.',"_");
        let token=format!("sw{}",uuid::Uuid::new_v4().simple());
        let request_path=format!("/org/freedesktop/portal/desktop/request/{sender}/{token}");
        let request=error(Proxy::new(c,DEST,request_path.as_str(),"org.freedesktop.portal.Request").await)?;
        let mut responses=error(request.receive_signal("Response").await)?;
        let mut pending=PendingRequest{connection:c.clone(),path:request_path.clone(),armed:true};
        let mut options=Options::new();options.insert("handle_token".into(),option(token));
        let proxy=self.proxy(interface).await?;
        let path:OwnedObjectPath=match build(options){
            RequestBody::Options(opts)=>error(proxy.call(method,&(opts,)).await)?,
            RequestBody::Session(path,opts)=>error(proxy.call(method,&(path,opts)).await)?,
            RequestBody::Start(path,opts)=>error(proxy.call(method,&(path,"",opts)).await)?,
            RequestBody::Screenshot(opts)=>error(proxy.call(method,&("",opts)).await)?,
        };
        if path.as_str()!=request_path{
            // Never miss a response by reconnecting after the call. An unexpected
            // path indicates a non-conforming backend and fails closed.
            return Err(Error::new(ErrorCode::BackendFailed,"Portal returned an unexpected request path"));
        }
        let response=tokio::select!{
            _=ctx.cancellation.cancelled()=>{let _=request.call::<_,_,()>("Close",&()).await;return Err(Error::new(ErrorCode::Cancelled,"Portal request cancelled"));},
            reply=tokio::time::timeout(Duration::from_secs(110),responses.next())=>reply,
        };
        let message=match response{
            Ok(Some(signal))=>signal,
            _=>{let _=request.call::<_,_,()>("Close",&()).await;return Err(Error::new(ErrorCode::Timeout,"Portal consent response did not arrive"));}
        };
        let (status,results):(u32,Options)=message.body().deserialize().map_err(|_|Error::new(ErrorCode::BackendFailed,"Invalid portal response signature"))?;
        pending.armed=false;
        match status{0=>Ok(results),1=>Err(Error::new(ErrorCode::ConsentRequired,"User cancelled portal consent")),_=>Err(Error::new(ErrorCode::PermissionDenied,"Portal denied the request"))}
    }
    async fn start(&self,ctx:&Context,args:&Value)->Result<Value>{
        let mut guard=self.session.lock().await;
        if guard.as_ref().is_some_and(|s|s.closed.load(std::sync::atomic::Ordering::SeqCst)){guard.take();*self.consent.lock().await=ConsentState::Expired;}
        if let Some(existing)=guard.as_ref(){return if existing.owner==ctx.session{Ok(json!({"consent":"granted","already_active":true,"devices":existing.devices}))}else{Err(Error::new(ErrorCode::Conflict,"Remote input is already bound to another broker session"))};}
        let current=*self.consent.lock().await;*self.consent.lock().await=transition(current,"request")?;
        let mut pending=PendingConsent{consent:self.consent.clone(),armed:true};
        let result=self.start_inner(ctx,args).await;pending.armed=false;
        match result{
            Ok(session)=>{let devices=session.devices;*guard=Some(session);*self.consent.lock().await=ConsentState::Granted;Ok(json!({"consent":"granted","devices":devices,"ephemeral":true,"input_route":"portal_notify","eis":"not_implemented"}))},
            Err(e)=>{*self.consent.lock().await=ConsentState::Denied;Err(e)}
        }
    }
    async fn start_inner(&self,ctx:&Context,args:&Value)->Result<Session>{
        let keyboard=args["keyboard"].as_bool().unwrap_or(true);let pointer=args["pointer"].as_bool().unwrap_or(true);
        let types=(if keyboard{1}else{0})|(if pointer{2}else{0});
        if types==0{return Err(Error::invalid("Select at least one remote device"));}
        let available:u32=error(self.proxy(REMOTE).await?.get_property("AvailableDeviceTypes").await)?;
        if types&available!=types{return Err(Error::new(ErrorCode::Unsupported,"Requested devices are not exposed by this portal backend"));}
        let results=self.request(ctx,REMOTE,"CreateSession",|mut opts|{
            opts.insert("session_handle_token".into(),option(format!("sw{}",uuid::Uuid::new_v4().simple())));RequestBody::Options(opts)
        }).await?;
        let value=results.get("session_handle").ok_or_else(||Error::new(ErrorCode::BackendFailed,"Portal omitted session handle"))?;
        // The historical portal session_handle is a string despite its path semantics.
        let raw=String::try_from(value.try_clone().map_err(|_|Error::new(ErrorCode::BackendFailed,"Invalid session handle"))?)
            .or_else(|_|value.try_clone().and_then(OwnedObjectPath::try_from).map(|p|p.to_string()))
            .map_err(|_|Error::new(ErrorCode::BackendFailed,"Invalid session handle type"))?;
        let path=OwnedObjectPath::try_from(raw).map_err(|_|Error::new(ErrorCode::BackendFailed,"Invalid portal session path"))?;
        let mut session=Session{connection:self.connection().await?.clone(),path:path.clone(),owner:ctx.session.clone(),devices:0,armed:true,closed:Arc::new(std::sync::atomic::AtomicBool::new(false)),watch:None};
        let closed=session.closed.clone();let consent=self.consent.clone();let session_proxy=error(Proxy::new(&session.connection,DEST,session.path.as_str(),"org.freedesktop.portal.Session").await)?;
        let mut signals=error(session_proxy.receive_signal("Closed").await)?;
        session.watch=Some(tokio::spawn(async move{let _=signals.next().await;closed.store(true,std::sync::atomic::Ordering::SeqCst);*consent.lock().await=ConsentState::Expired;}));
        self.request(ctx,REMOTE,"SelectDevices",|mut opts|{opts.insert("types".into(),option(types));RequestBody::Session(path.clone(),opts)}).await?;
        let results=self.request(ctx,REMOTE,"Start",|opts|RequestBody::Start(path,opts)).await?;
        let devices=results.get("devices").and_then(|v|v.try_clone().ok()).and_then(|v|u32::try_from(v).ok()).unwrap_or(0);
        if devices&types!=types{return Err(Error::new(ErrorCode::PermissionDenied,"Portal did not grant all requested devices"));}
        session.devices=devices;Ok(session)
    }
    async fn stop(&self,owner:Option<&str>)->Result<Value>{
        let mut guard=self.session.lock().await;
        if let Some(session)=guard.as_ref(){if owner.is_some_and(|o|o!=session.owner){return Err(Error::new(ErrorCode::PolicyDenied,"Cannot close another broker session's remote-input grant"));}}
        if let Some(mut session)=guard.take(){let proxy=error(Proxy::new(&session.connection,DEST,session.path.as_str(),"org.freedesktop.portal.Session").await)?;error(proxy.call::<_,_,()>("Close",&()).await)?;session.armed=false;}
        *self.consent.lock().await=ConsentState::Expired;Ok(json!({"consent":"expired"}))
    }
    async fn notify(&self,ctx:&Context,command:&str,args:&Value)->Result<Value>{
        let guard=self.session.lock().await;
        let session=guard.as_ref().filter(|s|s.owner==ctx.session).ok_or_else(||Error::new(ErrorCode::ConsentRequired,"Run portal.start and approve the desktop consent dialog first"))?;
        if session.closed.load(std::sync::atomic::Ordering::SeqCst){return Err(Error::new(ErrorCode::ConsentRequired,"Portal input session was revoked or closed"));}
        let p=self.proxy(REMOTE).await?;let path=&session.path;
        let empty=Options::new();ctx.check_cancelled()?;
        if command.starts_with("input.")&&session.devices&1==0{return Err(Error::new(ErrorCode::PermissionDenied,"Keyboard was not granted"));}
        if command.starts_with("pointer.")&&session.devices&2==0{return Err(Error::new(ErrorCode::PermissionDenied,"Pointer was not granted"));}
        match command{
            "input.key"=>{let key=args["keysym"].as_u64().ok_or_else(||Error::invalid("keysym required"))?as i32;
                error(p.call::<_,_,()>("NotifyKeyboardKeysym",&(path,&empty,key,1u32)).await).map_err(Error::uncertain)?;
                error(p.call::<_,_,()>("NotifyKeyboardKeysym",&(path,&empty,key,0u32)).await).map_err(Error::uncertain)?;
            }
            "input.type"=>{for character in arg_str(args,"text")?.chars(){ctx.check_cancelled()?;
                let code=character as u32;let keysym=match character{'\n'=>0xff0d,'\t'=>0xff09,_ if code<=0xff=>code,_=>0x01000000|code}as i32;
                error(p.call::<_,_,()>("NotifyKeyboardKeysym",&(path,&empty,keysym,1u32)).await).map_err(Error::uncertain)?;
                error(p.call::<_,_,()>("NotifyKeyboardKeysym",&(path,&empty,keysym,0u32)).await).map_err(Error::uncertain)?;
            }}
            "pointer.move"=>error(p.call::<_,_,()>("NotifyPointerMotion",&(path,&empty,args["dx"].as_f64().ok_or_else(||Error::invalid("dx required"))?,args["dy"].as_f64().ok_or_else(||Error::invalid("dy required"))?)).await).map_err(Error::uncertain)?,
            "pointer.click"=>{let button=match arg_str(args,"button")?{"left"=>272i32,"right"=>273,"middle"=>274,_=>return Err(Error::invalid("Unknown pointer button"))};
                error(p.call::<_,_,()>("NotifyPointerButton",&(path,&empty,button,1u32)).await).map_err(Error::uncertain)?;
                error(p.call::<_,_,()>("NotifyPointerButton",&(path,&empty,button,0u32)).await).map_err(Error::uncertain)?;
            }
            "pointer.scroll"=>{let mut opts=Options::new();opts.insert("finish".into(),option(true));error(p.call::<_,_,()>("NotifyPointerAxis",&(path,opts,args["dx"].as_f64().ok_or_else(||Error::invalid("dx required"))?,args["dy"].as_f64().ok_or_else(||Error::invalid("dy required"))?)).await).map_err(Error::uncertain)?;}
            _=>return Err(Error::new(ErrorCode::Unsupported,"Unknown remote-input command")),
        }Ok(json!({"sent":true,"backend_route":"portal_notify","coordinate_space":"relative_logical_delta"}))
    }
    async fn capture(&self,ctx:&Context)->Result<Value>{
        let result=self.request(ctx,"org.freedesktop.portal.Screenshot","Screenshot",|mut opts|{
            opts.insert("interactive".into(),option(true));RequestBody::Screenshot(opts)
        }).await?;
        let uri=result.get("uri").and_then(|v|v.try_clone().ok()).and_then(|v|String::try_from(v).ok()).ok_or_else(||Error::new(ErrorCode::BackendFailed,"Screenshot portal did not return a URI"))?;
        let source=url::Url::parse(&uri).ok().filter(|u|u.scheme()=="file").and_then(|u|u.to_file_path().ok()).ok_or_else(||Error::new(ErrorCode::PermissionDenied,"Only local screenshot file URIs are accepted"))?;
        let destination=self.artifacts.join(format!("capture-{}.png",uuid::Uuid::new_v4().simple()));
        copy_screenshot(&source,&destination)?;
        let cleanup=destination.clone();tokio::spawn(async move{tokio::time::sleep(Duration::from_secs(300)).await;let _=tokio::fs::remove_file(cleanup).await;});
        Ok(json!({"artifact":destination,"expires_in_seconds":300,"mime_type":"image/png","scope":"user_selected_by_portal","coordinate_space":"unknown_until_image_decoded","warning":"The selected capture may include sensitive content; image bytes are never audited"}))
    }
}
enum RequestBody{Options(Options),Session(OwnedObjectPath,Options),Start(OwnedObjectPath,Options),Screenshot(Options)}
fn copy_screenshot(source:&std::path::Path,destination:&std::path::Path)->Result<()>{
    use std::os::unix::fs::{OpenOptionsExt,MetadataExt};use std::io::{Read,Write};
    let mut file=std::fs::OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW|libc::O_NONBLOCK).open(source)?;
    let metadata=file.metadata()?;
    // SAFETY: getuid has no pointer arguments or other preconditions.
    if !metadata.is_file()||metadata.uid()!=unsafe{libc::getuid()}||metadata.len()>33_554_432{return Err(Error::new(ErrorCode::PermissionDenied,"Unsafe screenshot artifact"));}
    let mut signature=[0u8;8];file.read_exact(&mut signature)?;
    if signature!=*b"\x89PNG\r\n\x1a\n"{return Err(Error::new(ErrorCode::Unsupported,"This artifact path accepts PNG screenshots only"));}
    let mut output=std::fs::OpenOptions::new().create_new(true).write(true).mode(0o600).open(destination)?;output.write_all(&signature)?;
    let copied=std::io::copy(&mut file.take(33_554_432),&mut output)?;if copied+8>33_554_432{let _=std::fs::remove_file(destination);return Err(Error::new(ErrorCode::ResourceExhausted,"Screenshot grew beyond budget"));}output.sync_all()?;Ok(())
}
#[async_trait]impl Backend for Portal{
    fn name(&self)->&'static str{"portal"}
    fn supports(&self,c:&str)->bool{matches!(c,"portal.start"|"portal.stop"|"portal.status"|"input.key"|"input.type"|"pointer.move"|"pointer.click"|"pointer.scroll"|"screen.capture"|"screen.stream_info")}
    async fn probe(&self)->Vec<Feature>{
        let remote=self.version(REMOTE).await;let screenshot=self.version("org.freedesktop.portal.Screenshot").await;
        vec![Feature{backend:self.name().into(),capability:"input.consented".into(),status:if remote>0{CapabilityStatus::SupportedWithConsent}else{CapabilityStatus::Unavailable},reason:format!("RemoteDesktop interface version {remote}; Notify route, EIS not implemented"),remediation:"Install the matching xdg-desktop-portal backend and explicitly approve portal.start".into()},
            Feature{backend:self.name().into(),capability:"screen.capture".into(),status:if screenshot>0{CapabilityStatus::SupportedWithConsent}else{CapabilityStatus::Unavailable},reason:format!("Screenshot portal version {screenshot}"),remediation:"A user-facing screenshot chooser is required".into()}]
    }
    async fn execute(&self,ctx:&Context,c:&str,args:&Value)->Result<Value>{match c{
        "portal.start"=>self.start(ctx,args).await,"portal.stop"=>self.stop(Some(&ctx.session)).await,
        "portal.status"=>Ok(json!({"consent":*self.consent.lock().await,"remote_desktop_version":self.version(REMOTE).await,"persistent_tokens":false,"eis":"not_implemented","input_route":"portal_notify"})),
        "screen.capture"=>self.capture(ctx).await,
        "screen.stream_info"=>Ok(json!({"screencast_version":self.version("org.freedesktop.portal.ScreenCast").await,"pixel_stream":"unavailable","reason":"PipeWire decoder and EIS transport are not implemented; interactive screenshot is a separate supported route"})),
        c if c.starts_with("input.")||c.starts_with("pointer.")=>self.notify(ctx,c,args).await,
        _=>Err(Error::new(ErrorCode::Unsupported,"Unknown portal command")),
    }}
    async fn shutdown(&self)->Result<()>{self.stop(None).await.map(|_|())}
}
#[cfg(test)]mod tests{use super::*;
    #[test]fn consent_cannot_be_skipped(){assert!(transition(ConsentState::NotRequested,"grant").is_err());}
    #[test]fn consent_lifecycle(){let a=transition(ConsentState::NotRequested,"request").unwrap();let b=transition(a,"grant").unwrap();assert_eq!(transition(b,"close").unwrap(),ConsentState::Expired);}
    #[test]fn denial_can_be_requested_again(){assert_eq!(transition(ConsentState::Denied,"request").unwrap(),ConsentState::Pending);}
    #[test]fn screenshot_rejects_non_png(){let dir=tempfile::tempdir().unwrap();let source=dir.path().join("not-image");std::fs::write(&source,b"not a screenshot").unwrap();assert!(copy_screenshot(&source,&dir.path().join("out")).is_err());}
}
