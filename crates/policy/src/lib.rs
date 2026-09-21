//! Pure authorization decisions plus Linux descriptor-relative filesystem confinement.
//! Caller-provided backend preferences and confirmation flags never grant authority.
mod filesystem;
pub use filesystem::{Root, validate_relative_path};
use semwright_types::{CommandDescriptor, Error, ErrorCode, Result, Risk};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all="kebab-case")]
pub enum Profile { #[default] Observe, Desktop, Workspace, Developer }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FilesystemGrant {
    pub name: String,
    pub path: PathBuf,
    #[serde(default)] pub read: bool,
    #[serde(default)] pub write: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PolicyConfig {
    #[serde(default)] pub profile: Profile,
    #[serde(default)] pub allow: BTreeSet<String>,
    #[serde(default)] pub deny: BTreeSet<String>,
    #[serde(default)] pub apps: BTreeSet<String>,
    #[serde(default)] pub filesystem: Vec<FilesystemGrant>,
    /// Extra classes may require confirmation; built-in dangerous classes cannot be removed.
    #[serde(default)] pub confirm_mutations: bool,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision { Allow, Deny(&'static str), RequireConfirmation }
#[derive(Clone)]
pub struct Policy { config: PolicyConfig, granted: BTreeSet<String> }
impl Policy {
    pub fn new(config: PolicyConfig) -> Result<Self> {
        let mut granted:BTreeSet<String>=["desktop.observe","app.observe","window.observe","ui.observe","process.observe"]
            .into_iter().map(String::from).collect();
        if config.profile!=Profile::Observe {
            granted.extend(["window.manage","ui.invoke","notifications.send"].into_iter().map(String::from));
        }
        // Input, clipboard, app launch and adapters are explicit even in desktop/developer.
        // A profile label must never silently authorize another trust boundary.
        granted.extend(config.allow.iter().cloned());
        for denied in &config.deny {granted.remove(denied);}
        if granted.contains("shell.exec") {return Err(Error::invalid("This build deliberately exposes no unrestricted shell"));}
        let mut names=BTreeSet::new();
        for root in &config.filesystem {
            if root.name.is_empty() || root.name.len()>64 || !root.name.bytes().all(|b|b.is_ascii_alphanumeric()||matches!(b,b'_'|b'-')) || !names.insert(&root.name) {
                return Err(Error::invalid("Filesystem root names must be unique identifiers"));
            }
            if !root.path.is_absolute() || root.path==std::path::Path::new("/") {return Err(Error::invalid("Filesystem roots must be absolute and cannot be /"));}
        }
        Ok(Self{config,granted})
    }
    pub fn capabilities(&self)->&BTreeSet<String>{&self.granted}
    pub fn config(&self)->&PolicyConfig{&self.config}
    pub fn check(&self, command:&CommandDescriptor,args:&Value,target_app:Option<&str>)->Decision {
        for cap in &command.requires {
            if cap.starts_with("filesystem.") {
                let root=args.get("root").and_then(Value::as_str);
                let matching=self.config.filesystem.iter().find(|r|Some(r.name.as_str())==root);
                let permitted=matching.is_some_and(|r|if cap=="filesystem.read"{r.read}else{r.write});
                if !permitted || self.config.deny.contains(cap) {return Decision::Deny("No grant for this filesystem root and operation");}
            } else if !self.granted.contains(cap) {
                return Decision::Deny("Required capability is not granted to this broker session");
            }
        }
        if !self.config.apps.is_empty() {
            if let Some(app)=target_app.or_else(||args.get("app").and_then(Value::as_str)) {
                if !self.config.apps.contains(app){return Decision::Deny("Target application is outside the configured scope");}
            } else if command.risk.mutates() && command.name.starts_with("ui.") {
                return Decision::Deny("Scoped UI mutation requires a resolved application identity");
            }
        }
        if command.risk.sensitive() || (self.config.confirm_mutations && command.risk.mutates()) {
            Decision::RequireConfirmation
        } else {Decision::Allow}
    }
    pub fn enforce(&self,command:&CommandDescriptor,args:&Value,target_app:Option<&str>)->Result<bool>{
        match self.check(command,args,target_app){
            Decision::Allow=>Ok(false),Decision::RequireConfirmation=>Ok(true),
            Decision::Deny(reason)=>Err(Error::new(ErrorCode::PolicyDenied,reason)),
        }
    }
    pub fn grant_fingerprint(&self)->String{
        use sha2::{Digest,Sha256};
        let body=serde_json::to_vec(&self.config).unwrap_or_default();format!("{:x}",Sha256::digest(body))
    }
}
/// Select metadata suitable for the trusted operator, never the audit sink.
/// JSON escaping prevents a malicious field from injecting terminal control codes.
pub fn confirmation_summary(command:&str,args:&Value)->String{
    let mut redacted=serde_json::Map::new();
    if let Some(map)=args.as_object(){for (k,v) in map{
        if ["text","body","token","password","secret","inputs","recipe"].contains(&k.as_str()){
            redacted.insert(k.clone(),Value::String("[REDACTED]".into()));
        }else if !k.starts_with('_'){redacted.insert(k.clone(),v.clone());}
    }}
    format!("{} {}",serde_json::to_string(command).unwrap_or_default(),Value::Object(redacted))
}
#[cfg(test)] mod tests{
    use super::*; use semwright_types::Idempotency; use proptest::prelude::*;
    fn descriptor(cap:&str,risk:Risk)->CommandDescriptor{CommandDescriptor{
        name:"test.action".into(),version:"1.0".into(),description:String::new(),input_schema:serde_json::json!({}),output_schema:serde_json::json!({}),requires:vec![cap.into()],risk,
        idempotency:Idempotency::NonIdempotent,timeout_ms:1000,dry_run:true,interactive_consent:false,backends:vec!["fake".into()],
    }}
    #[test] fn observe_cannot_mutate(){let p=Policy::new(PolicyConfig::default()).unwrap();assert!(matches!(p.check(&descriptor("ui.invoke",Risk::Mutating),&serde_json::json!({}),None),Decision::Deny(_)));}
    #[test] fn dangerous_cannot_self_approve(){let mut c=PolicyConfig::default();c.allow.insert("clipboard.read".into());let p=Policy::new(c).unwrap();assert_eq!(p.check(&descriptor("clipboard.read",Risk::SecretAccess),&serde_json::json!({"confirmed":true}),None),Decision::RequireConfirmation);}
    #[test] fn explicit_deny_wins(){let mut c=PolicyConfig::default();c.allow.insert("ui.invoke".into());c.deny.insert("ui.invoke".into());let p=Policy::new(c).unwrap();assert!(!p.capabilities().contains("ui.invoke"));}
    #[test] fn desktop_does_not_grant_clipboard_or_shell(){let c=PolicyConfig{profile:Profile::Desktop,..Default::default()};let p=Policy::new(c).unwrap();for s in ["clipboard.read","shell.exec","browser.modify","input.keyboard"]{assert!(!p.capabilities().contains(s));}}
    #[test] fn terminal_controls_are_escaped(){let summary=confirmation_summary("test",&serde_json::json!({"name":"\u{1b}[2J","text":"supersecret"}));assert!(!summary.contains('\u{1b}'));assert!(!summary.contains("supersecret"));}
    proptest!{
        #[test] fn deny_is_monotone(cap in "[a-z]{1,20}\\.[a-z]{1,20}"){
            let mut c=PolicyConfig::default();c.allow.insert(cap.clone());c.deny.insert(cap.clone());let p=Policy::new(c).unwrap();prop_assert!(!p.capabilities().contains(&cap));
        }
    }
}
