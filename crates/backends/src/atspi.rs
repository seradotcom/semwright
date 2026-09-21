//! AT-SPI2 over its dedicated accessibility bus. Public API types never leak zbus.
//! Invalidates all UI generations conservatively on object/window events or bus loss.
use async_trait::async_trait;
use futures_util::StreamExt;
use semwright_backend_api::{Backend,Context,feature};
use semwright_types::*;
use serde_json::{Value,json};
use std::collections::{BTreeMap,BTreeSet,VecDeque};
use std::sync::{Arc,atomic::{AtomicU64,AtomicBool,Ordering}};
use std::time::Duration;
use tokio::sync::OnceCell;
use zbus::{Connection,Proxy,zvariant::OwnedObjectPath};

type Object=(String,OwnedObjectPath);
const ACCESSIBLE:&str="org.a11y.atspi.Accessible";
const ROOT:&str="/org/a11y/atspi/accessible/root";
pub struct Atspi{connection:OnceCell<Connection>,revision:Arc<AtomicU64>,events_live:Arc<AtomicBool>}
impl Default for Atspi{fn default()->Self{Self{connection:OnceCell::new(),revision:Arc::new(AtomicU64::new(1)),events_live:Arc::new(AtomicBool::new(false))}}}
fn dbus<T>(r:std::result::Result<T,impl std::fmt::Debug>)->Result<T>{r.map_err(|_|Error::new(ErrorCode::BackendFailed,"Accessibility bus operation failed; application may have disappeared"))}
async fn bounded<T>(f:impl std::future::Future<Output=zbus::Result<T>>)->Result<T>{
    tokio::time::timeout(Duration::from_millis(1200),f).await.map_err(|_|Error::new(ErrorCode::Timeout,"Accessibility object did not respond"))?.map_err(|_|Error::new(ErrorCode::BackendFailed,"Accessibility object/interface unavailable"))
}
pub fn normalize_role(role:&str)->String{match role{"push button"=>"button".into(),"text"=>"text".into(),"password text"=>"password-entry".into(),_=>role.to_lowercase().replace(' ',"-")}}
pub fn decode_states(bits:&[u32])->Vec<&'static str>{
    let states=[(1,"active"),(3,"busy"),(4,"checked"),(5,"collapsed"),(6,"defunct"),(7,"editable"),(8,"enabled"),(9,"expandable"),(10,"expanded"),(11,"focusable"),(12,"focused"),(16,"modal"),(20,"pressed"),(22,"selectable"),(23,"selected"),(24,"sensitive"),(25,"showing"),(27,"stale"),(30,"visible"),(41,"checkable"),(43,"read-only")];
    states.into_iter().filter(|(bit,_)|bits.get(bit/32).is_some_and(|word|word&(1u32<<(bit%32))!=0)).map(|(_,name)|name).collect()
}
fn object_id(o:&Object)->String{format!("{}|{}",o.0,o.1)}
fn object_from_id(id:&str)->Result<Object>{
    let (bus,path)=id.split_once('|').ok_or_else(||Error::invalid("Invalid accessibility identity"))?;
    if !bus.starts_with(':'){return Err(Error::invalid("Accessibility references must retain a unique bus owner"));}
    Ok((bus.into(),OwnedObjectPath::try_from(path.to_owned()).map_err(|_|Error::invalid("Invalid accessible object path"))?))
}
impl Atspi{
    async fn connect(&self)->Result<&Connection>{
        self.connection.get_or_try_init(||async{
            let session=dbus(Connection::session().await)?;
            let bus=dbus(Proxy::new(&session,"org.a11y.Bus","/org/a11y/bus","org.a11y.Bus").await)?;
            let address:String=bounded(bus.call("GetAddress",&())).await?;
            // Only the explicitly returned local accessibility bus; never a TCP bus.
            if !address.starts_with("unix:"){return Err(Error::new(ErrorCode::PermissionDenied,"Accessibility bus must be local Unix transport"));}
            let connection=dbus(dbus(zbus::connection::Builder::address(address.as_str()))?.build().await)?;
            self.start_events(&connection).await?;
            Ok(connection)
        }).await
    }
    async fn start_events(&self,connection:&Connection)->Result<()>{
        let registry=dbus(Proxy::new(connection,"org.a11y.atspi.Registry","/org/a11y/atspi/registry","org.a11y.atspi.Registry").await)?;
        // Subscribe before registering interest to avoid a registration/event race.
        let mut streams=vec![];
        for interface in ["org.a11y.atspi.Event.Object","org.a11y.atspi.Event.Window","org.freedesktop.DBus"]{
            let rule=dbus(zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(interface))?.build();
            streams.push(dbus(zbus::MessageStream::for_match_rule(rule,connection,Some(1024)).await)?);
        }
        for category in ["object:","window:"]{let _:()=bounded(registry.call("RegisterEvent",&(category,Vec::<String>::new(),""))).await?;}
        self.events_live.store(true,Ordering::SeqCst);
        for mut stream in streams{
            let revision=self.revision.clone();let live=self.events_live.clone();
            tokio::spawn(async move{
                while let Some(message)=stream.next().await{revision.fetch_add(1,Ordering::SeqCst);if message.is_err(){break;}}
                live.store(false,Ordering::SeqCst);revision.fetch_add(1,Ordering::SeqCst);
            });
        }Ok(())
    }
    async fn proxy<'a>(&self,c:&'a Connection,o:&'a Object,interface:&'a str)->Result<Proxy<'a>>{dbus(Proxy::new(c,o.0.as_str(),o.1.as_str(),interface).await)}
    async fn apps(&self,c:&Connection)->Result<Vec<Object>>{
        let root=dbus(Proxy::new(c,"org.a11y.atspi.Registry",ROOT,ACCESSIBLE).await)?;
        let objects:Vec<Object>=bounded(root.call("GetChildren",&())).await?;
        if objects.len()>512{return Err(Error::new(ErrorCode::ResourceExhausted,"Accessibility app count exceeds budget"));}Ok(objects)
    }
    async fn identity(&self,c:&Connection,o:&Object)->Result<(String,String,String)>{
        let p=self.proxy(c,o,ACCESSIBLE).await?;
        let role:String=bounded(p.call("GetRoleName",&())).await?;
        let name:String=bounded(p.get_property("Name")).await?;
        let role=normalize_role(&role);let fingerprint=format!("{role}:{name}");Ok((role,name,fingerprint))
    }
    async fn app_name(&self,c:&Connection,o:&Object)->Result<String>{
        let p=self.proxy(c,o,ACCESSIBLE).await?;
        let attrs:std::collections::HashMap<String,String>=bounded(p.call("GetAttributes",&())).await.unwrap_or_default();
        if let Some(id)=attrs.get("desktop-entry").or_else(||attrs.get("application-id")){return Ok(id.clone());}
        bounded(p.get_property::<String>("Name")).await
    }
    async fn actions(&self,c:&Connection,o:&Object)->Vec<String>{
        let Ok(proxy)=self.proxy(c,o,"org.a11y.atspi.Action").await else{return vec![];};
        let Ok(count)=bounded(proxy.get_property::<i32>("NActions")).await else{return vec![];};
        let mut names=vec![];
        // GetActions returns localized labels. GetName is the machine action name.
        for index in 0..count.clamp(0,32){if let Ok(name)=bounded(proxy.call::<_,_,String>("GetName",&(index,))).await{names.push(name);}}
        names
    }
    async fn snapshot(&self,ctx:&Context,args:&Value)->Result<Value>{
        let c=self.connect().await?;let revision=self.revision.load(Ordering::SeqCst);
        let budget=args["max_nodes"].as_u64().unwrap_or(200).min(2000)as usize;
        let max_depth=args["max_depth"].as_u64().unwrap_or(5).min(32)as usize;
        let actionable=args["actionable"].as_bool().unwrap_or(false);
        let mut queue=VecDeque::new();let mut known:BTreeMap<String,NativeTarget>=BTreeMap::new();
        for app in self.apps(c).await?{
            let app_name=match self.app_name(c,&app).await{Ok(n)=>n,Err(_)=>continue};
            if args["app"].as_str().is_some_and(|wanted|wanted!=app_name){continue;}
            queue.push_back((app,0usize,app_name,None::<String>));
        }
        let mut seen=BTreeSet::new();let mut nodes=vec![];let mut partial=false;let mut visited=0;
        while let Some((object,depth,app,parent))=queue.pop_front(){
            ctx.check_cancelled()?;
            if nodes.len()>=budget||visited>=budget.saturating_mul(4){partial=true;break;}
            visited+=1;let identity=object_id(&object);if !seen.insert(identity.clone()){partial=true;continue;}
            let (role,mut name,fingerprint)=match self.identity(c,&object).await{Ok(info)=>info,Err(_)=>{partial=true;continue;}};
            let proxy=self.proxy(c,&object,ACCESSIBLE).await?;
            let state_bits:Vec<u32>=bounded(proxy.call("GetState",&())).await.unwrap_or_default();let states=decode_states(&state_bits);
            if states.contains(&"defunct")||states.contains(&"stale"){partial=true;continue;}
            let actions=self.actions(c,&object).await;
            let target=NativeTarget{kind:"ui".into(),identity:identity.clone(),revision,fingerprint,app:app.clone()};known.insert(identity.clone(),target.clone());
            let children:Vec<Object>=bounded(proxy.call("GetChildren",&())).await.unwrap_or_else(|_|{partial=true;vec![]});
            let count=children.len();if count>2000{partial=true;}
            if depth<max_depth{
                for child in children.into_iter().take(2000){queue.push_back((child,depth+1,app.clone(),Some(identity.clone())));}
            }else if count>0{partial=true;}
            if actionable&&actions.is_empty()&&!states.contains(&"editable"){continue;}
            let mut description:String=bounded(proxy.get_property("Description")).await.unwrap_or_default();
            if role=="password-entry"{name="[protected control]".into();description.clear();}
            name=name.chars().take(1024).collect();description=description.chars().take(1024).collect();
            let bounds=match self.proxy(c,&object,"org.a11y.atspi.Component").await{
                Ok(component)=>bounded(component.call::<_,_,(i32,i32,i32,i32)>("GetExtents",&(0u32,))).await.ok().map(|(x,y,width,height)|json!({"x":x,"y":y,"width":width,"height":height,"coordinate_space":"atspi_screen_reported"})),Err(_)=>None,
            };
            nodes.push(json!({"ref":target_marker(target),"role":role,"name":name,"description":description,"states":states,"actions":actions,"app":app,
                "parent_ref":parent.and_then(|p|known.get(&p).cloned()).map(target_marker),"bounds":bounds,"children_count":count}));
        }
        let ending=self.revision.load(Ordering::SeqCst);
        if ending!=revision{partial=true;}
        Ok(json!({"nodes":nodes,"revision":revision,"partial":partial,"changed_during_snapshot":ending!=revision,
            "semantic_coverage":if partial{"partial"}else{"reported_tree"},"event_invalidation":self.events_live.load(Ordering::SeqCst),"visited":visited}))
    }
}
#[async_trait]impl Backend for Atspi{
    fn name(&self)->&'static str{"atspi"}
    fn supports(&self,c:&str)->bool{matches!(c,"app.list"|"ui.snapshot"|"ui.invoke"|"ui.set_text"|"ui.read_text"|"ui.get_value"|"ui.set_value"|"ui.toggle"|"ui.select"|"ui.expand")}
    async fn probe(&self)->Vec<Feature>{let ready=tokio::time::timeout(Duration::from_secs(3),self.connect()).await.is_ok_and(|r|r.is_ok());vec![feature(self.name(),"ui.observe",ready,"AT-SPI bus, registry and object-event subscription","Enable accessibility and run within the application user's D-Bus session")]}
    async fn execute(&self,ctx:&Context,command:&str,args:&Value)->Result<Value>{
        ctx.check_cancelled()?;
        if command=="ui.snapshot"{return self.snapshot(ctx,args).await;}
        let c=self.connect().await?;
        if command=="app.list"{let mut apps=vec![];for o in self.apps(c).await?{if let Ok((_,name,fingerprint))=self.identity(c,&o).await{
            let app=self.app_name(c,&o).await.unwrap_or_else(|_|name.clone());apps.push(json!({"ref":target_marker(NativeTarget{kind:"app".into(),identity:object_id(&o),revision:self.revision.load(Ordering::SeqCst),fingerprint,app:app.clone()}),"name":name,"app":app}));
        }}return Ok(json!({"apps":apps}));}
        let target=native_target(args)?;self.validate(&target).await?;let object=object_from_id(&target.identity)?;
        let (role,_,_)=self.identity(c,&object).await?;
        let result=match command{
            "ui.read_text"=>{
                if role=="password-entry"{return Err(Error::new(ErrorCode::PolicyDenied,"Reading protected/password fields is never exposed"));}
                let p=self.proxy(c,&object,"org.a11y.atspi.Text").await?;
                let count:i32=bounded(p.get_property("CharacterCount")).await?;let max=args["max_chars"].as_u64().unwrap_or(4096).min(65536)as i32;
                let text:String=bounded(p.call("GetText",&(0i32,count.clamp(0,max)))).await?;json!({"text":text,"truncated":count>max})
            }
            "ui.set_text"=>{let p=self.proxy(c,&object,"org.a11y.atspi.EditableText").await?;ctx.check_cancelled()?;let success:bool=bounded(p.call("SetTextContents",&(arg_str(args,"text")?,))).await.map_err(Error::uncertain)?;
                if !success{return Err(Error::new(ErrorCode::BackendFailed,"Application rejected editable text").uncertain());}json!({"changed":true})}
            "ui.get_value"|"ui.set_value"=>{
                let p=self.proxy(c,&object,"org.a11y.atspi.Value").await?;
                let minimum:f64=bounded(p.get_property("MinimumValue")).await?;let maximum:f64=bounded(p.get_property("MaximumValue")).await?;
                if command=="ui.set_value"{let value=args["value"].as_f64().ok_or_else(||Error::invalid("value required"))?;
                    if !value.is_finite()||value<minimum||value>maximum{return Err(Error::invalid("Value is outside the accessible range"));}
                    ctx.check_cancelled()?;dbus(p.set_property("CurrentValue",value).await).map_err(Error::uncertain)?;
                }
                let value:f64=bounded(p.get_property("CurrentValue")).await?;json!({"value":value,"minimum":minimum,"maximum":maximum})
            }
            "ui.select"=>{let p=self.proxy(c,&object,"org.a11y.atspi.Selection").await?;let index=args["index"].as_i64().ok_or_else(||Error::invalid("index required"))?as i32;
                let success:bool=bounded(p.call("SelectChild",&(index,))).await.map_err(Error::uncertain)?;if !success{return Err(Error::new(ErrorCode::BackendFailed,"Selection rejected").uncertain());}json!({"selected":true})}
            "ui.invoke"|"ui.toggle"|"ui.expand"=>{
                let names=self.actions(c,&object).await;
                let requested=args["action"].as_str().or(match command{"ui.toggle"=>Some("toggle"),"ui.expand"=>Some("expand"),_=>None});
                let index=if let Some(action)=requested{names.iter().position(|n|n==action)}else if names.len()==1{Some(0)}else{None};
                let index=index.ok_or_else(||Error::new(if names.len()>1&&requested.is_none(){ErrorCode::AmbiguousTarget}else{ErrorCode::Unsupported},"Choose an exact advertised action; no coordinate fallback is attempted"))?as i32;
                let p=self.proxy(c,&object,"org.a11y.atspi.Action").await?;self.validate(&target).await?;ctx.check_cancelled()?;
                let accepted:bool=bounded(p.call("DoAction",&(index,))).await.map_err(Error::uncertain)?;if !accepted{return Err(Error::new(ErrorCode::BackendFailed,"Application rejected the action").uncertain());}json!({"invoked":true})
            }
            _=>return Err(Error::new(ErrorCode::Unsupported,"Unknown semantic operation")),
        };
        if !matches!(command,"ui.read_text"|"ui.get_value"){self.revision.fetch_add(1,Ordering::SeqCst);}
        Ok(result)
    }
    async fn validate(&self,target:&NativeTarget)->Result<()>{
        let c=self.connect().await?;
        if !self.events_live.load(Ordering::SeqCst)||target.revision!=self.revision.load(Ordering::SeqCst){return Err(Error::new(ErrorCode::StaleReference,"Accessibility generation changed or event stream was lost; take a fresh snapshot"));}
        let object=object_from_id(&target.identity)?;
        let (_,_,fingerprint)=self.identity(c,&object).await.map_err(|_|Error::new(ErrorCode::StaleReference,"Accessibility object disappeared"))?;
        if fingerprint!=target.fingerprint{return Err(Error::new(ErrorCode::StaleReference,"Accessibility object identity changed"));}
        let p=self.proxy(c,&object,ACCESSIBLE).await?;let bits:Vec<u32>=bounded(p.call("GetState",&())).await?;
        if decode_states(&bits).iter().any(|s|matches!(*s,"defunct"|"stale")){return Err(Error::new(ErrorCode::StaleReference,"Accessible object is defunct/stale"));}Ok(())
    }
}
#[cfg(test)]mod tests{
    use super::*;
    #[test]fn state_bits_cross_words(){assert_eq!(decode_states(&[1<<8,1<<(43-32)]),vec!["enabled","read-only"]);}
    #[test]fn password_role_is_distinct(){assert_eq!(normalize_role("password text"),"password-entry");}
    #[test]fn machine_button_role(){assert_eq!(normalize_role("push button"),"button");}
    #[test]fn unique_owner_required(){assert!(object_from_id("org.app|/object").is_err());assert!(object_from_id(":1.5|/object").is_ok());}
}
